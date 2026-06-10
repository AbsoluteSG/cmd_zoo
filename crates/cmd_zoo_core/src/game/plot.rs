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
