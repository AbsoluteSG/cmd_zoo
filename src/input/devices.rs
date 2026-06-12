//! Active input-device tracking: keyboard/mouse vs gamepad.
//!
//! The UI and cursor adapt to whichever device the player last used. We flip
//! only on *fresh* input from the other device (a key/mouse-move/click, or a pad
//! button / stick-past-deadzone) so a resting stick or a still mouse never fights
//! for control. Hot-plug fallback lives in `GameApp` (when the active pad
//! disconnects we revert to keyboard/mouse).

use macroquad::input::{MouseButton, get_last_key_pressed, is_mouse_button_pressed, mouse_position, mouse_wheel};
use macroquad::math::{Vec2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ActiveDevice {
    #[default]
    KeyboardMouse,
    Gamepad,
}

pub struct DeviceTracker {
    active: ActiveDevice,
    last_mouse: Vec2,
}

impl Default for DeviceTracker {
    fn default() -> Self {
        let (mx, my) = mouse_position();
        Self { active: ActiveDevice::KeyboardMouse, last_mouse: vec2(mx, my) }
    }
}

impl DeviceTracker {
    pub fn active(&self) -> ActiveDevice {
        self.active
    }

    pub fn is_gamepad(&self) -> bool {
        self.active == ActiveDevice::Gamepad
    }

    /// Force a device (e.g. keyboard/mouse on pad disconnect).
    pub fn set(&mut self, device: ActiveDevice) {
        self.active = device;
    }

    /// Update the active device from this frame's fresh input. `pad_activity`
    /// comes from the gamepad snapshot. Keyboard/mouse wins ties so resting a
    /// hand on the keyboard reclaims the pointer.
    pub fn update(&mut self, pad_activity: bool) {
        let (mx, my) = mouse_position();
        let mouse = vec2(mx, my);
        let mouse_moved = (mouse - self.last_mouse).length() > 2.0;
        self.last_mouse = mouse;

        let kbm_fresh = get_last_key_pressed().is_some()
            || mouse_moved
            || is_mouse_button_pressed(MouseButton::Left)
            || is_mouse_button_pressed(MouseButton::Right)
            || mouse_wheel().1 != 0.0;

        if pad_activity {
            self.active = ActiveDevice::Gamepad;
        }
        if kbm_fresh {
            self.active = ActiveDevice::KeyboardMouse;
        }
    }
}
