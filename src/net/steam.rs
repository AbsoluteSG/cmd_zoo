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
//! Discovery: hosts create a private Steam lobby and stash the 6-char join
//! code as `lobby_data["code"]`. Visitors enumerate lobbies filtered on
//! that code and connect to the lobby owner's NetworkingSockets P2P
//! endpoint on virtual port 0.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use steamworks::{
    Client, ClientManager, LobbyId, LobbyType, Matchmaking, SteamId,
    networking_sockets::{ListenSocket, NetConnection, NetworkingSockets},
    networking_types::{
        ListenSocketEvent, NetConnectionEnd, NetworkingIdentity, SendFlags,
    },
};

use super::protocol::{NetMessage, PeerId};
use super::transport::{NetEvent, NetTransport};

/// Virtual port — both sides agree on 0. Higher ports let one app multiplex
/// multiple unrelated channels; we only need the one.
const VIRTUAL_PORT: i32 = 0;
/// Max in-flight messages drained per `poll`. Bounds frame budget.
const POLL_BATCH: usize = 64;

/// Host-side transport: owns the lobby + listen socket.
pub struct SteamTransport {
    client: Client<ClientManager>,
    single: steamworks::SingleClient<ClientManager>,
    sockets: NetworkingSockets<ClientManager>,
    matchmaking: Matchmaking<ClientManager>,
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
        lobby: LobbyId,
        listen: ListenSocket<ClientManager>,
    },
    Visitor {
        host_peer: PeerId,
    },
}

impl SteamTransport {
    /// Start hosting. Creates a private lobby tagged with `code` and a
    /// NetworkingSockets listen endpoint.
    pub fn host(code: &str) -> Result<Self> {
        let (client, single) = Client::init().context("steam init")?;
        let sockets = client.networking_sockets();
        let matchmaking = client.matchmaking();

        let lobby = create_lobby_sync(&matchmaking, LobbyType::Private, 4)?;
        matchmaking.set_lobby_data(lobby, "code", code);

        let listen = sockets
            .create_listen_socket_p2p(VIRTUAL_PORT, vec![])
            .map_err(|e| anyhow!("create_listen_socket_p2p: {e:?}"))?;

        Ok(Self {
            client,
            single,
            sockets,
            matchmaking,
            role: Role::Host { lobby, listen },
            peers: HashMap::new(),
            peer_steam_ids: HashMap::new(),
            inbox: Arc::new(Mutex::new(Vec::new())),
            next_peer_id: 1,
        })
    }

    /// Join a host by their 6-char code. Looks up the lobby, then opens a
    /// P2P NetConnection to the lobby owner.
    pub fn join(code: &str) -> Result<Self> {
        let (client, single) = Client::init().context("steam init")?;
        let sockets = client.networking_sockets();
        let matchmaking = client.matchmaking();

        let lobby = find_lobby_by_code_sync(&matchmaking, code)?;
        let owner: SteamId = matchmaking.lobby_owner(lobby);
        let identity = NetworkingIdentity::new_steam_id(owner);
        let conn = sockets
            .connect_p2p(identity, VIRTUAL_PORT, vec![])
            .map_err(|e| anyhow!("connect_p2p: {e:?}"))?;

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
            matchmaking,
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
        if let Role::Host { lobby, .. } = &self.role {
            self.matchmaking.leave_lobby(*lobby);
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Synchronous helpers wrapping Steam's async lobby APIs.
//
// Steam callbacks fire on the main thread when we tick `single.run_callbacks`.
// We poll-wait up to a small timeout — fine for a UI-driven action like
// "create lobby" / "look up code", where the user can tolerate ~1s latency.
// ──────────────────────────────────────────────────────────────────────────

fn create_lobby_sync(
    matchmaking: &Matchmaking<ClientManager>,
    kind: LobbyType,
    max_members: u32,
) -> Result<LobbyId> {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    matchmaking.create_lobby(kind, max_members, move |res| {
        let _ = tx.send(res);
    });
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| anyhow!("create_lobby timed out"))?
        .map_err(|e| anyhow!("create_lobby error: {e:?}"))
}

fn find_lobby_by_code_sync(
    matchmaking: &Matchmaking<ClientManager>,
    code: &str,
) -> Result<LobbyId> {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    matchmaking.request_lobby_list(move |res| {
        let _ = tx.send(res);
    });
    let lobbies = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| anyhow!("lobby search timed out"))?
        .map_err(|e| anyhow!("request_lobby_list error: {e:?}"))?;
    for l in lobbies {
        if matchmaking.lobby_data(l, "code").as_deref() == Some(code) {
            return Ok(l);
        }
    }
    Err(anyhow!("no lobby found with code {code}"))
}

/// Derive a stable visitor `player_id` from a SteamID. Same SteamID always
/// returns the same UUID, so a returning visitor matches their stored
/// `VisitorRecord` at every host that's ever seen them.
pub fn player_id_from_steam_id(steam_id: SteamId) -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, &steam_id.raw().to_be_bytes())
}
