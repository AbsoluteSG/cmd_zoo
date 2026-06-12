//! Focus-based UI navigation for the immediate-mode menus, so a gamepad can
//! drive every panel with no pointer.
//!
//! Two pieces:
//! - [`FocusNav`] is **persistent** (lives on `GameApp`). It holds the focused
//!   widget index and the widget rects collected on the previous frame, and
//!   moves focus to the geometrically-nearest widget in a direction.
//! - [`FocusFrame`] is **per-frame**, threaded into the menu draw via `Ctx`.
//!   Each focusable widget calls [`FocusFrame::register`] (in draw order) to get
//!   its index + whether it's focused, and to record its rect. Interior
//!   mutability lets `Ctx` stay a cheap shared reference.
//!
//! Mouse still works unchanged — widgets activate on `mouse-hover + click` OR
//! `focused + confirm`. The focus ring only shows while the gamepad is the
//! active device.

use std::cell::{Cell, RefCell};

use macroquad::math::{Rect, Vec2};

/// A cardinal navigation direction (from D-pad / left stick).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavDir {
    Up,
    Down,
    Left,
    Right,
}

/// Persistent focus state across frames.
#[derive(Default)]
pub struct FocusNav {
    /// Index of the focused widget (into the previous frame's registration order).
    focused: usize,
    /// Widget rects registered last frame, used to resolve directional moves.
    prev_rects: Vec<Rect>,
}

impl FocusNav {
    pub fn focused(&self) -> usize {
        self.focused
    }

    /// Reset focus to the first widget — call when a new screen opens so the
    /// highlight starts at the top.
    pub fn reset(&mut self) {
        self.focused = 0;
    }

    /// Move focus to the nearest widget in `dir`, using last frame's geometry.
    /// No-op if there's nothing in that direction.
    pub fn step(&mut self, dir: NavDir) {
        if self.prev_rects.is_empty() {
            return;
        }
        if self.focused >= self.prev_rects.len() {
            self.focused = 0;
            return;
        }
        let cur = center(self.prev_rects[self.focused]);
        let mut best: Option<(f32, usize)> = None;
        for (i, r) in self.prev_rects.iter().enumerate() {
            if i == self.focused {
                continue;
            }
            let d = center(*r) - cur;
            let in_dir = match dir {
                NavDir::Up => d.y < -1.0,
                NavDir::Down => d.y > 1.0,
                NavDir::Left => d.x < -1.0,
                NavDir::Right => d.x > 1.0,
            };
            if !in_dir {
                continue;
            }
            // Distance along the travel axis, plus a heavy penalty for drifting
            // off the perpendicular axis (keeps moves in a tidy column/row).
            let (primary, cross) = match dir {
                NavDir::Up | NavDir::Down => (d.y.abs(), d.x.abs()),
                NavDir::Left | NavDir::Right => (d.x.abs(), d.y.abs()),
            };
            let score = primary + cross * 2.5;
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, i));
            }
        }
        if let Some((_, i)) = best {
            self.focused = i;
        }
    }

    /// Store the rects registered this frame for next-frame navigation, clamping
    /// focus if the widget count shrank (dynamic lists).
    pub fn commit(&mut self, rects: Vec<Rect>) {
        if !rects.is_empty() && self.focused >= rects.len() {
            self.focused = rects.len() - 1;
        }
        self.prev_rects = rects;
    }

    /// Build the per-frame [`FocusFrame`] handed to the menu via `Ctx`.
    pub fn frame(&self, active: bool, confirm: bool) -> FocusFrame {
        FocusFrame {
            active,
            confirm,
            focused: self.focused,
            cursor: Cell::new(0),
            rects: RefCell::new(Vec::new()),
        }
    }
}

/// Per-frame focus accumulator. Shared `&` into `Ctx`; uses interior mutability
/// so each widget registration mutates the running cursor + rect list.
pub struct FocusFrame {
    /// Gamepad is the active device → draw focus rings and honor confirm.
    active: bool,
    /// Confirm (A / Enter) was pressed this frame.
    confirm: bool,
    focused: usize,
    cursor: Cell<usize>,
    rects: RefCell<Vec<Rect>>,
}

impl FocusFrame {
    /// Register a focusable widget at `rect` (in draw order). Returns whether it
    /// is the focused widget *and* focus mode is active (so the caller draws a
    /// ring) — and separately, callers test [`FocusFrame::confirmed`] for it.
    pub fn register(&self, rect: Rect) -> bool {
        let idx = self.cursor.get();
        self.cursor.set(idx + 1);
        self.rects.borrow_mut().push(rect);
        self.active && self.focused == idx
    }

    /// True if focus mode is active and confirm was pressed this frame — combine
    /// with the per-widget `is_focused` from [`register`].
    pub fn confirm(&self) -> bool {
        self.confirm
    }

    /// True while the gamepad is driving the UI (controls focus-ring drawing).
    pub fn active(&self) -> bool {
        self.active
    }

    /// Consume the frame, returning the rects registered (for `FocusNav::commit`).
    pub fn take_rects(&self) -> Vec<Rect> {
        std::mem::take(&mut self.rects.borrow_mut())
    }
}

fn center(r: Rect) -> Vec2 {
    vec2(r.x + r.w * 0.5, r.y + r.h * 0.5)
}

use macroquad::math::vec2;
