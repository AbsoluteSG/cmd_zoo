use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::structure_kind::{self, StructureKindId};

/// Structures keep the old uniform balanced scaling: rate × (1+0.5·(L-1)), cap × L.
const STRUCTURE_RATE_BONUS: f64 = 0.5;

pub const MAX_STRUCTURE_LEVEL: u8 = 10;
pub const STRUCTURE_TOTAL_CAP: usize = 4;

#[derive(Clone, Debug)]
pub struct Structure {
    pub id: Uuid,
    pub kind: StructureKindId,
    pub level: u8,
    pub last_collected_at: DateTime<Utc>,
}

impl Structure {
    pub fn new(kind: StructureKindId, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            level: 1,
            last_collected_at: now,
        }
    }

    pub fn food_rate(&self) -> f64 {
        let base = structure_kind::get(self.kind).base_food_per_sec;
        base * (1.0 + STRUCTURE_RATE_BONUS * (self.level.saturating_sub(1) as f64))
    }

    pub fn food_cap(&self) -> u64 {
        structure_kind::get(self.kind)
            .base_food_cap
            .saturating_mul(self.level as u64)
    }

    pub fn stored_at(&self, now: DateTime<Utc>) -> u64 {
        let elapsed_ms = (now - self.last_collected_at).num_milliseconds().max(0);
        let secs = elapsed_ms as f64 / 1000.0;
        let raw = (secs * self.food_rate()).floor() as i128;
        raw.clamp(0, self.food_cap() as i128) as u64
    }
}

/// Coins to buy the (current_count+1)-th structure. Same indexing rule as habitats.
pub fn structure_purchase_cost(current_count: usize) -> u64 {
    match current_count {
        0 => 250,
        1 => 750,
        2 => 1800,
        _ => 3500,
    }
}

/// Coins to advance a structure from `current_level` to `current_level + 1`.
pub fn structure_upgrade_cost(current_level: u8) -> u64 {
    let l = current_level as u64;
    200u64.saturating_mul(l).saturating_mul(l)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn structure_food_grows_linearly_until_cap() {
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        // hay_bale L1: 0.5/s, cap 80
        let s = Structure::new("hay_bale", base);
        assert_eq!(s.stored_at(base), 0);
        assert_eq!(s.stored_at(base + Duration::seconds(60)), 30);
        assert_eq!(s.stored_at(base + Duration::seconds(160)), 80);
        // Past cap stays capped.
        assert_eq!(s.stored_at(base + Duration::seconds(86_400)), 80);
    }

    #[test]
    fn structure_level_scales_food_rate_and_cap() {
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut s = Structure::new("hay_bale", base);
        s.level = 2;
        // 0.5 * 1.5 = 0.75/s; cap 80 * 2 = 160
        assert!((s.food_rate() - 0.75).abs() < 1e-9);
        assert_eq!(s.food_cap(), 160);
    }
}
