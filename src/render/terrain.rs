//! Procedural terrain-prop scatter.
//!
//! Decorative, non-interactive scenery (rocks, plants, trees, …) sprinkled
//! across the world as part of world-gen. Placement is deterministic per tile
//! (seeded from `world_seed`), so the scatter is stable frame-to-frame and
//! across sessions — exactly like the grass field and the wild-animal spawns.
//!
//! Art is dropped into `assets/terrain/<Biome>/*.png`; the build script keys
//! each PNG as `"<biome>/<name>"`. Each tile rolls (sparsely) whether to place a
//! prop, then picks one of its biome's props. The gathered instances are handed
//! back to the world renderer, which depth-sorts them with critters/avatars so a
//! tree can correctly sit in front of or behind a passing animal.

use macroquad::prelude::*;

use crate::game::biome;
use crate::game::species::HabitatTheme;
use crate::render::textures;

use super::view::{PLANE_H, PLANE_W};

/// World units per ground tile (must match `draw_scene`'s `BTILE`).
const TILE: f32 = 128.0;

/// Fraction of eligible tiles that receive a prop. Kept low so scenery reads as
/// a sparse sprinkle, not a forest wall.
const DENSITY: f32 = 0.12;

/// A single placed prop instance for this frame.
pub struct PropInstance {
    /// World-space anchor (the prop's feet/base).
    pub world: Vec2,
    /// `"<biome>/<name>"` texture id (look up via `Textures::terrain`).
    pub id: &'static str,
    /// Per-instance size jitter (multiplies the native sprite size).
    pub scale: f32,
    /// Horizontal mirror, for a touch more variety.
    pub flip: bool,
}

/// True when `pos` is inside the home plot (`home_c`/`home_h`); props are
/// suppressed there so the manicured enclosure stays clear.
fn in_zoo(pos: Vec2, home_c: Vec2, home_h: f32) -> bool {
    (pos.x - home_c.x).abs() <= home_h && (pos.y - home_c.y).abs() <= home_h
}

/// Like [`gather`], but for a **single-theme bounded arena** (an expedition
/// instance): every tile uses `theme` directly (no per-tile biome lookup, no zoo
/// exclusion), and props are confined to `[0, arena]`. Deterministic from
/// `(seed, theme)`. Empty when the theme has no bundled props.
pub fn gather_themed(
    seed: u64,
    theme: HabitatTheme,
    view_min: Vec2,
    view_max: Vec2,
    arena: Vec2,
) -> Vec<PropInstance> {
    let ids = textures::terrain_prop_ids(theme.name());
    if ids.is_empty() {
        return Vec::new();
    }
    // Visible tile range, clamped to the arena.
    let tx0 = (view_min.x / TILE).floor().max(0.0) as i32;
    let ty0 = (view_min.y / TILE).floor().max(0.0) as i32;
    let tx1 = ((view_max.x / TILE).ceil() as i32).min((arena.x / TILE).ceil() as i32);
    let ty1 = ((view_max.y / TILE).ceil() as i32).min((arena.y / TILE).ceil() as i32);

    let mut out: Vec<PropInstance> = Vec::new();
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            if unit(hash(tx, ty, 1, seed)) >= DENSITY {
                continue;
            }
            let jx = unit(hash(tx, ty, 3, seed));
            let jy = unit(hash(tx, ty, 4, seed));
            let world = vec2((tx as f32 + jx) * TILE, (ty as f32 + jy) * TILE);
            if world.x > arena.x || world.y > arena.y {
                continue; // jitter pushed it past the arena edge
            }
            let pick = (hash(tx, ty, 2, seed) as usize) % ids.len();
            let scale = 0.85 + unit(hash(tx, ty, 5, seed)) * 0.4;
            let flip = hash(tx, ty, 6, seed) & 1 == 0;
            out.push(PropInstance { world, id: ids[pick], scale, flip });
        }
    }
    out
}

/// Well-distributed 32-bit hash of `(tx, ty, salt)` salted by `seed` (lowbias32
/// finalizer — same scheme as the grass scatter, so props don't visibly
/// correlate with blades).
fn hash(tx: i32, ty: i32, salt: u32, seed: u64) -> u32 {
    let mut h = (tx as u32).wrapping_mul(0x8DA6_B343)
        ^ (ty as u32).wrapping_mul(0xD816_3841)
        ^ salt.wrapping_mul(0xCB1A_B31F)
        ^ (seed as u32)
        ^ ((seed >> 32) as u32).wrapping_mul(0x9E37_79B1);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    h
}

/// Map a hash to `[0, 1)`.
fn unit(h: u32) -> f32 {
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// Gather the props to draw for the visible tile range `[tx0..=tx1] × [ty0..=ty1]`.
/// Deterministic for a fixed `(tile, seed)`. Tiles outside the world plane or
/// inside the zoo plot, and biomes with no bundled props, contribute nothing.
pub fn gather(
    seed: u64,
    tx0: i32,
    tx1: i32,
    ty0: i32,
    ty1: i32,
    home_c: Vec2,
    home_h: f32,
) -> Vec<PropInstance> {
    // Cache the (possibly empty) prop id list per biome so we scan the embedded
    // table at most once per biome on screen, not once per tile.
    let mut cache: Vec<(HabitatTheme, Vec<&'static str>)> = Vec::new();
    let mut out: Vec<PropInstance> = Vec::new();

    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let center = vec2((tx as f32 + 0.5) * TILE, (ty as f32 + 0.5) * TILE);
            if center.x < 0.0 || center.x > PLANE_W || center.y < 0.0 || center.y > PLANE_H {
                continue;
            }
            if in_zoo(center, home_c, home_h) {
                continue;
            }
            // Sparse roll first (cheap) before resolving biome/props.
            if unit(hash(tx, ty, 1, seed)) >= DENSITY {
                continue;
            }
            let theme = biome::biome_at(center, seed);
            let idx = match cache.iter().position(|(t, _)| *t == theme) {
                Some(i) => i,
                None => {
                    let ids = textures::terrain_prop_ids(theme.name());
                    cache.push((theme, ids));
                    cache.len() - 1
                }
            };
            let ids = &cache[idx].1;
            if ids.is_empty() {
                continue;
            }
            let pick = (hash(tx, ty, 2, seed) as usize) % ids.len();
            // Jitter the anchor within the tile so props don't snap to a grid.
            let jx = unit(hash(tx, ty, 3, seed));
            let jy = unit(hash(tx, ty, 4, seed));
            let world = vec2((tx as f32 + jx) * TILE, (ty as f32 + jy) * TILE);
            let scale = 0.85 + unit(hash(tx, ty, 5, seed)) * 0.4;
            let flip = hash(tx, ty, 6, seed) & 1 == 0;
            out.push(PropInstance { world, id: ids[pick], scale, flip });
        }
    }
    out
}
