//! Catch-mode state: hover detection, fill progress, and catch events (§34).
//!
//! `CatchState` is owned by `GameApp` and updated every frame via `update`.
//! When the fill circle completes, `update` returns `Some(uuid)` — the caller
//! calls `world.remove_animal(uuid)` and spawns the tame animal.

use macroquad::math::{Vec2, vec2};
use uuid::Uuid;

use crate::game::species::SpeciesId;
use crate::game::wild_animal::CATCH_SCREEN_RADIUS_BASE;
use crate::render::view::{self, Camera, CRITTER_H};

/// A render/catch-ready view of one wild animal, decoupled from the full
/// `WildAnimal` so it can be fed either from the host's live world or from a
/// visitor's host-streamed `WildAnimalPose`s — the catch system treats both
/// identically.
#[derive(Clone, Copy, Debug)]
pub struct WildView {
    pub id: Uuid,
    pub species: SpeciesId,
    pub pos: Vec2,
    pub vel: Vec2,
    pub catches: u32,
    pub hidden: bool,
    pub fill_speed: f32,
}

// ── Distance-based fill tuning ─────────────────────────────────────────────────

/// World-space distance (avatar → animal) at/under which the catch ring fills
/// at full speed. Roughly an arm's reach around the player.
const CATCH_NEAR_DIST: f32 = 260.0;
/// Distance at/over which fill is reduced to `CATCH_MIN_MULT`. Past here the
/// catch crawls — you're meant to walk closer.
const CATCH_FAR_DIST: f32 = 1400.0;
/// Floor multiplier applied to fill speed at maximum distance. Never zero, so
/// a patient long-range catch is still *possible*, just slow.
const CATCH_MIN_MULT: f32 = 0.12;

/// Fill-speed multiplier as a function of avatar→animal world distance.
/// 1.0 when close, tapering linearly to `CATCH_MIN_MULT` at long range.
pub fn distance_fill_multiplier(dist: f32) -> f32 {
    if dist <= CATCH_NEAR_DIST {
        1.0
    } else if dist >= CATCH_FAR_DIST {
        CATCH_MIN_MULT
    } else {
        let t = (dist - CATCH_NEAR_DIST) / (CATCH_FAR_DIST - CATCH_NEAR_DIST);
        1.0 - t * (1.0 - CATCH_MIN_MULT)
    }
}

// ── Public state ──────────────────────────────────────────────────────────────

pub struct CatchState {
    /// Whether catch mode is toggled on (C key).
    pub active: bool,
    /// UUID of the animal currently being targeted (stable across frames).
    pub target: Option<Uuid>,
    /// Fill progress: 0.0 (empty) → 1.0 (complete / just caught).
    pub fill: f32,
}

impl Default for CatchState {
    fn default() -> Self {
        Self { active: false, target: None, fill: 0.0 }
    }
}

impl CatchState {
    /// Toggle catch mode on / off.  Resets fill when turning off.
    pub fn toggle(&mut self) {
        self.active = !self.active;
        if !self.active {
            self.target = None;
            self.fill = 0.0;
        }
    }

    /// Advance the catch state for one frame.
    ///
    /// `animals` is a slice of references to all catchable (active, visible,
    /// non-hidden) animals — typically from `WorldChunks::active_animals`.
    ///
    /// Returns `Some(uuid)` the moment a fill circle completes.  The caller
    /// should remove that animal from the world and spawn a tame copy.
    ///
    /// `player_pos` is the local avatar's world position — fill speed scales
    /// with how close the avatar is to the targeted animal.
    pub fn update(
        &mut self,
        mouse_screen: Vec2,
        player_pos: Vec2,
        animals: &[WildView],
        cam: &Camera,
        dt: f32,
    ) -> Option<Uuid> {
        if !self.active {
            self.target = None;
            self.fill = 0.0;
            return None;
        }

        let catch_r = CATCH_SCREEN_RADIUS_BASE * cam.zoom;

        // Find which animal (if any) the cursor is currently hovering over.
        let hovered: Option<Uuid> = animals.iter().find_map(|a| {
            let center = animal_screen_center(a.pos, cam);
            if (mouse_screen - center).length() < catch_r { Some(a.id) } else { None }
        });

        match (self.target, hovered) {
            // Continuing to hover over the same animal → advance fill.
            (Some(prev), Some(now)) if prev == now => {
                let target = animals.iter().find(|a| a.id == now);
                let base = target.map(|a| a.fill_speed).unwrap_or(0.4);
                let dist = target
                    .map(|a| (player_pos - a.pos).length())
                    .unwrap_or(0.0);
                let speed = base * distance_fill_multiplier(dist);
                self.fill = (self.fill + speed * dt).min(1.0);
                if self.fill >= 1.0 {
                    let caught = now;
                    self.target = None;
                    self.fill = 0.0;
                    return Some(caught);
                }
            }
            // Cursor moved onto a different animal → snap target, reset fill.
            (_, Some(now)) => {
                self.target = Some(now);
                self.fill = 0.0;
            }
            // Cursor not on any animal → drain fill slowly.
            (_, None) => {
                self.target = None;
                self.fill = (self.fill - dt * 0.9).max(0.0);
            }
        }

        None
    }
}

// ── Shared geometry helper ────────────────────────────────────────────────────

/// Screen-space center used for both hover hit-testing and as the catch circle
/// anchor.  Sits roughly mid-sprite, above the feet, matching the visual body.
pub fn animal_screen_center(world_pos: Vec2, cam: &Camera) -> Vec2 {
    let feet = view::world_to_screen(world_pos, cam);
    let sprite_h = CRITTER_H * cam.zoom;
    feet - vec2(0.0, sprite_h * 0.38)
}
