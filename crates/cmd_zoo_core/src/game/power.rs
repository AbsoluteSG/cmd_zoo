//! Power Score — progression-based **access gating** (MMORPG direction).
//!
//! A player's **Power Score** is derived purely from their *owned animals*
//! (breadth + rarity + rank + level) — **not** gear (gear is its own direct layer
//! on [`crate::game::catch::CatchStats`]). It gates which regions/expeditions a
//! player may enter, so a fresh player can't walk into an end-game region and
//! trivially catch top animals.
//!
//! Pure + headless — the same derivation runs client-side (HUD, expedition board,
//! solo launch) and inside the SpacetimeDB reducer (`enter_expedition`), so the
//! gate is authoritative online.
//!
//! ## Tuning
//! - Per-animal worth: the consts below ([`PER_ANIMAL`], [`TIER_POWER`], …).
//! - Per-region requirement: the `match` in [`required_power`].
//! Both are the one place to retune; everything else derives from them.

use crate::game::catch::catch_tier;
use crate::game::rank::rank_multiplier;
use crate::game::species::{self, HabitatTheme, SpeciesId};
use crate::game::zoo::Zoo;

// ── Power formula tuning (balanced, with a spike at the top rarities) ────────────

/// Flat power every owned animal contributes — the **breadth** reward (a wide
/// collection matters even at low rarity).
pub const PER_ANIMAL: f64 = 8.0;
/// Power per animal **level** above 1.
pub const PER_LEVEL: f64 = 1.5;
/// Power by **rarity tier** (`catch_tier`, 1..=5), indexed directly. Deliberately
/// **non-linear** — the 4→5 jump is steep so "super rares and onward" dominate.
/// (Index 0 is unused; tiers are 1..=5.)
pub const TIER_POWER: [f64; 6] = [0.0, 8.0, 20.0, 48.0, 120.0, 320.0];
/// Extra multiplier for **exotic** species (the rarest, exotic-shop tier).
pub const EXOTIC_MULT: f64 = 2.5;
/// Extra multiplier for **hybrid** (crossbred) species.
pub const HYBRID_MULT: f64 = 1.5;

/// Power contributed by a single owned animal. Breadth + rarity + level, all
/// scaled by its rank multiplier, with an extra spike for exotic/hybrid rares.
pub fn animal_power(species: SpeciesId, level: u8, stage: u8) -> u64 {
    let tier = catch_tier(species) as usize;
    let tier_power = TIER_POWER.get(tier).copied().unwrap_or(0.0);
    let base = PER_ANIMAL + tier_power + PER_LEVEL * (level.saturating_sub(1) as f64);
    let mut p = base * rank_multiplier(stage);
    if let Some(def) = species::try_get(species) {
        if def.exotic {
            p *= EXOTIC_MULT;
        }
        if def.hybrid {
            p *= HYBRID_MULT;
        }
    }
    p.round().max(0.0) as u64
}

/// Total Power Score of a zoo — the sum of [`animal_power`] over every owned
/// animal. (The zoo holds at most one animal per species, so this is breadth ×
/// per-animal worth.) Derived on demand; nothing is cached or persisted client-side.
pub fn power_score(zoo: &Zoo) -> u64 {
    zoo.animals
        .values()
        .map(|a| animal_power(a.species, a.level, a.stage))
        .sum()
}

// ── Region access gating ────────────────────────────────────────────────────────

/// Minimum Power Score required to enter a region's expedition. **Edit this table
/// to retune access progression.** Starter biomes are `0` so a fresh player
/// (Power ~0) always has somewhere to go.
pub fn required_power(theme: HabitatTheme) -> u64 {
    use HabitatTheme::*;
    match theme {
        Forest | Farmland | Food => 0, // starter — always open
        Wetland | Beach | Savanna => 150,
        Jungle | Highlands | Desert => 350,
        Ocean | Taiga | Badlands => 600,
        Arctic | Tundra => 900,
        Volcanic => 1_400,
        Festive => 1_800,
        Mythical => 2_600,
        Void => 3_500, // end-game
    }
}

/// A short difficulty label for a region, derived from its [`required_power`] band
/// — purely UI sugar (e.g. shown on the expedition board).
pub fn difficulty_label(theme: HabitatTheme) -> &'static str {
    match required_power(theme) {
        0 => "Starter",
        1..=199 => "Low",
        200..=499 => "Mid",
        500..=999 => "High",
        1_000..=1_999 => "Elite",
        2_000..=2_999 => "Mythic",
        _ => "Endgame",
    }
}

/// Whether a player with `power` may enter `theme`.
pub fn can_enter(power: u64, theme: HabitatTheme) -> bool {
    power >= required_power(theme)
}

/// `None` if the player may enter; otherwise a user-facing reason string for the
/// status line / online error toast.
pub fn gate_error(power: u64, theme: HabitatTheme) -> Option<String> {
    let req = required_power(theme);
    if power >= req {
        None
    } else {
        Some(format!(
            "{} requires Power {req} — you have {power}",
            theme.name()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::action::{apply_action, Action};
    use chrono::Utc;

    fn zoo_with(species: &[&'static str]) -> Zoo {
        let now = Utc::now();
        let mut zoo = Zoo::new(now);
        zoo.coins = u64::MAX; // afford anything
        for &s in species {
            apply_action(&mut zoo, Action::SpawnFreeform { species: s.into(), level: 1 }, now)
                .unwrap_or_else(|e| panic!("spawn {s}: {e:?}"));
        }
        zoo
    }

    #[test]
    fn empty_zoo_has_zero_power() {
        assert_eq!(power_score(&Zoo::new(Utc::now())), 0);
    }

    #[test]
    fn adding_any_animal_increases_power() {
        let before = power_score(&zoo_with(&[]));
        let after = power_score(&zoo_with(&["field_mouse"]));
        assert!(after > before, "an owned animal contributes power");
    }

    #[test]
    fn rarity_rank_and_level_each_raise_animal_power() {
        // Higher tier (rarer) beats a common at equal level/stage.
        let common = animal_power("field_mouse", 1, 0);
        let mut rarer = common;
        // Find a higher-tier species to compare; lion is a mid/high tier forest mob.
        if species::try_get("lion").is_some() {
            rarer = animal_power("lion", 1, 0);
            assert!(rarer > common, "rarer animal is worth more power");
        }
        // Level and rank each strictly increase a given animal's worth.
        assert!(animal_power("field_mouse", 5, 0) > common, "higher level → more power");
        assert!(animal_power("field_mouse", 1, 2) > common, "higher rank → more power");
        let _ = rarer;
    }

    #[test]
    fn can_enter_respects_thresholds() {
        // Starter is always enterable, even at zero power.
        assert!(can_enter(0, HabitatTheme::Forest));
        let req = required_power(HabitatTheme::Void);
        assert!(req > 0);
        assert!(!can_enter(req - 1, HabitatTheme::Void), "just under the bar is locked");
        assert!(can_enter(req, HabitatTheme::Void), "meeting the bar unlocks");
    }

    #[test]
    fn gate_error_agrees_with_can_enter() {
        for theme in [HabitatTheme::Forest, HabitatTheme::Arctic, HabitatTheme::Void] {
            let req = required_power(theme);
            assert!(gate_error(req, theme).is_none());
            if req > 0 {
                assert!(gate_error(req - 1, theme).is_some());
            }
        }
    }
}
