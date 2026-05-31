use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::species::{self, SpeciesId};

pub const MAX_ANIMAL_LEVEL: u8 = 10;

#[derive(Clone, Debug)]
pub enum AnimalState {
    Idle,
    /// Gestating. After `ends_at` the nest is "ready" — the player must
    /// explicitly call `Zoo::claim_completed_breeding` (the breeding-tab
    /// click-on-ready-nest action) to spawn the offspring and return both
    /// parents to `Idle`. Nothing happens automatically: economy::advance
    /// no longer touches breeding state.
    Breeding {
        partner_id: Uuid,
        ends_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug)]
pub struct Animal {
    pub id: Uuid,
    pub species: SpeciesId,
    pub level: u8,
    pub last_collected_at: DateTime<Utc>,
    pub state: AnimalState,
}

impl Animal {
    pub fn new(species: SpeciesId, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            species,
            level: 1,
            last_collected_at: now,
            state: AnimalState::Idle,
        }
    }

    pub fn rate_per_sec(&self) -> f64 {
        match self.state {
            AnimalState::Idle => {
                let def = species::get(self.species);
                def.base_rate_per_sec * level_rate_multiplier(def.level_rate_bonus, self.level)
            }
            AnimalState::Breeding { .. } => 0.0,
        }
    }

    pub fn storage_cap(&self) -> u64 {
        let def = species::get(self.species);
        let mult = level_cap_multiplier(def.level_cap_bonus, self.level);
        ((def.base_storage_cap as f64) * mult).floor() as u64
    }

    pub fn stored_at(&self, now: DateTime<Utc>) -> u64 {
        if !matches!(self.state, AnimalState::Idle) {
            return 0;
        }
        let elapsed_ms = (now - self.last_collected_at).num_milliseconds().max(0);
        let secs = elapsed_ms as f64 / 1000.0;
        let raw = (secs * self.rate_per_sec()).floor() as i128;
        raw.clamp(0, self.storage_cap() as i128) as u64
    }

    /// Only collectable when stored output has reached its species cap.
    /// Animals below cap "wait" — the player has to come back later to
    /// claim. This is the at-cap-only economy rule.
    pub fn is_at_cap(&self, now: DateTime<Utc>) -> bool {
        let cap = self.storage_cap();
        cap > 0 && self.stored_at(now) >= cap
    }
}

/// Per-species rate scaling. `bonus` is the additive multiplier per level above 1.
/// e.g. bonus=0.5 → L1: 1.0x, L2: 1.5x, L3: 2.0x. bonus=0.2 → L2: 1.2x, L3: 1.4x.
pub fn level_rate_multiplier(bonus: f64, level: u8) -> f64 {
    1.0 + bonus * (level.saturating_sub(1) as f64)
}

/// Per-species capacity scaling. Symmetric formula to `level_rate_multiplier`.
/// bonus=1.0 → cap doubles per level (the historical default).
pub fn level_cap_multiplier(bonus: f64, level: u8) -> f64 {
    1.0 + bonus * (level.saturating_sub(1) as f64)
}

/// Coins to advance from `current_level` to `current_level + 1`.
pub fn animal_level_up_cost(base_purchase_cost: u64, current_level: u8) -> u64 {
    base_purchase_cost.saturating_mul(current_level as u64).saturating_mul(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn stored_grows_linearly_until_cap() {
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        // mouse L1: 0.5/s, cap 60
        let a = Animal::new("field_mouse", base);
        assert_eq!(a.stored_at(base), 0);
        assert_eq!(a.stored_at(base + Duration::seconds(60)), 30);
        assert_eq!(a.stored_at(base + Duration::seconds(120)), 60);
        assert_eq!(a.stored_at(base + Duration::seconds(86_400)), 60);
    }

    #[test]
    fn stored_is_zero_before_last_collected_at() {
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let a = Animal::new("field_mouse", base);
        assert_eq!(a.stored_at(base - Duration::seconds(10)), 0);
    }

    #[test]
    fn balanced_species_scales_rate_and_cap_linearly() {
        // Mouse is BALANCED (0.5, 1.0): doubles cap and adds 0.5×base/s per level.
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut a = Animal::new("field_mouse", base);
        a.level = 2;
        assert!((a.rate_per_sec() - 0.75).abs() < 1e-9);
        assert_eq!(a.storage_cap(), 120);
        a.level = 3;
        assert!((a.rate_per_sec() - 1.0).abs() < 1e-9);
        assert_eq!(a.storage_cap(), 180);
    }

    #[test]
    fn tank_species_grows_cap_faster_than_rate() {
        // Frog is TANK (0.2, 1.6): base 0.8/s, cap 80.
        // L2: rate = 0.8 * (1+0.2) = 0.96/s; cap = 80 * (1+1.6) = 208.
        // L3: rate = 0.8 * 1.4 = 1.12/s; cap = 80 * 4.2 = 336.
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut a = Animal::new("treeFrog", base);
        a.level = 2;
        assert!((a.rate_per_sec() - 0.96).abs() < 1e-9);
        assert_eq!(a.storage_cap(), 208);
        a.level = 3;
        assert!((a.rate_per_sec() - 1.12).abs() < 1e-9);
        assert_eq!(a.storage_cap(), 336);
    }

    #[test]
    fn sprinter_species_grows_rate_faster_than_cap() {
        // Fox is SPRINTER (0.8, 0.4): base 1.5/s, cap 240.
        // L2: rate = 1.5 * (1+0.8) = 2.7/s; cap = 240 * (1+0.4) = 336.
        // L3: rate = 1.5 * 2.6 = 3.9/s; cap = 240 * 1.8 = 432.
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut a = Animal::new("fox", base);
        a.level = 2;
        assert!((a.rate_per_sec() - 2.7).abs() < 1e-9);
        assert_eq!(a.storage_cap(), 336);
        a.level = 3;
        assert!((a.rate_per_sec() - 3.9).abs() < 1e-9);
        assert_eq!(a.storage_cap(), 432);
    }

    #[test]
    fn level_up_cost_curve() {
        // mouse purchase 25 → L1→L2: 50, L2→L3: 100, L3→L4: 150
        assert_eq!(animal_level_up_cost(25, 1), 50);
        assert_eq!(animal_level_up_cost(25, 2), 100);
        assert_eq!(animal_level_up_cost(25, 3), 150);
    }

    /// Regression: a previous version left hybrid species at `purchase_cost: 0`,
    /// which made `animal_level_up_cost(0, L) = 0` and let players level up
    /// crossbred animals for free. Every species must have a positive cost.
    #[test]
    fn no_species_has_zero_level_up_cost() {
        for def in crate::game::species::all_purchasable()
            .chain(crate::game::species::all_hybrids())
            .chain(crate::game::species::all_exotics())
        {
            assert!(
                def.purchase_cost > 0,
                "species {} has purchase_cost 0 — hybrids must keep a nonzero cost",
                def.id
            );
            for level in 1..crate::game::animal::MAX_ANIMAL_LEVEL {
                let cost = animal_level_up_cost(def.purchase_cost, level);
                assert!(
                    cost > 0,
                    "{} L{}→L{} costs zero food",
                    def.id,
                    level,
                    level + 1
                );
            }
        }
    }
}

