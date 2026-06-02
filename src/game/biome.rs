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

/// Feature size (world units) of biome regions — large so each biome spans many
/// chunks.
const BIOME_SCALE: f32 = 42_000.0;
/// Domain-warp feature size and strength (world units) — bends biome borders so
/// they read as natural coastlines/treelines rather than smooth blobs.
const WARP_SCALE: f32 = 16_000.0;
const WARP_AMOUNT: f32 = 6_000.0;

/// A large pseudo-random offset into noise space, derived from the world seed
/// and a channel id, so each world (and each climate field) samples a different
/// region of the noise function.
fn seed_offset(seed: u64, channel: u64) -> Vec2 {
    let mut r = LcgRng::new(seed ^ channel.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    vec2(
        (r.next_f32() * 2.0 - 1.0) * 100_000.0,
        (r.next_f32() * 2.0 - 1.0) * 100_000.0,
    )
}

/// Continuous (temperature, moisture) climate at `pos` in [0, 1]², with domain
/// warp applied. Deterministic for a given `seed`.
fn climate_at(pos: Vec2, seed: u64) -> (f32, f32) {
    let wo = seed_offset(seed, 7);
    let wx = noise2d(pos.x + wo.x, pos.y + wo.y, WARP_SCALE) - 0.5;
    let wy = noise2d(pos.x + wo.x + 4096.0, pos.y + wo.y - 4096.0, WARP_SCALE) - 0.5;
    let warped = vec2(pos.x + wx * WARP_AMOUNT, pos.y + wy * WARP_AMOUNT);

    let to = seed_offset(seed, 1);
    let mo = seed_offset(seed, 2);
    let temp = noise2d(warped.x + to.x, warped.y + to.y, BIOME_SCALE);
    let moist = noise2d(warped.x + mo.x, warped.y + mo.y, BIOME_SCALE);
    (temp, moist)
}

/// Map a (temperature, moisture) pair to a biome. Thresholds are tuned so every
/// theme appears with a reasonable share of the world.
fn classify(temp: f32, moist: f32) -> HabitatTheme {
    use HabitatTheme::*;
    if temp < 0.28 {
        return Arctic;
    }
    if moist > 0.80 {
        return Ocean;
    }
    if temp > 0.72 {
        return if moist > 0.50 { Jungle } else { Savanna };
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

/// Hard biome at `pos` for the given world `seed`.
pub fn biome_at(pos: Vec2, seed: u64) -> HabitatTheme {
    let (t, m) = climate_at(pos, seed);
    classify(t, m)
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
        HabitatTheme::Forest   => Color::new(0.275, 0.451, 0.267, 1.0), // dark forest green
        HabitatTheme::Arctic   => Color::new(0.765, 0.855, 0.941, 1.0), // icy pale blue
        HabitatTheme::Savanna  => Color::new(0.686, 0.627, 0.373, 1.0), // golden tan
        HabitatTheme::Wetland  => Color::new(0.294, 0.451, 0.333, 1.0), // murky olive
        HabitatTheme::Jungle   => Color::new(0.149, 0.373, 0.216, 1.0), // deep tropical
        HabitatTheme::Ocean    => Color::new(0.216, 0.373, 0.608, 1.0), // cool blue
        HabitatTheme::Farmland => Color::new(0.608, 0.686, 0.412, 1.0), // light field
    }
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
    SpawnEntry { species: "fox",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Forest,   noise_min: 0.30, noise_max: 0.75, weight: 22 },
    SpawnEntry { species: "raccoon",        mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Forest,   noise_min: 0.30, noise_max: 0.78, weight: 18 },
    SpawnEntry { species: "snowyOwl",       mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Forest,   noise_min: 0.35, noise_max: 0.80, weight: 14 },
    SpawnEntry { species: "badger",         mk_moveset: Moveset::aggressor, biome: HabitatTheme::Forest,   noise_min: 0.40, noise_max: 0.85, weight: 14 },
    SpawnEntry { species: "lynx",           mk_moveset: Moveset::burster,   biome: HabitatTheme::Forest,   noise_min: 0.45, noise_max: 0.90, weight: 14 },
    SpawnEntry { species: "treeFrog",       mk_moveset: Moveset::venomous,  biome: HabitatTheme::Forest,   noise_min: 0.45, noise_max: 1.00, weight: 12 },
    SpawnEntry { species: "fox",            mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Forest,   noise_min: 0.55, noise_max: 1.00, weight: 16 },
    // High noise: the dangerous, pack-feeling edge of the forest.
    SpawnEntry { species: "boar",           mk_moveset: Moveset::aggressor, biome: HabitatTheme::Forest,   noise_min: 0.60, noise_max: 1.00, weight: 14 },
    SpawnEntry { species: "wolf",           mk_moveset: Moveset::basher,    biome: HabitatTheme::Forest,   noise_min: 0.65, noise_max: 1.00, weight: 14 },
    SpawnEntry { species: "fox",            mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Forest,   noise_min: 0.70, noise_max: 1.00, weight: 10 },
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
    SpawnEntry { species: "fox",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Savanna,  noise_min: 0.35, noise_max: 0.80, weight: 18 },
    SpawnEntry { species: "lion",           mk_moveset: Moveset::aggressor, biome: HabitatTheme::Savanna,  noise_min: 0.45, noise_max: 1.00, weight: 28 },
    SpawnEntry { species: "lion",           mk_moveset: Moveset::circler,   biome: HabitatTheme::Savanna,  noise_min: 0.70, noise_max: 1.00, weight: 14 },
    SpawnEntry { species: "lion",           mk_moveset: Moveset::basher,    biome: HabitatTheme::Savanna,  noise_min: 0.60, noise_max: 1.00, weight: 12 },
    // ── Wetland ───────────────────────────────────────────────────────────
    SpawnEntry { species: "blue_frog",      mk_moveset: Moveset::panicker,  biome: HabitatTheme::Wetland,  noise_min: 0.00, noise_max: 0.55, weight: 30 },
    SpawnEntry { species: "treeFrog",       mk_moveset: Moveset::freezer,   biome: HabitatTheme::Wetland,  noise_min: 0.00, noise_max: 0.50, weight: 24 },
    SpawnEntry { species: "treeFrog",       mk_moveset: Moveset::circler,   biome: HabitatTheme::Wetland,  noise_min: 0.30, noise_max: 0.75, weight: 18 },
    SpawnEntry { species: "snowyOwl",       mk_moveset: Moveset::vanisher,  biome: HabitatTheme::Wetland,  noise_min: 0.45, noise_max: 0.90, weight: 14 },
    SpawnEntry { species: "fox",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Wetland,  noise_min: 0.60, noise_max: 1.00, weight: 10 },
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
    SpawnEntry { species: "fox",            mk_moveset: Moveset::burster,   biome: HabitatTheme::Farmland, noise_min: 0.45, noise_max: 0.90, weight: 18 },
    SpawnEntry { species: "fox",            mk_moveset: Moveset::zigzagger, biome: HabitatTheme::Farmland, noise_min: 0.65, noise_max: 1.00, weight: 14 },
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
}

