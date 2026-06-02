//! 2.5D world projection — tilted, oblique (no perspective convergence).
//!
//! The world is a rectangular tile grid drawn as if seen from a camera angled
//! ~55° down: tiles are axis-aligned rectangles, with rows compressed
//! vertically by `TILT`. A tile `(tx, ty)` maps to screen as
//!   `screen = (tx·TILE_W, ty·ROW_H)·zoom + offset`,  `ROW_H = TILE_W·TILT`.
//! Because the map is linear and axis-aligned, `screen_to_tile` is the exact
//! inverse (divide + floor) — correct for click-to-place hit testing.
//!
//! Depth is handled by the renderer (painter's algorithm on screen-Y), with
//! upright billboard sprites; this module is just the flat ground mapping.

use macroquad::prelude::*;

/// Fallback tile width (px) used until a ground texture is loaded; the actual
/// width is auto-detected from that texture. Tiles are authored square and
/// drawn squashed to `ROW_H`.
pub const DEFAULT_TILE_W: f32 = 128.0;

/// Vertical foreshortening: on-screen row height = `TILE_W · TILT`. ~0.6 reads
/// as a camera tilted roughly 55° down (Don't Starve / Cult of the Lamb feel).
pub const TILT: f32 = 0.6;

/// Size of the freeform ground plane — canonical values live in
/// `game::world_chunks`; these aliases let render code keep its old names.
pub const PLANE_W: f32 = crate::game::world_chunks::WORLD_W;
pub const PLANE_H: f32 = crate::game::world_chunks::WORLD_H;

/// Base on-screen height (px, pre-zoom) of a critter sprite. Shared by the
/// renderer and the click hit-test so they stay in sync.
pub const CRITTER_H: f32 = 130.0;

/// Duration (s) of the scale-pop a critter plays when its income is redeemed.
pub const POP_DURATION: f32 = 0.3;

/// Pop scale multiplier for a critter with `pop` seconds of animation left.
/// Rises to ~1.25× at the midpoint and eases back to 1.0.
pub fn pop_scale(pop: f32) -> f32 {
    if pop <= 0.0 {
        return 1.0;
    }
    let t = 1.0 - (pop / POP_DURATION).clamp(0.0, 1.0);
    1.0 + (t * std::f32::consts::PI).sin() * 0.25
}

/// Project a continuous flat-world point onto the screen: the depth (y) axis is
/// compressed by `TILT`, then scaled by zoom and shifted by the camera pan.
pub fn world_to_screen(world: Vec2, cam: &Camera) -> Vec2 {
    vec2(world.x, world.y * TILT) * cam.zoom + cam.offset
}

/// Inverse of [`world_to_screen`]: screen position → continuous world-space
/// position.  Used to convert the mouse cursor into world coordinates for AI.
pub fn screen_to_world(screen: Vec2, cam: &Camera) -> Vec2 {
    let local = (screen - cam.offset) / cam.zoom;
    vec2(local.x, local.y / TILT)
}

/// World-space axis-aligned bounding box visible through the camera.
/// Returns `(top_left, bottom_right)` — used for chunk frustum culling.
pub fn camera_world_rect(cam: &Camera, screen_w: f32, screen_h: f32) -> (Vec2, Vec2) {
    let tl = screen_to_world(vec2(0.0, 0.0), cam);
    let br = screen_to_world(vec2(screen_w, screen_h), cam);
    (tl, br)
}

/// On-screen (pre-zoom) height of one tile row for a given tile width.
pub fn row_h(tile_w: f32) -> f32 {
    tile_w * TILT
}

/// Camera over the ground plane: a screen-space pan offset and a zoom factor.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub offset: Vec2,
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            offset: vec2(0.0, 0.0),
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// Smoothly chase a world-space target so it sits at the screen center.
    /// Exponential smoothing — no overshoot, framerate-independent. `stiffness`
    /// is in Hz (≈ how fast it catches up); 6–10 feels natural.
    pub fn follow(&mut self, world_target: Vec2, screen_size: Vec2, dt: f32, stiffness: f32) {
        let projected = vec2(world_target.x, world_target.y * TILT) * self.zoom;
        let target_offset = screen_size * 0.5 - projected;
        let t = 1.0 - (-stiffness * dt).exp();
        self.offset = self.offset.lerp(target_offset, t);
    }

    /// Snap the camera so `world_target` is exactly at screen center.
    pub fn snap_to(&mut self, world_target: Vec2, screen_size: Vec2) {
        let projected = vec2(world_target.x, world_target.y * TILT) * self.zoom;
        self.offset = screen_size * 0.5 - projected;
    }
}

/// Screen position of tile `(tx, ty)`'s top-left corner.
pub fn tile_to_screen(tx: i32, ty: i32, tile_w: f32, cam: &Camera) -> Vec2 {
    let rh = row_h(tile_w);
    vec2(tx as f32 * tile_w, ty as f32 * rh) * cam.zoom + cam.offset
}

/// On-screen size of a single tile rectangle.
pub fn tile_screen_size(tile_w: f32, cam: &Camera) -> Vec2 {
    vec2(tile_w, row_h(tile_w)) * cam.zoom
}

/// Top-left position and size of tile `(tx, ty)`'s rectangle.
pub fn tile_rect(tx: i32, ty: i32, tile_w: f32, cam: &Camera) -> (Vec2, Vec2) {
    (tile_to_screen(tx, ty, tile_w, cam), tile_screen_size(tile_w, cam))
}

/// Screen-space center of tile `(tx, ty)`.
pub fn tile_center(tx: i32, ty: i32, tile_w: f32, cam: &Camera) -> Vec2 {
    let (pos, size) = tile_rect(tx, ty, tile_w, cam);
    pos + size * 0.5
}

/// Inverse of [`tile_to_screen`]: which tile does a screen point fall on.
pub fn screen_to_tile(screen: Vec2, tile_w: f32, cam: &Camera) -> (i32, i32) {
    let local = (screen - cam.offset) / cam.zoom;
    let fx = local.x / tile_w;
    let fy = local.y / row_h(tile_w);
    (fx.floor() as i32, fy.floor() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_to_tile_inverts_tile_to_screen() {
        let cam = Camera {
            offset: vec2(400.0, 120.0),
            zoom: 1.5,
        };
        let tw = DEFAULT_TILE_W;
        for tx in -3..6 {
            for ty in -3..6 {
                let c = tile_center(tx, ty, tw, &cam);
                assert_eq!(
                    screen_to_tile(c, tw, &cam),
                    (tx, ty),
                    "tile ({tx},{ty}) center should map back to itself"
                );
            }
        }
    }
}
