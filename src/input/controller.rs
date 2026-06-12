use crate::game::avatar::PlayerAvatar;

// The avatar intent types (`ActionFlags`, `ControllerIntent`) now live in the
// engine-free core so the movement simulation can run headless. Re-exported here
// so `crate::input::{ActionFlags, ControllerIntent}` stays stable for the client.
pub use crate::game::intent::{ActionFlags, ControllerIntent};

/// Read-only world view a controller may inspect when sampling.
pub struct ControllerCtx<'a> {
    pub dt: f32,
    pub avatar: &'a PlayerAvatar,
    /// True while a menu overlay is open or tweening — controllers should
    /// return idle so the avatar stops while the player is in a menu.
    pub menu_open: bool,
    /// The current gamepad snapshot, if a pad is present. Polled once per frame
    /// on `GameApp` and passed in so the local controller reads it without a
    /// second poll. `None` when no pad / keyboard-only.
    pub pad: Option<&'a crate::input::gamepad::PadSnapshot>,
}

pub trait AvatarController {
    fn sample(&mut self, ctx: &ControllerCtx) -> ControllerIntent;
}
