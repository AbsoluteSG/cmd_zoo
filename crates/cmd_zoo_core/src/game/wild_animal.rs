//! Wild animals roaming a bounded expedition map — the catch targets for the
//! target-engage loop ([`crate::game::catch`]).
//!
//! Catching is now a stat check (click to target, deplete the catch-resistance
//! bar), **not** a chase/evasion minigame, so wild animals carry no per-species
//! movement AI: they simply wander their map. The currently engaged animal is
//! held still by [`crate::game::biome_instance::BiomeInstance::update`] while
//! it's being caught. (The old open-world evasion movesets were retired with the
//! infinite world in the Phase 3 cleanup.)

use glam::{Vec2, vec2};
use uuid::Uuid;

use crate::game::rng as rand;
use crate::game::species::SpeciesId;

/// Calm wander speed in world units/second.
const WANDER_SPEED: f32 = 55.0;
/// Radius (world units) within which an animal picks its next wander target, so
/// it roams its local neighborhood rather than striking out across the map.
const WANDER_RADIUS: f32 = 600.0;

#[derive(Clone, Debug)]
pub struct WildAnimal {
    /// Stable identity used by the catch system to track the target across frames.
    pub id: Uuid,
    pub species: SpeciesId,
    pub pos: Vec2,
    pub vel: Vec2,
    /// Lazy wander target; re-rolled when reached.
    wander_target: Vec2,
    /// Deterministic placement index within the instance — part of the animal's
    /// reproducible identity (the instance also stamps a seed-derived `id`).
    pub spawn_index: u16,
}

impl WildAnimal {
    pub fn new(species: SpeciesId, pos: Vec2, spawn_index: u16) -> Self {
        Self {
            id: crate::game::ids::new_id(),
            species,
            pos,
            vel: vec2(0.0, 0.0),
            wander_target: local_wander_point(pos, Vec2::splat(f32::INFINITY)),
            spawn_index,
        }
    }

    /// Advance calm wandering by `dt`, keeping the animal inside `[0, bounds]`.
    pub fn update(&mut self, dt: f32, bounds: Vec2) {
        let target_vel = self.wander_velocity(bounds);
        // Exponential approach to target velocity — snappy without being instant.
        let t = (750.0 * dt).min(1.0);
        self.vel += (target_vel - self.vel) * t;
        self.pos += self.vel * dt;
        self.pos.x = self.pos.x.clamp(0.0, bounds.x);
        self.pos.y = self.pos.y.clamp(0.0, bounds.y);
    }

    /// Target velocity while calmly wandering toward `wander_target`.
    fn wander_velocity(&mut self, bounds: Vec2) -> Vec2 {
        let to = self.wander_target - self.pos;
        let dist = to.length();
        if dist < 12.0 {
            self.wander_target = local_wander_point(self.pos, bounds);
            return vec2(0.0, 0.0);
        }
        // Occasional spontaneous pause mid-wander.
        if rand::gen_range(0.0f32, 1.0) < 0.003 {
            return vec2(0.0, 0.0);
        }
        safe_normalize(to) * WANDER_SPEED
    }
}

/// Normalize `v`; returns the zero vector when the length is negligible.
fn safe_normalize(v: Vec2) -> Vec2 {
    let len = v.length();
    if len < 0.001 { vec2(0.0, 0.0) } else { v / len }
}

/// A random wander target within `WANDER_RADIUS` of `from`, clamped to `[0, bounds]`.
pub fn local_wander_point(from: Vec2, bounds: Vec2) -> Vec2 {
    let dx = rand::gen_range(-WANDER_RADIUS, WANDER_RADIUS);
    let dy = rand::gen_range(-WANDER_RADIUS, WANDER_RADIUS);
    vec2(
        (from.x + dx).clamp(0.0, bounds.x),
        (from.y + dy).clamp(0.0, bounds.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn wanders_within_bounds() {
        let bounds = vec2(1000.0, 1000.0);
        let mut a = WildAnimal::new("field_mouse", vec2(500.0, 500.0), 0);
        for _ in 0..600 {
            a.update(DT, bounds);
            assert!(a.pos.x >= 0.0 && a.pos.x <= bounds.x);
            assert!(a.pos.y >= 0.0 && a.pos.y <= bounds.y);
        }
    }

    #[test]
    fn actually_moves() {
        let bounds = vec2(2000.0, 2000.0);
        let start = vec2(1000.0, 1000.0);
        let mut a = WildAnimal::new("field_mouse", start, 0);
        for _ in 0..240 {
            a.update(DT, bounds);
        }
        assert!((a.pos - start).length() > 1.0, "animal should roam from its start");
    }
}
