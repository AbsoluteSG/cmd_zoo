//! Pedestals — the first *placeable / moveable* zoo object.
//!
//! Unlike nests and food structures (which sit in fixed, derived rows along the
//! plot edges), a pedestal is dropped on any free tile by the player and can be
//! picked up and re-placed later. A pedestal holds one *dedicated* animal: while
//! parked there the animal can't roam, follow, or be bred, but its income is
//! swept into the wallet automatically whenever it fills — including offline.
//!
//! Geometry is stored as **centre-relative integer tile coordinates** on the
//! owning zoo's plot grid, so it survives zoo expansion (the plot centre is
//! fixed; only the valid range widens) and re-bases cleanly onto any plot
//! origin on a shared hub. The tile↔world mapping and bounds check live on
//! [`crate::game::zoo::Zoo`] (`tile_to_world`, `world_to_tile`, `tile_in_bounds`).

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

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
    /// Centre-relative tile coordinate on the owning zoo's plot grid; resolve to
    /// world space with [`crate::game::zoo::Zoo::tile_to_world`].
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
