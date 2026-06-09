//! Target-based catching (Phase 3) — the new core verb that replaces the old
//! hover-to-fill mechanic.
//!
//! The loop, MMORPG-style: the player clicks a wild animal to *target* it, then
//! *engages* (catches). The target carries a **catch-resistance bar** ("catch
//! health"); depleting it captures the animal. Depletion comes from three
//! sources, exactly as the roadmap specifies:
//!
//! 1. a **stat-driven roll loop** ticking the bar down over time (your catch
//!    power vs the target's resistance);
//! 2. **input abilities** (from equipped gear) that deplete the bar faster
//!    and/or **debuff** the target (weaken its resistance, prevent it fleeing);
//! 3. **random skill-check moments** (DBD-style timed inputs) whose frequency
//!    scales with the animal's **tier** — hitting them lands bonus depletion.
//!
//! This module is **pure logic over a seeded [`LcgRng`]** — no rendering, no
//! input polling, no wall-clock. The client (and, later, a SpacetimeDB reducer)
//! drives it by feeding `dt` and discrete events, so the very same engagement
//! can run client-side for Solo and server-authoritative online. Per-engager
//! `contribution` is tracked here as the hook the Phase 5 co-op/lock-battle
//! model builds on.

use crate::game::biome::LcgRng;
use crate::game::species::{self, SpeciesId};

/// Catch tier 1–5 — the difficulty/scarcity band of a target. Reuses the same
/// `purchase_cost`-derived rarity proxy as [`species::captures_required`] so the
/// catch difficulty and the codex rarity stay in step. A higher tier means a
/// deeper resistance bar *and* more frequent skill checks.
pub fn catch_tier(species: SpeciesId) -> u8 {
    species::captures_required(species).clamp(1, 5) as u8
}

/// The catch-relevant profile of a target, derived from its species (or built
/// directly in tests). Everything the engagement needs to know about *what is
/// being caught*; the engager's side lives in [`CatchStats`].
#[derive(Clone, Debug, PartialEq)]
pub struct TargetProfile {
    pub species: SpeciesId,
    pub tier: u8,
    /// Full depth of the catch-resistance bar ("catch health").
    pub resistance_max: f32,
    /// Expected skill-check moments per second while engaged. Scales with tier.
    pub skill_check_frequency: f32,
}

impl TargetProfile {
    /// Derive a target's catch profile from its species. Resistance and
    /// skill-check cadence both climb with tier.
    pub fn for_species(species: SpeciesId) -> Self {
        let tier = catch_tier(species);
        Self {
            species,
            tier,
            // 100 → 260 across tiers 1–5.
            resistance_max: 60.0 + 40.0 * tier as f32,
            // ~0.27/s at tier 1 up to ~0.75/s at tier 5 (a check every ~1.3–3.7s).
            skill_check_frequency: 0.15 + 0.12 * tier as f32,
        }
    }
}

/// The engager's catch power, summed from equipped gear and owned-collection
/// bonuses (see [`crate::game::gear`]). These are the "two sources" of catch
/// power from the roadmap; this struct is what they ultimately resolve to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CatchStats {
    /// Baseline resistance depleted per second by the stat-roll loop.
    pub catch_power: f32,
    /// Multiplier on the bonus depletion from a landed skill check.
    pub skill_bonus: f32,
    /// Multiplier on how strongly abilities debuff the target.
    pub debuff_power: f32,
    /// Maximum catch **stamina**. Each catch tick spends stamina in proportion
    /// to the target's resistance, so a bigger pool (earned through progression)
    /// is what lets a player sustain catching deeper-bar, rarer animals — the
    /// difficulty-scaling gate. Drained on the engager's side, not here.
    pub max_stamina: f32,
}

impl Default for CatchStats {
    /// Bare-handed baseline (no gear, empty collection): catchable but slow, and
    /// a starter stamina pool that comfortably handles low-tier catches only.
    fn default() -> Self {
        Self { catch_power: 14.0, skill_bonus: 1.0, debuff_power: 1.0, max_stamina: 200.0 }
    }
}

/// Live debuffs an engagement has stacked on its target via abilities. All
/// decay over time; the engagement applies their effects each tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Debuffs {
    /// Fraction (0..1) added to effective depletion — a "softened" target.
    /// Decays linearly back to 0 over its remaining duration.
    pub weaken: f32,
    /// Seconds remaining during which the target cannot flee (lure/tether).
    pub flee_lock_secs: f32,
    /// Seconds remaining on the current `weaken` stack.
    pub weaken_secs: f32,
}

/// A DBD-style timed skill-check window. Surfaced on the engagement for the
/// client to render; the player calls [`CatchEngagement::hit_skill_check`]
/// while it is live to claim the bonus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillCheck {
    /// Seconds left to hit before the window lapses.
    pub remaining: f32,
    /// Total window length (for rendering a shrinking gauge).
    pub window: f32,
    /// Resistance depleted if hit (already tier-scaled; `skill_bonus` applies
    /// on top at hit time).
    pub reward: f32,
}

/// An equippable ability the player triggers mid-engagement. Sourced from gear;
/// the engagement resolves each into instant depletion and/or a debuff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbilityKind {
    /// A net: a big burst of instant catch-resistance depletion.
    Net,
    /// A lure/tether: locks the target in place so it can't flee (a support
    /// role in a group); minor depletion.
    Lure,
    /// A trap: weakens the target's resistance over time (a depletion debuff).
    Trap,
}

/// How an engagement resolved this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngagementOutcome {
    /// Still in progress.
    Ongoing,
    /// The bar emptied — the animal is captured.
    Captured,
    /// The target fled (reserved for the flee model; not yet emitted by `tick`).
    Fled,
    /// The engager ran out of stamina mid-catch — the attempt ends without a
    /// capture (the animal is left in the world to try again later).
    Exhausted,
}

/// A single in-flight catch engagement against one target. Owns the resistance
/// bar, active debuffs, the current skill-check window, and its own RNG stream
/// so the whole thing is deterministic and reproducible from `(target, seed)`.
#[derive(Clone, Debug)]
pub struct CatchEngagement {
    pub target: TargetProfile,
    /// Current catch-resistance remaining; capture fires at `<= 0`.
    pub resistance: f32,
    pub debuffs: Debuffs,
    /// The live skill-check window, if one is currently up.
    pub skill_check: Option<SkillCheck>,
    /// Total resistance this engager has depleted — the per-engager
    /// **contribution** the Phase 5 co-op/ownership model keys on.
    pub contribution: f32,
    /// Seconds until the next skill-check window may spawn.
    next_check_in: f32,
    /// Accumulated time toward the next discrete depletion tick.
    tick_accum: f32,
    rng: LcgRng,
}

/// Cadence of the stat-roll depletion. The bar steps down once per tick rather
/// than draining continuously, so progress reads as increments — and it mirrors
/// an authoritative server tick when this runs inside a SpacetimeDB reducer.
pub const CATCH_TICK_SECS: f32 = 0.5;

/// Stamina spent **per catch tick, per point of the target's `resistance_max`**.
/// Cost-per-tick = this × resistance, so rarer/deeper-bar animals burn the pool
/// faster: a player whose `max_stamina` can't cover the whole catch is exhausted
/// before the bar empties. Tuned with [`CatchStats::default`]'s pool so a fresh
/// player handles low tiers and must progress to sustain the high ones.
pub const STAMINA_PER_TICK_PER_RESISTANCE: f32 = 0.1;

/// Base resistance a landed skill check removes, before tier and `skill_bonus`
/// scaling. Tuned so a well-timed check is a meaningful chunk of a low-tier bar.
const SKILL_CHECK_BASE_REWARD: f32 = 18.0;
/// How long a skill-check window stays hittable.
const SKILL_CHECK_WINDOW: f32 = 1.1;

impl CatchEngagement {
    /// Begin engaging `target`. `seed` makes the skill-check cadence
    /// deterministic (derive it from the target's instance id online).
    pub fn new(target: TargetProfile, seed: u64) -> Self {
        let resistance = target.resistance_max;
        let mut rng = LcgRng::new(seed);
        let next_check_in = Self::roll_next_check(&target, &mut rng);
        Self {
            target,
            resistance,
            debuffs: Debuffs::default(),
            skill_check: None,
            contribution: 0.0,
            next_check_in,
            tick_accum: 0.0,
            rng,
        }
    }

    /// Fraction of the bar already depleted, 0..1 — for rendering the bar.
    pub fn progress(&self) -> f32 {
        if self.target.resistance_max <= 0.0 {
            return 1.0;
        }
        (1.0 - self.resistance / self.target.resistance_max).clamp(0.0, 1.0)
    }

    /// Advance the engagement by `dt` seconds under the engager's `stats`,
    /// spending from the engager's `stamina` pool. Runs the stepped stat-roll
    /// depletion, decays debuffs, and spawns/expires skill-check windows. Each
    /// catch tick costs stamina proportional to the target's resistance; if the
    /// pool can't cover a tick the attempt ends [`EngagementOutcome::Exhausted`].
    pub fn tick(&mut self, dt: f32, stats: &CatchStats, stamina: &mut f32) -> EngagementOutcome {
        if dt <= 0.0 {
            return self.outcome();
        }
        self.decay_debuffs(dt);

        // Stat-roll loop, stepped: accumulate time and deplete one increment per
        // discrete tick, so the bar drops in chunks rather than draining smooth.
        // Each tick also spends stamina ∝ the target's resistance.
        let effective_dps = stats.catch_power * (1.0 + self.debuffs.weaken);
        let stamina_cost = STAMINA_PER_TICK_PER_RESISTANCE * self.target.resistance_max;
        self.tick_accum += dt;
        while self.tick_accum >= CATCH_TICK_SECS {
            self.tick_accum -= CATCH_TICK_SECS;
            if *stamina < stamina_cost {
                return EngagementOutcome::Exhausted;
            }
            *stamina -= stamina_cost;
            self.deplete(effective_dps * CATCH_TICK_SECS);
            if self.is_captured() {
                break;
            }
        }

        // Skill-check lifecycle: expire a live window, or count down to the next.
        if let Some(sc) = &mut self.skill_check {
            sc.remaining -= dt;
            if sc.remaining <= 0.0 {
                self.skill_check = None;
            }
        } else {
            self.next_check_in -= dt;
            if self.next_check_in <= 0.0 {
                self.spawn_skill_check();
            }
        }
        self.outcome()
    }

    /// Trigger an equipped ability. Resolves to instant depletion and/or a
    /// debuff, scaled by the engager's stats. Safe to call any time.
    pub fn use_ability(&mut self, ability: AbilityKind, stats: &CatchStats) {
        match ability {
            AbilityKind::Net => {
                // A burst proportional to baseline power — the bread-and-butter
                // active. Scales with tier so it stays relevant on deep bars.
                self.deplete(stats.catch_power * 2.5 + 8.0 * self.target.tier as f32);
            }
            AbilityKind::Lure => {
                self.deplete(stats.catch_power * 0.5);
                self.debuffs.flee_lock_secs = self.debuffs.flee_lock_secs.max(4.0);
            }
            AbilityKind::Trap => {
                // Stack a weaken debuff that speeds all depletion for a while.
                self.debuffs.weaken = (self.debuffs.weaken + 0.4 * stats.debuff_power).min(1.5);
                self.debuffs.weaken_secs = self.debuffs.weaken_secs.max(5.0);
            }
        }
    }

    /// The player hit the live skill check. Applies its (skill-bonus-scaled)
    /// reward and clears the window. Returns `true` if a window was live.
    pub fn hit_skill_check(&mut self, stats: &CatchStats) -> bool {
        let Some(sc) = self.skill_check.take() else {
            return false;
        };
        self.deplete(sc.reward * stats.skill_bonus);
        // The next window cadence resumes from here.
        self.next_check_in = Self::roll_next_check(&self.target, &mut self.rng);
        true
    }

    /// The player let the skill check lapse (or deliberately skipped it).
    /// Clears the window with no reward; the cadence resumes.
    pub fn miss_skill_check(&mut self) {
        if self.skill_check.take().is_some() {
            self.next_check_in = Self::roll_next_check(&self.target, &mut self.rng);
        }
    }

    /// True once the catch-resistance bar is empty.
    pub fn is_captured(&self) -> bool {
        self.resistance <= 0.0
    }

    // ── internals ─────────────────────────────────────────────────────────────

    fn outcome(&self) -> EngagementOutcome {
        if self.is_captured() {
            EngagementOutcome::Captured
        } else {
            EngagementOutcome::Ongoing
        }
    }

    /// Remove `amount` from the bar (clamped at 0) and bank it as contribution.
    fn deplete(&mut self, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        let applied = amount.min(self.resistance.max(0.0));
        self.resistance -= amount;
        self.contribution += applied;
    }

    fn decay_debuffs(&mut self, dt: f32) {
        if self.debuffs.flee_lock_secs > 0.0 {
            self.debuffs.flee_lock_secs = (self.debuffs.flee_lock_secs - dt).max(0.0);
        }
        if self.debuffs.weaken_secs > 0.0 {
            self.debuffs.weaken_secs = (self.debuffs.weaken_secs - dt).max(0.0);
            if self.debuffs.weaken_secs == 0.0 {
                self.debuffs.weaken = 0.0;
            }
        }
    }

    fn spawn_skill_check(&mut self) {
        let reward = SKILL_CHECK_BASE_REWARD * (0.6 + 0.2 * self.target.tier as f32);
        self.skill_check = Some(SkillCheck {
            remaining: SKILL_CHECK_WINDOW,
            window: SKILL_CHECK_WINDOW,
            reward,
        });
    }

    /// Roll the gap until the next skill-check window from the target's
    /// frequency, with ±40% jitter so the cadence never feels metronomic.
    fn roll_next_check(target: &TargetProfile, rng: &mut LcgRng) -> f32 {
        let mean = 1.0 / target.skill_check_frequency.max(0.05);
        let jitter = 0.6 + 0.8 * rng.next_f32(); // 0.6..1.4
        (mean * jitter).max(0.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(tier_resistance: f32) -> TargetProfile {
        TargetProfile {
            species: "field_mouse",
            tier: 1,
            resistance_max: tier_resistance,
            skill_check_frequency: 0.5,
        }
    }

    #[test]
    fn stat_roll_eventually_captures() {
        let mut e = CatchEngagement::new(profile(100.0), 1);
        let stats = CatchStats { catch_power: 50.0, ..Default::default() };
        // 50/s vs 100 → captured after ~2s. Step in small ticks.
        let mut outcome = EngagementOutcome::Ongoing;
        for _ in 0..240 {
            outcome = e.tick(1.0 / 60.0, &stats, &mut 1.0e9_f32);
            if outcome == EngagementOutcome::Captured {
                break;
            }
        }
        assert_eq!(outcome, EngagementOutcome::Captured);
        assert!(e.is_captured());
        // Contribution never exceeds the bar depth (clamped at the kill).
        assert!(e.contribution <= 100.0 + f32::EPSILON);
    }

    #[test]
    fn depletion_is_stepped_not_continuous() {
        let mut e = CatchEngagement::new(profile(1000.0), 11);
        let stats = CatchStats { catch_power: 20.0, ..Default::default() };
        // A small sub-tick step shouldn't move the bar yet.
        e.tick(CATCH_TICK_SECS * 0.4, &stats, &mut 1.0e9_f32);
        assert_eq!(e.resistance, 1000.0, "no depletion within a tick");
        // Crossing the tick boundary applies exactly one increment.
        e.tick(CATCH_TICK_SECS * 0.7, &stats, &mut 1.0e9_f32);
        assert!((e.resistance - (1000.0 - 20.0 * CATCH_TICK_SECS)).abs() < 0.01);
    }

    #[test]
    fn net_ability_bursts_the_bar() {
        let mut e = CatchEngagement::new(profile(100.0), 7);
        let before = e.resistance;
        e.use_ability(AbilityKind::Net, &CatchStats::default());
        assert!(e.resistance < before, "net should deplete the bar");
        assert!(e.contribution > 0.0);
    }

    #[test]
    fn trap_weaken_speeds_depletion() {
        let stats = CatchStats::default();
        // Two identical engagements; one gets a Trap weaken first.
        let mut plain = CatchEngagement::new(profile(500.0), 3);
        let mut trapped = CatchEngagement::new(profile(500.0), 3);
        trapped.use_ability(AbilityKind::Trap, &stats);
        for _ in 0..30 {
            plain.tick(0.1, &stats, &mut 1.0e9_f32);
            trapped.tick(0.1, &stats, &mut 1.0e9_f32);
        }
        assert!(
            trapped.resistance < plain.resistance,
            "weakened target should have less resistance left"
        );
    }

    #[test]
    fn skill_check_spawns_and_hitting_it_helps() {
        let mut e = CatchEngagement::new(profile(1000.0), 42);
        let stats = CatchStats::default();
        // Run until a window appears.
        let mut spawned = false;
        for _ in 0..600 {
            e.tick(0.05, &stats, &mut 1.0e9_f32);
            if e.skill_check.is_some() {
                spawned = true;
                break;
            }
        }
        assert!(spawned, "a skill check should spawn within ~30s");
        let before = e.resistance;
        assert!(e.hit_skill_check(&stats));
        assert!(e.resistance < before, "hitting the check should deplete the bar");
        assert!(e.skill_check.is_none(), "window clears after a hit");
    }

    #[test]
    fn lure_locks_flee() {
        let mut e = CatchEngagement::new(profile(100.0), 5);
        e.use_ability(AbilityKind::Lure, &CatchStats::default());
        assert!(e.debuffs.flee_lock_secs > 0.0);
        e.tick(1.0, &CatchStats::default(), &mut 1.0e9_f32);
        assert!(e.debuffs.flee_lock_secs > 0.0, "lock persists a few seconds");
    }

    #[test]
    fn deterministic_from_seed() {
        let run = || {
            let mut e = CatchEngagement::new(TargetProfile::for_species("penguin"), 99);
            let stats = CatchStats::default();
            let mut checks = 0;
            for _ in 0..400 {
                e.tick(0.1, &stats, &mut 1.0e9_f32);
                if e.skill_check.is_some() {
                    checks += 1;
                    e.miss_skill_check();
                }
            }
            (e.resistance, checks)
        };
        assert_eq!(run(), run(), "same seed → same engagement evolution");
    }

    #[test]
    fn tier_scales_resistance() {
        // A cheap common (tier 1) is shallower than a premium (tier 5).
        let mouse = TargetProfile::for_species("field_mouse");
        assert_eq!(mouse.tier, 1);
        // Build a synthetic tier-5 to compare the derivation monotonicity.
        let t5 = TargetProfile { tier: 5, resistance_max: 60.0 + 40.0 * 5.0, ..mouse.clone() };
        assert!(t5.resistance_max > mouse.resistance_max);
    }

    #[test]
    fn running_out_of_stamina_exhausts_the_attempt() {
        // Deep bar (1000) → 100 stamina per tick; a 30-pool can't cover one tick.
        let mut e = CatchEngagement::new(profile(1000.0), 1);
        let stats = CatchStats { catch_power: 20.0, ..Default::default() };
        let mut stamina = 30.0;
        let outcome = e.tick(CATCH_TICK_SECS, &stats, &mut stamina);
        assert_eq!(outcome, EngagementOutcome::Exhausted);
        assert!(!e.is_captured(), "no capture on exhaustion");
        assert_eq!(stamina, 30.0, "an unaffordable tick spends nothing");
    }

    #[test]
    fn stamina_is_spent_proportionally_and_a_big_pool_captures() {
        let mut e = CatchEngagement::new(profile(100.0), 2);
        let stats = CatchStats { catch_power: 50.0, ..Default::default() };
        let mut stamina = 10_000.0;
        let mut outcome = EngagementOutcome::Ongoing;
        for _ in 0..240 {
            outcome = e.tick(1.0 / 60.0, &stats, &mut stamina);
            if outcome == EngagementOutcome::Captured {
                break;
            }
        }
        assert_eq!(outcome, EngagementOutcome::Captured);
        assert!(stamina < 10_000.0, "stamina was spent during the catch");
    }
}
