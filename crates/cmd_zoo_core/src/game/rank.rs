//! Per-species **Rank**: capturing/breeding a duplicate of an animal you already
//! own advances its rank (Regular → Silver → Gold → … → Neon) instead of adding a
//! second copy. Rank boosts the animal's income + storage and recolors its inspect
//! panel. The number of duplicates needed climbs geometrically (×3 each step).
//!
//! The domain side here is pure data (thresholds, names, stat multiplier); the
//! panel colours live render-side, keyed by the same stage index.

/// Highest rank index (Neon). Extra duplicates past this keep accumulating in
/// `species_dupes` but the stage clamps here.
pub const MAX_RANK: u8 = 6;

/// Cumulative duplicate counts required to *reach* each rank, indexed by stage−1.
/// Silver=10, Gold=30, Platinum=90, Diamond=270, Ruby=810, Neon=2430 (10·3ⁿ).
const THRESHOLDS: [u32; MAX_RANK as usize] = [10, 30, 90, 270, 810, 2430];

/// Display names indexed by stage (0 = Regular).
const NAMES: [&str; MAX_RANK as usize + 1] =
    ["Regular", "Silver", "Gold", "Platinum", "Diamond", "Ruby", "Neon"];

/// Rank stage (0..=`MAX_RANK`) for a given lifetime duplicate count.
pub fn rank_for_dupes(dupes: u32) -> u8 {
    let mut stage = 0u8;
    for (i, t) in THRESHOLDS.iter().enumerate() {
        if dupes >= *t {
            stage = (i + 1) as u8;
        }
    }
    stage
}

/// Cumulative duplicates needed to reach the *next* rank, or `None` at the cap.
pub fn next_threshold(stage: u8) -> Option<u32> {
    THRESHOLDS.get(stage as usize).copied()
}

/// Lifetime-duplicate count that puts a species at the max rank (Neon). Used to
/// grant collection-reward animals straight at max rank.
pub fn max_rank_dupes() -> u32 {
    THRESHOLDS[MAX_RANK as usize - 1]
}

pub fn rank_name(stage: u8) -> &'static str {
    NAMES[(stage as usize).min(MAX_RANK as usize)]
}

/// Income + storage multiplier for a rank: Regular 1.0, +0.5 per stage.
pub fn rank_multiplier(stage: u8) -> f64 {
    1.0 + 0.5 * stage as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_map_to_stages() {
        assert_eq!(rank_for_dupes(0), 0);
        assert_eq!(rank_for_dupes(9), 0);
        assert_eq!(rank_for_dupes(10), 1); // Silver
        assert_eq!(rank_for_dupes(29), 1);
        assert_eq!(rank_for_dupes(30), 2); // Gold
        assert_eq!(rank_for_dupes(90), 3); // Platinum
        assert_eq!(rank_for_dupes(2430), 6); // Neon
        assert_eq!(rank_for_dupes(1_000_000), MAX_RANK); // clamps
    }

    #[test]
    fn names_and_multiplier() {
        assert_eq!(rank_name(0), "Regular");
        assert_eq!(rank_name(2), "Gold");
        assert!((rank_multiplier(0) - 1.0).abs() < 1e-9);
        assert!((rank_multiplier(2) - 2.0).abs() < 1e-9);
    }
}
