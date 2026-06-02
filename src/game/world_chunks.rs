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
use crate::game::wild_animal::{AiHit, WildAnimal};

// ── World & chunk dimensions ──────────────────────────────────────────────────

pub const WORLD_W: f32 = 500_000.0;
pub const WORLD_H: f32 = 500_000.0;
/// Square chunk edge length in world units.
pub const CHUNK_SIZE: f32 = 512.0;
/// Number of chunk columns in the world grid.
pub const CHUNKS_COLS: i32 = (WORLD_W / CHUNK_SIZE) as i32; // ~976
/// Number of chunk rows in the world grid.
pub const CHUNKS_ROWS: i32 = (WORLD_H / CHUNK_SIZE) as i32; // ~976

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

// ── Chunk data ────────────────────────────────────────────────────────────────

pub struct ChunkData {
    pub animals: Vec<WildAnimal>,
}

/// Player-caused deviations from a chunk's procedural generation. This is the
/// *only* wild-world state persisted to the save file — everything else is
/// regenerated deterministically from the world seed. Keyed in `WorldChunks`
/// by chunk coord; an empty delta is never stored.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct ChunkDelta {
    /// Spawn indices of animals that have been fully captured/removed and must
    /// not respawn when the chunk regenerates.
    pub removed: Vec<u16>,
    /// In-progress partial-catch counts, `(spawn_index, catches)`, restored onto
    /// regenerated animals so multi-catch progress survives a reload.
    pub partial: Vec<(u16, u32)>,
}

// ── WorldChunks ───────────────────────────────────────────────────────────────

pub struct WorldChunks {
    /// In-memory chunk storage for chunks near the player only. Chunks beyond
    /// the cull radius are evicted and regenerated deterministically on re-entry.
    /// Animals are stored under the chunk they currently stand in (see
    /// `migrate_animals`), never necessarily their origin chunk.
    pub data: HashMap<(i32, i32), ChunkData>,
    /// The subset of `data` keys currently within the load radius.
    active: HashSet<(i32, i32)>,
    /// Per-world procedural-generation seed. Drives all biome/spawn determinism.
    world_seed: u64,
    /// Persisted deltas: captures/partial-catch progress per chunk. Re-applied
    /// whenever a chunk is (re)generated so captured animals stay gone.
    deltas: HashMap<(i32, i32), ChunkDelta>,
}

impl WorldChunks {
    pub fn new(world_seed: u64, deltas: HashMap<(i32, i32), ChunkDelta>) -> Self {
        Self {
            data: HashMap::new(),
            active: HashSet::new(),
            world_seed,
            deltas,
        }
    }

    /// The world's procedural seed (for syncing back into the save snapshot).
    pub fn world_seed(&self) -> u64 {
        self.world_seed
    }

    /// Clone the persisted deltas for serialization into the save snapshot.
    pub fn export_deltas(&self) -> HashMap<(i32, i32), ChunkDelta> {
        self.deltas.clone()
    }

    /// Record a delta mutation for `coord`, creating the entry on demand.
    fn delta_mut(&mut self, coord: (i32, i32)) -> &mut ChunkDelta {
        self.deltas.entry(coord).or_default()
    }

    // ── Streaming ─────────────────────────────────────────────────────────────

    /// Call once per frame with the player's world position.
    /// Loads newly-in-range chunks (spawning animals on first visit) and
    /// deactivates chunks that have drifted outside the cull radius.
    pub fn update(&mut self, player_pos: Vec2) {
        let pc = world_to_chunk(player_pos);

        // Re-home moved animals into whatever chunk they now physically occupy.
        // This must run *before* eviction so an animal that has wandered/fled out
        // of its current chunk is relocated to its real chunk — guaranteeing it
        // never disappears merely because its old chunk drifts off-screen.
        self.migrate_animals();

        // Evict chunks outside the cull radius entirely (free memory). Captures
        // are already recorded in `deltas`, so re-entry regenerates the chunk
        // minus the removed animals.
        self.data.retain(|coord, _| chebyshev(*coord, pc) <= CULL_RADIUS);
        self.active.retain(|coord| chebyshev(*coord, pc) <= CULL_RADIUS);

        // Identities already loaded somewhere — so a chunk regenerating doesn't
        // duplicate an animal that previously migrated into a still-loaded chunk.
        let mut loaded: HashSet<(i32, i32, u16)> = HashSet::new();
        for chunk in self.data.values() {
            for a in &chunk.animals {
                loaded.insert((a.origin_chunk.0, a.origin_chunk.1, a.spawn_index));
            }
        }

        // Activate + (re)generate chunks inside the load radius.
        for dy in -LOAD_RADIUS..=LOAD_RADIUS {
            for dx in -LOAD_RADIUS..=LOAD_RADIUS {
                let coord = (pc.0 + dx, pc.1 + dy);
                if !chunk_in_bounds(coord) { continue; }
                if chebyshev(coord, pc) > LOAD_RADIUS { continue; }

                self.active.insert(coord);

                if !self.data.contains_key(&coord) {
                    let animals = self.regenerate_chunk(coord, &loaded);
                    for a in &animals {
                        loaded.insert((a.origin_chunk.0, a.origin_chunk.1, a.spawn_index));
                    }
                    self.data.insert(coord, ChunkData { animals });
                }
            }
        }
    }

    /// Deterministically regenerate `coord` from the world seed, then apply its
    /// persisted delta: drop captured spawn indices, restore partial-catch
    /// counts, and skip any identity already loaded elsewhere (anti-duplication).
    fn regenerate_chunk(
        &self,
        coord: (i32, i32),
        loaded: &HashSet<(i32, i32, u16)>,
    ) -> Vec<WildAnimal> {
        let delta = self.deltas.get(&coord);
        let mut out = spawn_chunk_animals(coord, self.world_seed);
        out.retain(|a| {
            if loaded.contains(&(coord.0, coord.1, a.spawn_index)) {
                return false;
            }
            if let Some(d) = delta {
                if d.removed.contains(&a.spawn_index) {
                    return false;
                }
            }
            true
        });
        if let Some(d) = delta {
            for a in &mut out {
                if let Some((_, c)) = d.partial.iter().find(|(idx, _)| *idx == a.spawn_index) {
                    a.catches = *c;
                }
            }
        }
        out
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
                .or_insert_with(|| ChunkData { animals: Vec::new() })
                .animals
                .push(animal);
        }
    }

    // ── AI tick ───────────────────────────────────────────────────────────────

    /// Advance wild-animal AI for every animal in an active chunk. Returns the
    /// aggression events landed this frame (Basher charges, Venomous lunges,
    /// Thrower releases) so the caller can trigger hitstop / shake / effects.
    /// Empty on the vast majority of frames.
    pub fn update_animal_ai(
        &mut self,
        dt: f32,
        cursor_world: Vec2,
        player_pos: Vec2,
        catch_mode: bool,
    ) -> Vec<AiHit> {
        let mut hits = Vec::new();
        for coord in &self.active {
            if let Some(chunk) = self.data.get_mut(coord) {
                for animal in &mut chunk.animals {
                    if let Some(hit) = animal.update(dt, cursor_world, player_pos, catch_mode) {
                        hits.push(hit);
                    }
                }
            }
        }
        hits
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
        // Locate the animal and read its persistent identity + new count.
        let mut found: Option<((i32, i32), u16, &'static str, u32)> = None;
        for coord in self.active.iter() {
            if let Some(chunk) = self.data.get_mut(coord) {
                if let Some(animal) = chunk.animals.iter_mut().find(|a| a.id == id) {
                    animal.catches += 1;
                    found = Some((animal.origin_chunk, animal.spawn_index, animal.species, animal.catches));
                    break;
                }
            }
        }
        let (origin, idx, species, count) = found?;
        // Persist the partial-catch progress so it survives evict/regeneration.
        let delta = self.delta_mut(origin);
        match delta.partial.iter_mut().find(|(i, _)| *i == idx) {
            Some((_, c)) => *c = count,
            None => delta.partial.push((idx, count)),
        }
        Some((species, count))
    }

    /// Remove an animal by UUID across all active chunks, recording its removal
    /// in the chunk delta so it never respawns. Returns its species ID if found.
    pub fn remove_animal(&mut self, id: uuid::Uuid) -> Option<&'static str> {
        let mut found: Option<((i32, i32), u16, &'static str)> = None;
        for coord in self.active.iter() {
            if let Some(chunk) = self.data.get_mut(coord) {
                if let Some(pos) = chunk.animals.iter().position(|a| a.id == id) {
                    let a = chunk.animals.remove(pos);
                    found = Some((a.origin_chunk, a.spawn_index, a.species));
                    break;
                }
            }
        }
        let (origin, idx, species) = found?;
        let delta = self.delta_mut(origin);
        if !delta.removed.contains(&idx) {
            delta.removed.push(idx);
        }
        // Once removed, partial progress is irrelevant — drop it.
        delta.partial.retain(|(i, _)| *i != idx);
        Some(species)
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

/// Deterministically spawn the wild animals for `coord` from the world seed.
/// This is a *pure* function of `(coord, world_seed)` — it must not depend on
/// the player's position or any runtime state, so that the same chunk always
/// regenerates the same ordered animal list (giving each animal a stable
/// `spawn_index` for delta tracking).
///
/// Pipeline: reject the zoo plot → classify biome + noise at chunk centre →
/// roll the encounter gate → Poisson-disk candidates → weighted species roll.
fn spawn_chunk_animals(coord: (i32, i32), world_seed: u64) -> Vec<WildAnimal> {
    let origin = chunk_origin(coord);
    let chunk_centre = origin + vec2(CHUNK_SIZE * 0.5, CHUNK_SIZE * 0.5);

    // Home plot is kept clear of wild encounters.
    if in_zoo_exclusion(chunk_centre) {
        return Vec::new();
    }

    // Biome classification and noise value at chunk centre (seed-driven).
    let biome = biome::biome_at(chunk_centre, world_seed);
    let noise = biome::noise2d(
        chunk_centre.x + (world_seed & 0xFFFF) as f32,
        chunk_centre.y + ((world_seed >> 16) & 0xFFFF) as f32,
        NOISE_SCALE,
    );

    // Deterministic seed from world seed + chunk coordinates.
    let seed = world_seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((coord.0 as u64).wrapping_mul(73_856_093))
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

    let mut animals = Vec::new();

    for pos in candidates {
        if animals.len() >= max_here { break; }
        if in_zoo_exclusion(pos) { continue; }

        // Weighted roll: biome + noise → species or None.
        if let Some((species, mk)) = biome::weighted_spawn(biome, noise, &mut rng) {
            let spawn_index = animals.len() as u16;
            animals.push(WildAnimal::new(species, pos, mk(), coord, spawn_index));
        }
    }

    animals
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regenerating the same chunk from the same seed must yield an identical
    /// ordered animal list — this is what makes spawn indices a stable identity.
    #[test]
    fn spawn_is_deterministic_per_seed() {
        let seed = 0x00AB_CDEF;
        for cy in 40..55 {
            for cx in 40..55 {
                let a = spawn_chunk_animals((cx, cy), seed);
                let b = spawn_chunk_animals((cx, cy), seed);
                assert_eq!(a.len(), b.len(), "len differs at ({cx},{cy})");
                for (x, y) in a.iter().zip(b.iter()) {
                    assert_eq!(x.species, y.species);
                    assert_eq!(x.spawn_index, y.spawn_index);
                    assert_eq!(x.origin_chunk, y.origin_chunk);
                    assert_eq!(x.pos, y.pos);
                }
            }
        }
    }

    /// Different world seeds must produce different worlds.
    #[test]
    fn different_seeds_produce_different_worlds() {
        let mut differs = false;
        for cy in 40..70 {
            for cx in 40..70 {
                let a = spawn_chunk_animals((cx, cy), 1);
                let b = spawn_chunk_animals((cx, cy), 2);
                if a.len() != b.len()
                    || a.iter().zip(b.iter()).any(|(x, y)| x.species != y.species)
                {
                    differs = true;
                }
            }
        }
        assert!(differs, "two different seeds generated identical worlds");
    }

    /// A capture recorded as a chunk delta must survive eviction + regeneration:
    /// the captured animal does not reappear when the chunk reloads.
    #[test]
    fn capture_delta_survives_regeneration() {
        // Find a seed that places at least one animal near a non-zoo position.
        let pos = vec2(100_000.0, 100_000.0);
        let mut seed = 0u64;
        let mut world = None;
        for s in 1..3000u64 {
            let mut w = WorldChunks::new(s, HashMap::new());
            w.update(pos);
            if !w.active_animals().is_empty() {
                seed = s;
                world = Some(w);
                break;
            }
        }
        let mut w = world.expect("no seed produced a nearby animal");

        let (id, origin, idx) = {
            let a = w.active_animals()[0];
            (a.id, a.origin_chunk, a.spawn_index)
        };
        assert!(w.remove_animal(id).is_some());
        let deltas = w.export_deltas();
        assert!(!deltas.is_empty(), "removal did not record a delta");

        // Fresh world from the same seed + persisted deltas, reload same area.
        let mut w2 = WorldChunks::new(seed, deltas);
        w2.update(pos);
        let reappeared = w2
            .active_animals()
            .iter()
            .any(|a| a.origin_chunk == origin && a.spawn_index == idx);
        assert!(!reappeared, "captured animal reappeared after regeneration");
    }
}
