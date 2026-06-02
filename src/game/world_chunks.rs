//! Chunk-based world streaming (§33 of the design doc).
//!
//! The world is divided into fixed-size chunks.  As the player moves, chunks
//! entering the **load radius** are spawned (animals placed, state created);
//! chunks passing the **cull radius** are deactivated but their data is kept
//! in a `HashMap` so re-entry is instant.  Only active chunks receive AI ticks
//! and appear in the rendering pass.
//!
//! World coordinates: continuous `f32` on `[0, WORLD_W] × [0, WORLD_H]`.
//! Chunk coordinates: integer grid derived by flooring world / CHUNK_SIZE.

use std::collections::{HashMap, HashSet};

use macroquad::math::{Vec2, vec2};

use crate::game::biome::{self, LcgRng};
use crate::game::wild_animal::{ALERT_RADIUS, WildAnimal};

// ── World & chunk dimensions ──────────────────────────────────────────────────

pub const WORLD_W: f32 = 8192.0;
pub const WORLD_H: f32 = 8192.0;
/// Square chunk edge length in world units.
pub const CHUNK_SIZE: f32 = 512.0;
/// Number of chunk columns in the world grid.
pub const CHUNKS_COLS: i32 = (WORLD_W / CHUNK_SIZE) as i32; // 16
/// Number of chunk rows in the world grid.
pub const CHUNKS_ROWS: i32 = (WORLD_H / CHUNK_SIZE) as i32; // 16

// ── Streaming radii (Chebyshev / "chessboard king" distance) ─────────────────

/// Chunks within this Chebyshev distance from the player are actively loaded.
/// Load area = (2·LOAD_RADIUS + 1)² = 25 chunks.
pub const LOAD_RADIUS: i32 = 2;
/// Chunks beyond this distance are deactivated (but data is cached).
/// The gap between LOAD and CULL creates hysteresis to avoid thrashing.
pub const CULL_RADIUS: i32 = 3;

// ── Zoo plot geometry ─────────────────────────────────────────────────────────

/// The player's home zoo is an enclosed square plot of this many tiles per side,
/// centred on the world. Wild animals never spawn inside it (+ a buffer), and
/// tame animals stay within it.
pub const ZOO_TILES: i32 = 9;
/// World units per tile — must match `avatar_system::TILE_W` and the render grid.
pub const ZOO_TILE_W: f32 = 128.0;

/// World-space centre of the zoo plot.
pub fn zoo_center() -> Vec2 {
    vec2(WORLD_W * 0.5, WORLD_H * 0.5)
}

/// Half the zoo plot's edge length, in world units (so the plot spans
/// `center ± zoo_half_extent` on each axis).
pub fn zoo_half_extent() -> f32 {
    ZOO_TILES as f32 * ZOO_TILE_W * 0.5
}

/// True when `pos` lies inside the zoo plot plus a no-spawn buffer beyond the
/// fence — used to keep wild encounters out of (and slightly away from) home.
fn in_zoo_exclusion(pos: Vec2) -> bool {
    let c = zoo_center();
    let half = zoo_half_extent() + ZOO_SPAWN_BUFFER;
    (pos.x - c.x).abs() <= half && (pos.y - c.y).abs() <= half
}

/// Extra margin beyond the fence kept clear of wild spawns.
const ZOO_SPAWN_BUFFER: f32 = 200.0;

/// Push a position out of the zoo plot if it has crossed the fence, sliding it
/// to the nearest fence edge. Wild animals call this every frame so they can
/// never enter the player's home plot — not even while chasing the cursor.
/// Unlike spawning, no buffer is applied: animals may roam right up to the fence.
pub fn resolve_zoo_collision(pos: Vec2) -> Vec2 {
    let c = zoo_center();
    let half = zoo_half_extent();
    let dx = pos.x - c.x;
    let dy = pos.y - c.y;
    // Outside the plot on at least one axis → no collision.
    if dx.abs() >= half || dy.abs() >= half {
        return pos;
    }
    // Inside: eject along the axis of least penetration to the nearest edge.
    let pen_x = half - dx.abs();
    let pen_y = half - dy.abs();
    let mut out = pos;
    if pen_x <= pen_y {
        out.x = c.x + half * dx.signum();
    } else {
        out.y = c.y + half * dy.signum();
    }
    out
}

// ── Animal spawning ───────────────────────────────────────────────────────────
//
// Encounters are intentionally rare so each one feels special. Instead of
// filling every chunk to a cap, each chunk first rolls an *encounter gate*:
// most chunks come up empty. Across the ~25 active chunks this yields only a
// handful of animals on screen at once.

/// Base probability that a chunk contains any wild animals at all.
const ENCOUNTER_BASE: f32 = 0.12;
/// Additional encounter probability contributed at maximum noise (busy zones).
const ENCOUNTER_NOISE_BONUS: f32 = 0.16;
/// Noise threshold above which an encounter may become a small pack.
const PACK_NOISE: f32 = 0.70;
/// Chance, within a high-noise encounter, of spawning a second animal (a pack).
const PACK_CHANCE: f32 = 0.35;

/// Minimum separation (world units) between Poisson-disk candidates.
const POISSON_MIN_DIST: f32 = 90.0;

/// Perlin noise feature scale (world units) used for spawn-table noise sampling.
const NOISE_SCALE: f32 = 950.0;

/// Wild animals won't spawn closer than this to the player's current position.
const MIN_SPAWN_DIST: f32 = ALERT_RADIUS * 3.0;

// ── Chunk data ────────────────────────────────────────────────────────────────

pub struct ChunkData {
    pub animals: Vec<WildAnimal>,
    /// True once this chunk has been visited at least once.
    pub discovered: bool,
}

// ── WorldChunks ───────────────────────────────────────────────────────────────

pub struct WorldChunks {
    /// Persistent chunk storage — includes active and cached chunks. Animals
    /// are always stored under the chunk they are *currently* standing in
    /// (see `migrate_animals`), never their spawn chunk.
    pub data: HashMap<(i32, i32), ChunkData>,
    /// The subset of `data` keys currently within the load radius.
    active: HashSet<(i32, i32)>,
    /// Chunks whose one-time spawn roll has already run. Tracked separately
    /// from `data` so that migrating an animal *into* a not-yet-visited chunk
    /// (which creates a `data` entry) doesn't suppress that chunk's own future
    /// spawn — and so re-entering a chunk never double-spawns.
    spawned: HashSet<(i32, i32)>,
}

impl WorldChunks {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            active: HashSet::new(),
            spawned: HashSet::new(),
        }
    }

    // ── Streaming ─────────────────────────────────────────────────────────────

    /// Call once per frame with the player's world position.
    /// Loads newly-in-range chunks (spawning animals on first visit) and
    /// deactivates chunks that have drifted outside the cull radius.
    pub fn update(&mut self, player_pos: Vec2) {
        let pc = world_to_chunk(player_pos);

        // Re-home moved animals into whatever chunk they now physically occupy.
        // This must run *before* culling so an animal that has wandered/fled out
        // of its origin chunk is relocated to its real chunk — guaranteeing it
        // never disappears merely because its origin chunk drifts off-screen.
        self.migrate_animals();

        // Deactivate chunks outside the cull radius.
        let prev: Vec<(i32, i32)> = self.active.iter().copied().collect();
        for coord in prev {
            if chebyshev(coord, pc) > CULL_RADIUS {
                self.active.remove(&coord);
                // ChunkData stays in self.data (cached for instant re-entry).
            }
        }

        // Activate / spawn chunks inside the load radius.
        for dy in -LOAD_RADIUS..=LOAD_RADIUS {
            for dx in -LOAD_RADIUS..=LOAD_RADIUS {
                let coord = (pc.0 + dx, pc.1 + dy);
                if !chunk_in_bounds(coord) { continue; }
                if chebyshev(coord, pc) > LOAD_RADIUS { continue; }

                self.active.insert(coord);

                // First spawn-roll for this chunk → place animals, avoiding the
                // player. Gated by `spawned` (not `data`) so that an animal
                // migrating *into* a never-visited chunk doesn't suppress its
                // own future spawn, and re-entry never double-spawns.
                if !self.spawned.contains(&coord) {
                    self.spawned.insert(coord);
                    let animals = spawn_chunk_animals(coord, player_pos);
                    self.data
                        .entry(coord)
                        .or_insert_with(|| ChunkData { animals: Vec::new(), discovered: true })
                        .animals
                        .extend(animals);
                }
            }
        }
    }

    /// Relocate every animal that has moved out of the chunk it is stored under
    /// into the chunk it now physically occupies, creating the destination
    /// `ChunkData` if needed. Animals are never dropped — only moved — so an
    /// animal can never vanish while inside any loaded chunk.
    fn migrate_animals(&mut self) {
        let mut moved: Vec<((i32, i32), WildAnimal)> = Vec::new();
        for (&coord, chunk) in self.data.iter_mut() {
            let mut i = 0;
            while i < chunk.animals.len() {
                let actual = world_to_chunk(chunk.animals[i].pos);
                if actual != coord {
                    moved.push((actual, chunk.animals.swap_remove(i)));
                } else {
                    i += 1;
                }
            }
        }
        for (dest, animal) in moved {
            self.data
                .entry(dest)
                .or_insert_with(|| ChunkData { animals: Vec::new(), discovered: true })
                .animals
                .push(animal);
        }
    }

    // ── AI tick ───────────────────────────────────────────────────────────────

    /// Advance wild-animal AI for every animal in an active chunk.
    /// Returns `true` if any Basher connected a charge with the player this
    /// frame (so the caller can trigger hitstop / camera shake).
    pub fn update_animal_ai(
        &mut self,
        dt: f32,
        cursor_world: Vec2,
        player_pos: Vec2,
        catch_mode: bool,
    ) -> bool {
        let mut bashed = false;
        for coord in &self.active {
            if let Some(chunk) = self.data.get_mut(coord) {
                for animal in &mut chunk.animals {
                    if animal.update(dt, cursor_world, player_pos, catch_mode) {
                        bashed = true;
                    }
                }
            }
        }
        bashed
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// All non-hidden animals in active chunks.  Used by the catch system.
    pub fn active_animals(&self) -> Vec<&WildAnimal> {
        self.active
            .iter()
            .filter_map(|cc| self.data.get(cc))
            .flat_map(|cd| cd.animals.iter())
            .filter(|a| !a.hidden)
            .collect()
    }

    /// Animals in active chunks whose chunk overlaps the given world-space
    /// view rectangle.  Used by the renderer for frustum culling.
    pub fn visible_animals(&self, cam_tl: Vec2, cam_br: Vec2) -> Vec<&WildAnimal> {
        self.active
            .iter()
            .filter(|cc| chunk_overlaps_rect(**cc, cam_tl, cam_br))
            .filter_map(|cc| self.data.get(cc))
            .flat_map(|cd| cd.animals.iter())
            .filter(|a| !a.hidden)
            .collect()
    }

    /// Record a successful catch on an animal instance (by UUID) without
    /// removing it. Returns `(species, new_catch_count)` if found. The caller
    /// compares the count against `species::captures_required` to decide whether
    /// the animal is now fully captured (then calls `remove_animal`).
    pub fn register_catch(&mut self, id: uuid::Uuid) -> Option<(&'static str, u32)> {
        for coord in &self.active {
            if let Some(chunk) = self.data.get_mut(coord) {
                if let Some(animal) = chunk.animals.iter_mut().find(|a| a.id == id) {
                    animal.catches += 1;
                    return Some((animal.species, animal.catches));
                }
            }
        }
        None
    }

    /// Remove an animal by UUID across all active chunks.
    /// Returns its species ID if found, or `None` if not found.
    pub fn remove_animal(&mut self, id: uuid::Uuid) -> Option<&'static str> {
        for coord in &self.active {
            if let Some(chunk) = self.data.get_mut(coord) {
                if let Some(pos) = chunk.animals.iter().position(|a| a.id == id) {
                    let species = chunk.animals[pos].species;
                    chunk.animals.remove(pos);
                    return Some(species);
                }
            }
        }
        None
    }
}

// ── Free helpers ──────────────────────────────────────────────────────────────

/// Convert a continuous world position to a chunk grid coordinate.
pub fn world_to_chunk(pos: Vec2) -> (i32, i32) {
    (
        (pos.x / CHUNK_SIZE).floor() as i32,
        (pos.y / CHUNK_SIZE).floor() as i32,
    )
}

/// Top-left corner of a chunk in world space.
pub fn chunk_origin(coord: (i32, i32)) -> Vec2 {
    vec2(coord.0 as f32 * CHUNK_SIZE, coord.1 as f32 * CHUNK_SIZE)
}

fn chunk_in_bounds(coord: (i32, i32)) -> bool {
    coord.0 >= 0 && coord.0 < CHUNKS_COLS && coord.1 >= 0 && coord.1 < CHUNKS_ROWS
}

/// Chebyshev ("king move") distance between two chunk coordinates.
fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// True if the chunk's world rect overlaps [tl, br].
fn chunk_overlaps_rect(coord: (i32, i32), tl: Vec2, br: Vec2) -> bool {
    let cx = coord.0 as f32 * CHUNK_SIZE;
    let cy = coord.1 as f32 * CHUNK_SIZE;
    cx < br.x && cx + CHUNK_SIZE > tl.x && cy < br.y && cy + CHUNK_SIZE > tl.y
}

/// Spawn wild animals for a newly-discovered chunk using the full biome pipeline:
///   1. Reject chunks inside the zoo plot (home is animal-free wilderness-wise)
///   2. Identify biome + noise at chunk centre
///   3. Roll the per-chunk encounter gate — most chunks come up empty
///   4. Generate Poisson-disk candidate positions (min separation enforced)
///   5. Weighted roll per candidate (biome × noise → species or "no spawn")
///   6. Skip candidates inside the player-safe radius or the zoo exclusion
fn spawn_chunk_animals(coord: (i32, i32), player_pos: Vec2) -> Vec<WildAnimal> {
    let origin = chunk_origin(coord);
    let chunk_centre = origin + vec2(CHUNK_SIZE * 0.5, CHUNK_SIZE * 0.5);

    // Home plot is kept clear of wild encounters.
    if in_zoo_exclusion(chunk_centre) {
        return Vec::new();
    }

    // Biome classification and noise value at chunk centre.
    let biome = biome::biome_at(chunk_centre);
    let noise = biome::noise2d(chunk_centre.x, chunk_centre.y, NOISE_SCALE);

    // Deterministic seed from chunk coordinates.
    let seed = (coord.0 as u64)
        .wrapping_mul(73_856_093)
        .wrapping_add((coord.1 as u64).wrapping_mul(19_349_663));
    let mut rng = LcgRng::new(seed);

    // Encounter gate — keeps the world sparse so each find feels special.
    // Higher noise → busier zones, but still mostly empty.
    let encounter_chance = ENCOUNTER_BASE + noise * ENCOUNTER_NOISE_BONUS;
    if rng.next_f32() > encounter_chance {
        return Vec::new();
    }

    // Most encounters are a single animal; high-noise zones can form a pack.
    let max_here = if noise > PACK_NOISE && rng.next_f32() < PACK_CHANCE { 2 } else { 1 };

    // Poisson-disk candidates — spatially well-distributed within the chunk.
    let candidates = biome::poisson_disk(origin, CHUNK_SIZE, CHUNK_SIZE, POISSON_MIN_DIST, seed);

    let min_dist_sq = MIN_SPAWN_DIST * MIN_SPAWN_DIST;
    let mut animals = Vec::new();

    for pos in candidates {
        if animals.len() >= max_here { break; }
        if in_zoo_exclusion(pos) { continue; }
        if (pos - player_pos).length_squared() < min_dist_sq { continue; }

        // Weighted roll: biome + noise → species or None.
        if let Some((species, mk)) = biome::weighted_spawn(biome, noise, &mut rng) {
            animals.push(WildAnimal::new(species, pos, mk()));
        }
    }

    animals
}
