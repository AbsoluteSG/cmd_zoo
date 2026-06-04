//! World biome system — four layered sub-systems that work together:
//!
//! 1. **Voronoi biomes** — fixed seed points across the world; any position
//!    belongs to the theme of its nearest seed. The zoo centre is Forest so
//!    the early game is safe; rarer/aggressive biomes are further out.
//!
//! 2. **Value noise** — a smooth 2-D noise field (0–1) sampled at chunk
//!    resolution. Drives spawn-density and species selection within each biome.
//!    Low noise → peaceful, slow animals. High noise → aggressive packs.
//!
//! 3. **Poisson-disk candidate placement** — Bridson's algorithm generates
//!    spatially well-distributed candidate positions inside each chunk with a
//!    guaranteed minimum separation distance, preventing clusters.
//!
//! 4. **Weighted spawn table** — each (biome × noise band) entry has a weight.
//!    A "no spawn" baseline weight keeps animal density sparse. The roller
//!    picks one eligible entry or returns None.

use macroquad::color::Color;
use macroquad::math::{Vec2, vec2};

use crate::game::species::HabitatTheme;
use crate::game::wild_animal::Moveset;

// ── 1. Seed-driven climate biomes ─────────────────────────────────────────────
//
// Biomes are derived on the fly from two low-frequency "climate" noise fields —
// temperature and moisture — sampled in a per-world seed-shifted noise space and
// lightly domain-warped for organic edges. This scales to an arbitrarily large
// world (no fixed seed-point table) and produces a different layout per save.

// ── BiomeTuning — one-stop knobs for the whole generator ───────────────────────
//
// Sizes are feature sizes in world units (larger = bigger blobs). The natural
// biomes are carved from temperature / moisture / elevation fields; the four
// fantastical biomes are scattered as rare patches by a separate "special"
// field. Everything is a pure function of (pos, world_seed).
//
//   Natural biome SIZE      → BIOME_FEATURE_SCALE   (smaller = more, smaller biomes)
//   Special patch SIZE      → SPECIAL_FEATURE_SCALE
//   Special RARITY/coverage → SPECIAL_THRESHOLD     (higher = rarer)
//   Per-special WEIGHT      → SPECIAL_BAND_* cut points
//   Natural placement       → the TEMP_/MOIST_/ELEV_ thresholds below
//   Intra-biome texture     → TONE_SCALE / TONE_AMOUNT / TONE_HUE

/// Feature size (world units) of natural biome regions. Lowered from the old
/// 42k so the 500k world spans ~23 feature-widths — more biomes, finer shapes.
const BIOME_FEATURE_SCALE: f32 = 22_000.0;
/// Domain-warp feature size and strength (world units) — bends biome borders so
/// they read as natural coastlines/treelines rather than smooth blobs.
const WARP_SCALE: f32 = 11_000.0;
const WARP_AMOUNT: f32 = 5_000.0;

// Natural-biome classification thresholds (all on [0,1] climate fields).
const TEMP_ARCTIC: f32 = 0.20;
const TEMP_TUNDRA: f32 = 0.30;
const TEMP_TAIGA: f32 = 0.40;
const TEMP_HOT: f32 = 0.70;
const MOIST_OCEAN: f32 = 0.80;
const MOIST_BEACH: f32 = 0.75;
const ELEV_HIGHLANDS: f32 = 0.74;
const ELEV_VOLCANIC: f32 = 0.82;

// Fantastical / rare biome controls.
const SPECIAL_FEATURE_SCALE: f32 = 30_000.0;
/// Coverage gate: a patch appears only where the special field exceeds this.
/// Raise toward 1.0 to make special biomes rarer.
const SPECIAL_THRESHOLD: f32 = 0.70;
// Cumulative selector cut points carving [0,1] into the four specials. The gaps
// between successive values are each biome's share (weight).
const SPECIAL_BAND_MYTHICAL: f32 = 0.30;
const SPECIAL_BAND_VOID: f32 = 0.55;
const SPECIAL_BAND_FESTIVE: f32 = 0.80;
// (>= SPECIAL_BAND_FESTIVE → Food)

// Intra-biome tonal texture.
const TONE_SCALE: f32 = 1_500.0;
const TONE_AMOUNT: f32 = 0.09;
const TONE_HUE: f32 = 0.04;

/// A large pseudo-random offset into noise space, derived from the world seed
/// and a channel id, so each world (and each climate field) samples a different
/// region of the noise function.
fn seed_offset(seed: u64, channel: u64) -> Vec2 {
    let mut r = LcgRng::new(seed ^ channel.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    vec2(
        (r.next_f32() * 2.0 - 1.0) * 1_000_000.0,
        (r.next_f32() * 2.0 - 1.0) * 1_000_000.0,
    )
}

/// Continuous (temperature, moisture, elevation) climate at `pos`, each in
/// [0, 1], with domain warp applied. Driven by seeded fractal noise so distinct
/// seeds produce fully independent layouts. Deterministic for a given `seed`.
fn climate_at(pos: Vec2, seed: u64) -> (f32, f32, f32) {
    let wo = seed_offset(seed, 7);
    let wx = fbm_seeded(pos.x + wo.x, pos.y + wo.y, WARP_SCALE, seed, 2) - 0.5;
    let wy = fbm_seeded(pos.x + wo.x + 4096.0, pos.y + wo.y - 4096.0, WARP_SCALE, seed, 2) - 0.5;
    let warped = vec2(pos.x + wx * WARP_AMOUNT, pos.y + wy * WARP_AMOUNT);

    let to = seed_offset(seed, 1);
    let mo = seed_offset(seed, 2);
    let eo = seed_offset(seed, 3);
    let temp = fbm_seeded(warped.x + to.x, warped.y + to.y, BIOME_FEATURE_SCALE, seed, 3);
    let moist = fbm_seeded(warped.x + mo.x, warped.y + mo.y, BIOME_FEATURE_SCALE, seed, 3);
    let elev = fbm_seeded(warped.x + eo.x, warped.y + eo.y, BIOME_FEATURE_SCALE, seed, 3);
    (temp, moist, elev)
}

/// Map a (temperature, moisture, elevation) triple to a natural biome. The
/// thresholds (see the BiomeTuning block) are the placement knobs.
fn classify(temp: f32, moist: f32, elev: f32) -> HabitatTheme {
    use HabitatTheme::*;
    // High ground overrides climate → mountainous terrain.
    if elev > ELEV_VOLCANIC && temp > 0.62 {
        return Volcanic;
    }
    if elev > ELEV_HIGHLANDS {
        return Highlands;
    }
    // Water and its sandy coastal fringe.
    if moist > MOIST_OCEAN {
        return Ocean;
    }
    if moist > MOIST_BEACH {
        return Beach;
    }
    // Cold band: ice → tundra → boreal forest.
    if temp < TEMP_ARCTIC {
        return Arctic;
    }
    if temp < TEMP_TUNDRA {
        return Tundra;
    }
    if temp < TEMP_TAIGA {
        return if moist > 0.40 { Taiga } else { Tundra };
    }
    // Hot band: drier → desert/badlands, wetter → savanna/jungle.
    if temp > TEMP_HOT {
        if moist < 0.20 {
            return Desert;
        }
        if moist < 0.35 {
            return Badlands;
        }
        if moist < 0.55 {
            return Savanna;
        }
        return Jungle;
    }
    // Temperate band.
    if moist > 0.60 {
        Wetland
    } else if moist > 0.38 {
        Forest
    } else {
        Farmland
    }
}

/// A rare fantastical biome at `pos`, if the special field is high enough here.
/// Returns `None` across the vast majority of the world. `SPECIAL_THRESHOLD`
/// controls rarity, `SPECIAL_FEATURE_SCALE` the patch size, and the
/// `SPECIAL_BAND_*` cut points each biome's share.
fn special_biome(pos: Vec2, seed: u64) -> Option<HabitatTheme> {
    use HabitatTheme::*;
    let so = seed_offset(seed, 5);
    let s = fbm_seeded(pos.x + so.x, pos.y + so.y, SPECIAL_FEATURE_SCALE, seed ^ 0x5, 3);
    if s <= SPECIAL_THRESHOLD {
        return None;
    }
    let ho = seed_offset(seed, 6);
    let sel = noise2d_seeded(pos.x + ho.x, pos.y + ho.y, SPECIAL_FEATURE_SCALE * 0.5, seed ^ 0x6);
    Some(if sel < SPECIAL_BAND_MYTHICAL {
        Mythical
    } else if sel < SPECIAL_BAND_VOID {
        Void
    } else if sel < SPECIAL_BAND_FESTIVE {
        Festive
    } else {
        Food
    })
}

/// Hard biome at `pos` for the given world `seed`. Rare special patches win
/// over the natural climate classification.
pub fn biome_at(pos: Vec2, seed: u64) -> HabitatTheme {
    if let Some(special) = special_biome(pos, seed) {
        return special;
    }
    let (t, m, e) = climate_at(pos, seed);
    classify(t, m, e)
}

/// Smoothly blended ground colour at `pos`. Averages the biome colour of a few
/// nearby samples so biome boundaries read as gradients rather than hard edges.
pub fn biome_color_at(pos: Vec2, seed: u64) -> Color {
    const O: f32 = 1400.0;
    let samples = [
        pos,
        pos + vec2(O, 0.0),
        pos + vec2(-O, 0.0),
        pos + vec2(0.0, O),
        pos + vec2(0.0, -O),
    ];
    let (mut r, mut g, mut b) = (0.0_f32, 0.0_f32, 0.0_f32);
    for s in samples {
        let c = biome_color(biome_at(s, seed));
        r += c.r;
        g += c.g;
        b += c.b;
    }
    let n = samples.len() as f32;
    Color::new(r / n, g / n, b / n, 1.0)
}

/// Per-biome ground colour (used by `biome_color_at` for blending).
pub fn biome_color(theme: HabitatTheme) -> Color {
    match theme {
        HabitatTheme::Forest    => Color::new(0.275, 0.451, 0.267, 1.0), // dark forest green
        HabitatTheme::Arctic    => Color::new(0.765, 0.855, 0.941, 1.0), // icy pale blue
        HabitatTheme::Savanna   => Color::new(0.686, 0.627, 0.373, 1.0), // golden tan
        HabitatTheme::Wetland   => Color::new(0.294, 0.451, 0.333, 1.0), // murky olive
        HabitatTheme::Jungle    => Color::new(0.149, 0.373, 0.216, 1.0), // deep tropical
        HabitatTheme::Ocean     => Color::new(0.216, 0.373, 0.608, 1.0), // cool blue
        HabitatTheme::Farmland  => Color::new(0.608, 0.686, 0.412, 1.0), // light field
        HabitatTheme::Desert    => Color::new(0.850, 0.780, 0.550, 1.0), // pale ochre sand
        HabitatTheme::Tundra    => Color::new(0.660, 0.700, 0.700, 1.0), // frosted grey
        HabitatTheme::Taiga     => Color::new(0.310, 0.440, 0.360, 1.0), // dark pine
        HabitatTheme::Volcanic  => Color::new(0.260, 0.180, 0.180, 1.0), // basalt + ember
        HabitatTheme::Badlands  => Color::new(0.660, 0.380, 0.260, 1.0), // rusty red
        HabitatTheme::Beach     => Color::new(0.900, 0.840, 0.620, 1.0), // light sand
        HabitatTheme::Highlands => Color::new(0.520, 0.540, 0.500, 1.0), // rocky grey-green
        HabitatTheme::Mythical  => Color::new(0.620, 0.440, 0.780, 1.0), // violet
        HabitatTheme::Void      => Color::new(0.100, 0.070, 0.150, 1.0), // near-black purple
        HabitatTheme::Festive   => Color::new(0.800, 0.260, 0.340, 1.0), // crimson
        HabitatTheme::Food      => Color::new(0.870, 0.560, 0.420, 1.0), // caramel/salmon
    }
}

/// Ground colour for a render tile: the blended biome colour, then perturbed by
/// fine seeded noise so each biome reads as textured patches of related tones
/// rather than one flat fill. Deterministic for a fixed `(pos, seed)`. Used by
/// the world ground renderer and the biome-debug view.
pub fn biome_tile_color(pos: Vec2, seed: u64) -> Color {
    let base = biome_color_at(pos, seed);
    // Brightness wobble (±TONE_AMOUNT).
    let n = noise2d_seeded(pos.x, pos.y, TONE_SCALE, seed ^ 0x7_0E0);
    let b = 1.0 + (n - 0.5) * 2.0 * TONE_AMOUNT;
    // Subtle warm/cool tint shift from an independent sample.
    let n2 = noise2d_seeded(pos.x + 1234.0, pos.y - 5678.0, TONE_SCALE * 1.7, seed ^ 0xABCD);
    let tint = (n2 - 0.5) * 2.0 * TONE_HUE;
    Color::new(
        (base.r * b + tint).clamp(0.0, 1.0),
        (base.g * b).clamp(0.0, 1.0),
        (base.b * b - tint).clamp(0.0, 1.0),
        1.0,
    )
}

// ── 2. Value noise ────────────────────────────────────────────────────────────

/// Smooth 2-D value noise in [0, 1].
/// `scale` is the feature size in world units — larger = broader gradients.
pub fn noise2d(px: f32, py: f32, scale: f32) -> f32 {
    let x = px / scale;
    let y = py / scale;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();

    let v00 = hash_f(ix,     iy    );
    let v10 = hash_f(ix + 1, iy    );
    let v01 = hash_f(ix,     iy + 1);
    let v11 = hash_f(ix + 1, iy + 1);

    let sx = smoothstep(fx);
    let sy = smoothstep(fy);
    let row0 = v00 + (v10 - v00) * sx;
    let row1 = v01 + (v11 - v01) * sx;
    row0 + (row1 - row0) * sy
}

fn hash_f(x: i32, y: i32) -> f32 {
    let mut h: u32 = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263));
    h ^= h >> 13;
    h = h.wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    h as f32 / u32::MAX as f32
}

/// Seed-aware lattice hash. Folding `seed` into the initial state means each
/// seed addresses a completely independent noise field — the key fix for
/// "reseeding gives near-identical layouts".
fn hash_seeded(x: i32, y: i32, seed: u64) -> f32 {
    let s = (seed ^ (seed >> 32)) as u32;
    let mut h: u32 = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263))
        .wrapping_add(s.wrapping_mul(2_246_822_519));
    h ^= h >> 13;
    h = h.wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    h as f32 / u32::MAX as f32
}

/// Seeded value noise in [0, 1] — like [`noise2d`] but each `seed` yields an
/// independent field (see [`hash_seeded`]).
pub fn noise2d_seeded(px: f32, py: f32, scale: f32, seed: u64) -> f32 {
    let x = px / scale;
    let y = py / scale;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();

    let v00 = hash_seeded(ix,     iy,     seed);
    let v10 = hash_seeded(ix + 1, iy,     seed);
    let v01 = hash_seeded(ix,     iy + 1, seed);
    let v11 = hash_seeded(ix + 1, iy + 1, seed);

    let sx = smoothstep(fx);
    let sy = smoothstep(fy);
    let row0 = v00 + (v10 - v00) * sx;
    let row1 = v01 + (v11 - v01) * sx;
    row0 + (row1 - row0) * sy
}

/// Fractal (fBm) seeded noise in [0, 1]: sum `octaves` layers, each at half the
/// amplitude and twice the frequency of the last, then normalize. Produces
/// natural multi-scale borders instead of single-frequency blobs.
pub fn fbm_seeded(px: f32, py: f32, scale: f32, seed: u64, octaves: u32) -> f32 {
    let mut amp = 1.0_f32;
    let mut freq = 1.0_f32;
    let mut sum = 0.0_f32;
    let mut norm = 0.0_f32;
    for o in 0..octaves.max(1) {
        // Vary the seed per octave so layers don't align.
        let s = seed ^ (o as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        sum += amp * noise2d_seeded(px, py, scale / freq, s);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

// ── 3. Poisson-disk candidate placement ──────────────────────────────────────

/// Deterministic LCG RNG — exported so callers can drive the weighted roll
/// with the same seed stream as the Poisson-disk placement.
pub struct LcgRng(u64);

impl LcgRng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_add(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 33) as f32 / (1u64 << 31) as f32
    }
}

/// Bridson's Poisson-disk sampling inside `[origin, origin + (w, h)]`.
/// Guarantees every pair of samples is at least `min_dist` apart.
/// The `seed` makes output fully deterministic for the given chunk.
pub fn poisson_disk(origin: Vec2, w: f32, h: f32, min_dist: f32, seed: u64) -> Vec<Vec2> {
    let cell = min_dist / 2.0_f32.sqrt();
    let cols = (w / cell).ceil() as usize + 1;
    let rows = (h / cell).ceil() as usize + 1;

    let mut grid: Vec<Option<Vec2>> = vec![None; cols * rows];
    let mut active: Vec<Vec2> = Vec::new();
    let mut result: Vec<Vec2> = Vec::new();
    let mut rng = LcgRng::new(seed);

    let first = origin + vec2(rng.next_f32() * w, rng.next_f32() * h);
    let gi = grid_cell(first, origin, cell, cols);
    grid[gi] = Some(first);
    active.push(first);
    result.push(first);

    while !active.is_empty() {
        let ai = (rng.next_u64() as usize) % active.len();
        let base = active[ai];
        let mut placed = false;

        for _ in 0..22 {
            let angle = rng.next_f32() * std::f32::consts::TAU;
            let dist  = min_dist + rng.next_f32() * min_dist; // [r, 2r)
            let cand  = base + vec2(angle.cos(), angle.sin()) * dist;

            if cand.x < origin.x || cand.x >= origin.x + w
            || cand.y < origin.y || cand.y >= origin.y + h
            {
                continue;
            }
            let gi = grid_cell(cand, origin, cell, cols);
            if !neighbors_ok(&grid, cand, origin, cell, cols, rows, min_dist) {
                continue;
            }
            grid[gi] = Some(cand);
            active.push(cand);
            result.push(cand);
            placed = true;
            break;
        }
        if !placed {
            active.swap_remove(ai);
        }
    }
    result
}

fn grid_cell(p: Vec2, origin: Vec2, cell: f32, cols: usize) -> usize {
    let gx = ((p.x - origin.x) / cell).floor() as usize;
    let gy = ((p.y - origin.y) / cell).floor() as usize;
    gy * cols + gx
}

fn neighbors_ok(
    grid: &[Option<Vec2>],
    p: Vec2,
    origin: Vec2,
    cell: f32,
    cols: usize,
    rows: usize,
    r: f32,
) -> bool {
    let gx = ((p.x - origin.x) / cell).floor() as i32;
    let gy = ((p.y - origin.y) / cell).floor() as i32;
    let r2 = r * r;
    for dy in -2i32..=2 {
        for dx in -2i32..=2 {
            let nx = gx + dx;
            let ny = gy + dy;
            if nx < 0 || ny < 0 || nx >= cols as i32 || ny >= rows as i32 { continue; }
            if let Some(q) = grid[ny as usize * cols + nx as usize] {
                let d = p - q;
                if d.dot(d) < r2 { return false; }
            }
        }
    }
    true
}

// ── 4. Weighted spawn table ───────────────────────────────────────────────────

pub struct SpawnEntry {
    pub species:     &'static str,
    pub mk_moveset:  fn() -> Moveset,
    pub biome:       HabitatTheme,
    /// Inclusive noise range this entry is active over.
    pub noise_min:   f32,
    pub noise_max:   f32,
    pub weight:      u32,
}

/// Low noise  (0.0–0.40) → docile, slow animals.
/// Medium noise (0.35–0.70) → mixed; some faster movesets.
/// High noise (0.60–1.0) → aggressive, pack-feeling zones.
/// Bands intentionally overlap so the transition is gradual.
const SPAWN_TABLE: &[SpawnEntry] = &[
    // ── Forest (starting zone — kept rich so the early game has variety) ────
    // Low noise: docile starters + small woodland critters.
    SpawnEntry { species: "field_mouse",    mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Forest,   noise_min: 0.00, noise_max: 0.50, weight: 32 },
    SpawnEntry { species: "blue_frog",      mk_moveset: Moveset::panicker,  biome: HabitatTheme::Forest,   noise_min: 0.00, noise_max: 0.45, weight: 24 },
    SpawnEntry { species: "treeFrog",       mk_moveset: Moveset::freezer,   biome: HabitatTheme::Forest,   noise_min: 0.00, noise_max: 0.40, weight: 16 },
    SpawnEntry { species: "robin",          mk_moveset: Moveset::panicker,  biome: HabitatTheme::Forest,   noise_min: 0.00, noise_max: 0.50, weight: 20 },
    SpawnEntry { species: "squirrel",       mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Forest,   noise_min: 0.00, noise_max: 0.55, weight: 22 },
    SpawnEntry { species: "hedgehog",       mk_moveset: Moveset::freezer,   biome: HabitatTheme::Forest,   noise_min: 0.00, noise_max: 0.50, weight: 16 },
    SpawnEntry { species: "mole",           mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Forest,   noise_min: 0.00, noise_max: 0.45, weight: 12 },
    SpawnEntry { species: "albinoDeer",     mk_moveset: Moveset::burster,   biome: HabitatTheme::Forest,   noise_min: 0.10, noise_max: 0.60, weight: 10 },
    // Medium noise: foxes and busier woodland life.
    SpawnEntry { species: "red_fox",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Forest,   noise_min: 0.30, noise_max: 0.75, weight: 22 },
    SpawnEntry { species: "raccoon",        mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Forest,   noise_min: 0.30, noise_max: 0.78, weight: 18 },
    SpawnEntry { species: "snowyOwl",       mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Forest,   noise_min: 0.35, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "badger",         mk_moveset: Moveset::aggressor, biome: HabitatTheme::Forest,   noise_min: 0.40, noise_max: 0.85, weight: 14 },
    SpawnEntry { species: "lynx",           mk_moveset: Moveset::burster,   biome: HabitatTheme::Forest,   noise_min: 0.45, noise_max: 0.90, weight: 14 },
    SpawnEntry { species: "treeFrog",       mk_moveset: Moveset::venomous,  biome: HabitatTheme::Forest,   noise_min: 0.45, noise_max: 1.00, weight: 12 },
    SpawnEntry { species: "red_fox",            mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Forest,   noise_min: 0.55, noise_max: 1.00, weight: 16 },
    // High noise: the dangerous, pack-feeling edge of the forest.
    SpawnEntry { species: "boar",           mk_moveset: Moveset::aggressor, biome: HabitatTheme::Forest,   noise_min: 0.60, noise_max: 1.00, weight: 14 },
    SpawnEntry { species: "grey_wolf",      mk_moveset: Moveset::basher,    biome: HabitatTheme::Forest,   noise_min: 0.65, noise_max: 1.00, weight: 14 },
    SpawnEntry { species: "red_fox",            mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Forest,   noise_min: 0.70, noise_max: 1.00, weight: 10 },
    SpawnEntry { species: "lion",           mk_moveset: Moveset::aggressor, biome: HabitatTheme::Forest,   noise_min: 0.72, noise_max: 1.00, weight: 8  },
    // ── Arctic ────────────────────────────────────────────────────────────
    SpawnEntry { species: "penguin",        mk_moveset: Moveset::circler,   biome: HabitatTheme::Arctic,   noise_min: 0.00, noise_max: 0.55, weight: 30 },
    SpawnEntry { species: "snowyOwl",       mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Arctic,   noise_min: 0.20, noise_max: 0.70, weight: 22 },
    SpawnEntry { species: "penguin",        mk_moveset: Moveset::burster,   biome: HabitatTheme::Arctic,   noise_min: 0.45, noise_max: 0.85, weight: 14 },
    SpawnEntry { species: "snowyOwl",       mk_moveset: Moveset::aggressor, biome: HabitatTheme::Arctic,   noise_min: 0.65, noise_max: 1.00, weight: 12 },
    SpawnEntry { species: "lion",           mk_moveset: Moveset::aggressor, biome: HabitatTheme::Arctic,   noise_min: 0.75, noise_max: 1.00, weight: 6  },
    SpawnEntry { species: "polar_bear",     mk_moveset: Moveset::basher,    biome: HabitatTheme::Arctic,   noise_min: 0.70, noise_max: 1.00, weight: 8  },
    // ── Savanna ───────────────────────────────────────────────────────────
    SpawnEntry { species: "giantTortoise",  mk_moveset: Moveset::freezer,   biome: HabitatTheme::Savanna,  noise_min: 0.00, noise_max: 0.50, weight: 22 },
    SpawnEntry { species: "goldenToucan",   mk_moveset: Moveset::panicker,  biome: HabitatTheme::Savanna,  noise_min: 0.00, noise_max: 0.55, weight: 18 },
    SpawnEntry { species: "red_fox",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Savanna,  noise_min: 0.35, noise_max: 0.80, weight: 18 },
    SpawnEntry { species: "lion",           mk_moveset: Moveset::aggressor, biome: HabitatTheme::Savanna,  noise_min: 0.45, noise_max: 1.00, weight: 28 },
    SpawnEntry { species: "lion",           mk_moveset: Moveset::circler,   biome: HabitatTheme::Savanna,  noise_min: 0.70, noise_max: 1.00, weight: 14 },
    SpawnEntry { species: "lion",           mk_moveset: Moveset::basher,    biome: HabitatTheme::Savanna,  noise_min: 0.60, noise_max: 1.00, weight: 12 },
    // ── Wetland ───────────────────────────────────────────────────────────
    SpawnEntry { species: "blue_frog",      mk_moveset: Moveset::panicker,  biome: HabitatTheme::Wetland,  noise_min: 0.00, noise_max: 0.55, weight: 30 },
    SpawnEntry { species: "treeFrog",       mk_moveset: Moveset::freezer,   biome: HabitatTheme::Wetland,  noise_min: 0.00, noise_max: 0.50, weight: 24 },
    SpawnEntry { species: "treeFrog",       mk_moveset: Moveset::circler,   biome: HabitatTheme::Wetland,  noise_min: 0.30, noise_max: 0.75, weight: 18 },
    SpawnEntry { species: "snowyOwl",       mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Wetland,  noise_min: 0.45, noise_max: 0.90, weight: 14 },
    SpawnEntry { species: "red_fox",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Wetland,  noise_min: 0.60, noise_max: 1.00, weight: 10 },
    SpawnEntry { species: "treeFrog",       mk_moveset: Moveset::venomous,  biome: HabitatTheme::Wetland,  noise_min: 0.40, noise_max: 1.00, weight: 14 },
    // ── Jungle ────────────────────────────────────────────────────────────
    SpawnEntry { species: "goldenToucan",   mk_moveset: Moveset::panicker,  biome: HabitatTheme::Jungle,   noise_min: 0.00, noise_max: 0.50, weight: 24 },
    SpawnEntry { species: "monkey",         mk_moveset: Moveset::panicker,  biome: HabitatTheme::Jungle,   noise_min: 0.00, noise_max: 0.50, weight: 20 },
    SpawnEntry { species: "monkey",         mk_moveset: Moveset::burster,   biome: HabitatTheme::Jungle,   noise_min: 0.35, noise_max: 0.80, weight: 24 },
    SpawnEntry { species: "goldenToucan",   mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Jungle,   noise_min: 0.40, noise_max: 0.85, weight: 14 },
    SpawnEntry { species: "monkey",         mk_moveset: Moveset::aggressor, biome: HabitatTheme::Jungle,   noise_min: 0.60, noise_max: 1.00, weight: 16 },
    SpawnEntry { species: "lion",           mk_moveset: Moveset::aggressor, biome: HabitatTheme::Jungle,   noise_min: 0.68, noise_max: 1.00, weight: 10 },
    SpawnEntry { species: "monkey",         mk_moveset: Moveset::basher,    biome: HabitatTheme::Jungle,   noise_min: 0.55, noise_max: 1.00, weight: 12 },
    SpawnEntry { species: "monkey",         mk_moveset: Moveset::thrower,   biome: HabitatTheme::Jungle,   noise_min: 0.40, noise_max: 1.00, weight: 14 },
    // ── Ocean ─────────────────────────────────────────────────────────────
    SpawnEntry { species: "penguin",        mk_moveset: Moveset::circler,   biome: HabitatTheme::Ocean,    noise_min: 0.00, noise_max: 0.55, weight: 28 },
    SpawnEntry { species: "blue_frog",      mk_moveset: Moveset::panicker,  biome: HabitatTheme::Ocean,    noise_min: 0.00, noise_max: 0.50, weight: 14 },
    SpawnEntry { species: "penguin",        mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Ocean,    noise_min: 0.35, noise_max: 0.80, weight: 20 },
    SpawnEntry { species: "penguin",        mk_moveset: Moveset::burster,   biome: HabitatTheme::Ocean,    noise_min: 0.55, noise_max: 1.00, weight: 14 },
    SpawnEntry { species: "snowyOwl",       mk_moveset: Moveset::aggressor, biome: HabitatTheme::Ocean,    noise_min: 0.70, noise_max: 1.00, weight: 8  },
    // ── Farmland ──────────────────────────────────────────────────────────
    SpawnEntry { species: "field_mouse",    mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Farmland, noise_min: 0.00, noise_max: 0.55, weight: 35 },
    SpawnEntry { species: "giantTortoise",  mk_moveset: Moveset::freezer,   biome: HabitatTheme::Farmland, noise_min: 0.00, noise_max: 0.50, weight: 18 },
    SpawnEntry { species: "field_mouse",    mk_moveset: Moveset::panicker,  biome: HabitatTheme::Farmland, noise_min: 0.30, noise_max: 0.75, weight: 24 },
    SpawnEntry { species: "red_fox",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Farmland, noise_min: 0.45, noise_max: 0.90, weight: 18 },
    SpawnEntry { species: "red_fox",            mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Farmland, noise_min: 0.65, noise_max: 1.00, weight: 14 },
    // ── Desert ────────────────────────────────────────────────────────────
    SpawnEntry { species: "jerboa",          mk_moveset: Moveset::panicker,  biome: HabitatTheme::Desert,   noise_min: 0.00, noise_max: 0.45, weight: 28 },
    SpawnEntry { species: "fennec_fox",      mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Desert,   noise_min: 0.00, noise_max: 0.50, weight: 26 },
    SpawnEntry { species: "desert_hare",     mk_moveset: Moveset::burster,   biome: HabitatTheme::Desert,   noise_min: 0.00, noise_max: 0.50, weight: 22 },
    SpawnEntry { species: "horned_lizard",   mk_moveset: Moveset::freezer,   biome: HabitatTheme::Desert,   noise_min: 0.00, noise_max: 0.50, weight: 16 },
    SpawnEntry { species: "desert_tortoise", mk_moveset: Moveset::freezer,   biome: HabitatTheme::Desert,   noise_min: 0.00, noise_max: 0.50, weight: 12 },
    SpawnEntry { species: "camel",           mk_moveset: Moveset::freezer,   biome: HabitatTheme::Desert,   noise_min: 0.00, noise_max: 0.55, weight: 14 },
    SpawnEntry { species: "scorpion",        mk_moveset: Moveset::venomous,  biome: HabitatTheme::Desert,   noise_min: 0.20, noise_max: 0.70, weight: 16 },
    SpawnEntry { species: "roadrunner",      mk_moveset: Moveset::burster,   biome: HabitatTheme::Desert,   noise_min: 0.30, noise_max: 0.80, weight: 18 },
    SpawnEntry { species: "sand_viper",      mk_moveset: Moveset::venomous,  biome: HabitatTheme::Desert,   noise_min: 0.30, noise_max: 0.85, weight: 14 },
    SpawnEntry { species: "vulture",         mk_moveset: Moveset::circler,   biome: HabitatTheme::Desert,   noise_min: 0.40, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "sidewinder",      mk_moveset: Moveset::venomous,  biome: HabitatTheme::Desert,   noise_min: 0.50, noise_max: 1.00, weight: 12 },
    SpawnEntry { species: "dust_jackal",     mk_moveset: Moveset::aggressor, biome: HabitatTheme::Desert,   noise_min: 0.60, noise_max: 1.00, weight: 12 },
    // ── Tundra ────────────────────────────────────────────────────────────
    SpawnEntry { species: "lemming",         mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Tundra,   noise_min: 0.00, noise_max: 0.45, weight: 28 },
    SpawnEntry { species: "arctic_hare",     mk_moveset: Moveset::panicker,  biome: HabitatTheme::Tundra,   noise_min: 0.00, noise_max: 0.50, weight: 26 },
    SpawnEntry { species: "snow_vole",       mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Tundra,   noise_min: 0.00, noise_max: 0.45, weight: 20 },
    SpawnEntry { species: "snow_bunting",    mk_moveset: Moveset::panicker,  biome: HabitatTheme::Tundra,   noise_min: 0.00, noise_max: 0.50, weight: 18 },
    SpawnEntry { species: "ptarmigan",       mk_moveset: Moveset::freezer,   biome: HabitatTheme::Tundra,   noise_min: 0.00, noise_max: 0.50, weight: 16 },
    SpawnEntry { species: "caribou",         mk_moveset: Moveset::burster,   biome: HabitatTheme::Tundra,   noise_min: 0.10, noise_max: 0.70, weight: 16 },
    SpawnEntry { species: "stoat",           mk_moveset: Moveset::burster,   biome: HabitatTheme::Tundra,   noise_min: 0.20, noise_max: 0.70, weight: 16 },
    SpawnEntry { species: "ermine",          mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Tundra,   noise_min: 0.30, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "snow_fox",        mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Tundra,   noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "musk_ox",         mk_moveset: Moveset::basher,    biome: HabitatTheme::Tundra,   noise_min: 0.20, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "wolverine",       mk_moveset: Moveset::aggressor, biome: HabitatTheme::Tundra,   noise_min: 0.50, noise_max: 1.00, weight: 12 },
    SpawnEntry { species: "tundra_wolf",     mk_moveset: Moveset::basher,    biome: HabitatTheme::Tundra,   noise_min: 0.60, noise_max: 1.00, weight: 12 },
    // ── Taiga ─────────────────────────────────────────────────────────────
    SpawnEntry { species: "chipmunk",        mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Taiga,    noise_min: 0.00, noise_max: 0.45, weight: 28 },
    SpawnEntry { species: "red_squirrel",    mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Taiga,    noise_min: 0.00, noise_max: 0.50, weight: 24 },
    SpawnEntry { species: "crossbill",       mk_moveset: Moveset::panicker,  biome: HabitatTheme::Taiga,    noise_min: 0.00, noise_max: 0.50, weight: 18 },
    SpawnEntry { species: "pine_marten",     mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Taiga,    noise_min: 0.00, noise_max: 0.60, weight: 18 },
    SpawnEntry { species: "capercaillie",    mk_moveset: Moveset::freezer,   biome: HabitatTheme::Taiga,    noise_min: 0.00, noise_max: 0.55, weight: 16 },
    SpawnEntry { species: "elk",             mk_moveset: Moveset::burster,   biome: HabitatTheme::Taiga,    noise_min: 0.10, noise_max: 0.70, weight: 16 },
    SpawnEntry { species: "sable",           mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Taiga,    noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "boreal_owl",      mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Taiga,    noise_min: 0.30, noise_max: 0.85, weight: 14 },
    SpawnEntry { species: "siberian_lynx",   mk_moveset: Moveset::burster,   biome: HabitatTheme::Taiga,    noise_min: 0.40, noise_max: 0.90, weight: 14 },
    SpawnEntry { species: "timber_wolf",     mk_moveset: Moveset::basher,    biome: HabitatTheme::Taiga,    noise_min: 0.60, noise_max: 1.00, weight: 12 },
    SpawnEntry { species: "moose",           mk_moveset: Moveset::aggressor, biome: HabitatTheme::Taiga,    noise_min: 0.50, noise_max: 1.00, weight: 10 },
    SpawnEntry { species: "brown_bear",      mk_moveset: Moveset::basher,    biome: HabitatTheme::Taiga,    noise_min: 0.50, noise_max: 1.00, weight: 10 },
    // ── Volcanic ──────────────────────────────────────────────────────────
    SpawnEntry { species: "ash_beetle",      mk_moveset: Moveset::freezer,   biome: HabitatTheme::Volcanic, noise_min: 0.00, noise_max: 0.50, weight: 24 },
    SpawnEntry { species: "lava_newt",       mk_moveset: Moveset::freezer,   biome: HabitatTheme::Volcanic, noise_min: 0.00, noise_max: 0.55, weight: 20 },
    SpawnEntry { species: "cinder_lizard",   mk_moveset: Moveset::burster,   biome: HabitatTheme::Volcanic, noise_min: 0.20, noise_max: 0.75, weight: 18 },
    SpawnEntry { species: "obsidian_toad",   mk_moveset: Moveset::venomous,  biome: HabitatTheme::Volcanic, noise_min: 0.20, noise_max: 0.80, weight: 16 },
    SpawnEntry { species: "ember_moth",      mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Volcanic, noise_min: 0.20, noise_max: 0.80, weight: 16 },
    SpawnEntry { species: "magma_crab",      mk_moveset: Moveset::basher,    biome: HabitatTheme::Volcanic, noise_min: 0.30, noise_max: 0.85, weight: 14 },
    SpawnEntry { species: "fire_salamander", mk_moveset: Moveset::venomous,  biome: HabitatTheme::Volcanic, noise_min: 0.30, noise_max: 0.90, weight: 14 },
    SpawnEntry { species: "ashen_vulture",   mk_moveset: Moveset::circler,   biome: HabitatTheme::Volcanic, noise_min: 0.40, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "rock_python",     mk_moveset: Moveset::aggressor, biome: HabitatTheme::Volcanic, noise_min: 0.40, noise_max: 0.95, weight: 12 },
    SpawnEntry { species: "sulfur_serpent",  mk_moveset: Moveset::venomous,  biome: HabitatTheme::Volcanic, noise_min: 0.50, noise_max: 1.00, weight: 12 },
    SpawnEntry { species: "magma_hound",     mk_moveset: Moveset::aggressor, biome: HabitatTheme::Volcanic, noise_min: 0.60, noise_max: 1.00, weight: 10 },
    SpawnEntry { species: "cinder_drake",    mk_moveset: Moveset::thrower,   biome: HabitatTheme::Volcanic, noise_min: 0.70, noise_max: 1.00, weight: 8  },
    // ── Badlands ──────────────────────────────────────────────────────────
    SpawnEntry { species: "prairie_dog",     mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Badlands, noise_min: 0.00, noise_max: 0.45, weight: 28 },
    SpawnEntry { species: "jackrabbit",      mk_moveset: Moveset::panicker,  biome: HabitatTheme::Badlands, noise_min: 0.00, noise_max: 0.50, weight: 26 },
    SpawnEntry { species: "gila_woodpecker", mk_moveset: Moveset::panicker,  biome: HabitatTheme::Badlands, noise_min: 0.00, noise_max: 0.50, weight: 18 },
    SpawnEntry { species: "horned_toad",     mk_moveset: Moveset::freezer,   biome: HabitatTheme::Badlands, noise_min: 0.00, noise_max: 0.50, weight: 16 },
    SpawnEntry { species: "armadillo",       mk_moveset: Moveset::freezer,   biome: HabitatTheme::Badlands, noise_min: 0.00, noise_max: 0.55, weight: 16 },
    SpawnEntry { species: "rattlesnake",     mk_moveset: Moveset::venomous,  biome: HabitatTheme::Badlands, noise_min: 0.30, noise_max: 0.85, weight: 16 },
    SpawnEntry { species: "kit_fox",         mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Badlands, noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "coyote",          mk_moveset: Moveset::aggressor, biome: HabitatTheme::Badlands, noise_min: 0.40, noise_max: 0.95, weight: 14 },
    SpawnEntry { species: "turkey_vulture",  mk_moveset: Moveset::circler,   biome: HabitatTheme::Badlands, noise_min: 0.40, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "bighorn_sheep",   mk_moveset: Moveset::basher,    biome: HabitatTheme::Badlands, noise_min: 0.30, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "cougar",          mk_moveset: Moveset::burster,   biome: HabitatTheme::Badlands, noise_min: 0.60, noise_max: 1.00, weight: 10 },
    SpawnEntry { species: "bison",           mk_moveset: Moveset::aggressor, biome: HabitatTheme::Badlands, noise_min: 0.50, noise_max: 1.00, weight: 10 },
    // ── Beach ─────────────────────────────────────────────────────────────
    SpawnEntry { species: "sandpiper",       mk_moveset: Moveset::panicker,  biome: HabitatTheme::Beach,    noise_min: 0.00, noise_max: 0.50, weight: 26 },
    SpawnEntry { species: "hermit_crab",     mk_moveset: Moveset::freezer,   biome: HabitatTheme::Beach,    noise_min: 0.00, noise_max: 0.50, weight: 22 },
    SpawnEntry { species: "fiddler_crab",    mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Beach,    noise_min: 0.00, noise_max: 0.50, weight: 20 },
    SpawnEntry { species: "sanderling",      mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Beach,    noise_min: 0.00, noise_max: 0.50, weight: 18 },
    SpawnEntry { species: "seagull",         mk_moveset: Moveset::circler,   biome: HabitatTheme::Beach,    noise_min: 0.00, noise_max: 0.60, weight: 18 },
    SpawnEntry { species: "ghost_crab",      mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Beach,    noise_min: 0.20, noise_max: 0.70, weight: 16 },
    SpawnEntry { species: "horseshoe_crab",  mk_moveset: Moveset::freezer,   biome: HabitatTheme::Beach,    noise_min: 0.00, noise_max: 0.50, weight: 14 },
    SpawnEntry { species: "sea_turtle",      mk_moveset: Moveset::freezer,   biome: HabitatTheme::Beach,    noise_min: 0.00, noise_max: 0.50, weight: 12 },
    SpawnEntry { species: "pelican",         mk_moveset: Moveset::burster,   biome: HabitatTheme::Beach,    noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "osprey",          mk_moveset: Moveset::burster,   biome: HabitatTheme::Beach,    noise_min: 0.40, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "sea_lion",        mk_moveset: Moveset::basher,    biome: HabitatTheme::Beach,    noise_min: 0.30, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "coastal_jackal",  mk_moveset: Moveset::aggressor, biome: HabitatTheme::Beach,    noise_min: 0.60, noise_max: 1.00, weight: 10 },
    // ── Highlands ─────────────────────────────────────────────────────────
    SpawnEntry { species: "pika",            mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Highlands, noise_min: 0.00, noise_max: 0.45, weight: 28 },
    SpawnEntry { species: "marmot",          mk_moveset: Moveset::freezer,   biome: HabitatTheme::Highlands, noise_min: 0.00, noise_max: 0.50, weight: 22 },
    SpawnEntry { species: "alpine_hare",     mk_moveset: Moveset::panicker,  biome: HabitatTheme::Highlands, noise_min: 0.00, noise_max: 0.50, weight: 22 },
    SpawnEntry { species: "rock_ptarmigan",  mk_moveset: Moveset::freezer,   biome: HabitatTheme::Highlands, noise_min: 0.00, noise_max: 0.55, weight: 16 },
    SpawnEntry { species: "mountain_goat",   mk_moveset: Moveset::burster,   biome: HabitatTheme::Highlands, noise_min: 0.10, noise_max: 0.70, weight: 16 },
    SpawnEntry { species: "chamois",         mk_moveset: Moveset::burster,   biome: HabitatTheme::Highlands, noise_min: 0.20, noise_max: 0.80, weight: 16 },
    SpawnEntry { species: "ibex",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Highlands, noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "condor",          mk_moveset: Moveset::circler,   biome: HabitatTheme::Highlands, noise_min: 0.40, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "golden_eagle",    mk_moveset: Moveset::circler,   biome: HabitatTheme::Highlands, noise_min: 0.40, noise_max: 0.95, weight: 12 },
    SpawnEntry { species: "yak",             mk_moveset: Moveset::basher,    biome: HabitatTheme::Highlands, noise_min: 0.30, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "highland_wolf",   mk_moveset: Moveset::aggressor, biome: HabitatTheme::Highlands, noise_min: 0.60, noise_max: 1.00, weight: 10 },
    SpawnEntry { species: "snow_leopard",    mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Highlands, noise_min: 0.60, noise_max: 1.00, weight: 10 },
    // ── Mythical ──────────────────────────────────────────────────────────
    SpawnEntry { species: "gnome",           mk_moveset: Moveset::freezer,   biome: HabitatTheme::Mythical, noise_min: 0.00, noise_max: 0.55, weight: 20 },
    SpawnEntry { species: "jackalope",       mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Mythical, noise_min: 0.00, noise_max: 0.60, weight: 20 },
    SpawnEntry { species: "pixie",           mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Mythical, noise_min: 0.00, noise_max: 0.60, weight: 20 },
    SpawnEntry { species: "faun",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Mythical, noise_min: 0.00, noise_max: 0.70, weight: 16 },
    SpawnEntry { species: "will_o_wisp",     mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Mythical, noise_min: 0.20, noise_max: 0.80, weight: 16 },
    SpawnEntry { species: "griffon_chick",   mk_moveset: Moveset::circler,   biome: HabitatTheme::Mythical, noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "kelpie",          mk_moveset: Moveset::aggressor, biome: HabitatTheme::Mythical, noise_min: 0.30, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "unicorn_foal",    mk_moveset: Moveset::burster,   biome: HabitatTheme::Mythical, noise_min: 0.30, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "sprite_stag",     mk_moveset: Moveset::burster,   biome: HabitatTheme::Mythical, noise_min: 0.40, noise_max: 0.95, weight: 12 },
    SpawnEntry { species: "phoenix_chick",   mk_moveset: Moveset::burster,   biome: HabitatTheme::Mythical, noise_min: 0.50, noise_max: 1.00, weight: 10 },
    SpawnEntry { species: "basilisk",        mk_moveset: Moveset::venomous,  biome: HabitatTheme::Mythical, noise_min: 0.60, noise_max: 1.00, weight: 8  },
    SpawnEntry { species: "wyvern",          mk_moveset: Moveset::thrower,   biome: HabitatTheme::Mythical, noise_min: 0.60, noise_max: 1.00, weight: 8  },
    // ── Void ──────────────────────────────────────────────────────────────
    SpawnEntry { species: "gloom_bat",       mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Void,     noise_min: 0.00, noise_max: 0.50, weight: 22 },
    SpawnEntry { species: "void_moth",       mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Void,     noise_min: 0.00, noise_max: 0.55, weight: 20 },
    SpawnEntry { species: "shade_wisp",      mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Void,     noise_min: 0.00, noise_max: 0.60, weight: 18 },
    SpawnEntry { species: "cosmic_jelly",    mk_moveset: Moveset::freezer,   biome: HabitatTheme::Void,     noise_min: 0.00, noise_max: 0.60, weight: 18 },
    SpawnEntry { species: "null_crawler",    mk_moveset: Moveset::freezer,   biome: HabitatTheme::Void,     noise_min: 0.20, noise_max: 0.80, weight: 16 },
    SpawnEntry { species: "dusk_raven",      mk_moveset: Moveset::circler,   biome: HabitatTheme::Void,     noise_min: 0.20, noise_max: 0.80, weight: 16 },
    SpawnEntry { species: "phantom_stag",    mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Void,     noise_min: 0.30, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "nightmare_foal",  mk_moveset: Moveset::burster,   biome: HabitatTheme::Void,     noise_min: 0.40, noise_max: 0.95, weight: 12 },
    SpawnEntry { species: "eclipse_hound",   mk_moveset: Moveset::aggressor, biome: HabitatTheme::Void,     noise_min: 0.50, noise_max: 1.00, weight: 12 },
    SpawnEntry { species: "abyss_serpent",   mk_moveset: Moveset::venomous,  biome: HabitatTheme::Void,     noise_min: 0.60, noise_max: 1.00, weight: 10 },
    SpawnEntry { species: "star_eater",      mk_moveset: Moveset::aggressor, biome: HabitatTheme::Void,     noise_min: 0.60, noise_max: 1.00, weight: 8  },
    SpawnEntry { species: "singularity_wyrm", mk_moveset: Moveset::thrower,  biome: HabitatTheme::Void,     noise_min: 0.70, noise_max: 1.00, weight: 8  },
    // ── Festive ───────────────────────────────────────────────────────────
    SpawnEntry { species: "peppermint_hare", mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Festive,  noise_min: 0.00, noise_max: 0.50, weight: 24 },
    SpawnEntry { species: "candy_cardinal",  mk_moveset: Moveset::panicker,  biome: HabitatTheme::Festive,  noise_min: 0.00, noise_max: 0.50, weight: 22 },
    SpawnEntry { species: "cocoa_pup",       mk_moveset: Moveset::panicker,  biome: HabitatTheme::Festive,  noise_min: 0.00, noise_max: 0.55, weight: 20 },
    SpawnEntry { species: "jingle_fox",      mk_moveset: Moveset::burster,   biome: HabitatTheme::Festive,  noise_min: 0.00, noise_max: 0.60, weight: 18 },
    SpawnEntry { species: "gift_goose",      mk_moveset: Moveset::circler,   biome: HabitatTheme::Festive,  noise_min: 0.00, noise_max: 0.60, weight: 16 },
    SpawnEntry { species: "reindeer_calf",   mk_moveset: Moveset::burster,   biome: HabitatTheme::Festive,  noise_min: 0.10, noise_max: 0.70, weight: 16 },
    SpawnEntry { species: "starlight_dove",  mk_moveset: Moveset::circler,   biome: HabitatTheme::Festive,  noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "tinsel_cat",      mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Festive,  noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "sugarplum_doe",   mk_moveset: Moveset::burster,   biome: HabitatTheme::Festive,  noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "garland_owl",     mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Festive,  noise_min: 0.30, noise_max: 0.85, weight: 14 },
    SpawnEntry { species: "frostbell_stag",  mk_moveset: Moveset::basher,    biome: HabitatTheme::Festive,  noise_min: 0.30, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "sleigh_hound",    mk_moveset: Moveset::aggressor, biome: HabitatTheme::Festive,  noise_min: 0.60, noise_max: 1.00, weight: 10 },
    // ── Food ──────────────────────────────────────────────────────────────
    SpawnEntry { species: "muffin_mouse",    mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Food,     noise_min: 0.00, noise_max: 0.45, weight: 28 },
    SpawnEntry { species: "cheddar_rat",     mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Food,     noise_min: 0.00, noise_max: 0.45, weight: 24 },
    SpawnEntry { species: "berry_finch",     mk_moveset: Moveset::panicker,  biome: HabitatTheme::Food,     noise_min: 0.00, noise_max: 0.50, weight: 20 },
    SpawnEntry { species: "popcorn_quail",   mk_moveset: Moveset::panicker,  biome: HabitatTheme::Food,     noise_min: 0.00, noise_max: 0.50, weight: 18 },
    SpawnEntry { species: "jelly_slug",      mk_moveset: Moveset::freezer,   biome: HabitatTheme::Food,     noise_min: 0.00, noise_max: 0.55, weight: 18 },
    SpawnEntry { species: "cookie_crab",     mk_moveset: Moveset::freezer,   biome: HabitatTheme::Food,     noise_min: 0.00, noise_max: 0.55, weight: 16 },
    SpawnEntry { species: "marshmallow_lamb", mk_moveset: Moveset::freezer,  biome: HabitatTheme::Food,     noise_min: 0.00, noise_max: 0.55, weight: 16 },
    SpawnEntry { species: "pancake_turtle",  mk_moveset: Moveset::freezer,   biome: HabitatTheme::Food,     noise_min: 0.00, noise_max: 0.50, weight: 12 },
    SpawnEntry { species: "donut_seal",      mk_moveset: Moveset::basher,    biome: HabitatTheme::Food,     noise_min: 0.20, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "noodle_serpent",  mk_moveset: Moveset::venomous,  biome: HabitatTheme::Food,     noise_min: 0.30, noise_max: 0.85, weight: 14 },
    SpawnEntry { species: "caramel_stag",    mk_moveset: Moveset::burster,   biome: HabitatTheme::Food,     noise_min: 0.30, noise_max: 0.90, weight: 12 },
    SpawnEntry { species: "honey_badger",    mk_moveset: Moveset::aggressor, biome: HabitatTheme::Food,     noise_min: 0.50, noise_max: 1.00, weight: 12 },
];

/// Weight assigned to "no spawn". World-level sparsity is now controlled by
/// the per-chunk encounter gate in `world_chunks`, so this stays low — once a
/// chunk has rolled an encounter, an animal should reliably appear. A little
/// weight here still adds biome/noise variety to which candidate slots fill.
const NO_SPAWN_WEIGHT: u32 = 25;

/// Attempt to pick a species for a candidate position.
/// Returns `None` when the weighted roll lands on "no spawn" or when no
/// table entry matches the biome + noise combination.
pub fn weighted_spawn(
    biome:     HabitatTheme,
    noise:     f32,
    rng:       &mut LcgRng,
) -> Option<(&'static str, fn() -> Moveset)> {
    let eligible: Vec<&SpawnEntry> = SPAWN_TABLE
        .iter()
        .filter(|e| e.biome == biome && noise >= e.noise_min && noise <= e.noise_max)
        .collect();

    if eligible.is_empty() {
        return None;
    }

    let spawn_weight: u32 = eligible.iter().map(|e| e.weight).sum();
    let total = spawn_weight + NO_SPAWN_WEIGHT;
    let roll = (rng.next_u64() % total as u64) as u32;

    if roll >= spawn_weight {
        return None; // "no spawn" slot
    }

    let mut acc = 0u32;
    for e in &eligible {
        acc += e.weight;
        if roll < acc {
            return Some((e.species, e.mk_moveset));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The seeded climate biomes should produce a varied distribution across the
    /// full 500k world, not one giant biome.
    #[test]
    fn biomes_are_varied_across_world() {
        let seed = 0xC0FFEE;
        let mut seen: HashSet<HabitatTheme> = HashSet::new();
        let n = 60;
        let span = crate::game::world_chunks::WORLD_W;
        for i in 0..n {
            for j in 0..n {
                let x = (i as f32 / n as f32) * span;
                let y = (j as f32 / n as f32) * span;
                seen.insert(biome_at(vec2(x, y), seed));
            }
        }
        assert!(seen.len() >= 4, "expected >=4 biomes across world, got {}", seen.len());
    }

    /// Biome layout must be deterministic for a seed and differ between seeds.
    #[test]
    fn biome_layout_depends_on_seed() {
        let p = vec2(123_456.0, 78_910.0);
        assert_eq!(biome_at(p, 42), biome_at(p, 42));
        let mut differs = false;
        for k in 0..200 {
            let q = vec2(k as f32 * 2500.0, k as f32 * 1700.0);
            if biome_at(q, 1) != biome_at(q, 999) {
                differs = true;
                break;
            }
        }
        assert!(differs, "biome layout identical across seeds");
    }

    /// Two different seeds should now disagree on the biome at a large majority
    /// of points — the seeded lattice hash decorrelates layouts (the fix for
    /// "reseeding gives near-identical blobs").
    #[test]
    fn seeds_are_strongly_decorrelated() {
        let span = crate::game::world_chunks::WORLD_W;
        let n = 40;
        let (mut total, mut differ) = (0, 0);
        for i in 0..n {
            for j in 0..n {
                let p = vec2((i as f32 / n as f32) * span, (j as f32 / n as f32) * span);
                total += 1;
                if biome_at(p, 0xAAAA) != biome_at(p, 0x5555) {
                    differ += 1;
                }
            }
        }
        // Expect well over half the sampled points to differ between seeds.
        assert!(
            differ * 2 > total,
            "seeds too correlated: only {differ}/{total} points differ"
        );
    }

    /// The rare fantastical biomes should appear somewhere across the world.
    #[test]
    fn special_biomes_appear() {
        use HabitatTheme::*;
        let seed = 0xC0FFEE;
        let span = crate::game::world_chunks::WORLD_W;
        let n = 200;
        let mut seen_special = false;
        for i in 0..n {
            for j in 0..n {
                let p = vec2((i as f32 / n as f32) * span, (j as f32 / n as f32) * span);
                if matches!(biome_at(p, seed), Mythical | Void | Festive | Food) {
                    seen_special = true;
                    break;
                }
            }
            if seen_special {
                break;
            }
        }
        assert!(seen_special, "no fantastical biome found across the world");
    }

    /// Every species named in the spawn table must exist in the species
    /// catalog (guards against typos in the large new-biome rosters), and each
    /// new biome must field at least 10 distinct species.
    #[test]
    fn spawn_table_species_exist_and_biomes_are_stocked() {
        use crate::game::species;
        use std::collections::HashSet;
        for e in SPAWN_TABLE {
            assert!(
                species::try_get(e.species).is_some(),
                "spawn-table species {:?} missing from catalog",
                e.species
            );
        }
        let new_biomes = [
            HabitatTheme::Desert, HabitatTheme::Tundra, HabitatTheme::Taiga,
            HabitatTheme::Volcanic, HabitatTheme::Badlands, HabitatTheme::Beach,
            HabitatTheme::Highlands, HabitatTheme::Mythical, HabitatTheme::Void,
            HabitatTheme::Festive, HabitatTheme::Food,
        ];
        for b in new_biomes {
            let count = SPAWN_TABLE
                .iter()
                .filter(|e| e.biome == b)
                .map(|e| e.species)
                .collect::<HashSet<_>>()
                .len();
            assert!(count >= 10, "{} has only {count} species (<10)", b.name());
        }
    }

    /// Tile colouring must be deterministic for a fixed (pos, seed).
    #[test]
    fn tile_color_is_deterministic() {
        let p = vec2(54_321.0, 12_345.0);
        let a = biome_tile_color(p, 7);
        let b = biome_tile_color(p, 7);
        assert_eq!((a.r, a.g, a.b), (b.r, b.g, b.b));
    }
}

