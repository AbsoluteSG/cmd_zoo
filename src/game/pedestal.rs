//! Pedestals — the first *placeable / moveable* zoo object.
//!
//! Unlike nests and food structures (which sit in fixed, derived rows along the
//! plot edges), a pedestal is dropped on any free tile by the player and can be
//! picked up and re-placed later. A pedestal holds one *dedicated* animal: while
//! parked there the animal can't roam, follow, or be bred, but its income is
//! swept into the wallet automatically whenever it fills — including offline.
//!
//! Geometry is stored as **centre-relative integer tile coordinates** so it
//! survives zoo expansion (the plot centre is fixed; only the valid range
//! widens).

use chrono::{DateTime, Duration, Utc};
use glam::{Vec2, vec2};
use uuid::Uuid;

use crate::game::world_chunks::{ZOO_TILE_W, zoo_center, zoo_tiles};

/// Hard cap on owned pedestals (placed + unplaced in the hotbar).
pub const MAX_PEDESTALS: usize = 6;

/// Pedestal-dedicated animals bank up to this multiple of their normal storage
/// cap while offline. Online they're swept at 1× every tick (see
/// [`crate::game::zoo::Zoo::collect_pedestals`]).
pub const PEDESTAL_OFFLINE_CAP_MULT: f64 = 10.0;

/// How long a freshly dedicated animal is locked to its pedestal (can't be
/// released or sold during this window).
pub fn pedestal_lock() -> Duration {
    Duration::hours(48)
}

/// How long a pedestal is on cooldown after its animal is released, before it
/// can hold a new one.
pub fn pedestal_cooldown() -> Duration {
    Duration::hours(24)
}

/// A placeable income stand holding (at most) one dedicated animal.
#[derive(Clone, Debug)]
pub struct Pedestal {
    pub id: Uuid,
    /// Centre-relative tile coordinate: world = `zoo_center() + tile * ZOO_TILE_W`.
    pub tile: (i32, i32),
    /// The dedicated animal, if one has been placed on the pedestal.
    pub animal: Option<Uuid>,
    /// When the current animal was dedicated. Drives the 48h lock. `None` when
    /// empty (or a pre-lock save).
    pub dedicated_at: Option<DateTime<Utc>>,
    /// When the pedestal becomes free to hold a new animal after a release.
    /// `None` when not on cooldown.
    pub cooldown_until: Option<DateTime<Utc>>,
}

impl Pedestal {
    pub fn new(tile: (i32, i32)) -> Self {
        Self { id: Uuid::new_v4(), tile, animal: None, dedicated_at: None, cooldown_until: None }
    }

    /// True while the dedicated animal is still inside its 48h lock.
    pub fn is_locked(&self, now: DateTime<Utc>) -> bool {
        matches!(self.dedicated_at, Some(t) if now < t + pedestal_lock())
    }

    /// Instant the lock expires, if an animal is currently locked.
    pub fn lock_until(&self) -> Option<DateTime<Utc>> {
        self.dedicated_at.map(|t| t + pedestal_lock())
    }

    /// True while the pedestal is cooling down and can't accept a new animal.
    pub fn on_cooldown(&self, now: DateTime<Utc>) -> bool {
        matches!(self.cooldown_until, Some(t) if now < t)
    }
}

/// DNA-Helix cost of the next pedestal given how many are already owned (placed
/// + unplaced). Escalating; `None` once [`MAX_PEDESTALS`] are owned.
pub fn pedestal_cost(owned: usize) -> Option<u64> {
    match owned {
        0 => Some(10),
        1 => Some(49),
        2 => Some(99),
        3 => Some(299),
        4 => Some(499),
        5 => Some(999),
        _ => None,
    }
}

/// World-space centre of the tile a pedestal at `tile` occupies.
pub fn pedestal_world(tile: (i32, i32)) -> Vec2 {
    zoo_center() + vec2(tile.0 as f32 * ZOO_TILE_W, tile.1 as f32 * ZOO_TILE_W)
}

/// Snap a world position to the nearest pedestal tile.
pub fn world_to_pedestal_tile(world: Vec2) -> (i32, i32) {
    let rel = (world - zoo_center()) / ZOO_TILE_W;
    (rel.x.round() as i32, rel.y.round() as i32)
}

/// True when `tile` is inside the home plot (keeping the pedestal off the fence).
pub fn pedestal_tile_in_bounds(tile: (i32, i32)) -> bool {
    // Plot spans `zoo_tiles()` tiles centred on tile 0; the outermost ring of
    // tiles sits on the fence, so allow up to one in from the edge.
    let lim = (zoo_tiles() - 1) / 2;
    tile.0.abs() <= lim && tile.1.abs() <= lim
}
