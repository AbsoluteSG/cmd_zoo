//! Per-tick avatar update as an ordered chain of `Behavior` impls. Adding a
//! dash or knockback later is purely additive — insert a new behavior into
//! the chain rather than rewriting movement.

use macroquad::math::{Vec2, vec2};

use crate::game::habitat::{Habitat, footprint};
use crate::input::{ActionFlags, ControllerIntent};

use super::avatar::{
    AFTERIMAGE_CAP, AFTERIMAGE_LIFE, Afterimage, AvatarState, DASH_COOLDOWN, DASH_DURATION,
    DASH_SPEED, Facing, PlayerAvatar, SPRINT_MULT,
};

/// Frame-rate-independent exponential approach: move `from` toward `to` by a
/// fraction set by stiffness `k` (Hz-ish) over `dt`. No overshoot.
fn approach(from: f32, to: f32, dt: f32, k: f32) -> f32 {
    from + (to - from) * (1.0 - (-k * dt).exp())
}
fn approach_vec(from: Vec2, to: Vec2, dt: f32, k: f32) -> Vec2 {
    from + (to - from) * (1.0 - (-k * dt).exp())
}

/// World units per logical tile. Matches `render::view::DEFAULT_TILE_W` —
/// kept local so the domain doesn't depend on the render layer. If we ever
/// move to per-zoo tile sizing this becomes a parameter on `World`.
pub const TILE_W: f32 = 128.0;
/// Flat-world plane bounds — must stay in sync with `game::world_chunks`.
pub const PLANE_W: f32 = crate::game::world_chunks::WORLD_W;
pub const PLANE_H: f32 = crate::game::world_chunks::WORLD_H;

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
        let moving = intent.move_dir.length_squared() > 1e-4;

        // ── Dash: a one-frame burst in the movement (or facing) direction ──
        if a.dash_cooldown > 0.0 {
            a.dash_cooldown = (a.dash_cooldown - dt).max(0.0);
        }
        if intent.actions.contains(ActionFlags::DASH) && a.dash_cooldown <= 0.0 && !a.is_dashing() {
            a.dash_dir = if moving {
                intent.move_dir.normalize()
            } else {
                a.facing.to_vec()
            };
            a.dash_time = DASH_DURATION;
            a.dash_cooldown = DASH_COOLDOWN;
        }
        let dashing = a.is_dashing();

        // ── Target velocity: instant during a dash, accel-ramped otherwise ──
        if dashing {
            a.vel = a.dash_dir * DASH_SPEED;
            a.dash_time = (a.dash_time - dt).max(0.0);
        } else {
            let mut speed = stats.max_speed;
            if intent.actions.contains(ActionFlags::SPRINT) && moving {
                speed *= SPRINT_MULT;
            }
            let target_v = intent.move_dir * speed;
            let dv = target_v - a.vel;
            let max_dv = stats.accel * dt;
            let len = dv.length();
            a.vel += if len <= max_dv || len == 0.0 { dv } else { dv * (max_dv / len) };
        }

        // ── After-images: age existing ghosts; spawn a fresh one while dashing ──
        for img in &mut a.afterimages {
            img.life -= dt;
        }
        a.afterimages.retain(|i| i.life > 0.0);
        if dashing {
            a.afterimages.push(Afterimage {
                pos: a.pos,
                facing: a.facing,
                life: AFTERIMAGE_LIFE,
                max_life: AFTERIMAGE_LIFE,
            });
            if a.afterimages.len() > AFTERIMAGE_CAP {
                let excess = a.afterimages.len() - AFTERIMAGE_CAP;
                a.afterimages.drain(0..excess);
            }
        }

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

/// Update the avatar's smoothed render-only animation state (`viz`) from its
/// velocity. Drives the toon-ball's directional lighting, walk bob, idle
/// breathing, squash/stretch and lean — all damped so nothing snaps. Pure
/// presentation: never touches gameplay fields.
pub struct VizBehavior;

impl Behavior for VizBehavior {
    fn apply(&self, a: &mut PlayerAvatar, _intent: &ControllerIntent, _world: &World, dt: f32) {
        let max = a.stats.max_speed.max(1.0);
        let speed = a.vel.length();
        let intensity = (speed / max).clamp(0.0, 1.0);
        let moving = speed > max * 0.06;

        // Light trails the movement direction, biased upward so the ball always
        // reads as lit from roughly above. Held steady (last value) when idle.
        if moving {
            let mscreen = a.vel.normalize_or_zero();
            let target = (mscreen * 0.85 + vec2(0.0, -0.35)).normalize_or(a.viz.light_dir);
            a.viz.light_dir = approach_vec(a.viz.light_dir, target, dt, 7.0).normalize_or(target);
        }

        // Bob phase advances always (slow idle breathing keeps some life), but
        // faster while moving. Amplitude is the smoothed walk intensity.
        a.viz.bob_phase += dt * (10.0 * (0.35 + 0.65 * intensity));
        a.viz.breathe_phase += dt * 2.2;
        a.viz.bob_amp = approach(a.viz.bob_amp, intensity, dt, 8.0);

        // Lean toward horizontal screen movement, capped at ±8°.
        let target_lean = (a.vel.x / max).clamp(-1.0, 1.0) * 8.0_f32.to_radians();
        a.viz.lean = approach(a.viz.lean, target_lean, dt, 8.0);
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

/// The default behavior chain for M1: move, update facing, then refresh the
/// render-only animation state.
pub fn default_behaviors() -> Vec<Box<dyn Behavior>> {
    vec![Box::new(MoveBehavior), Box::new(FacingBehavior), Box::new(VizBehavior)]
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
    fn sprint_outpaces_walk() {
        let habs = empty_world();
        let world = World { habitats: &habs };
        let behaviors = default_behaviors();
        let start = vec2(1000.0, 1000.0);
        let mut walk = PlayerAvatar::new(Uuid::nil(), start);
        let mut run = PlayerAvatar::new(Uuid::nil(), start);
        let walk_intent = ControllerIntent { move_dir: vec2(1.0, 0.0), actions: ActionFlags::NONE };
        let mut run_actions = ActionFlags::NONE;
        run_actions.insert(ActionFlags::SPRINT);
        let run_intent = ControllerIntent { move_dir: vec2(1.0, 0.0), actions: run_actions };
        for _ in 0..30 {
            step(&mut walk, &walk_intent, &world, 1.0 / 60.0, &behaviors);
            step(&mut run, &run_intent, &world, 1.0 / 60.0, &behaviors);
        }
        assert!(
            run.pos.x > walk.pos.x + 1.0,
            "sprint should outpace walk: {} vs {}",
            run.pos.x,
            walk.pos.x
        );
    }

    #[test]
    fn dash_bursts_and_sets_cooldown() {
        let habs = empty_world();
        let world = World { habitats: &habs };
        let behaviors = default_behaviors();
        let start = vec2(1000.0, 1000.0);
        let mut a = PlayerAvatar::new(Uuid::nil(), start);
        let mut act = ActionFlags::NONE;
        act.insert(ActionFlags::DASH);
        let dash_intent = ControllerIntent { move_dir: vec2(1.0, 0.0), actions: act };

        step(&mut a, &dash_intent, &world, 1.0 / 60.0, &behaviors);
        assert!(a.is_dashing(), "dash should be active right after trigger");
        assert!(a.dash_cooldown > 0.0, "dash should start its cooldown");
        assert!(!a.afterimages.is_empty(), "dash should spawn after-images");
        assert!(a.pos.x > start.x, "dash moves immediately");

        // A second DASH while on cooldown shouldn't re-trigger (cooldown intact).
        let cd = a.dash_cooldown;
        for _ in 0..20 {
            step(&mut a, &dash_intent, &world, 1.0 / 60.0, &behaviors);
        }
        assert!(a.dash_cooldown < cd, "cooldown should be ticking down, not refreshed");
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
