//! Exotic shop — a recurring time-windowed catalog of hybrid-tier animals.
//!
//! The shop runs on a wall-clock cycle: **4 hours open → 45 minutes closed
//! → repeat**, with no persisted state. Each open window deterministically
//! rolls a small pool of offerings from the hybrid catalog, seeded by the
//! window index so all clients (multi-instance + reload after the window
//! closes) agree on what's for sale right now.
//!
//! Determinism rationale: the only persistent breadcrumb is the wall-clock
//! itself. A second client opening mid-window sees identical offerings
//! without any cross-process coordination, and a player who comes back six
//! hours later sees the *current* window's offerings — not the historical
//! one they almost bought from.

use chrono::{DateTime, TimeZone, Utc};

use super::species::{self, IncomeKind, SpeciesId, xorshift64};

/// Seconds the shop is *open* per cycle.
pub const WINDOW_OPEN_SECS: i64 = 4 * 3600;
/// Total cycle length: 4 hours open + 45 minutes closed.
pub const WINDOW_CYCLE_SECS: i64 = WINDOW_OPEN_SECS + 45 * 60;
/// How many offerings to roll per window.
pub const OFFERINGS_PER_WINDOW: usize = 3;
/// DNA Helix cost to skip the 45-minute closed gap and open the next window
/// early. Spent once per gap; the override self-expires when that window
/// naturally opens.
pub const SKIP_WAIT_DNA_COST: u64 = 50;

/// One slot in the current exotic shop window — a hybrid species priced in
/// either coins or DNA Helix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExoticOffering {
    pub species: SpeciesId,
    pub price: Price,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Price {
    Coins(u64),
    Dna(u64),
}

/// Summary of an open shop window. `None` when the shop is between cycles.
#[derive(Debug, Clone)]
pub struct ExoticWindow {
    pub index: i64,
    pub opens_at: DateTime<Utc>,
    pub closes_at: DateTime<Utc>,
    pub offerings: Vec<ExoticOffering>,
}

/// The window index at `now`. Stable across clock skew because it uses
/// `div_euclid` against a fixed cycle length.
pub fn window_index(now: DateTime<Utc>) -> i64 {
    now.timestamp().div_euclid(WINDOW_CYCLE_SECS)
}

/// True when `now` falls in the open portion of its window (the first
/// `WINDOW_OPEN_SECS` of the cycle).
pub fn is_open(now: DateTime<Utc>) -> bool {
    now.timestamp().rem_euclid(WINDOW_CYCLE_SECS) < WINDOW_OPEN_SECS
}

/// Instant the current window first opened. Same value whether you call it
/// at 0 minutes in or 3h59m in.
pub fn window_opened_at(now: DateTime<Utc>) -> DateTime<Utc> {
    let idx = window_index(now);
    Utc.timestamp_opt(idx * WINDOW_CYCLE_SECS, 0).unwrap()
}

/// Instant the *next* shop window opens. If the shop is currently open,
/// that's the start of the *next* cycle (one cycle from now's window).
pub fn next_open_at(now: DateTime<Utc>) -> DateTime<Utc> {
    let opens = window_opened_at(now);
    if is_open(now) {
        opens + chrono::Duration::seconds(WINDOW_CYCLE_SECS)
    } else {
        opens + chrono::Duration::seconds(WINDOW_CYCLE_SECS)
    }
}

/// The current open window, or `None` if `now` is in the 45-minute gap.
pub fn current_window(now: DateTime<Utc>) -> Option<ExoticWindow> {
    if !is_open(now) {
        return None;
    }
    let opens = window_opened_at(now);
    let closes = opens + chrono::Duration::seconds(WINDOW_OPEN_SECS);
    let idx = window_index(now);
    Some(ExoticWindow {
        index: idx,
        opens_at: opens,
        closes_at: closes,
        offerings: roll_offerings(idx),
    })
}

/// The window the player can shop right now, accounting for a paid skip.
///
/// When the shop is naturally open, returns that window. When it's in the
/// 45-minute gap, returns the *upcoming* window's offerings **only if**
/// `skip_window` matches it — i.e. the player paid 50 DNA Helix during this
/// gap. The override is keyed to `window_index(now) + 1`, so it's honored
/// only for the current gap and ignored once that window opens for real
/// (no need to clear it explicitly).
pub fn effective_window(now: DateTime<Utc>, skip_window: Option<i64>) -> Option<ExoticWindow> {
    if let Some(w) = current_window(now) {
        return Some(w);
    }
    let next_idx = window_index(now) + 1;
    if skip_window == Some(next_idx) {
        let opens = next_open_at(now);
        let closes = opens + chrono::Duration::seconds(WINDOW_OPEN_SECS);
        return Some(ExoticWindow {
            index: next_idx,
            opens_at: opens,
            closes_at: closes,
            offerings: roll_offerings(next_idx),
        });
    }
    None
}

/// Whether the player can buy right now, accounting for a paid skip.
pub fn is_open_with_skip(now: DateTime<Utc>, skip_window: Option<i64>) -> bool {
    effective_window(now, skip_window).is_some()
}

/// Time remaining (in seconds) until the current window closes, or until
/// the next window opens if the shop is currently closed. Always positive.
pub fn seconds_until_state_change(now: DateTime<Utc>) -> i64 {
    let pos = now.timestamp().rem_euclid(WINDOW_CYCLE_SECS);
    if pos < WINDOW_OPEN_SECS {
        WINDOW_OPEN_SECS - pos
    } else {
        WINDOW_CYCLE_SECS - pos
    }
}

/// Pick `OFFERINGS_PER_WINDOW` distinct exotics from the catalog, seeded by
/// the window index. Each offering is priced exactly as defined on the
/// species: `purchase_cost` in the species' `purchase_currency`. The window
/// seed only chooses *which* exotics appear, not the price — so a species
/// listed at "39 DNA Helix" always shows up at 39 DNA Helix.
pub fn roll_offerings(window_idx: i64) -> Vec<ExoticOffering> {
    let mut exotics: Vec<&'static species::SpeciesDef> = species::all_exotics().collect();
    exotics.sort_by_key(|d| d.id); // stable input order
    if exotics.is_empty() {
        return Vec::new();
    }
    let n = exotics.len();
    let want = OFFERINGS_PER_WINDOW.min(n);

    let mut state = xorshift64(window_idx as u64);
    let mut picks: Vec<usize> = Vec::with_capacity(want);
    while picks.len() < want {
        state = xorshift64(state);
        let idx = (state as usize) % n;
        if !picks.contains(&idx) {
            picks.push(idx);
        }
    }

    let mut offerings: Vec<ExoticOffering> = Vec::with_capacity(want);
    for exotic_idx in picks {
        let def = exotics[exotic_idx];
        // Price straight from the catalog: the species decides both the
        // amount and the currency.
        let price = match def.purchase_currency {
            IncomeKind::DnaHelix => Price::Dna(def.purchase_cost),
            IncomeKind::Coin => Price::Coins(def.purchase_cost),
        };
        offerings.push(ExoticOffering { species: def.id, price });
    }
    offerings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn cycle_is_open_for_4h_then_closed_for_45m() {
        // Window 0 opens at epoch.
        assert!(is_open(at(0)));
        assert!(is_open(at(WINDOW_OPEN_SECS - 1)));
        assert!(!is_open(at(WINDOW_OPEN_SECS)));
        assert!(!is_open(at(WINDOW_CYCLE_SECS - 1)));
        // Window 1 reopens at the cycle boundary.
        assert!(is_open(at(WINDOW_CYCLE_SECS)));
    }

    #[test]
    fn same_window_index_returns_identical_offerings() {
        // Two times inside the same open window must produce the same pool.
        let early = at(60);
        let late = at(WINDOW_OPEN_SECS - 60);
        let a = current_window(early).unwrap();
        let b = current_window(late).unwrap();
        assert_eq!(a.index, b.index);
        assert_eq!(a.offerings, b.offerings);
    }

    #[test]
    fn distinct_windows_can_yield_different_offerings() {
        // Sanity check the rolling diverges across windows (statistical, not
        // strict). Even when the catalog has only a few exotics (so every
        // window shows them all), the pick order and per-window prices still
        // vary, so the full offering vectors should differ. Run a few windows.
        let w0 = current_window(at(60)).unwrap();
        let w2 = current_window(at(WINDOW_CYCLE_SECS * 2 + 60)).unwrap();
        let w4 = current_window(at(WINDOW_CYCLE_SECS * 4 + 60)).unwrap();
        assert!(
            w0.offerings != w2.offerings || w2.offerings != w4.offerings,
            "expected at least one of the rolls to differ across windows"
        );
    }

    #[test]
    fn closed_window_returns_none() {
        let mid_gap = at(WINDOW_OPEN_SECS + 1);
        assert!(current_window(mid_gap).is_none());
    }

    #[test]
    fn seconds_until_state_change_matches_phase() {
        // 1 second into open window → expect (4h - 1) seconds left until close.
        assert_eq!(seconds_until_state_change(at(1)), WINDOW_OPEN_SECS - 1);
        // Just after close → next open is 45 minutes away.
        assert_eq!(
            seconds_until_state_change(at(WINDOW_OPEN_SECS)),
            45 * 60
        );
    }

    #[test]
    fn paid_skip_opens_next_window_only_during_its_gap() {
        // In the closed gap of window 0, the matching skip is index 1.
        let gap = at(WINDOW_OPEN_SECS + 10);
        assert!(current_window(gap).is_none());
        assert!(effective_window(gap, None).is_none());
        // Skip keyed to the wrong window is ignored.
        assert!(effective_window(gap, Some(0)).is_none());
        // Skip keyed to the upcoming window opens it early with that window's pool.
        let w = effective_window(gap, Some(1)).expect("skip should open next window");
        assert_eq!(w.index, 1);
        assert_eq!(w.offerings, roll_offerings(1));
        assert!(is_open_with_skip(gap, Some(1)));
        // A naturally-open window ignores the skip entirely.
        let open = at(60);
        let w0 = effective_window(open, Some(999)).unwrap();
        assert_eq!(w0.index, 0);
        // Once window 1 actually opens, the old skip (index 1) is stale —
        // the next gap's skip would be index 2, so Some(1) no longer applies.
        let next_gap = at(WINDOW_CYCLE_SECS + WINDOW_OPEN_SECS + 10);
        assert!(effective_window(next_gap, Some(1)).is_none());
    }
}
