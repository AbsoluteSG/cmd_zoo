use macroquad::input::{KeyCode, is_key_down, is_key_pressed};
use macroquad::math::vec2;

use super::controller::{ActionFlags, AvatarController, ControllerCtx, ControllerIntent};

/// WASD / arrow-key driven avatar control. Stateless — held-key polling each
/// sample.
#[derive(Default)]
pub struct KeyboardController;

impl AvatarController for KeyboardController {
    fn sample(&mut self, ctx: &ControllerCtx) -> ControllerIntent {
        if ctx.menu_open {
            return ControllerIntent::idle();
        }
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
        let len = dir.length();
        if len > 1.0 {
            dir /= len;
        }
        let mut actions = ActionFlags::NONE;
        if is_key_pressed(KeyCode::E) {
            actions.insert(ActionFlags::INTERACT);
        }
        ControllerIntent { move_dir: dir, actions }
    }
}
