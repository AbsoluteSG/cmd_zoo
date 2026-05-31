//! `RemoteController` — fed by network `Intent` packets via a per-peer queue.
//! From the avatar pipeline's perspective it's identical to any other
//! controller; the host instantiates one per connected visitor and updates
//! its queued intent each tick from inbound messages.

use macroquad::math::vec2;

use crate::net::protocol::WireIntent;

use super::controller::{ActionFlags, AvatarController, ControllerCtx, ControllerIntent};

/// Drives an avatar from a stream of `WireIntent`s. The owner (Session)
/// pokes the latest received intent in via `set_intent` each frame.
#[derive(Default)]
pub struct RemoteController {
    latest: WireIntent,
}

impl RemoteController {
    pub fn set_intent(&mut self, intent: WireIntent) {
        self.latest = intent;
    }
}

impl AvatarController for RemoteController {
    fn sample(&mut self, _ctx: &ControllerCtx) -> ControllerIntent {
        ControllerIntent {
            move_dir: vec2(self.latest.move_x, self.latest.move_y),
            actions: ActionFlags(self.latest.actions),
        }
    }
}
