use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use super::species::HabitatTheme;

pub const MAX_HABITAT_LEVEL: u8 = 10;

#[derive(Clone, Debug)]
pub struct Habitat {
    pub id: Uuid,
    pub theme: HabitatTheme,
    pub level: u8,
    pub animal_ids: Vec<Uuid>,
    /// When `Some`, a level-up from `level` to `level + 1` is in flight; it
    /// completes at this instant and the player must explicitly invoke
    /// `Zoo::claim_habitat_upgrade` to apply it. Mirrors the breeding-nest
    /// "ready to redeem" pattern so all timed actions feel the same.
    pub upgrade_finishes_at: Option<DateTime<Utc>>,
}

impl Habitat {
    pub fn new(theme: HabitatTheme) -> Self {
        Self {
            id: Uuid::new_v4(),
            theme,
            level: 1,
            animal_ids: Vec::new(),
            upgrade_finishes_at: None,
        }
    }

    pub fn capacity(&self) -> usize {
        3 + (self.level as usize - 1) * 2
    }
}

/// Coins to advance habitat from `current_level` to `current_level + 1`.
pub fn habitat_upgrade_cost(current_level: u8) -> u64 {
    let l = current_level as u64;
    200u64.saturating_mul(l).saturating_mul(l)
}

/// Coins to buy the first habitat of a theme. With single-habitat-per-theme
/// the old `habitat_purchase_cost(n)` table collapses to one constant —
/// kept as a function so callers can stay structurally similar.
pub fn habitat_purchase_cost() -> u64 {
    500
}

/// Time it takes to grow a habitat from `current_level` to `current_level + 1`.
/// Scales with the level so high-tier upgrades feel weighty without being
/// punishing early on: L1→L2 = 1 minute, L9→L10 = 9 minutes.
pub fn habitat_upgrade_duration(current_level: u8) -> Duration {
    Duration::seconds(60 * (current_level.max(1) as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_grows_with_level() {
        let mut h = Habitat::new(HabitatTheme::Forest);
        assert_eq!(h.capacity(), 3);
        h.level = 2;
        assert_eq!(h.capacity(), 5);
        h.level = 3;
        assert_eq!(h.capacity(), 7);
    }

    #[test]
    fn upgrade_cost_curve() {
        // L1→L2: 200, L2→L3: 800, L3→L4: 1800
        assert_eq!(habitat_upgrade_cost(1), 200);
        assert_eq!(habitat_upgrade_cost(2), 800);
        assert_eq!(habitat_upgrade_cost(3), 1800);
    }

    #[test]
    fn upgrade_duration_curve() {
        assert_eq!(habitat_upgrade_duration(1).num_seconds(), 60);
        assert_eq!(habitat_upgrade_duration(5).num_seconds(), 300);
        assert_eq!(habitat_upgrade_duration(9).num_seconds(), 540);
    }
}
