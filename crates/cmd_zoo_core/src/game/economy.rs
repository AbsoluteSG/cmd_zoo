//! Tick hook for time-driven game state.
//!
//! With redeem-on-click breeding (the player explicitly claims a finished
//! gestation by clicking the ready nest), there is no longer any timer-
//! triggered state to advance: animal coin storage is derived on demand from
//! `Animal::stored_at`, and breeding completion is user-driven via
//! `Zoo::claim_completed_breeding`.
//!
//! This module is kept as a no-op so callers in `main.rs` / `app::tick` can
//! continue calling `economy::advance(...)` without conditional compilation,
//! and so a future time-driven feature (decay timers, scheduled events)
//! has an obvious home.

use chrono::{DateTime, Utc};

use super::zoo::Zoo;

#[allow(unused_variables, unused_mut)]
pub fn advance(zoo: &mut Zoo, now: DateTime<Utc>) {
    // Intentionally empty.
}
