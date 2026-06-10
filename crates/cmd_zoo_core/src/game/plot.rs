//! Hub-plot geometry — the per-player zoo plot constants and helpers that
//! survived the removal of the old infinite open world.
//!
//! Each player's zoo sits on a square plot somewhere in the shared hub world.
//! All plot-relative geometry (nests, food structures, pedestals) derives from
//! the owning [`Zoo`](crate::game::zoo::Zoo)'s own `plot_origin` + `zoo_level`
//! via `Zoo::tile_to_world` / `world_to_tile` — there is **no** process-global
//! plot singleton, so many plots can coexist on one hub.

use glam::{Vec2, vec2};

// ── Hub world extent ───────────────────────────────────────────────────────────

/// Width of the shared hub world in world units. Plots are placed within this
/// span; the renderer uses it as the ground-plane size. (Expedition instances
/// are separate bounded maps with their own local coordinates — see
/// [`crate::game::biome_instance`].)
pub const WORLD_W: f32 = 500_000.0;
/// Height of the shared hub world in world units.
pub const WORLD_H: f32 = 500_000.0;

/// World-space centre of the hub. A solo zoo's `plot_origin` defaults here; on a
/// shared hub each player is assigned a distinct origin around the hub instead.
pub fn world_center() -> Vec2 {
    vec2(WORLD_W * 0.5, WORLD_H * 0.5)
}

// ── Zoo plot dimensions ─────────────────────────────────────────────────────────

/// Base (un-upgraded) plot edge length in tiles. Each expansion grows it by
/// [`ZOO_GROWTH_PER_SIDE`] tiles on every side.
pub const ZOO_TILES_BASE: i32 = 9;
/// Tiles added to *each* side of the plot per expansion level — one level grows
/// the edge length by `2 * ZOO_GROWTH_PER_SIDE` tiles while staying centred. The
/// matching capacity bump lives in `zoo.rs`.
pub const ZOO_GROWTH_PER_SIDE: i32 = 2;
/// World units per tile — must match `avatar_system::TILE_W` and the render grid.
pub const ZOO_TILE_W: f32 = 128.0;

/// Plot edge length (tiles) for a given expansion `level` (0 = base).
pub fn zoo_tiles_for_level(level: u8) -> i32 {
    ZOO_TILES_BASE + level as i32 * 2 * ZOO_GROWTH_PER_SIDE
}

// ── Hub plot slots ──────────────────────────────────────────────────────────────
//
// On a shared hub, each player is assigned a numbered **slot** whose world
// origin comes from `hub_plot_origin`. Slots spiral out from the hub centre on a
// fixed grid so any number of players get a distinct, non-overlapping plot, and
// slot N is always the same place regardless of join order. This is the single
// layout both a local loopback demo and the future SpacetimeDB hub use to place
// zoos — assign a slot, set `Zoo::plot_origin = hub_plot_origin(slot)`.

/// World pitch between adjacent hub plot slots: a base plot edge plus a lane, so
/// neighbouring plots never touch even at a couple of expansion levels.
pub const HUB_PLOT_PITCH: f32 = ZOO_TILES_BASE as f32 * ZOO_TILE_W * 2.6;

/// World-space plot origin for hub `slot` (slot 0 = hub centre). Slots fill a
/// square spiral around the centre, spaced by [`HUB_PLOT_PITCH`].
pub fn hub_plot_origin(slot: u32) -> Vec2 {
    let (gx, gy) = spiral_cell(slot);
    world_center() + vec2(gx as f32, gy as f32) * HUB_PLOT_PITCH
}

/// Map index 0,1,2,… to distinct integer grid cells spiralling out from (0,0):
/// ring `r` (Chebyshev distance `r`) holds 8·r cells, indices
/// `(2r-1)² .. (2r+1)²`.
fn spiral_cell(n: u32) -> (i32, i32) {
    if n == 0 {
        return (0, 0);
    }
    let mut r = 1i32;
    loop {
        let inner = (2 * r - 1).pow(2) as u32; // first index of ring r
        let outer = (2 * r + 1).pow(2) as u32; // first index of ring r+1
        if n < outer {
            let i = (n - inner) as i32; // 0-based position along the ring
            let side = 2 * r; // cells per side
            let (leg, pos) = (i / side, i % side);
            return match leg {
                0 => (r, -r + 1 + pos),  // right edge, going up
                1 => (r - 1 - pos, r),   // top edge, going left
                2 => (-r, r - 1 - pos),  // left edge, going down
                _ => (-r + 1 + pos, -r), // bottom edge, going right
            };
        }
        r += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn slot_zero_is_hub_centre() {
        assert_eq!(hub_plot_origin(0), world_center());
    }

    #[test]
    fn slots_are_distinct_and_non_overlapping() {
        // Every slot maps to a unique cell, and adjacent cells are a full pitch
        // apart, so plots (even a few levels expanded) never overlap.
        let mut seen = HashSet::new();
        for slot in 0..200u32 {
            let (gx, gy) = spiral_cell(slot);
            assert!(seen.insert((gx, gy)), "slot {slot} reused cell ({gx},{gy})");
        }
    }

    #[test]
    fn ring_membership_matches_chebyshev_distance() {
        // The first cells land on the expected rings (0 at centre, 1..8 on ring 1).
        assert_eq!(spiral_cell(0), (0, 0));
        for slot in 1..=8u32 {
            let (gx, gy) = spiral_cell(slot);
            assert_eq!(gx.abs().max(gy.abs()), 1, "slot {slot} should be on ring 1");
        }
        // Ring 2 starts at index 9 (=(2·1+1)²).
        let (gx, gy) = spiral_cell(9);
        assert_eq!(gx.abs().max(gy.abs()), 2);
    }
}
