//! Steam relay transport. Gated behind the `steam` Cargo feature.
//!
//! Build with `cargo build --features steam` once the Steamworks SDK is
//! discoverable (the `steamworks` crate links against `steam_api.lib`).
//! On Windows the SDK redistributable `steam_api64.dll` must sit next to
//! the produced exe at runtime.
//!
//! Identity: a visitor's stable `player_id` is `UUIDv5(NAMESPACE_OID,
//! steam_id_bytes)` so reinstalls don't lose their `VisitorRecord` at any
//! host they've visited.
//!
//! Discovery: codeless. The host's share code *is* its SteamID (account id in
//! base32, see [`steam_id_to_code`]); the visitor decodes it and opens a
//! NetworkingSockets P2P connection straight to that SteamID on virtual port 0.
//! No Steam lobby is involved — lobby matchmaking is unreliable on the shared
//! 480 test app, and isn't needed when the code already names the host.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use steamworks::{
    Client, ClientManager, SteamId,
    networking_sockets::{ListenSocket, NetConnection, NetworkingSockets},
    networking_types::{
        ListenSocketEvent, NetConnectionEnd, NetworkingIdentity, SendFlags,
    },
};

use super::protocol::{JoinCode, NetMessage, PeerId};
use super::transport::{NetEvent, NetTransport};

/// Virtual port — both sides agree on 0. Higher ports let one app multiplex
/// multiple unrelated channels; we only need the one.
const VIRTUAL_PORT: i32 = 0;
/// Max in-flight messages drained per `poll`. Bounds frame budget.
const POLL_BATCH: usize = 64;

/// Host-side transport: owns a P2P listen socket. Discovery is codeless —
/// visitors connect straight to the host's SteamID, which is encoded in the
/// share code (see [`steam_id_to_code`]) — so no Steam lobby is involved.
pub struct SteamTransport {
    client: Client<ClientManager>,
    single: steamworks::SingleClient<ClientManager>,
    sockets: NetworkingSockets<ClientManager>,
    role: Role,
    /// Live connections keyed by the local-numeric PeerId we expose to the
    /// session layer; we map back to NetConnection for sends.
    peers: HashMap<PeerId, NetConnection<ClientManager>>,
    /// Remote SteamId per peer. NetConnection has no public method to ask
    /// "who is on the other end?", so we capture it at Connected and look
    /// it up here when a Disconnected event names the same SteamId.
    peer_steam_ids: HashMap<PeerId, SteamId>,
    /// Inbound events the session will pick up next poll.
    inbox: Arc<Mutex<Vec<NetEvent>>>,
    next_peer_id: u64,
}

enum Role {
    Host {
        listen: ListenSocket<ClientManager>,
    },
    Visitor {
        host_peer: PeerId,
    },
}

impl SteamTransport {
    /// Start hosting: open a P2P listen socket. No lobby — visitors reach us by
    /// connecting straight to our SteamID, which is what the share code encodes
    /// (see [`local_join_code`](Self::local_join_code)). This sidesteps Steam's
    /// lobby matchmaking entirely (unreliable on the shared 480 test app).
    pub fn host() -> Result<Self> {
        crate::net_log!("HOST: initializing Steam client…");
        let (client, single) = Client::init().map_err(|e| {
            crate::net_log!("HOST: steam init FAILED: {e:?}");
            anyhow!("steam init: {e:?}")
        })?;
        let steam_id = client.user().steam_id();
        crate::net_log!(
            "HOST: steam init ok (steam_id={}, code={})",
            steam_id.raw(),
            steam_id_to_code(steam_id)
        );
        let sockets = client.networking_sockets();

        let listen = sockets
            .create_listen_socket_p2p(VIRTUAL_PORT, vec![])
            .map_err(|e| {
                crate::net_log!("HOST: create_listen_socket_p2p FAILED: {e:?}");
                anyhow!("create_listen_socket_p2p: {e:?}")
            })?;
        crate::net_log!("HOST: listen socket open on virtual port {VIRTUAL_PORT}; ready for visitors");

        Ok(Self {
            client,
            single,
            sockets,
            role: Role::Host { listen },
            peers: HashMap::new(),
            peer_steam_ids: HashMap::new(),
            inbox: Arc::new(Mutex::new(Vec::new())),
            next_peer_id: 1,
        })
    }

    /// The share code for this host: the local user's SteamID, encoded so a
    /// visitor can reconstruct it and connect directly. Stable across sessions.
    pub fn local_join_code(&self) -> JoinCode {
        JoinCode(steam_id_to_code(self.client.user().steam_id()))
    }

    /// Stable networked identity for the local user, derived from their SteamID.
    /// Used as the session/avatar key so two players never collide — even when
    /// testing from a copied `save.json` that shares `zoo.player.id`.
    pub fn local_player_id(&self) -> uuid::Uuid {
        player_id_from_steam_id(self.client.user().steam_id())
    }

    /// Join a host by their share code. The code *is* the host's SteamID, so we
    /// decode it and open a P2P connection straight to them — no lobby lookup.
    /// Steam routes the rendezvous; the relay carries the traffic.
    pub fn join(code: &str) -> Result<Self> {
        crate::net_log!("JOIN: initializing Steam client…");
        let (client, single) = Client::init().map_err(|e| {
            crate::net_log!("JOIN: steam init FAILED: {e:?}");
            anyhow!("steam init: {e:?}")
        })?;
        let owner = code_to_steam_id(code).ok_or_else(|| {
            crate::net_log!("JOIN: invalid share code {code:?}");
            anyhow!("invalid join code")
        })?;
        crate::net_log!(
            "JOIN: steam init ok (me={}); code {code} -> host steam_id={}",
            client.user().steam_id().raw(),
            owner.raw()
        );
        let sockets = client.networking_sockets();

        let identity = NetworkingIdentity::new_steam_id(owner);
        let conn = sockets
            .connect_p2p(identity, VIRTUAL_PORT, vec![])
            .map_err(|e| {
                crate::net_log!("JOIN: connect_p2p to host FAILED: {e:?}");
                anyhow!("connect_p2p: {e:?}")
            })?;
        crate::net_log!("JOIN: P2P connection opened to host {}; waiting for Welcome snapshot", owner.raw());

        let host_peer = PeerId(owner.raw());
        let mut peers = HashMap::new();
        peers.insert(host_peer, conn);
        let mut peer_steam_ids = HashMap::new();
        peer_steam_ids.insert(host_peer, owner);

        let inbox = Arc::new(Mutex::new(Vec::new()));
        inbox.lock().unwrap().push(NetEvent::PeerConnected(host_peer));

        Ok(Self {
            client,
            single,
            sockets,
            role: Role::Visitor { host_peer },
            peers,
            peer_steam_ids,
            inbox,
            next_peer_id: 2,
        })
    }

    fn pump_listen_events(&mut self) {
        let Role::Host { ref mut listen, .. } = self.role else {
            return;
        };
        while let Some(ev) = listen.try_receive_event() {
            match ev {
                ListenSocketEvent::Connecting(req) => {
                    // Accept everyone; capacity gating is enforced by the
                    // session layer on `Hello`.
                    let _ = req.accept();
                }
                ListenSocketEvent::Connected(c) => {
                    let remote_sid = c.remote().steam_id();
                    let conn = c.take_connection();
                    let id = PeerId(self.next_peer_id);
                    self.next_peer_id = self.next_peer_id.wrapping_add(1);
                    self.peers.insert(id, conn);
                    if let Some(sid) = remote_sid {
                        self.peer_steam_ids.insert(id, sid);
                    }
                    self.inbox.lock().unwrap().push(NetEvent::PeerConnected(id));
                }
                ListenSocketEvent::Disconnected(d) => {
                    let Some(user) = d.remote().steam_id() else {
                        continue;
                    };
                    // Look up the local PeerId tagged with this SteamId.
                    let to_drop: Vec<PeerId> = self
                        .peer_steam_ids
                        .iter()
                        .filter_map(|(pid, sid)| (*sid == user).then_some(*pid))
                        .collect();
                    for pid in to_drop {
                        self.peers.remove(&pid);
                        self.peer_steam_ids.remove(&pid);
                        self.inbox
                            .lock()
                            .unwrap()
                            .push(NetEvent::PeerDisconnected(pid));
                    }
                }
            }
        }
    }

    fn pump_connection_messages(&mut self) {
        for (peer_id, conn) in self.peers.iter_mut() {
            let mut messages = conn.receive_messages(POLL_BATCH).unwrap_or_default();
            for raw in messages.drain(..) {
                let bytes = raw.data();
                match serde_json::from_slice::<NetMessage>(bytes) {
                    Ok(msg) => self
                        .inbox
                        .lock()
                        .unwrap()
                        .push(NetEvent::Message { from: *peer_id, msg }),
                    Err(e) => eprintln!("malformed NetMessage from {peer_id:?}: {e}"),
                }
            }
        }
    }
}

impl NetTransport for SteamTransport {
    fn poll(&mut self) -> Vec<NetEvent> {
        // Run Steam callbacks (lobby joins, matchmaking, sockets) before draining.
        self.single.run_callbacks();
        self.pump_listen_events();
        self.pump_connection_messages();
        std::mem::take(&mut *self.inbox.lock().unwrap())
    }
    fn send(&mut self, to: PeerId, msg: NetMessage) {
        let Some(conn) = self.peers.get_mut(&to) else {
            return;
        };
        let Ok(bytes) = serde_json::to_vec(&msg) else {
            return;
        };
        let _ = conn.send_message(&bytes, SendFlags::RELIABLE);
    }
    fn broadcast(&mut self, msg: NetMessage, except: Option<PeerId>) {
        let Ok(bytes) = serde_json::to_vec(&msg) else {
            return;
        };
        for (pid, conn) in self.peers.iter_mut() {
            if Some(*pid) == except {
                continue;
            }
            let _ = conn.send_message(&bytes, SendFlags::RELIABLE);
        }
    }
    fn disconnect(&mut self, peer: PeerId) {
        if let Some(conn) = self.peers.remove(&peer) {
            conn.close(NetConnectionEnd::AppGeneric, Some("leaving"), true);
            self.peer_steam_ids.remove(&peer);
            self.inbox
                .lock()
                .unwrap()
                .push(NetEvent::PeerDisconnected(peer));
        }
    }
    fn shutdown(&mut self) {
        let peers: Vec<PeerId> = self.peers.keys().copied().collect();
        for p in peers {
            self.disconnect(p);
        }
        // No lobby to leave — discovery is codeless P2P.
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Share-code codec: the host's SteamID encoded as a short, typeable string.
//
// A user SteamID64 is `INDIVIDUAL_BASE + account_id`, where `account_id` is the
// low 32 bits. We encode just the account id in base32 (7 chars) and rebuild
// the full id assuming a normal individual/public account — true for every
// human Steam user. This makes the code self-resolving: no lobby, no backend.
// ──────────────────────────────────────────────────────────────────────────

use super::protocol::CODE_LEN;

/// Same Crockford-style base32 alphabet as `JoinCode`; the index is the digit.
const CODE_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// SteamID64 of account id 0 for an individual account in the public universe
/// (`0x0110_0001_0000_0000`). Full id = this + account_id. `CODE_LEN` (7) base32
/// digits cover the full 32-bit account id space.
const STEAMID64_INDIVIDUAL_BASE: u64 = 76_561_197_960_265_728;

/// Encode a user SteamID into its share code (account id, base32, [`CODE_LEN`]).
fn steam_id_to_code(id: SteamId) -> String {
    let mut v = id.account_id().raw();
    let mut buf = [b'0'; CODE_LEN];
    for slot in buf.iter_mut().rev() {
        *slot = CODE_ALPHABET[(v % 32) as usize];
        v /= 32;
    }
    String::from_utf8(buf.to_vec()).expect("base32 alphabet is ASCII")
}

/// Decode a share code back into the host's SteamID. Case-insensitive; returns
/// `None` if the code is empty or contains a character outside the alphabet.
fn code_to_steam_id(code: &str) -> Option<SteamId> {
    let mut account_id: u64 = 0;
    let mut digits = 0;
    for c in code.trim().bytes() {
        let up = c.to_ascii_uppercase();
        let idx = CODE_ALPHABET.iter().position(|&a| a == up)?;
        account_id = account_id * 32 + idx as u64;
        digits += 1;
    }
    if digits == 0 {
        return None;
    }
    let raw = STEAMID64_INDIVIDUAL_BASE + (account_id & 0xFFFF_FFFF);
    Some(SteamId::from_raw(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_round_trips_through_steam_id() {
        // A couple of real-world-shaped account ids plus edge values.
        for raw in [
            STEAMID64_INDIVIDUAL_BASE,
            STEAMID64_INDIVIDUAL_BASE + 1,
            76_561_198_220_452_768, // from a test machine
            76_561_198_139_757_552, // the other test machine
            STEAMID64_INDIVIDUAL_BASE + u32::MAX as u64,
        ] {
            let id = SteamId::from_raw(raw);
            let code = steam_id_to_code(id);
            assert_eq!(code.len(), CODE_LEN);
            assert_eq!(code_to_steam_id(&code).map(|d| d.raw()), Some(raw));
        }
    }

    #[test]
    fn decode_is_case_insensitive_and_rejects_junk() {
        let id = SteamId::from_raw(76_561_198_220_452_768);
        let code = steam_id_to_code(id);
        assert_eq!(code_to_steam_id(&code.to_lowercase()).map(|d| d.raw()), Some(id.raw()));
        assert!(code_to_steam_id("").is_none());
        assert!(code_to_steam_id("ABC!XYZ").is_none()); // '!' not in alphabet
    }
}

/// Stable online identity for SpacetimeDB: `(player_key, persona_name)` read
/// from the local Steam client. `player_key` is `"steam:<steamid64>"` — stable
/// for the Steam account across machines/reinstalls — so the hub reuses the same
/// server account instead of spawning a duplicate each connect. Returns `None`
/// if Steam isn't running / the client can't init.
pub fn online_identity() -> Option<(String, String)> {
    let (client, _single) = Client::init().ok()?;
    let steam_id = client.user().steam_id().raw();
    let name = client.friends().name();
    Some((format!("steam:{steam_id}"), name))
}

/// Derive a stable visitor `player_id` from a SteamID. Same SteamID always
/// returns the same UUID, so a returning visitor matches their stored
/// `VisitorRecord` at every host that's ever seen them.
pub fn player_id_from_steam_id(steam_id: SteamId) -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, &steam_id.raw().to_be_bytes())
}
