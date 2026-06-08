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
}

pub trait AvatarController {
    fn sample(&mut self, ctx: &ControllerCtx) -> ControllerIntent;
}
