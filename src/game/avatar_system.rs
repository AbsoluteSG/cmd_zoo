//! Per-tick avatar update as an ordered chain of `Behavior` impls. Adding a
//! dash or knockback later is purely additive — insert a new behavior into
//! the chain rather than rewriting movement.

use macroquad::math::{Vec2, vec2};

use crate::game::habitat::{Habitat, footprint};
use crate::input::ControllerIntent;

use super::avatar::{AvatarState, Facing, PlayerAvatar};

/// World units per logical tile. Matches `render::view::DEFAULT_TILE_W` —
/// kept local so the domain doesn't depend on the render layer. If we ever
/// move to per-zoo tile sizing this becomes a parameter on `World`.
pub const TILE_W: f32 = 128.0;
/// Flat-world plane bounds (units). Matches `render::view::PLANE_{W,H}`.
pub const PLANE_W: f32 = 2048.0;
pub const PLANE_H: f32 = 2048.0;

/// Read-only slice of the simulation a behavior may inspect.
pub struct World<'a> {
    pub habitats: &'a [Habitat],
}

pub trait Behavior {
    fn apply(&self, avatar: &mut PlayerAvatar, intent: &ControllerIntent, world: &World, dt: f32);
}

/// Accel toward `intent.move_dir * max_speed`, then integrate with per-axis
/// sliding against habitat footprints and the plane bounds.
pub struct MoveBehavior;

impl Behavior for MoveBehavior {
    fn apply(&self, a: &mut PlayerAvatar, intent: &ControllerIntent, world: &World, dt: f32) {
        let stats = a.stats;
        let target_v = intent.move_dir * stats.max_speed;
        let dv = target_v - a.vel;
        let max_dv = stats.accel * dt;
        let len = dv.length();
        a.vel += if len <= max_dv || len == 0.0 { dv } else { dv * (max_dv / len) };

        let step = a.vel * dt;
        let mut p = a.pos;
        let try_x = vec2(p.x + step.x, p.y);
        if !blocked(try_x, stats.radius, world) {
            p.x = try_x.x;
        } else {
            a.vel.x = 0.0;
        }
        let try_y = vec2(p.x, p.y + step.y);
        if !blocked(try_y, stats.radius, world) {
            p.y = try_y.y;
        } else {
            a.vel.y = 0.0;
        }
        a.pos = p;

        a.state = if a.vel.length_squared() > 4.0 {
            AvatarState::Moving
        } else {
            AvatarState::Idle
        };
    }
}

/// Update `facing` from velocity (sticky when stopped).
pub struct FacingBehavior;

impl Behavior for FacingBehavior {
    fn apply(&self, a: &mut PlayerAvatar, _intent: &ControllerIntent, _world: &World, _dt: f32) {
        a.facing = Facing::from_vel(a.vel, a.facing);
    }
}

fn in_bounds(p: Vec2, r: f32) -> bool {
    p.x - r >= 0.0 && p.x + r <= PLANE_W && p.y - r >= 0.0 && p.y + r <= PLANE_H
}

/// Circle-vs-AABB against every habitat footprint, plus plane bounds.
fn blocked(p: Vec2, r: f32, world: &World) -> bool {
    if !in_bounds(p, r) {
        return true;
    }
    for h in world.habitats {
        let (fw, fh) = footprint(h.theme);
        let min_x = h.tile.0 as f32 * TILE_W;
        let min_y = h.tile.1 as f32 * TILE_W;
        let max_x = min_x + fw as f32 * TILE_W;
        let max_y = min_y + fh as f32 * TILE_W;
        let cx = p.x.clamp(min_x, max_x);
        let cy = p.y.clamp(min_y, max_y);
        let dx = p.x - cx;
        let dy = p.y - cy;
        if dx * dx + dy * dy < r * r {
            return true;
        }
    }
    false
}

/// Run the behavior chain in order.
pub fn step(
    avatar: &mut PlayerAvatar,
    intent: &ControllerIntent,
    world: &World,
    dt: f32,
    behaviors: &[Box<dyn Behavior>],
) {
    for b in behaviors {
        b.apply(avatar, intent, world, dt);
    }
}

/// The default behavior chain for M1: move, then update facing from velocity.
pub fn default_behaviors() -> Vec<Box<dyn Behavior>> {
    vec![Box::new(MoveBehavior), Box::new(FacingBehavior)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::avatar::{AVATAR_RADIUS, PlayerAvatar};
    use crate::input::{ActionFlags, ControllerIntent};
    use uuid::Uuid;

    fn empty_world() -> Vec<Habitat> {
        Vec::new()
    }

    #[test]
    fn move_advances_position_along_intent() {
        let habs = empty_world();
        let world = World { habitats: &habs };
        let mut a = PlayerAvatar::new(Uuid::nil(), vec2(500.0, 500.0));
        let behaviors = default_behaviors();
        let intent = ControllerIntent {
            move_dir: vec2(1.0, 0.0),
            actions: ActionFlags::default(),
        };
        // Run several frames so velocity ramps up.
        for _ in 0..30 {
            step(&mut a, &intent, &world, 1.0 / 60.0, &behaviors);
        }
        assert!(a.pos.x > 500.0, "should have moved right, got {}", a.pos.x);
        assert!((a.pos.y - 500.0).abs() < 0.01, "no vertical drift");
        assert_eq!(a.facing, Facing::E);
    }

    #[test]
    fn plane_bounds_block_movement() {
        let habs = empty_world();
        let world = World { habitats: &habs };
        let mut a = PlayerAvatar::new(Uuid::nil(), vec2(100.0, 1000.0));
        let behaviors = default_behaviors();
        let intent = ControllerIntent {
            move_dir: vec2(-1.0, 0.0),
            actions: ActionFlags::default(),
        };
        for _ in 0..120 {
            step(&mut a, &intent, &world, 1.0 / 60.0, &behaviors);
        }
        assert!(a.pos.x >= AVATAR_RADIUS, "clamped to plane, got {}", a.pos.x);
    }
}
