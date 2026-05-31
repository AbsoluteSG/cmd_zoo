//! Player avatar — an in-world presence that replaces the cursor as the
//! "player." Position lives in continuous flat-world units (same space as
//! `Critter`); rendering is handled by the depth-sorted pass in
//! `render::world`. State lives here rather than on the controller so a
//! controller swap (keyboard ↔ remote, in M2) doesn't lose mid-action state.

use macroquad::math::{Vec2, vec2};
use uuid::Uuid;

/// World units per second at full input.
pub const DEFAULT_AVATAR_SPEED: f32 = 260.0;
/// Velocity ramp toward target velocity. High enough that input feels snappy
/// without being instant; low enough that vel survives a one-frame stutter.
pub const DEFAULT_AVATAR_ACCEL: f32 = 1600.0;
/// Collision radius (world units) used by the move behavior.
pub const AVATAR_RADIUS: f32 = 22.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    N,
    S,
    E,
    W,
}

impl Facing {
    /// Pick a facing from a velocity vector; keep `fallback` when essentially stopped.
    pub fn from_vel(v: Vec2, fallback: Facing) -> Facing {
        if v.length_squared() < 1.0 {
            return fallback;
        }
        if v.x.abs() > v.y.abs() {
            if v.x > 0.0 { Facing::E } else { Facing::W }
        } else if v.y > 0.0 {
            Facing::S
        } else {
            Facing::N
        }
    }
}

#[derive(Clone, Debug)]
pub enum AvatarState {
    Idle,
    Moving,
    // Future: Dashing { ends_at: DateTime<Utc>, dir: Vec2 }, Interacting { target: Uuid }
}

#[derive(Clone, Copy, Debug)]
pub struct AvatarStats {
    pub max_speed: f32,
    pub accel: f32,
    pub radius: f32,
}

impl Default for AvatarStats {
    fn default() -> Self {
        Self {
            max_speed: DEFAULT_AVATAR_SPEED,
            accel: DEFAULT_AVATAR_ACCEL,
            radius: AVATAR_RADIUS,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PlayerAvatar {
    /// Ties back to `Zoo.player` (one avatar per local player in M1).
    pub player_id: Uuid,
    pub pos: Vec2,
    pub vel: Vec2,
    pub facing: Facing,
    pub state: AvatarState,
    pub stats: AvatarStats,
}

impl PlayerAvatar {
    pub fn new(player_id: Uuid, pos: Vec2) -> Self {
        Self {
            player_id,
            pos,
            vel: vec2(0.0, 0.0),
            facing: Facing::S,
            state: AvatarState::Idle,
            stats: AvatarStats::default(),
        }
    }
}
