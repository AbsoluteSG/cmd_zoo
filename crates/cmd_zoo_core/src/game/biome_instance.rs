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
use crate::game::wild_animal::WildAnimal;

/// A bounded, seeded biome map launched from the hub — a *mini open world*. The
/// avatar walks it freely and its [`WildAnimal`]s roam with the full wild AI
/// ([`crate::game::wild_animal`]); left-click engages the nearest one with the
/// target-engage catch loop ([`crate::game::catch`]).
#[derive(Clone, Debug)]
pub struct BiomeInstance {
    pub id: Uuid,
    pub theme: HabitatTheme,
    pub seed: u64,
    /// Map extent in world units; valid positions are `[0, size]` on each axis.
    pub size: Vec2,
    /// The roaming, catchable wild animals. Captured animals are removed, so the
    /// whole list is always "live".
    pub animals: Vec<WildAnimal>,
}

/// Default expedition map size (world units): a ~10k×10k single-biome arena —
/// a small open world, not the retired 500k procgen world.
pub const DEFAULT_SIZE: Vec2 = Vec2::new(10_000.0, 10_000.0);
/// Minimum separation between spawns (world units) — keeps targets spread out
/// across the larger arena.
const SPAWN_MIN_DIST: f32 = 520.0;
/// Hard cap on animals per instance, so a dense seed still bounds the work.
const MAX_SPAWNS: usize = 40;

impl BiomeInstance {
    /// Deterministically arrange an instance of `theme` from `seed`. Candidate
    /// positions come from Poisson-disk sampling inside the bounded map; each is
    /// rolled against the theme's spawn table for a themed, well-spread starting
    /// population. (Roaming AI then moves them non-deterministically client-side;
    /// authoritative movement arrives with the Phase 4 server.)
    pub fn generate(theme: HabitatTheme, seed: u64, size: Vec2) -> Self {
        let id = Uuid::from_u64_pair(seed, seed.rotate_left(32) ^ 0xB10E_5EED);
        let candidates = poisson_disk(Vec2::ZERO, size.x, size.y, SPAWN_MIN_DIST, seed);
        let mut rng = LcgRng::new(seed ^ 0x5CA1_AB1E);
        let mut animals = Vec::new();
        for (i, pos) in candidates.into_iter().enumerate() {
            if animals.len() >= MAX_SPAWNS {
                break;
            }
            // Per-candidate noise drives which band of the theme's table rolls.
            let noise = rng.next_f32();
            if let Some((species, make_moveset)) = weighted_spawn(theme, noise, &mut rng) {
                let mut animal = WildAnimal::new(species, pos, make_moveset(), (0, 0), i as u16);
                // Stable, seed-derived id so engagements are reproducible.
                animal.id = Uuid::from_u64_pair(seed.wrapping_add(i as u64 + 1), i as u64);
                animals.push(animal);
            }
        }
        Self { id, theme, seed, size, animals }
    }

    /// Convenience: generate at [`DEFAULT_SIZE`].
    pub fn new(theme: HabitatTheme, seed: u64) -> Self {
        Self::generate(theme, seed, DEFAULT_SIZE)
    }

    /// Advance every roaming animal's AI by `dt`, then keep it inside the arena.
    /// Animals just wander (catching is a stat check, not an evasion minigame),
    /// and the **currently engaged** animal (`engaged`) holds completely still
    /// while it's being caught.
    pub fn update(&mut self, dt: f32, avatar_pos: Vec2, engaged: Option<Uuid>) {
        let size = self.size;
        for a in &mut self.animals {
            if Some(a.id) == engaged {
                // Being caught: sit still (a pure stat check, no fleeing).
                a.vel = Vec2::ZERO;
                continue;
            }
            a.update(dt, avatar_pos, avatar_pos, false);
            a.pos.x = a.pos.x.clamp(0.0, size.x);
            a.pos.y = a.pos.y.clamp(0.0, size.y);
        }
    }

    /// True when `pos` lies inside the bounded map.
    pub fn in_bounds(&self, pos: Vec2) -> bool {
        pos.x >= 0.0 && pos.y >= 0.0 && pos.x <= self.size.x && pos.y <= self.size.y
    }

    /// Live (not-yet-caught) animals — captured ones are removed, so this is all
    /// of them. Kept as an iterator for call-site symmetry.
    pub fn live(&self) -> impl Iterator<Item = &WildAnimal> {
        self.animals.iter()
    }

    /// Number of animals left to catch.
    pub fn remaining(&self) -> usize {
        self.animals.len()
    }

    /// True once every animal has been captured — the expedition is cleared.
    pub fn is_cleared(&self) -> bool {
        self.animals.is_empty()
    }

    /// Look up an animal by id.
    pub fn animal(&self, id: Uuid) -> Option<&WildAnimal> {
        self.animals.iter().find(|a| a.id == id)
    }

    /// The nearest live animal to `pos` (for click/aim assist), if any.
    pub fn nearest_live(&self, pos: Vec2) -> Option<&WildAnimal> {
        self.animals.iter().min_by(|a, b| {
            let da = (a.pos - pos).length_squared();
            let db = (b.pos - pos).length_squared();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Begin a catch engagement against animal `id`. The engagement's RNG is
    /// derived from the instance seed + the animal id so its skill-check cadence
    /// is deterministic. Returns `None` for an unknown animal.
    pub fn engage(&self, id: Uuid) -> Option<CatchEngagement> {
        let animal = self.animal(id)?;
        let (lo, hi) = animal.id.as_u64_pair();
        let engage_seed = self.seed ^ lo ^ hi.rotate_left(17);
        Some(CatchEngagement::new(TargetProfile::for_species(animal.species), engage_seed))
    }

    /// Capture animal `id` (the engagement's bar emptied): remove it and return
    /// its species so the caller can grant it to the hub zoo. `None` if unknown.
    pub fn capture(&mut self, id: Uuid) -> Option<SpeciesId> {
        let idx = self.animals.iter().position(|a| a.id == id)?;
        Some(self.animals.remove(idx).species)
    }

    /// Catch tier (1–5) of the animal `id`, for UI.
    pub fn tier_of(&self, id: Uuid) -> Option<u8> {
        self.animal(id).map(|a| catch_tier(a.species))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Initial placement (species + position + id) is deterministic from the
    /// seed, even though roaming motion afterwards is not.
    fn placement(inst: &BiomeInstance) -> Vec<(SpeciesId, Vec2, Uuid)> {
        inst.animals.iter().map(|a| (a.species, a.pos, a.id)).collect()
    }

    #[test]
    fn generation_is_deterministic() {
        let a = BiomeInstance::new(HabitatTheme::Forest, 1234);
        let b = BiomeInstance::new(HabitatTheme::Forest, 1234);
        assert_eq!(placement(&a), placement(&b));
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn animals_are_bounded_and_themed() {
        let inst = BiomeInstance::new(HabitatTheme::Forest, 77);
        assert!(!inst.animals.is_empty(), "forest seed should populate");
        assert!(inst.animals.len() <= MAX_SPAWNS);
        for a in &inst.animals {
            assert!(inst.in_bounds(a.pos), "spawn {:?} out of bounds", a.pos);
            assert!((1..=5).contains(&inst.tier_of(a.id).unwrap()));
        }
    }

    #[test]
    fn different_seeds_differ() {
        let a = BiomeInstance::new(HabitatTheme::Forest, 1);
        let b = BiomeInstance::new(HabitatTheme::Forest, 2);
        assert_ne!(placement(&a), placement(&b), "distinct seeds arrange differently");
    }

    #[test]
    fn update_roams_within_bounds() {
        let mut inst = BiomeInstance::new(HabitatTheme::Forest, 21);
        let before: Vec<Vec2> = inst.animals.iter().map(|a| a.pos).collect();
        let avatar = inst.size * 0.5;
        for _ in 0..120 {
            inst.update(1.0 / 60.0, avatar, None);
        }
        // Every animal stays inside the arena…
        for a in &inst.animals {
            assert!(inst.in_bounds(a.pos), "animal left the arena at {:?}", a.pos);
        }
        // …and at least one has actually moved (they roam).
        let moved = inst.animals.iter().zip(&before).any(|(a, p)| (a.pos - *p).length() > 1.0);
        assert!(moved, "animals should roam");
    }

    #[test]
    fn engaged_animal_sits_still() {
        let mut inst = BiomeInstance::new(HabitatTheme::Forest, 88);
        let id = inst.animals[0].id;
        let start = inst.animals[0].pos;
        let avatar = inst.size * 0.5;
        for _ in 0..120 {
            inst.update(1.0 / 60.0, avatar, Some(id));
        }
        // The engaged target never moves while it's being caught.
        assert_eq!(inst.animal(id).unwrap().pos, start);
    }

    #[test]
    fn capture_removes_and_clears_the_instance() {
        let mut inst = BiomeInstance::new(HabitatTheme::Forest, 5);
        let total = inst.animals.len();
        assert_eq!(inst.remaining(), total);
        let ids: Vec<Uuid> = inst.animals.iter().map(|a| a.id).collect();
        for (i, id) in ids.iter().enumerate() {
            assert!(inst.capture(*id).is_some());
            assert_eq!(inst.remaining(), total - i - 1);
            // The animal is gone; re-capturing the same id is a no-op.
            assert!(inst.capture(*id).is_none());
        }
        assert!(inst.is_cleared());
    }

    #[test]
    fn engage_builds_an_engagement_for_live_animals_only() {
        let mut inst = BiomeInstance::new(HabitatTheme::Forest, 9);
        let id = inst.animals[0].id;
        let species = inst.animals[0].species;
        let eng = inst.engage(id).expect("live animal engages");
        assert_eq!(eng.target.species, species);
        inst.capture(id);
        assert!(inst.engage(id).is_none(), "captured animal no longer engages");
    }

    #[test]
    fn nearest_live_picks_closest() {
        let inst = BiomeInstance::new(HabitatTheme::Forest, 3);
        let target = inst.animals[0].pos;
        let near = inst.nearest_live(target).unwrap();
        // The nearest to an animal's own position is itself (distance 0).
        assert_eq!(near.pos, target);
    }
}
