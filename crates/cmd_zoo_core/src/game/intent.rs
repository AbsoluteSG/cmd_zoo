//! Engine-free avatar **intent** types — the gameplay-level "what the avatar
//! wants to do this tick", independent of how it was produced (keyboard, gamepad,
//! network, AI). Lives in the core so `avatar_system` (the movement simulation)
//! and the future authoritative server share the same shape; the client's
//! `input` module maps real devices onto these.

use glam::{Vec2, vec2};

/// Bitset of single-frame action requests. Plain u32 instead of pulling in
/// `bitflags` for a handful of slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionFlags(pub u32);

impl ActionFlags {
    pub const NONE: ActionFlags = ActionFlags(0);
    pub const INTERACT: ActionFlags = ActionFlags(1 << 0);
    pub const DASH: ActionFlags = ActionFlags(1 << 1);
    pub const SPRINT: ActionFlags = ActionFlags(1 << 2);

    pub fn contains(self, other: ActionFlags) -> bool {
        (self.0 & other.0) == other.0 && other.0 != 0
    }
    pub fn insert(&mut self, other: ActionFlags) {
        self.0 |= other.0;
    }
}

/// What the avatar wants to do this tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct ControllerIntent {
    /// Unit-ish movement vector in flat-world space. Length 0 = no input.
    pub move_dir: Vec2,
    pub actions: ActionFlags,
}

impl ControllerIntent {
    pub fn idle() -> Self {
        Self {
            move_dir: vec2(0.0, 0.0),
            actions: ActionFlags::NONE,
        }
    }
}
