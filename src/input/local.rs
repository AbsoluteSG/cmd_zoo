//! The local player's avatar controller: keyboard **and** gamepad, merged.
//!
//! Reads WASD/arrows + Shift/Space (as [`KeyboardController`]) and, when a pad
//! snapshot is present on the [`ControllerCtx`], the left stick + face/shoulder
//! buttons. Whichever device the player touches drives the avatar, so device
//! switching for movement is automatic — no mode flag needed here.

use macroquad::input::{KeyCode, is_key_down, is_key_pressed};
use macroquad::math::vec2;

use super::controller::{ActionFlags, AvatarController, ControllerCtx, ControllerIntent};
use super::gamepad::PadButton;

#[derive(Default)]
pub struct LocalController;

impl AvatarController for LocalController {
    fn sample(&mut self, ctx: &ControllerCtx) -> ControllerIntent {
        if ctx.menu_open {
            return ControllerIntent::idle();
        }

        // ── Keyboard ──────────────────────────────────────────────────────
        let mut dir = vec2(0.0, 0.0);
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
            dir.x -= 1.0;
        }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
            dir.x += 1.0;
        }
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            dir.y -= 1.0;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            dir.y += 1.0;
        }
        let mut actions = ActionFlags::NONE;
        if is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift) {
            actions.insert(ActionFlags::SPRINT);
        }
        if is_key_pressed(KeyCode::Space) {
            actions.insert(ActionFlags::DASH);
        }

        // ── Gamepad (overrides movement when the stick is pushed) ─────────
        if let Some(pad) = ctx.pad {
            let stick = pad.move_vector();
            if stick.length() > 0.0 {
                dir = stick;
            }
            // South (A) dashes; LeftBumper / LeftTrigger sprints.
            if pad.just_pressed(PadButton::South) {
                actions.insert(ActionFlags::DASH);
            }
            if pad.held(PadButton::LeftBumper) || pad.lt > 0.5 {
                actions.insert(ActionFlags::SPRINT);
            }
        }

        let len = dir.length();
        if len > 1.0 {
            dir /= len;
        }

        ControllerIntent { move_dir: dir, actions }
    }
}
