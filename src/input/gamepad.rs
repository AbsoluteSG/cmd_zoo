//! Gamepad input side-channel, backed by `gilrs`.
//!
//! macroquad/miniquad expose no usable gamepad API, so we poll `gilrs`
//! independently — once per frame at the top of `GameApp::handle_input` — into a
//! [`PadSnapshot`]. Everything downstream (the movement [`GamepadController`],
//! the semantic `Inputs` layer, focus navigation) reads that single snapshot, so
//! there is exactly one poll per frame.
//!
//! Desktop only: the whole module is compiled out on wasm (see the `cfg`-gated
//! re-export in `input::mod`), where a no-op shim stands in.

use macroquad::math::{Vec2, vec2};

/// Sticks/triggers below this magnitude read as zero (avoids drift/jitter).
pub const DEADZONE: f32 = 0.30;

/// A stable, backend-independent gamepad button. Decouples the rest of the game
/// from `gilrs::Button` so the mapping lives in one place. Names follow the Xbox
/// layout; `South`/`East`/`West`/`North` are the face buttons (A/B/X/Y).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PadButton {
    South = 0, // A
    East,      // B
    West,      // X
    North,     // Y
    LeftBumper,
    RightBumper,
    LeftTrigger,  // digital LT (also exposed as an axis)
    RightTrigger, // digital RT
    Select,       // Back/View
    Start,        // Menu
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    LeftThumb,  // L3
    RightThumb, // R3
}

impl PadButton {
    const COUNT: usize = 16;
    const ALL: [PadButton; Self::COUNT] = [
        PadButton::South, PadButton::East, PadButton::West, PadButton::North,
        PadButton::LeftBumper, PadButton::RightBumper, PadButton::LeftTrigger, PadButton::RightTrigger,
        PadButton::Select, PadButton::Start,
        PadButton::DPadUp, PadButton::DPadDown, PadButton::DPadLeft, PadButton::DPadRight,
        PadButton::LeftThumb, PadButton::RightThumb,
    ];
}

/// A small fixed-size bitset over [`PadButton`].
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonSet(u32);

impl ButtonSet {
    fn insert(&mut self, b: PadButton) {
        self.0 |= 1 << (b as u8);
    }
    pub fn contains(&self, b: PadButton) -> bool {
        self.0 & (1 << (b as u8)) != 0
    }
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }
}

/// One frame of gamepad state for the active pad. Rebuilt every `poll()`.
#[derive(Clone, Copy, Default)]
pub struct PadSnapshot {
    /// Buttons that transitioned to pressed this frame.
    pub just_pressed: ButtonSet,
    /// Buttons that transitioned to released this frame.
    pub just_released: ButtonSet,
    /// Buttons currently held.
    pub held: ButtonSet,
    /// Left/right sticks in gilrs convention (x right+, **y up+**), deadzoned.
    pub left_stick: Vec2,
    pub right_stick: Vec2,
    /// Analog triggers in `[0,1]`.
    pub lt: f32,
    pub rt: f32,
    /// True if any button/stick moved this frame — feeds active-device switching.
    pub any_activity: bool,
}

impl PadSnapshot {
    pub fn just_pressed(&self, b: PadButton) -> bool {
        self.just_pressed.contains(b)
    }
    pub fn held(&self, b: PadButton) -> bool {
        self.held.contains(b)
    }
    /// Left stick with screen-space Y (down positive), matching avatar `move_dir`
    /// and keyboard WASD (W = up = negative Y).
    pub fn move_vector(&self) -> Vec2 {
        vec2(self.left_stick.x, -self.left_stick.y)
    }
}

/// Hot-plug notifications surfaced from a `poll()`.
#[derive(Clone, Copy, Debug)]
pub enum PadEvent {
    Connected,
    Disconnected,
}

/// Owns the `gilrs` context and the current pad snapshot. One per `GameApp`.
pub struct GamepadHub {
    gilrs: gilrs::Gilrs,
    active: Option<gilrs::GamepadId>,
    snapshot: PadSnapshot,
}

impl GamepadHub {
    /// Initialise gilrs. `None` if the backend fails to start (the game then runs
    /// keyboard/mouse-only).
    pub fn new() -> Option<Self> {
        match gilrs::Gilrs::new() {
            Ok(gilrs) => {
                let active = gilrs.gamepads().next().map(|(id, _)| id);
                Some(Self { gilrs, active, snapshot: PadSnapshot::default() })
            }
            Err(_) => None,
        }
    }

    pub fn snapshot(&self) -> &PadSnapshot {
        &self.snapshot
    }

    /// True once a pad is connected and adopted as active.
    pub fn has_pad(&self) -> bool {
        self.active.is_some()
    }

    /// Pump gilrs events, rebuild the snapshot for the active pad, and return any
    /// connect/disconnect transitions. Call exactly once per frame.
    pub fn poll(&mut self) -> Vec<PadEvent> {
        use gilrs::EventType;
        let mut events = Vec::new();
        let mut just_pressed = ButtonSet::default();
        let mut just_released = ButtonSet::default();

        while let Some(ev) = self.gilrs.next_event() {
            match ev.event {
                EventType::Connected => {
                    if self.active.is_none() {
                        self.active = Some(ev.id);
                    }
                    events.push(PadEvent::Connected);
                }
                EventType::Disconnected => {
                    if self.active == Some(ev.id) {
                        self.active = None;
                    }
                    events.push(PadEvent::Disconnected);
                }
                EventType::ButtonPressed(btn, _) => {
                    if self.active == Some(ev.id) {
                        if let Some(b) = map_button(btn) {
                            just_pressed.insert(b);
                        }
                    }
                }
                EventType::ButtonReleased(btn, _) => {
                    if self.active == Some(ev.id) {
                        if let Some(b) = map_button(btn) {
                            just_released.insert(b);
                        }
                    }
                }
                _ => {}
            }
        }

        // Adopt the first available pad if we don't have one yet (covers pads
        // already connected before the first event pump).
        if self.active.is_none() {
            self.active = self.gilrs.gamepads().next().map(|(id, _)| id);
        }

        let mut snap = PadSnapshot { just_pressed, just_released, ..Default::default() };
        if let Some(id) = self.active {
            let gp = self.gilrs.gamepad(id);
            for b in PadButton::ALL {
                if let Some(gb) = to_gilrs(b) {
                    if gp.is_pressed(gb) {
                        snap.held.insert(b);
                    }
                }
            }
            snap.left_stick = deadzone(vec2(gp.value(gilrs::Axis::LeftStickX), gp.value(gilrs::Axis::LeftStickY)));
            snap.right_stick = deadzone(vec2(gp.value(gilrs::Axis::RightStickX), gp.value(gilrs::Axis::RightStickY)));
            snap.lt = gp.value(gilrs::Axis::LeftZ).clamp(0.0, 1.0);
            snap.rt = gp.value(gilrs::Axis::RightZ).clamp(0.0, 1.0);
        }
        snap.any_activity = !snap.just_pressed.is_empty()
            || snap.left_stick.length() > 0.0
            || snap.right_stick.length() > 0.0;
        self.snapshot = snap;
        events
    }
}

/// Apply a radial deadzone, renormalising so the live range starts at 0.
fn deadzone(v: Vec2) -> Vec2 {
    let len = v.length();
    if len <= DEADZONE {
        return Vec2::ZERO;
    }
    let scaled = (len - DEADZONE) / (1.0 - DEADZONE);
    v / len * scaled.min(1.0)
}

/// gilrs button → our stable [`PadButton`] (unmapped buttons are ignored).
fn map_button(b: gilrs::Button) -> Option<PadButton> {
    use gilrs::Button as G;
    Some(match b {
        G::South => PadButton::South,
        G::East => PadButton::East,
        G::West => PadButton::West,
        G::North => PadButton::North,
        G::LeftTrigger => PadButton::LeftBumper,
        G::RightTrigger => PadButton::RightBumper,
        G::LeftTrigger2 => PadButton::LeftTrigger,
        G::RightTrigger2 => PadButton::RightTrigger,
        G::Select => PadButton::Select,
        G::Start => PadButton::Start,
        G::DPadUp => PadButton::DPadUp,
        G::DPadDown => PadButton::DPadDown,
        G::DPadLeft => PadButton::DPadLeft,
        G::DPadRight => PadButton::DPadRight,
        G::LeftThumb => PadButton::LeftThumb,
        G::RightThumb => PadButton::RightThumb,
        _ => return None,
    })
}

/// Our [`PadButton`] → gilrs button, for held-state polling.
fn to_gilrs(b: PadButton) -> Option<gilrs::Button> {
    use gilrs::Button as G;
    Some(match b {
        PadButton::South => G::South,
        PadButton::East => G::East,
        PadButton::West => G::West,
        PadButton::North => G::North,
        PadButton::LeftBumper => G::LeftTrigger,
        PadButton::RightBumper => G::RightTrigger,
        PadButton::LeftTrigger => G::LeftTrigger2,
        PadButton::RightTrigger => G::RightTrigger2,
        PadButton::Select => G::Select,
        PadButton::Start => G::Start,
        PadButton::DPadUp => G::DPadUp,
        PadButton::DPadDown => G::DPadDown,
        PadButton::DPadLeft => G::DPadLeft,
        PadButton::DPadRight => G::DPadRight,
        PadButton::LeftThumb => G::LeftThumb,
        PadButton::RightThumb => G::RightThumb,
    })
}
