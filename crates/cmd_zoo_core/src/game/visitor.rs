//! Persisted state for a non-owner player who has visited this zoo.
//!
//! The host's save carries one `VisitorRecord` per known visitor so their
//! position, name, and gift inbox survive disconnect → reconnect cycles.
//! Visitors that have never connected are not present; visiting just once
//! creates the record on first `Hello`.

use chrono::{DateTime, Utc};
use glam::{Vec2, vec2};
use uuid::Uuid;

use crate::game::species::SpeciesId;

/// Per-visitor permissions the host has granted. A simple bit set so it's
/// trivial to persist and to extend with future capabilities (kick, build-only,
/// etc.). Defaults to no special permissions — visitors can already do
/// everything except the bits listed here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PermissionSet(pub u32);

impl PermissionSet {
    /// May sell animals from the shared zoo.
    pub const SELL: u32 = 1 << 0;

    pub fn has(self, bit: u32) -> bool {
        self.0 & bit != 0
    }

    pub fn set(&mut self, bit: u32, on: bool) {
        if on {
            self.0 |= bit;
        } else {
            self.0 &= !bit;
        }
    }
}

/// A gift dropped at this host by a visitor. The visitor can collect it on
/// any future visit. Self-contained so the persisted form doesn't depend on
/// the `share` module's wire codec.
#[derive(Clone, Debug)]
pub struct GiftRecord {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub sender_name: String,
    pub species: SpeciesId,
    pub level: u8,
    pub dropped_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct VisitorRecord {
    /// Stable identity. In M2 this is derived from the visitor's SteamID
    /// (UUIDv5) so a returning visitor matches their stored record across
    /// reinstalls. For loopback / offline testing it's any UUIDv4.
    pub player_id: Uuid,
    pub display_name: String,
    pub first_visited_at: DateTime<Utc>,
    pub last_visited_at: DateTime<Utc>,
    /// World-space position where the visitor was last seen; on rejoin
    /// they spawn back here so the world feels persistent.
    pub last_pos: Vec2,
    /// Gifts dropped at the host that this visitor can collect.
    pub gift_inbox: Vec<GiftRecord>,
    /// Capabilities the host has granted this visitor (e.g. selling). Persisted
    /// so grants survive disconnect/reconnect.
    pub permissions: PermissionSet,
}

impl VisitorRecord {
    pub fn new(player_id: Uuid, display_name: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            player_id,
            display_name: display_name.into(),
            first_visited_at: now,
            last_visited_at: now,
            last_pos: vec2(crate::game::avatar_system::PLANE_W * 0.5, crate::game::avatar_system::PLANE_H * 0.5),
            gift_inbox: Vec::new(),
            permissions: PermissionSet::default(),
        }
    }
}
