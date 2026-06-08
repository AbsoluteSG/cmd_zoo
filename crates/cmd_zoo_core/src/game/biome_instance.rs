//! Instanced biomes (Phase 3) — the discrete, bounded "expedition" maps that
//! replace the infinite 500k procgen world.
//!
//! From the hub's expedition board the player launches a [`BiomeInstance`]: a
//! small, *bounded* map of one [`HabitatTheme`], procedurally **arranged** from
//! a seed (not an endlessly streamed world). You hunt and catch its wild animals
//! with the target-engage loop ([`crate::game::catch`]) and return your catches
//! to the hub zoo. Because the map is finite and seeded there is no chunk
//! streaming, interest management is trivial, and party scoping (Phase 5) falls
//! out naturally — an instance *is* the scope.
//!
//! Pure + deterministic from `(theme, seed, size)` — the same generation runs
//! client-side for Solo and inside a SpacetimeDB reducer online.

use glam::Vec2;
use uuid::Uuid;

use crate::game::biome::{LcgRng, poisson_disk, weighted_spawn};
use crate::game::catch::{CatchEngagement, TargetProfile, catch_tier};
use crate::game::species::{HabitatTheme, SpeciesId};

/// One catchable wild animal placed in an instance. Lean by design — just
/// identity, where it stands, and whether it's been caught. The live catch
/// state lives in a [`CatchEngagement`], not here, so a spawn is cheap to sync.
#[derive(Clone, Debug, PartialEq)]
pub struct WildSpawn {
    pub id: Uuid,
    pub species: SpeciesId,
    pub pos: Vec2,
    pub tier: u8,
    /// Set once the animal has been captured; it stays in the list (so indices
    /// /ids are stable) but no longer counts as a live target.
    pub captured: bool,
}

/// A bounded, seeded biome map launched from the hub.
#[derive(Clone, Debug)]
pub struct BiomeInstance {
    pub id: Uuid,
    pub theme: HabitatTheme,
    pub seed: u64,
    /// Map extent in world units; valid positions are `[0, size]` on each axis.
    pub size: Vec2,
    pub spawns: Vec<WildSpawn>,
}

/// Default expedition map size (world units). Comfortably bigger than a zoo plot
/// but a finite arena, not a world.
pub const DEFAULT_SIZE: Vec2 = Vec2::new(4_000.0, 3_000.0);
/// Minimum separation between spawns (world units) — keeps targets distinct.
const SPAWN_MIN_DIST: f32 = 160.0;
/// Hard cap on spawns per instance, so a dense seed still bounds the work.
const MAX_SPAWNS: usize = 24;

impl BiomeInstance {
    /// Deterministically arrange an instance of `theme` from `seed`. Candidate
    /// positions come from Poisson-disk sampling inside the bounded map; each is
    /// rolled against the theme's spawn table, so the population is themed,
    /// well-spread, and identical for a given `(theme, seed, size)`.
    pub fn generate(theme: HabitatTheme, seed: u64, size: Vec2) -> Self {
        let id = Uuid::from_u64_pair(seed, seed.rotate_left(32) ^ 0xB10E_5EED);
        let candidates = poisson_disk(Vec2::ZERO, size.x, size.y, SPAWN_MIN_DIST, seed);
        let mut rng = LcgRng::new(seed ^ 0x5CA1_AB1E);
        let mut spawns = Vec::new();
        for (i, pos) in candidates.into_iter().enumerate() {
            if spawns.len() >= MAX_SPAWNS {
                break;
            }
            // Per-candidate noise drives which band of the theme's table rolls.
            let noise = rng.next_f32();
            if let Some((species, _moveset)) = weighted_spawn(theme, noise, &mut rng) {
                spawns.push(WildSpawn {
                    id: Uuid::from_u64_pair(seed.wrapping_add(i as u64 + 1), i as u64),
                    species,
                    pos,
                    tier: catch_tier(species),
                    captured: false,
                });
            }
        }
        Self { id, theme, seed, size, spawns }
    }

    /// Convenience: generate at [`DEFAULT_SIZE`].
    pub fn new(theme: HabitatTheme, seed: u64) -> Self {
        Self::generate(theme, seed, DEFAULT_SIZE)
    }

    /// True when `pos` lies inside the bounded map.
    pub fn in_bounds(&self, pos: Vec2) -> bool {
        pos.x >= 0.0 && pos.y >= 0.0 && pos.x <= self.size.x && pos.y <= self.size.y
    }

    /// Live (not-yet-caught) spawns.
    pub fn live(&self) -> impl Iterator<Item = &WildSpawn> {
        self.spawns.iter().filter(|s| !s.captured)
    }

    /// Number of animals left to catch.
    pub fn remaining(&self) -> usize {
        self.live().count()
    }

    /// True once every spawn has been captured — the expedition is cleared.
    pub fn is_cleared(&self) -> bool {
        self.remaining() == 0
    }

    /// Look up a spawn by id.
    pub fn spawn(&self, id: Uuid) -> Option<&WildSpawn> {
        self.spawns.iter().find(|s| s.id == id)
    }

    /// The nearest live spawn to `pos` (for click/aim assist), if any.
    pub fn nearest_live(&self, pos: Vec2) -> Option<&WildSpawn> {
        self.live().min_by(|a, b| {
            let da = (a.pos - pos).length_squared();
            let db = (b.pos - pos).length_squared();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Begin a catch engagement against spawn `id`. The engagement's RNG is
    /// derived from the instance seed + the spawn id so its skill-check cadence
    /// is deterministic and reproducible. Returns `None` for an unknown or
    /// already-captured spawn.
    pub fn engage(&self, id: Uuid) -> Option<CatchEngagement> {
        let spawn = self.spawn(id).filter(|s| !s.captured)?;
        let (lo, hi) = spawn.id.as_u64_pair();
        let engage_seed = self.seed ^ lo ^ hi.rotate_left(17);
        Some(CatchEngagement::new(TargetProfile::for_species(spawn.species), engage_seed))
    }

    /// Mark spawn `id` captured (the engagement's bar emptied). Returns the
    /// captured species so the caller can grant it to the hub zoo. Idempotent:
    /// returns `None` if unknown or already captured.
    pub fn capture(&mut self, id: Uuid) -> Option<SpeciesId> {
        let spawn = self.spawns.iter_mut().find(|s| s.id == id && !s.captured)?;
        spawn.captured = true;
        Some(spawn.species)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic() {
        let a = BiomeInstance::new(HabitatTheme::Forest, 1234);
        let b = BiomeInstance::new(HabitatTheme::Forest, 1234);
        assert_eq!(a.spawns, b.spawns);
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn spawns_are_bounded_and_themed() {
        let inst = BiomeInstance::new(HabitatTheme::Forest, 77);
        assert!(!inst.spawns.is_empty(), "forest seed should populate");
        assert!(inst.spawns.len() <= MAX_SPAWNS);
        for s in &inst.spawns {
            assert!(inst.in_bounds(s.pos), "spawn {:?} out of bounds", s.pos);
            assert!((1..=5).contains(&s.tier));
        }
    }

    #[test]
    fn different_seeds_differ() {
        let a = BiomeInstance::new(HabitatTheme::Forest, 1);
        let b = BiomeInstance::new(HabitatTheme::Forest, 2);
        assert_ne!(a.spawns, b.spawns, "distinct seeds should arrange differently");
    }

    #[test]
    fn capture_clears_the_instance() {
        let mut inst = BiomeInstance::new(HabitatTheme::Forest, 5);
        let total = inst.spawns.len();
        assert_eq!(inst.remaining(), total);
        let ids: Vec<Uuid> = inst.spawns.iter().map(|s| s.id).collect();
        for (i, id) in ids.iter().enumerate() {
            let sp = inst.capture(*id);
            assert!(sp.is_some());
            assert_eq!(inst.remaining(), total - i - 1);
            // Re-capturing the same id is a no-op.
            assert!(inst.capture(*id).is_none());
        }
        assert!(inst.is_cleared());
    }

    #[test]
    fn engage_builds_an_engagement_for_live_spawns_only() {
        let mut inst = BiomeInstance::new(HabitatTheme::Forest, 9);
        let id = inst.spawns[0].id;
        let eng = inst.engage(id).expect("live spawn engages");
        assert_eq!(eng.target.species, inst.spawns[0].species);
        inst.capture(id);
        assert!(inst.engage(id).is_none(), "captured spawn no longer engages");
    }

    #[test]
    fn nearest_live_picks_closest() {
        let inst = BiomeInstance::new(HabitatTheme::Forest, 3);
        let target = inst.spawns[0].pos;
        let near = inst.nearest_live(target).unwrap();
        // The nearest to a spawn's own position is itself (distance 0).
        assert_eq!(near.pos, target);
    }
}
