//! Session: the live set of avatars + connected peers + per-peer controllers.
//! Owns no networking guts itself — it drives a `Box<dyn NetTransport>` and
//! reacts to its events. Solo play uses `Session::solo()` and no transport.

use std::collections::HashMap;

use chrono::Utc;
use macroquad::math::{Vec2, vec2};
use uuid::Uuid;

use crate::game::action::Action;
use crate::game::avatar::{Facing, PlayerAvatar};
use crate::game::avatar_system::{PLANE_H, PLANE_W};
use crate::game::visitor::VisitorRecord;
use crate::game::zoo::Zoo;

use super::protocol::{AvatarPose, ByeReason, JoinCode, NetMessage, PeerId, WireIntent};
use super::transport::{NetEvent, NetTransport};

pub const MAX_PEERS: usize = 3; // up to 3 visitors → 4-player zoo (host + 3)

/// Live peer connected to the host.
pub struct Peer {
    pub peer_id: PeerId,
    pub player_id: Uuid,
    pub display_name: String,
    /// Last intent received from this peer; the RemoteController for the
    /// peer reads from here.
    pub last_intent: WireIntent,
}

pub enum SessionRole {
    Solo,
    Host {
        code: JoinCode,
        peers: HashMap<PeerId, Peer>,
    },
    Visiting {
        host_peer: PeerId,
        my_player_id: Uuid,
        /// Whether the host has welcomed us yet (host_player_id known etc.).
        welcomed: bool,
    },
}

pub struct Session {
    pub role: SessionRole,
    /// All in-world avatars including the local player. Keyed by `player_id`.
    pub avatars: HashMap<Uuid, PlayerAvatar>,
    /// The local player's `player_id` — used to find "my" avatar.
    pub local_player_id: Uuid,
    /// Optional transport; `None` in Solo.
    pub transport: Option<Box<dyn NetTransport>>,
    /// Monotonic tick counter for delta messages.
    pub tick: u32,
    /// Set on the visitor when the host goes away (sent us `Bye`, or the
    /// transport dropped). `GameApp` reads this to show the disconnect screen.
    pub host_gone: Option<ByeReason>,
}

impl Session {
    pub fn solo(local_player_id: Uuid, spawn: Vec2) -> Self {
        let mut avatars = HashMap::new();
        avatars.insert(local_player_id, PlayerAvatar::new(local_player_id, spawn));
        Self {
            role: SessionRole::Solo,
            avatars,
            local_player_id,
            transport: None,
            tick: 0,
            host_gone: None,
        }
    }

    /// Promote a solo session to a host session with the given transport.
    /// The local avatar stays put; remote peers get added as they connect.
    pub fn become_host(&mut self, transport: Box<dyn NetTransport>, code: JoinCode) {
        self.transport = Some(transport);
        self.role = SessionRole::Host {
            code,
            peers: HashMap::new(),
        };
    }

    /// Tear down hosting; revert to solo. Notifies peers via `Bye(HostShutdown)`.
    pub fn end_hosting(&mut self) {
        if let SessionRole::Host { peers, .. } = &mut self.role {
            if let Some(t) = self.transport.as_mut() {
                for peer_id in peers.keys().copied().collect::<Vec<_>>() {
                    t.send(peer_id, NetMessage::Bye(ByeReason::HostShutdown));
                    t.disconnect(peer_id);
                }
                t.shutdown();
            }
        }
        // Drop remote avatars; keep the local one.
        let mine = self.local_player_id;
        self.avatars.retain(|id, _| *id == mine);
        self.transport = None;
        self.role = SessionRole::Solo;
    }

    /// Tear down a visiting session: politely tell the host we're leaving and
    /// drop the transport. `GameApp` then rebuilds a fresh solo session from the
    /// player's own (reloaded) zoo, so we don't bother reverting state here.
    pub fn end_visiting(&mut self) {
        if let SessionRole::Visiting { host_peer, .. } = self.role {
            if let Some(t) = self.transport.as_mut() {
                t.send(host_peer, NetMessage::Goodbye);
                t.shutdown();
            }
        }
        self.transport = None;
        self.role = SessionRole::Solo;
        self.host_gone = None;
    }

    /// Construct a visiting session — the local player is a guest in someone
    /// else's zoo. `host_peer` is the transport-level handle for the host.
    pub fn visit(
        local_player_id: Uuid,
        transport: Box<dyn NetTransport>,
        host_peer: PeerId,
    ) -> Self {
        let mut avatars = HashMap::new();
        avatars.insert(
            local_player_id,
            PlayerAvatar::new(local_player_id, vec2(PLANE_W * 0.5, PLANE_H * 0.5)),
        );
        Self {
            role: SessionRole::Visiting {
                host_peer,
                my_player_id: local_player_id,
                welcomed: false,
            },
            avatars,
            local_player_id,
            transport: Some(transport),
            tick: 0,
            host_gone: None,
        }
    }

    /// Re-key the local player's identity (and their avatar) to `new_id`. Used
    /// to switch from the save-derived `zoo.player.id` to a stable, unique
    /// networked id (SteamID-derived) when a session goes online, so the host
    /// and visitors never collide on the same `avatars` key.
    pub fn set_local_player_id(&mut self, new_id: Uuid) {
        let old = self.local_player_id;
        if old == new_id {
            return;
        }
        if let Some(mut a) = self.avatars.remove(&old) {
            a.player_id = new_id;
            self.avatars.insert(new_id, a);
        }
        self.local_player_id = new_id;
    }

    pub fn join_code(&self) -> Option<&JoinCode> {
        match &self.role {
            SessionRole::Host { code, .. } => Some(code),
            _ => None,
        }
    }

    pub fn peer_count(&self) -> usize {
        match &self.role {
            SessionRole::Host { peers, .. } => peers.len(),
            _ => 0,
        }
    }

    pub fn my_avatar_mut(&mut self) -> &mut PlayerAvatar {
        let id = self.local_player_id;
        self.avatars.get_mut(&id).expect("local avatar exists")
    }

    pub fn my_avatar(&self) -> &PlayerAvatar {
        let id = self.local_player_id;
        self.avatars.get(&id).expect("local avatar exists")
    }

    /// Poll the transport and dispatch events. Returns `(intents, commands)`:
    /// inbound `WireIntent`s keyed by `player_id` (for per-peer
    /// `RemoteController`s) and inbound visitor `Action`s keyed by `player_id`
    /// (for the host to apply authoritatively). Both empty on the visitor side.
    pub fn pump(&mut self, zoo: &mut Zoo) -> (HashMap<Uuid, WireIntent>, Vec<(Uuid, Action)>) {
        let mut intents: HashMap<Uuid, WireIntent> = HashMap::new();
        let mut commands: Vec<(Uuid, Action)> = Vec::new();
        let Some(t) = self.transport.as_mut() else {
            return (intents, commands);
        };
        let events = t.poll();
        for ev in events {
            match ev {
                NetEvent::PeerConnected(peer) => {
                    // Visitor side: as soon as we're connected to the host, send
                    // `Hello` so it registers us, spawns our avatar in its world,
                    // and replies with `Welcome` + a full snapshot. Without this
                    // the connection sits idle and nothing happens in-game.
                    let greet = match &self.role {
                        SessionRole::Visiting { host_peer, my_player_id, .. }
                            if peer == *host_peer =>
                        {
                            Some((*host_peer, *my_player_id))
                        }
                        _ => None,
                    };
                    if let Some((host_peer, my_player_id)) = greet {
                        let display_name = zoo.player.name.clone();
                        if let Some(t) = self.transport.as_mut() {
                            crate::net_log!(
                                "JOIN: connected to host; sending Hello (player_id={my_player_id}, name={display_name})"
                            );
                            t.send(
                                host_peer,
                                NetMessage::Hello { player_id: my_player_id, display_name },
                            );
                        }
                    }
                }
                NetEvent::PeerDisconnected(peer) => {
                    // Visitor losing the host's connection → flag a disconnect so
                    // GameApp can surface the screen and restore the own zoo.
                    if matches!(&self.role, SessionRole::Visiting { host_peer, .. } if peer == *host_peer) {
                        self.host_gone = Some(ByeReason::ConnectionLost);
                    }
                    handle_disconnect(&mut self.role, &mut self.avatars, zoo, peer);
                }
                NetEvent::Message { from, msg } => {
                    let from_host =
                        matches!(&self.role, SessionRole::Visiting { host_peer, .. } if from == *host_peer);
                    match msg {
                        // Visitor-only host→client streams handled here so the
                        // free `handle_message` can stay zoo/avatar focused.
                        NetMessage::Bye(reason) if from_host => {
                            self.host_gone = Some(reason);
                        }
                        other => {
                            let new_local = handle_message(
                                &mut self.role,
                                &mut self.avatars,
                                zoo,
                                self.transport.as_mut().unwrap(),
                                self.local_player_id,
                                from,
                                other,
                                &mut intents,
                                &mut commands,
                            );
                            if let Some(id) = new_local {
                                self.local_player_id = id;
                            }
                        }
                    }
                }
            }
        }
        (intents, commands)
    }

    /// Broadcast a full authoritative ZooSnapshot to every peer. Host-only.
    /// Called by the host on a low-frequency cadence (or after a known
    /// mutation) so visitors see currency/animal/habitat changes.
    pub fn broadcast_world_snapshot(&mut self, zoo: &Zoo) {
        let SessionRole::Host { .. } = &self.role else {
            return;
        };
        let Some(t) = self.transport.as_mut() else {
            return;
        };
        let snapshot = crate::persistence::snapshot_from_zoo(zoo);
        t.broadcast(NetMessage::WorldSnapshot(snapshot), None);
    }

    /// Broadcast the current avatar poses to all connected peers. Host-only.
    pub fn broadcast_avatars(&mut self) {
        let SessionRole::Host { .. } = &self.role else {
            return;
        };
        let Some(t) = self.transport.as_mut() else {
            return;
        };
        self.tick = self.tick.wrapping_add(1);
        let avatars: Vec<AvatarPose> = self
            .avatars
            .values()
            .map(|a| AvatarPose {
                player_id: a.player_id,
                pos_x: a.pos.x,
                pos_y: a.pos.y,
                vel_x: a.vel.x,
                vel_y: a.vel.y,
                facing: encode_facing(a.facing),
            })
            .collect();
        t.broadcast(
            NetMessage::AvatarsDelta {
                tick: self.tick,
                avatars,
            },
            None,
        );
    }
}

fn handle_disconnect(
    role: &mut SessionRole,
    avatars: &mut HashMap<Uuid, PlayerAvatar>,
    zoo: &mut Zoo,
    peer: PeerId,
) {
    match role {
        SessionRole::Host { peers, .. } => {
            if let Some(p) = peers.remove(&peer) {
                // Persist the visitor's last position before dropping their
                // avatar so a future rejoin lands where they left off.
                if let Some(a) = avatars.remove(&p.player_id) {
                    if let Some(rec) = zoo.visitors.get_mut(&p.player_id) {
                        rec.last_pos = a.pos;
                        rec.last_visited_at = Utc::now();
                    }
                }
            }
        }
        SessionRole::Visiting { .. } => {
            // Host went away; caller (GameApp) reverts to solo by replacing
            // the session.
        }
        SessionRole::Solo => {}
    }
}

/// Returns `Some(new_local_player_id)` when the host's Welcome reassigned us
/// to a different id (so the session can re-key its local pointer).
#[allow(clippy::too_many_arguments)]
fn handle_message(
    role: &mut SessionRole,
    avatars: &mut HashMap<Uuid, PlayerAvatar>,
    zoo: &mut Zoo,
    transport: &mut Box<dyn NetTransport>,
    local_player_id: Uuid,
    from: PeerId,
    msg: NetMessage,
    intents: &mut HashMap<Uuid, WireIntent>,
    commands: &mut Vec<(Uuid, Action)>,
) -> Option<Uuid> {
    match (role, msg) {
        (
            SessionRole::Host { peers, .. },
            NetMessage::Hello {
                player_id,
                display_name,
            },
        ) => {
            if peers.len() >= MAX_PEERS {
                transport.send(from, NetMessage::Bye(ByeReason::Full));
                transport.disconnect(from);
                return None;
            }
            // Restore last_pos from VisitorRecord if known, else spawn at center.
            let now = Utc::now();
            let spawn = zoo
                .visitors
                .get(&player_id)
                .map(|v| v.last_pos)
                .unwrap_or(vec2(PLANE_W * 0.5, PLANE_H * 0.5));
            let rec = zoo
                .visitors
                .entry(player_id)
                .or_insert_with(|| VisitorRecord::new(player_id, &display_name, now));
            rec.display_name = display_name.clone();
            rec.last_visited_at = now;

            peers.insert(
                from,
                Peer {
                    peer_id: from,
                    player_id,
                    display_name,
                    last_intent: WireIntent::default(),
                },
            );
            avatars.insert(player_id, PlayerAvatar::new(player_id, spawn));
            let snapshot = crate::persistence::snapshot_from_zoo(zoo);
            transport.send(
                from,
                NetMessage::Welcome {
                    your_player_id: player_id,
                    host_name: zoo.player.name.clone(),
                    spawn_x: spawn.x,
                    spawn_y: spawn.y,
                    snapshot,
                },
            );
            crate::net_log!(
                "HOST: got Hello → welcomed player_id={player_id} (peer {from:?}); now {} visitor(s)",
                peers.len()
            );
        }
        (SessionRole::Host { peers, .. }, NetMessage::Intent(intent)) => {
            if let Some(p) = peers.get_mut(&from) {
                p.last_intent = intent;
                intents.insert(p.player_id, intent);
            }
        }
        (SessionRole::Host { peers, .. }, NetMessage::Command(action)) => {
            // Surface the visitor's action for the host to apply authoritatively
            // (GameApp drains `commands` after pump and runs `dispatch_remote`).
            if let Some(p) = peers.get(&from) {
                commands.push((p.player_id, action));
            }
        }
        (SessionRole::Host { peers, .. }, NetMessage::DropGift { species, level }) => {
            // Resolve species; only catalog-known species are accepted.
            let Some(p) = peers.get(&from) else {
                return None;
            };
            let Some(def) = crate::game::species::try_get(&species) else {
                return None;
            };
            let now = Utc::now();
            let rec = zoo
                .visitors
                .entry(p.player_id)
                .or_insert_with(|| {
                    crate::game::visitor::VisitorRecord::new(p.player_id, &p.display_name, now)
                });
            rec.gift_inbox.push(crate::game::visitor::GiftRecord {
                id: Uuid::new_v4(),
                sender_id: p.player_id,
                sender_name: p.display_name.clone(),
                species: def.id,
                level,
                dropped_at: now,
            });
        }
        (SessionRole::Host { peers, .. }, NetMessage::Goodbye) => {
            if let Some(p) = peers.remove(&from) {
                if let Some(a) = avatars.remove(&p.player_id) {
                    if let Some(rec) = zoo.visitors.get_mut(&p.player_id) {
                        rec.last_pos = a.pos;
                        rec.last_visited_at = Utc::now();
                    }
                }
            }
            transport.disconnect(from);
        }
        (
            SessionRole::Visiting {
                my_player_id,
                welcomed,
                ..
            },
            NetMessage::Welcome {
                your_player_id,
                spawn_x,
                spawn_y,
                snapshot,
                ..
            },
        ) => {
            *my_player_id = your_player_id;
            *welcomed = true;
            crate::net_log!(
                "JOIN: received Welcome → player_id={your_player_id}, spawning at ({spawn_x:.0},{spawn_y:.0}); loading host's world"
            );
            if let Some(mut a) = avatars.remove(&local_player_id) {
                a.player_id = your_player_id;
                a.pos = vec2(spawn_x, spawn_y);
                avatars.insert(your_player_id, a);
            }
            // Replace local zoo with the host's authoritative state.
            apply_snapshot_to_zoo(zoo, snapshot);
            return Some(your_player_id);
        }
        (SessionRole::Visiting { .. }, NetMessage::WorldSnapshot(snapshot)) => {
            apply_snapshot_to_zoo(zoo, snapshot);
        }
        (
            SessionRole::Visiting { .. },
            NetMessage::AvatarsDelta { avatars: poses, .. },
        ) => {
            for pose in poses {
                let entry = avatars
                    .entry(pose.player_id)
                    .or_insert_with(|| PlayerAvatar::new(pose.player_id, vec2(pose.pos_x, pose.pos_y)));
                entry.pos = vec2(pose.pos_x, pose.pos_y);
                entry.vel = vec2(pose.vel_x, pose.vel_y);
                entry.facing = decode_facing(pose.facing);
            }
        }
        _ => { /* ignore unexpected — be liberal in what we accept */ }
    }
    None
}

/// Apply an authoritative ZooSnapshot from the host into the visitor's
/// local Zoo. Failure modes (unknown species etc.) fall back to keeping the
/// previous state; visitor isn't authoritative so silent tolerance is fine.
fn apply_snapshot_to_zoo(zoo: &mut Zoo, snapshot: crate::persistence::schema::ZooSnapshot) {
    match crate::persistence::zoo_from_snapshot(snapshot) {
        Ok(loaded) => {
            // Preserve only our own player identity (name/id) so our avatar and
            // Hello keep working. We DO adopt the host's `visitors` map — while
            // visiting, the shared zoo is the host's, and it carries our own
            // granted permissions (read for the Sell button). Our real zoo's
            // visitor records are offloaded and restored from disk on leave.
            let local_player = zoo.player.clone();
            *zoo = loaded.zoo;
            zoo.player = local_player;
        }
        Err(e) => eprintln!("ignored world snapshot: {e}"),
    }
}

pub fn encode_facing(f: Facing) -> u8 {
    match f {
        Facing::N => 0,
        Facing::E => 1,
        Facing::S => 2,
        Facing::W => 3,
    }
}

pub fn decode_facing(v: u8) -> Facing {
    match v % 4 {
        0 => Facing::N,
        1 => Facing::E,
        2 => Facing::S,
        _ => Facing::W,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rekey_local_id_moves_avatar_and_updates_pointer() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut s = Session::solo(a, vec2(10.0, 20.0));
        assert!(s.avatars.contains_key(&a));

        s.set_local_player_id(b);
        assert_eq!(s.local_player_id, b);
        assert!(s.avatars.contains_key(&b), "avatar re-keyed to new id");
        assert!(!s.avatars.contains_key(&a), "old key removed");
        assert_eq!(s.avatars.len(), 1, "no duplicate avatar");
        // Position/identity carried over.
        assert_eq!(s.my_avatar().pos, vec2(10.0, 20.0));
        assert_eq!(s.my_avatar().player_id, b);
    }

    #[test]
    fn rekey_to_same_id_is_noop() {
        let a = Uuid::new_v4();
        let mut s = Session::solo(a, vec2(0.0, 0.0));
        s.set_local_player_id(a);
        assert_eq!(s.local_player_id, a);
        assert_eq!(s.avatars.len(), 1);
    }
}
