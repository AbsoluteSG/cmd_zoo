use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use super::species::HabitatTheme;

pub const MAX_HABITAT_LEVEL: u8 = 10;

/// Logical isometric grid dimensions (in tiles). Placement and collision are
/// validated against these bounds in the domain so the rules are testable
/// without a renderer. The Macroquad world draws this same grid.
pub const GRID_W: i32 = 16;
pub const GRID_H: i32 = 16;

/// Tiles a habitat of `theme` occupies, as (width, height) in grid units.
/// Currently a flat 2×2 for every theme — split per-theme here when art with
/// different footprints lands. Tunable in one place.
pub fn footprint(_theme: HabitatTheme) -> (i32, i32) {
    (2, 2)
}

#[derive(Clone, Debug)]
pub struct Habitat {
    pub id: Uuid,
    pub theme: HabitatTheme,
    pub level: u8,
    pub animal_ids: Vec<Uuid>,
    /// Anchor tile (top/origin corner) of this habitat's footprint, in grid
    /// coordinates — NOT screen pixels. The renderer converts to screen via
    /// the isometric transform. Logical placement, persisted game state.
    pub tile: (i32, i32),
    /// When `Some`, a level-up from `level` to `level + 1` is in flight; it
    /// completes at this instant and the player must explicitly invoke
    /// `Zoo::claim_habitat_upgrade` to apply it. Mirrors the breeding-nest
    /// "ready to redeem" pattern so all timed actions feel the same.
    pub upgrade_finishes_at: Option<DateTime<Utc>>,
}

impl Habitat {
    /// Create a habitat at the grid origin. Callers that place it on the grid
    /// use `new_at`; this is kept for tests and migration defaults.
    pub fn new(theme: HabitatTheme) -> Self {
        Self::new_at(theme, (0, 0))
    }

    pub fn new_at(theme: HabitatTheme, tile: (i32, i32)) -> Self {
        Self {
            id: crate::game::ids::new_id(),
            theme,
            level: 1,
            animal_ids: Vec::new(),
            tile,
            upgrade_finishes_at: None,
        }
    }

    pub fn capacity(&self) -> usize {
        3 + (self.level as usize - 1) * 2
    }

    /// Every grid tile this habitat's footprint covers, anchored at `tile`.
    pub fn occupied_tiles(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        let (w, h) = footprint(self.theme);
        let (ax, ay) = self.tile;
        (0..h).flat_map(move |dy| (0..w).map(move |dx| (ax + dx, ay + dy)))
    }

    /// True when the footprint anchored at `tile` lies fully inside the grid.
    pub fn footprint_in_bounds(theme: HabitatTheme, tile: (i32, i32)) -> bool {
        let (w, h) = footprint(theme);
        let (x, y) = tile;
        x >= 0 && y >= 0 && x + w <= GRID_W && y + h <= GRID_H
    }
}

/// Whether two footprints overlap: footprint `a` (theme `at` at `a`) vs
/// footprint `b` (theme `bt` at `b`). Axis-aligned rectangle intersection.
pub fn footprints_overlap(
    at: HabitatTheme,
    a: (i32, i32),
    bt: HabitatTheme,
    b: (i32, i32),
) -> bool {
    let (aw, ah) = footprint(at);
    let (bw, bh) = footprint(bt);
    let (ax, ay) = a;
    let (bx, by) = b;
    ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
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

    #[test]
    fn occupied_tiles_cover_full_footprint() {
        let h = Habitat::new_at(HabitatTheme::Forest, (3, 4));
        let tiles: Vec<_> = h.occupied_tiles().collect();
        // 2×2 footprint anchored at (3,4).
        assert_eq!(tiles, vec![(3, 4), (4, 4), (3, 5), (4, 5)]);
    }

    #[test]
    fn footprints_overlap_detects_collisions() {
        // Adjacent (touching edges) does NOT overlap.
        assert!(!footprints_overlap(
            HabitatTheme::Forest,
            (0, 0),
            HabitatTheme::Wetland,
            (2, 0)
        ));
        // Shared corner tile overlaps.
        assert!(footprints_overlap(
            HabitatTheme::Forest,
            (0, 0),
            HabitatTheme::Wetland,
            (1, 1)
        ));
    }

    #[test]
    fn footprint_bounds_respected() {
        assert!(Habitat::footprint_in_bounds(HabitatTheme::Forest, (0, 0)));
        assert!(Habitat::footprint_in_bounds(
            HabitatTheme::Forest,
            (GRID_W - 2, GRID_H - 2)
        ));
        // Anchor too far right: footprint spills off-grid.
        assert!(!Habitat::footprint_in_bounds(
            HabitatTheme::Forest,
            (GRID_W - 1, 0)
        ));
        assert!(!Habitat::footprint_in_bounds(HabitatTheme::Forest, (-1, 0)));
    }
}
