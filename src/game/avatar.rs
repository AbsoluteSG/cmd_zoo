//! Player avatar — an in-world presence that replaces the cursor as the
//! "player." Position lives in continuous flat-world units (same space as
//! `Critter`); rendering is handled by the depth-sorted pass in
//! `render::world`. State lives here rather than on the controller so a
//! controller swap (keyboard ↔ remote, in M2) doesn't lose mid-action state.

use macroquad::math::{Vec2, vec2};
use uuid::Uuid;

/// World units per second at full input.
pub const DEFAULT_AVATAR_SPEED: f32 = 340.0;
/// Velocity ramp toward target velocity. High enough that input feels snappy
/// without being instant; low enough that vel survives a one-frame stutter.
pub const DEFAULT_AVATAR_ACCEL: f32 = 1600.0;
/// Collision radius (world units) used by the move behavior.
pub const AVATAR_RADIUS: f32 = 22.0;

/// Top-speed multiplier while sprinting (held Shift).
pub const SPRINT_MULT: f32 = 1.7;
/// Burst speed (world units/s) during a dash.
pub const DASH_SPEED: f32 = 1500.0;
/// How long a dash's burst lasts (seconds).
pub const DASH_DURATION: f32 = 0.18;
/// Cooldown between dashes (seconds).
pub const DASH_COOLDOWN: f32 = 0.65;
/// Lifetime (seconds) of a dash after-image ghost.
pub const AFTERIMAGE_LIFE: f32 = 0.32;
/// Max after-image ghosts retained at once.
pub const AFTERIMAGE_CAP: usize = 24;

/// A faded ghost of the avatar left behind during a dash.
#[derive(Clone, Copy, Debug)]
pub struct Afterimage {
    pub pos: Vec2,
    pub facing: Facing,
    /// Seconds of life remaining; drives the fade.
    pub life: f32,
    pub max_life: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    N,
    S,
    E,
    W,
}

impl Facing {
    /// Unit direction vector for this facing (flat-world space, +y = south).
    pub fn to_vec(self) -> Vec2 {
        match self {
            Facing::N => vec2(0.0, -1.0),
            Facing::S => vec2(0.0, 1.0),
            Facing::E => vec2(1.0, 0.0),
            Facing::W => vec2(-1.0, 0.0),
        }
    }

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

/// Smoothed, render-only animation state for the procedural toon-ball avatar.
/// Updated each tick by `VizBehavior` from the avatar's velocity so the
/// renderer stays a pure read. All values are damped so nothing snaps.
#[derive(Clone, Copy, Debug)]
pub struct AvatarViz {
    /// Smoothed fake-light direction in screen space (+x right, +y down). The
    /// lit highlight is offset toward this; it trails the movement direction.
    pub light_dir: Vec2,
    /// Ever-advancing bob phase (radians); speed scales with movement.
    pub bob_phase: f32,
    /// Smoothed walk intensity 0..1 (drives bob amplitude, squash, lean).
    pub bob_amp: f32,
    /// Slow idle "breathing" phase (radians).
    pub breathe_phase: f32,
    /// Smoothed lean angle (radians); tilts toward horizontal movement.
    pub lean: f32,
}

impl Default for AvatarViz {
    fn default() -> Self {
        Self {
            // Rest pose: lit from the upper-right so a still ball reads as round.
            light_dir: vec2(0.35, -0.5).normalize(),
            bob_phase: 0.0,
            bob_amp: 0.0,
            breathe_phase: 0.0,
            lean: 0.0,
        }
    }
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
    /// Seconds left in the current dash burst (>0 while dashing).
    pub dash_time: f32,
    /// Seconds until another dash may be triggered.
    pub dash_cooldown: f32,
    /// Locked-in dash direction (unit vector).
    pub dash_dir: Vec2,
    /// Trailing ghosts spawned during a dash; rendered faded behind the avatar.
    pub afterimages: Vec<Afterimage>,
    /// Smoothed render-only animation state (toon-ball lighting + bob + lean).
    pub viz: AvatarViz,
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
            dash_time: 0.0,
            dash_cooldown: 0.0,
            dash_dir: vec2(0.0, 1.0),
            afterimages: Vec::new(),
            viz: AvatarViz::default(),
        }
    }

    /// True while a dash burst is active.
    pub fn is_dashing(&self) -> bool {
        self.dash_time > 0.0
    }
}
