//! Stylized 2D grass layer.
//!
//! A purely cosmetic field of waving grass blades, drawn between the ground
//! tiles and the critter pass so animals occlude it. Blades are built as a
//! per-frame triangle mesh (no textures, no custom shader): each blade is a
//! thin triangle with a root→tip colour gradient, an occasional highlight tint,
//! and its tip swayed by a wind function (procedural noise + time), giving the
//! "wind from a noise texture" look in 2D.
//!
//! Placement is deterministic per tile (seeded from `world_seed`), so the field
//! is stable frame-to-frame and across sessions. Density and colour are tuned
//! per biome, with a separate "manicured" profile inside the home zoo plot.

use macroquad::prelude::*;

use crate::app::GameApp;
use crate::game::biome::{self, noise2d_seeded};
use crate::game::species::HabitatTheme;

use super::view::{self, PLANE_H, PLANE_W};

/// World units per ground tile (must match `draw_scene`'s `BTILE`).
const BTILE: f32 = 128.0;

/// Below this camera zoom the field is too small/expensive to be worth drawing.
const MIN_ZOOM: f32 = 0.18;

/// Safety cap on blades emitted per frame (bounds worst-case overdraw/cost).
const MAX_BLADES: usize = 44_000;

/// macroquad clamps any single `draw_mesh` at 10000 verts / 5000 indices (it
/// drops the overflow with a warning), so flush each batch comfortably under the
/// tighter index limit. Each tuft is a quad = 4 verts / 6 indices, so this caps
/// a batch at ~800 tufts (and ~3200 verts, well under 10000).
const MAX_IDX_PER_FLUSH: usize = 4800;

// Wind tuning. The gust is now a procedural value-noise field evaluated in the
// vertex shader; these feed its uniforms. `WIND_AMP` is the tip sway in screen
// px at zoom 1 (scaled by zoom at draw time); `WIND_SCALE` is the spatial
// frequency over world units; `WIND_SPEED` advances the noise field over time.
const WIND_AMP: f32 = 9.0;
const WIND_SCALE: f32 = 0.015;
const WIND_SPEED: f32 = 0.6;

/// Grass detail level. Off by default; opt in (and pick density) only via the
/// `GRASS` environment variable at launch — there is no in-game toggle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrassQuality {
    Off,
    Subtle,
    Lush,
}

impl Default for GrassQuality {
    fn default() -> Self {
        GrassQuality::Off
    }
}

impl GrassQuality {
    /// Resolve the launch grass level from the `GRASS` environment variable
    /// (case-insensitive): `lush` / `subtle` enable the field; anything else —
    /// including unset — leaves it `Off`. This is the only way to turn grass on.
    pub fn from_env() -> Self {
        let q = match std::env::var("GRASS").ok().as_deref().map(str::trim) {
            Some(v) if v.eq_ignore_ascii_case("lush") => GrassQuality::Lush,
            Some(v) if v.eq_ignore_ascii_case("subtle") => GrassQuality::Subtle,
            _ => GrassQuality::Off,
        };
        eprintln!("grass: {} (set GRASS=lush|subtle to enable)", q.label());
        q
    }

    /// Base tuft count per ground tile before the per-biome density multiplier.
    /// Moderate counts: the shader's dithered alpha keeps dense overlap crisp, so
    /// we can pack more blades than the translucent-tuft era without muddying.
    pub fn blades_per_tile(self) -> u32 {
        match self {
            GrassQuality::Off => 0,
            GrassQuality::Subtle => 14,
            GrassQuality::Lush => 28,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GrassQuality::Off => "Off",
            GrassQuality::Subtle => "Subtle",
            GrassQuality::Lush => "Lush",
        }
    }
}

/// Per-context grass styling. Density 0 means "no grass here" (water, ice, sand).
#[derive(Clone, Copy)]
struct GrassProfile {
    density: f32,
    /// Opaque-ish ground fill painted under the tufts so the field reads as a
    /// seamless painted terrain (no ground showing through the gaps).
    base: Color,
    root: Color,
    tip: Color,
    highlight: Color,
    /// Tuft height in screen px at zoom 1 (large — tufts overlap into a carpet).
    height: f32,
}

fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color::new(r, g, b, 1.0)
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color::new(r, g, b, a)
}

/// Multiply a colour's RGB by `f` (for per-tuft cloud brightness), keeping alpha.
fn mul_rgb(c: Color, f: f32) -> Color {
    Color::new((c.r * f).min(1.0), (c.g * f).min(1.0), (c.b * f).min(1.0), c.a)
}

/// Grass profile for a biome. The atlas is white→alpha, so these colours are the
/// final grass hue. Tufts are large + translucent and layered over a `base` fill,
/// so the field blends into a painterly carpet rather than distinct sprites.
fn biome_profile(theme: HabitatTheme) -> GrassProfile {
    use HabitatTheme::*;
    let p = |density, base: [f32; 3], root: [f32; 3], tip: [f32; 3], hi: [f32; 3], height| GrassProfile {
        density,
        base: rgba(base[0], base[1], base[2], 0.97),
        root: rgba(root[0], root[1], root[2], 0.82),
        tip: rgba(tip[0], tip[1], tip[2], 0.82),
        highlight: rgba(hi[0], hi[1], hi[2], 0.88),
        height,
    };
    match theme {
        Forest | Taiga => p(1.0, [0.34, 0.48, 0.22], [0.20, 0.38, 0.15], [0.40, 0.60, 0.26], [0.58, 0.76, 0.34], 38.0),
        Jungle | Wetland => p(1.0, [0.30, 0.48, 0.24], [0.16, 0.40, 0.18], [0.36, 0.60, 0.28], [0.54, 0.78, 0.34], 40.0),
        Farmland => p(1.0, [0.52, 0.62, 0.30], [0.36, 0.50, 0.22], [0.60, 0.74, 0.32], [0.78, 0.88, 0.46], 36.0),
        // Pale chartreuse yellow-green to match the painted reference field.
        Savanna | Highlands => {
            p(1.0, [0.74, 0.79, 0.46], [0.58, 0.66, 0.32], [0.82, 0.86, 0.50], [0.92, 0.94, 0.60], 36.0)
        }
        Badlands => p(0.7, [0.58, 0.56, 0.34], [0.44, 0.42, 0.22], [0.66, 0.64, 0.36], [0.80, 0.78, 0.46], 32.0),
        // Ocean, Arctic, Desert, Tundra, Volcanic, Beach, Mythical, Void,
        // Festive, Food → no grass.
        _ => GrassProfile {
            density: 0.0,
            base: rgba(0.0, 0.0, 0.0, 0.0),
            root: rgb(0.0, 0.0, 0.0),
            tip: rgb(0.0, 0.0, 0.0),
            highlight: rgb(0.0, 0.0, 0.0),
            height: 0.0,
        },
    }
}

/// Manicured lawn profile used inside the home zoo plot — lush green, independent
/// of the underlying biome.
fn zoo_profile() -> GrassProfile {
    GrassProfile {
        density: 1.0,
        base: rgba(0.30, 0.46, 0.22, 0.95),
        root: rgba(0.16, 0.34, 0.14, 0.82),
        tip: rgba(0.34, 0.56, 0.26, 0.82),
        highlight: rgba(0.52, 0.74, 0.34, 0.85),
        height: 34.0,
    }
}

/// True when `pos` is inside the plot centred at `home_c` with half-extent
/// `home_h` (the local player's home plot).
fn in_zoo(pos: Vec2, home_c: Vec2, home_h: f32) -> bool {
    (pos.x - home_c.x).abs() <= home_h && (pos.y - home_c.y).abs() <= home_h
}

/// Grass profile for tile `(tx, ty)` — the manicured zoo lawn inside the home
/// plot (`home_c`/`home_h`), else the biome profile.
fn tile_profile(tx: i32, ty: i32, seed: u64, home_c: Vec2, home_h: f32) -> GrassProfile {
    let center = vec2((tx as f32 + 0.5) * BTILE, (ty as f32 + 0.5) * BTILE);
    if in_zoo(center, home_c, home_h) {
        zoo_profile()
    } else {
        biome_profile(biome::biome_at(center, seed))
    }
}

/// Well-distributed 32-bit hash of `(a, b, i)` salted by `seed`. Full avalanche
/// (lowbias32 finalizer) so every output bit depends on `i` — otherwise tufts
/// within a tile collapse into a narrow band instead of scattering.
fn hash(a: i32, b: i32, i: u32, seed: u64) -> u32 {
    let mut h = (a as u32).wrapping_mul(0x8DA6_B343)
        ^ (b as u32).wrapping_mul(0xD816_3841)
        ^ i.wrapping_mul(0xCB1A_B31F)
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

/// Emit blades for tile `(tx, ty)` into the vertex/index buffers. Returns the
/// number of blades added.
#[allow(clippy::too_many_arguments)]
fn emit_tile(
    tx: i32,
    ty: i32,
    seed: u64,
    quality: GrassQuality,
    cam: &view::Camera,
    verts: &mut Vec<Vertex>,
    idx: &mut Vec<u16>,
    budget: usize,
    home_c: Vec2,
    home_h: f32,
) -> usize {
    let tile_cx = (tx as f32 + 0.5) * BTILE;
    let tile_cy = (ty as f32 + 0.5) * BTILE;
    if tile_cx < 0.0 || tile_cx > PLANE_W || tile_cy < 0.0 || tile_cy > PLANE_H {
        return 0;
    }
    let profile = tile_profile(tx, ty, seed, home_c, home_h);
    if profile.density <= 0.0 {
        return 0;
    }
    let count = ((quality.blades_per_tile() as f32 * profile.density).round() as usize).min(budget);
    let zoom = cam.zoom;
    let mut added = 0;

    for i in 0..count as u32 {
        // Deterministic per-tuft randomness.
        let hx = hash(tx, ty, i * 7 + 1, seed);
        let hy = hash(tx, ty, i * 7 + 2, seed);
        let hh = hash(tx, ty, i * 7 + 3, seed);
        let hl = hash(tx, ty, i * 7 + 5, seed);
        let hvar = hash(tx, ty, i * 7 + 6, seed);

        let wx = tx as f32 * BTILE + unit(hx) * BTILE;
        let wy = ty as f32 * BTILE + unit(hy) * BTILE;
        let world = vec2(wx, wy);

        // Size + slight per-tuft variation (overlapping tufts read as a field).
        let height = profile.height * (0.82 + unit(hh) * 0.4) * zoom;
        // Half-width > height*0.5: wide bushy tufts overlap horizontally so the
        // hard-cutoff silhouettes merge into a seamless field (no gaps).
        let hw = height * 0.62;

        // Per-tuft cloud brightness from a slow large-scale noise → soft light/
        // dark patches across the field (the painterly look), not per-blade.
        let bright = 0.8 + 0.4 * noise2d_seeded(world.x, world.y, 1500.0, seed ^ 0x6727);
        let root_c = mul_rgb(profile.root, bright);
        let tip_base = if unit(hl) < 0.15 { profile.highlight } else { profile.tip };
        let tip_color = mul_rgb(tip_base, bright);

        let root = view::world_to_screen(world, cam);

        // Per-blade seed (0..1) drives the highlight-sparkle branch in the shader.
        let blade_seed = unit(hl);

        // Pick one of the 2×2 atlas cells; base at the cell's bottom, tip at top.
        let variant = hvar % 4;
        let cu = (variant % 2) as f32 * 0.5;
        let cv = (variant / 2) as f32 * 0.5;
        let (u0, u1) = (cu, cu + 0.5);
        let (v_top, v_bot) = (cv, cv + 0.5);

        // Quad: bottom-left, bottom-right, top-right, top-left (screen space).
        // Wind is applied in the vertex shader: `normal = (worldX, worldY, bend,
        // seed)`, with bend = 0 on the two base verts (planted root) and 1 on the
        // two tip verts (full sway). No CPU sway — tip x is time-independent here.
        let by = root.y;
        let ty_ = root.y - height;
        let base = verts.len() as u16;
        verts.push(blade_vert(root.x - hw, by, u0, v_bot, root_c, world, 0.0, blade_seed));
        verts.push(blade_vert(root.x + hw, by, u1, v_bot, root_c, world, 0.0, blade_seed));
        verts.push(blade_vert(root.x + hw, ty_, u1, v_top, tip_color, world, 1.0, blade_seed));
        verts.push(blade_vert(root.x - hw, ty_, u0, v_top, tip_color, world, 1.0, blade_seed));
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        added += 1;
    }
    added
}

/// Build a grass vertex, packing per-blade data into the free `normal` slot
/// (`worldX, worldY, bend, seed`) for the grass shader's wind + sparkle.
#[allow(clippy::too_many_arguments)]
fn blade_vert(x: f32, y: f32, u: f32, v: f32, c: Color, world: Vec2, bend: f32, seed: f32) -> Vertex {
    Vertex {
        position: vec3(x, y, 0.0),
        uv: vec2(u, v),
        color: c.into(),
        normal: vec4(world.x, world.y, bend, seed),
    }
}

/// Move the accumulated geometry into a mesh and draw it, sampling `atlas` (the
/// blade shape, white-on-alpha) modulated by the per-vertex colours. Leaves the
/// buffers empty.
fn flush(verts: &mut Vec<Vertex>, idx: &mut Vec<u16>, atlas: Option<&Texture2D>) {
    if verts.is_empty() {
        return;
    }
    let mesh = Mesh {
        vertices: std::mem::take(verts),
        indices: std::mem::take(idx),
        texture: atlas.cloned(),
    };
    draw_mesh(&mesh);
}

/// Draw the grass field for the current camera view. No-op when quality is Off,
/// the camera is zoomed too far out, or the tuft `atlas` is missing.
///
/// The tuft mesh is drawn through `material` (the custom grass shader: GPU wind,
/// dithered alpha, cylindrical shading). The base-fill pass runs *before* the
/// material is bound, so its rectangles go through the default material; the
/// material is unbound again right after the mesh. If `material` is `None`
/// (shader failed to compile) the mesh falls back to the default material.
pub fn draw_grass(app: &GameApp, atlas: Option<&Texture2D>, material: Option<&Material>) {
    let quality = app.grass_quality;
    if quality == GrassQuality::Off {
        return;
    }
    if atlas.is_none() {
        return;
    }
    let cam = app.camera;
    if cam.zoom < MIN_ZOOM {
        return;
    }
    let seed = app.zoo.world_seed;
    // The local player's home plot gets the manicured lawn profile.
    let home_c = app.zoo.plot_origin;
    let home_h = app.zoo.plot_half_extent();

    let (cam_tl, cam_br) = view::camera_world_rect(&cam, screen_width(), screen_height());
    let tx0 = (cam_tl.x / BTILE).floor() as i32;
    let tx1 = (cam_br.x / BTILE).ceil() as i32;
    let ty0 = (cam_tl.y / BTILE).floor() as i32;
    let ty1 = (cam_br.y / BTILE).ceil() as i32;

    // Pass 1: paint a grass-coloured base under every grassy tile, so the field
    // reads as a seamless painted terrain (the soft Voronoi ground still shows
    // through the < 1.0 alpha, giving gentle variation). Drawn before the tufts
    // so all tufts layer on top.
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let center = vec2((tx as f32 + 0.5) * BTILE, (ty as f32 + 0.5) * BTILE);
            if center.x < 0.0 || center.x > PLANE_W || center.y < 0.0 || center.y > PLANE_H {
                continue;
            }
            let profile = tile_profile(tx, ty, seed, home_c, home_h);
            if profile.density <= 0.0 {
                continue;
            }
            let (pos, size) = view::tile_rect(tx, ty, BTILE, &cam);
            draw_rectangle(pos.x, pos.y, size.x + 1.0, size.y + 1.0, profile.base);
        }
    }

    // Pass 2: the tuft mesh, drawn through the grass shader. Bind the material
    // (if built) and feed its wind uniforms; `wind_amp` is scaled by zoom so the
    // tip sway stays a consistent screen-space size at any zoom.
    if let Some(mat) = material {
        mat.set_uniform("time", get_time() as f32);
        mat.set_uniform("wind_amp", WIND_AMP * cam.zoom);
        mat.set_uniform("wind_scale", WIND_SCALE);
        mat.set_uniform("wind_speed", WIND_SPEED);
        gl_use_material(mat);
    }

    let mut verts: Vec<Vertex> = Vec::new();
    let mut idx: Vec<u16> = Vec::new();
    let mut budget = MAX_BLADES;

    // Worst-case indices a single tile can add (so we flush *before* overflowing
    // the 5000-index drawcall limit rather than after).
    let tile_max_idx = quality.blades_per_tile() as usize * 6;
    'outer: for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            if budget == 0 {
                break 'outer;
            }
            if idx.len() + tile_max_idx >= MAX_IDX_PER_FLUSH {
                flush(&mut verts, &mut idx, atlas);
            }
            let added = emit_tile(tx, ty, seed, quality, &cam, &mut verts, &mut idx, budget, home_c, home_h);
            budget -= added;
        }
    }
    flush(&mut verts, &mut idx, atlas);

    if material.is_some() {
        gl_use_default_material();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_quality_emits_nothing() {
        let mut verts = Vec::new();
        let mut idx = Vec::new();
        let cam = view::Camera { offset: vec2(0.0, 0.0), zoom: 1.0 };
        let home = crate::game::plot::world_center();
        // A clearly-grassy tile but Off quality → no blades.
        let n = emit_tile(0, 0, 42, GrassQuality::Off, &cam, &mut verts, &mut idx, 1000, home, 600.0);
        assert_eq!(n, 0);
        assert!(verts.is_empty());
    }

    #[test]
    fn barren_biome_emits_no_grass() {
        // Force a profile with zero density and confirm no geometry.
        let p = biome_profile(HabitatTheme::Ocean);
        assert_eq!(p.density, 0.0);
        let p2 = biome_profile(HabitatTheme::Desert);
        assert_eq!(p2.density, 0.0);
    }

    #[test]
    fn grassy_biome_has_density() {
        assert!(biome_profile(HabitatTheme::Forest).density > 0.0);
        assert!(biome_profile(HabitatTheme::Jungle).density > 0.0);
    }

    #[test]
    fn placement_is_deterministic() {
        let cam = view::Camera { offset: vec2(120.0, 80.0), zoom: 1.0 };
        let mut va = Vec::new();
        let mut ia = Vec::new();
        let mut vb = Vec::new();
        let mut ib = Vec::new();
        // A tile at the home plot centre gets the (always-grassy) manicured
        // profile, so the count is stable regardless of biome classification.
        let za = crate::game::plot::world_center();
        let home_h = 600.0;
        let tx = (za.x / BTILE) as i32;
        let ty = (za.y / BTILE) as i32;
        let a = emit_tile(tx, ty, 7, GrassQuality::Lush, &cam, &mut va, &mut ia, 1000, za, home_h);
        let b = emit_tile(tx, ty, 7, GrassQuality::Lush, &cam, &mut vb, &mut ib, 1000, za, home_h);
        assert!(a > 0);
        assert_eq!(a, b, "same tile+seed+quality → same tuft count");
        // Each tuft is a textured quad: 4 verts / 6 indices.
        assert_eq!(va.len(), a * 4);
        assert_eq!(ia.len(), a * 6);
        // Wind now lives in the shader: every CPU vertex position is
        // time-independent, so the two emissions are bit-identical.
        assert_eq!(va[2].position.x, vb[2].position.x, "tip x is time-independent on the CPU");
        // The free `normal` slot packs per-blade data: base verts get bend = 0
        // (planted root), tip verts bend = 1 (full sway).
        assert_eq!(va[0].normal.z, 0.0, "base vert bend = 0");
        assert_eq!(va[2].normal.z, 1.0, "tip vert bend = 1");
        // worldX/worldY are shared across a tuft's four verts.
        assert_eq!(va[0].normal.x, va[2].normal.x);
        assert_eq!(va[0].normal.y, va[2].normal.y);
    }
}
