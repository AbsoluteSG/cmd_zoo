//! Wire protocol. Host-authoritative: visitors stream `Intent`, host streams
//! avatar deltas and world changes. Serialization is JSON for now — small,
//! debuggable, and reuses the existing `serde` derives on game DTOs. Swap to
//! `bincode` later if bandwidth becomes a concern.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::persistence::schema::ZooSnapshot;

/// Opaque transport-level peer handle. The loopback transport uses small
/// integers; the steam transport will wrap `SteamId`. The session layer never
/// inspects the inside — it just maps `PeerId` to game-level `player_id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId(pub u64);

/// Length of a share code, in characters. The Steam transport encodes the
/// host's 32-bit SteamID account id as base32, which needs 7 digits; the UI
/// (input cap + Join-enabled check) reads this so everything stays in sync.
pub const CODE_LEN: usize = 7;

/// Share code a visitor types to join a host. With the Steam transport this is
/// the host's SteamID encoded as uppercase Crockford base32 (no ambiguous
/// I/L/O/U), so a verbal hand-off is unambiguous and self-resolving — no lobby.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JoinCode(pub String);

impl JoinCode {
    /// Build a random 6-char code. Uses `macroquad::rand` so we don't pull
    /// in a separate RNG crate. The transport layer is responsible for
    /// making this resolvable (Steam lobby metadata, etc.).
    pub fn random() -> Self {
        const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let mut s = String::with_capacity(6);
        for _ in 0..6 {
            let i = macroquad::rand::gen_range(0u32, ALPHABET.len() as u32) as usize;
            s.push(ALPHABET[i] as char);
        }
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Movement intent shipped from visitor → host every tick. Mirrors the local
/// `ControllerIntent` but with a primitive-only shape so it serializes
/// cleanly. Tick number lets the host detect drops / out-of-order delivery.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct WireIntent {
    pub tick: u32,
    pub move_x: f32,
    pub move_y: f32,
    pub actions: u32,
}

/// Avatar pose broadcast from host to all peers. Sent at the avatar-delta
/// rate (~20 Hz).
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct AvatarPose {
    pub player_id: Uuid,
    pub pos_x: f32,
    pub pos_y: f32,
    pub vel_x: f32,
    pub vel_y: f32,
    /// Facing as a small int: 0=N, 1=E, 2=S, 3=W.
    pub facing: u8,
}

/// One host-authoritative wild animal, streamed to visitors so everyone sees
/// (and can catch) the same creatures. Carries `fill_speed` directly so the
/// visitor's catch ring fills correctly without knowing the animal's moveset.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WildAnimalPose {
    pub id: Uuid,
    pub species: String,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub catches: u32,
    pub hidden: bool,
    pub fill_speed: f32,
}

/// Reason the host disconnected a visitor.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub enum ByeReason {
    Full,
    Kicked,
    HostShutdown,
    InvalidProtocol,
    /// The transport dropped (host crashed/closed/network lost) — synthesized
    /// locally on the visitor, never sent over the wire.
    ConnectionLost,
}

impl ByeReason {
    /// Player-facing line for the disconnect screen.
    pub fn message(self) -> &'static str {
        match self {
            ByeReason::Full => "The host's zoo is full.",
            ByeReason::Kicked => "The host removed you from their zoo.",
            ByeReason::HostShutdown => "The host closed their zoo.",
            ByeReason::InvalidProtocol => "Version mismatch with the host.",
            ByeReason::ConnectionLost => "Lost connection to the host.",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NetMessage {
    // visitor → host
    Hello {
        /// Stable visitor identity (UUIDv5 from SteamID in real co-op; any
        /// UUIDv4 in loopback/testing).
        player_id: Uuid,
        display_name: String,
    },
    Intent(WireIntent),
    DropGift {
        species: String,
        level: u8,
    },
    /// A visitor's request to mutate the host's authoritative zoo (collect, buy,
    /// breed, etc.). The host validates permissions, applies it, and broadcasts
    /// the result. Visitors never mutate the shared zoo locally.
    Command(crate::game::action::Action),
    Goodbye,

    // host → visitor
    Welcome {
        your_player_id: Uuid,
        host_name: String,
        spawn_x: f32,
        spawn_y: f32,
        /// Full world snapshot so the visitor's local `Zoo` mirrors the host's
        /// state. Replayed via `WorldSnapshot` on each subsequent host change.
        snapshot: ZooSnapshot,
    },
    /// Host's authoritative world state. Sent to all peers after a host-side
    /// mutation (with rate-limiting on the host). Visitor replaces local zoo.
    WorldSnapshot(ZooSnapshot),
    AvatarsDelta {
        tick: u32,
        avatars: Vec<AvatarPose>,
    },
    /// Host-authoritative wild animals near the players, streamed at ~10 Hz so
    /// visitors render and catch the same creatures. Visitors stop simulating
    /// their own wild world while connected.
    WildDelta {
        tick: u32,
        animals: Vec<WildAnimalPose>,
    },
    /// World-state changes the visitor needs to know about (coins changed,
    /// animal added, etc.). Kept as a tagged enum so it can grow without
    /// reshuffling on-wire ordinals.
    WorldDelta(WorldDelta),
    Bye(ByeReason),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WorldDelta {
    /// Coarse-grained "something changed, re-sync" signal for early M2 —
    /// host follows up with a full snapshot. Replace with surgical deltas
    /// once the system is exercised.
    ResyncRequested,
    /// A visitor record was updated (e.g. last_pos on goodbye).
    VisitorTouched {
        player_id: Uuid,
        at: DateTime<Utc>,
    },
}
