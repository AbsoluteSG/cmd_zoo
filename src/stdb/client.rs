//! Hand-written online adapter over the generated [`super::bindings`].
//!
//! Wraps a SpacetimeDB [`DbConnection`] driven on a background thread
//! (`run_threaded`): callbacks + cache updates happen off the render thread, and
//! the game reads the subscribed row cache each frame via the snapshot methods
//! here. Player actions are sent as reducer calls.
//!
//! Identity is **anonymous** for now — the SDK mints a fresh identity on first
//! connect. Linking it to the Steam id (UUIDv5, see `crate::net::steam`) comes
//! later. Solo / offline play does not touch this module.

use glam::{Vec2, vec2};
use spacetimedb_sdk::{DbContext, Identity, Table};
use uuid::Uuid;

use std::collections::HashMap;

use super::bindings::{
    AccountTableAccess, AvatarPoseTableAccess, CaptureEventTableAccess, DbConnection,
    InstanceAnimalTableAccess, InstanceMemberTableAccess, InstanceTableAccess,
    PartyInviteTableAccess, PartyMemberTableAccess,
    ZooTableAccess, accept_party_invite, apply_action, capture_animal, decline_party_invite,
    engage_animal, enter_expedition, invite_to_party, join_hub, leave_expedition, leave_party,
    move_avatar, release_animal,
};

/// Maincloud endpoint (the managed SpacetimeDB host).
pub const MAINCLOUD_URI: &str = "https://maincloud.spacetimedb.com";
/// The published module / database name (see `spacetime publish`).
pub const DEFAULT_MODULE: &str = "critter-cove";

/// A flattened view of a subscribed `zoo` row for the renderer/app.
#[derive(Clone, Debug)]
pub struct ZooView {
    pub owner_key: String,
    pub slot: u64,
    pub plot: (f32, f32),
    pub coins: u64,
    /// Server-derived Power Score (progression metric); shown on nameplates / the
    /// party panel without deserializing the snapshot.
    pub power_score: u64,
    pub snapshot_json: String,
}

/// A live connection to the online hub. Dropping it stops the worker thread.
pub struct OnlineClient {
    conn: DbConnection,
    /// Our **stable** player key (SteamID-derived, `"steam:<id>"` / `"local:<uuid>"`).
    /// This — not the ephemeral SpacetimeDB identity — is how the server and we
    /// agree on which account/zoo/avatar is "ours" across reconnects.
    my_key: String,
    /// Display name sent to the server on join (Steam persona name when available).
    my_name: String,
    _worker: std::thread::JoinHandle<()>,
}

impl OnlineClient {
    /// Connect to `module` at `uri` as the player identified by the stable
    /// `player_key` (+ display `name`), subscribe to the hub tables, and start
    /// the background message pump. Non-blocking: the connection completes
    /// asynchronously (watch the `on_connect`/`on_connect_error` logs).
    pub fn connect(uri: &str, module: &str, player_key: &str, name: &str) -> anyhow::Result<Self> {
        let conn = DbConnection::builder()
            .with_uri(uri)
            .with_database_name(module)
            .on_connect(|_ctx, identity, _token| {
                log_line(format!("connected as {identity}"));
            })
            .on_connect_error(|_ctx, err| {
                log_line(format!("connect error: {err}"));
            })
            .on_disconnect(|_ctx, err| {
                log_line(format!("disconnected: {err:?}"));
            })
            .build()?;

        conn.subscription_builder()
            .on_applied(|_ctx| log_line("subscription applied".into()))
            .on_error(|_ctx, err| log_line(format!("subscription error: {err}")))
            .subscribe([
                "SELECT * FROM account",
                "SELECT * FROM zoo",
                "SELECT * FROM avatar_pose",
                "SELECT * FROM party",
                "SELECT * FROM party_member",
                "SELECT * FROM party_invite",
                "SELECT * FROM instance",
                "SELECT * FROM instance_member",
                "SELECT * FROM instance_animal",
                "SELECT * FROM instance_engagement",
                "SELECT * FROM capture_event",
            ]);

        let worker = conn.run_threaded();
        Ok(Self {
            conn,
            my_key: player_key.to_string(),
            my_name: name.to_string(),
            _worker: worker,
        })
    }

    /// Connect to the default Maincloud deployment as `player_key` / `name`.
    pub fn connect_maincloud(player_key: &str, name: &str) -> anyhow::Result<Self> {
        Self::connect(MAINCLOUD_URI, DEFAULT_MODULE, player_key, name)
    }

    /// Our stable player key (see [`OnlineClient::my_key`]).
    pub fn my_key(&self) -> &str {
        &self.my_key
    }

    /// Our own display name (Steam persona name when available).
    pub fn my_name(&self) -> &str {
        &self.my_name
    }

    /// Display names of every known account, keyed by the same render UUID used
    /// for avatar identity/colour ([`key_uuid`]). Lets the renderer label each
    /// avatar with its owner's username.
    pub fn account_names(&self) -> HashMap<Uuid, String> {
        self.conn
            .db
            .account()
            .iter()
            .map(|a| (key_uuid(&a.player_key), a.name))
            .collect()
    }

    /// Our own SpacetimeDB connection identity, once established. (Ephemeral —
    /// prefer [`my_key`](Self::my_key) for "is this me?" checks.)
    pub fn identity(&self) -> Option<Identity> {
        self.conn.try_identity()
    }

    // ── Reducer calls (mutations) ───────────────────────────────────────────────

    /// Place us on the hub under our stable key (assigns a plot slot + creates
    /// our zoo on first join; rebinds the existing account on a rejoin).
    pub fn join_hub(&self) -> anyhow::Result<()> {
        self.conn
            .reducers
            .join_hub(self.my_key.clone(), self.my_name.clone())
            .map_err(|e| anyhow::anyhow!("join_hub: {e}"))
    }

    /// Update our avatar position (presence).
    pub fn move_avatar(&self, x: f32, y: f32) -> anyhow::Result<()> {
        self.conn
            .reducers
            .move_avatar(x, y)
            .map_err(|e| anyhow::anyhow!("move_avatar: {e}"))
    }

    /// Apply a serialized [`crate::game::action::Action`] to our zoo server-side.
    pub fn apply_action_json(&self, action_json: String) -> anyhow::Result<()> {
        self.conn
            .reducers
            .apply_action(action_json)
            .map_err(|e| anyhow::anyhow!("apply_action: {e}"))
    }

    // ── Party reducer calls ─────────────────────────────────────────────────────

    /// Invite a nearby player (by their stable key) to a party (server
    /// proximity-gates the call).
    pub fn invite_to_party(&self, target_key: &str) -> anyhow::Result<()> {
        self.conn
            .reducers
            .invite_to_party(target_key.to_string())
            .map_err(|e| anyhow::anyhow!("invite_to_party: {e}"))
    }

    /// Accept a pending party invite addressed to us.
    pub fn accept_party_invite(&self, invite_id: u64) -> anyhow::Result<()> {
        self.conn
            .reducers
            .accept_party_invite(invite_id)
            .map_err(|e| anyhow::anyhow!("accept_party_invite: {e}"))
    }

    /// Decline a pending party invite addressed to us.
    pub fn decline_party_invite(&self, invite_id: u64) -> anyhow::Result<()> {
        self.conn
            .reducers
            .decline_party_invite(invite_id)
            .map_err(|e| anyhow::anyhow!("decline_party_invite: {e}"))
    }

    /// Leave our current party.
    pub fn leave_party(&self) -> anyhow::Result<()> {
        self.conn
            .reducers
            .leave_party()
            .map_err(|e| anyhow::anyhow!("leave_party: {e}"))
    }

    // ── Expedition instance reducer calls ───────────────────────────────────────

    /// Enter (or resume) the shared expedition instance for `theme` (party-scoped
    /// routing happens server-side).
    pub fn enter_expedition(&self, theme: &str) -> anyhow::Result<()> {
        self.conn
            .reducers
            .enter_expedition(theme.to_string())
            .map_err(|e| anyhow::anyhow!("enter_expedition: {e}"))
    }

    /// Leave our current expedition instance (back to the hub).
    pub fn leave_expedition(&self) -> anyhow::Result<()> {
        self.conn
            .reducers
            .leave_expedition()
            .map_err(|e| anyhow::anyhow!("leave_expedition: {e}"))
    }

    /// Begin a co-op engagement against `animal_uuid` (marks us a participant).
    pub fn engage_animal(&self, animal_uuid: &str) -> anyhow::Result<()> {
        self.conn
            .reducers
            .engage_animal(animal_uuid.to_string())
            .map_err(|e| anyhow::anyhow!("engage_animal: {e}"))
    }

    /// Stop engaging `animal_uuid` (dropped participation).
    pub fn release_animal(&self, animal_uuid: &str) -> anyhow::Result<()> {
        self.conn
            .reducers
            .release_animal(animal_uuid.to_string())
            .map_err(|e| anyhow::anyhow!("release_animal: {e}"))
    }

    /// Catch the animal `animal_uuid`: server credits every participant + removes
    /// it from the shared instance.
    pub fn capture_animal(&self, animal_uuid: &str) -> anyhow::Result<()> {
        self.conn
            .reducers
            .capture_animal(animal_uuid.to_string())
            .map_err(|e| anyhow::anyhow!("capture_animal: {e}"))
    }

    // ── Subscribed-state snapshots (reads of the client cache) ───────────────────

    /// Our current expedition as `(instance_id, theme, seed)` if we're in one.
    pub fn my_instance(&self) -> Option<(u64, String, u64)> {
        let me = self.my_key.as_str();
        let instance_id = self
            .conn
            .db
            .instance_member()
            .iter()
            .find(|m| m.member_key == me)?
            .instance_id;
        self.conn
            .db
            .instance()
            .iter()
            .find(|i| i.instance_id == instance_id)
            .map(|i| (i.instance_id, i.theme, i.seed))
    }

    /// Party capture toasts addressed to us as `(event_id, catcher_name, species)`
    /// — a partymate caught something. The caller dedupes by `event_id`.
    pub fn my_capture_events(&self) -> Vec<(u64, String, String)> {
        let me = self.my_key.as_str();
        self.conn
            .db
            .capture_event()
            .iter()
            .filter(|e| e.recipient_key == me)
            .map(|e| (e.event_id, e.catcher_name, e.species))
            .collect()
    }

    /// The still-alive animal UUIDs in `instance_id` (captured ones are gone).
    pub fn instance_alive_uuids(&self, instance_id: u64) -> std::collections::HashSet<String> {
        self.conn
            .db
            .instance_animal()
            .iter()
            .filter(|a| a.instance_id == instance_id)
            .map(|a| a.animal_uuid)
            .collect()
    }

    /// Our current party id, if we're in one.
    pub fn my_party(&self) -> Option<u64> {
        let me = self.my_key.as_str();
        self.conn
            .db
            .party_member()
            .iter()
            .find(|m| m.member_key == me)
            .map(|m| m.party_id)
    }

    /// The stable keys of every member of our party (including us), or empty if
    /// we're not in a party.
    pub fn party_members(&self) -> Vec<String> {
        let Some(pid) = self.my_party() else {
            return Vec::new();
        };
        self.conn
            .db
            .party_member()
            .iter()
            .filter(|m| m.party_id == pid)
            .map(|m| m.member_key.clone())
            .collect()
    }

    /// Display names of our party members **excluding ourselves**, for the party
    /// panel. Empty when not in a party.
    pub fn party_member_names(&self) -> Vec<String> {
        let Some(pid) = self.my_party() else {
            return Vec::new();
        };
        let me = self.my_key.as_str();
        let names: HashMap<String, String> = self
            .conn
            .db
            .account()
            .iter()
            .map(|a| (a.player_key, a.name))
            .collect();
        self.conn
            .db
            .party_member()
            .iter()
            .filter(|m| m.party_id == pid && m.member_key != me)
            .map(|m| {
                names
                    .get(&m.member_key)
                    .cloned()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "Zookeeper".to_string())
            })
            .collect()
    }

    /// Party members **excluding ourselves** as `(name, power_score)`, for the
    /// party panel. Empty when not in a party.
    pub fn party_member_info(&self) -> Vec<(String, u64)> {
        let Some(pid) = self.my_party() else {
            return Vec::new();
        };
        let me = self.my_key.as_str();
        let names: HashMap<String, String> =
            self.conn.db.account().iter().map(|a| (a.player_key, a.name)).collect();
        let power: HashMap<String, u64> =
            self.conn.db.zoo().iter().map(|z| (z.owner_key, z.power_score)).collect();
        self.conn
            .db
            .party_member()
            .iter()
            .filter(|m| m.party_id == pid && m.member_key != me)
            .map(|m| {
                let name = names
                    .get(&m.member_key)
                    .cloned()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "Zookeeper".to_string());
                (name, power.get(&m.member_key).copied().unwrap_or(0))
            })
            .collect()
    }

    /// Pending invites addressed to us as `(invite_id, from_key)`.
    pub fn my_invites(&self) -> Vec<(u64, String)> {
        let me = self.my_key.as_str();
        self.conn
            .db
            .party_invite()
            .iter()
            .filter(|inv| inv.to_key == me)
            .map(|inv| (inv.invite_id, inv.from_key.clone()))
            .collect()
    }

    /// All known avatar poses `(owner_key, x, y)`.
    pub fn avatar_poses(&self) -> Vec<(String, f32, f32)> {
        self.conn
            .db
            .avatar_pose()
            .iter()
            .map(|p| (p.owner_key.clone(), p.x, p.y))
            .collect()
    }

    /// Other players' avatar targets as `(stable per-player key UUID, world pos)`,
    /// excluding our own, **restricted to our current space**: pass `my_space =
    /// Some(instance_id)` to get only partymates inside that instance (their poses
    /// are in instance-local coords), or `None` for hub players (excludes anyone
    /// currently off in an instance). This keeps the two coordinate spaces from
    /// bleeding into each other. The UUID drives stable avatar colour.
    pub fn peer_targets_in_space(&self, my_space: Option<u64>) -> Vec<(Uuid, Vec2)> {
        let me = self.my_key.as_str();
        // member_key → instance_id for everyone currently in an instance.
        let in_instance: HashMap<String, u64> = self
            .conn
            .db
            .instance_member()
            .iter()
            .map(|m| (m.member_key, m.instance_id))
            .collect();
        self.conn
            .db
            .avatar_pose()
            .iter()
            .filter(|p| p.owner_key != me)
            .filter(|p| in_instance.get(&p.owner_key).copied() == my_space)
            .map(|p| (key_uuid(&p.owner_key), vec2(p.x, p.y)))
            .collect()
    }

    /// All known zoos on the hub.
    pub fn zoos(&self) -> Vec<ZooView> {
        self.conn
            .db
            .zoo()
            .iter()
            .map(|z| ZooView {
                owner_key: z.owner_key,
                slot: z.slot,
                plot: (z.plot_x, z.plot_y),
                coins: z.coins,
                power_score: z.power_score,
                snapshot_json: z.snapshot_json,
            })
            .collect()
    }

    /// Every known player's server-derived Power Score, keyed by the same render
    /// UUID used for avatar identity ([`key_uuid`]) — so nameplates can label a
    /// peer with their Power.
    pub fn power_by_key_uuid(&self) -> HashMap<Uuid, u64> {
        self.conn
            .db
            .zoo()
            .iter()
            .map(|z| (key_uuid(&z.owner_key), z.power_score))
            .collect()
    }

    /// A specific player's Power Score by their stable `owner_key` (for the party
    /// panel, which is keyed by member key). `None` if unknown.
    pub fn power_for_key(&self, owner_key: &str) -> Option<u64> {
        self.conn
            .db
            .zoo()
            .iter()
            .find(|z| z.owner_key == owner_key)
            .map(|z| z.power_score)
    }

    /// Cleanly close the connection.
    pub fn disconnect(&self) {
        let _ = self.conn.disconnect();
    }
}

/// Best-effort log line — stderr under `cargo run`, plus the net diag log so it
/// surfaces in windowed builds too (mirrors `crate::net::diag`).
fn log_line(msg: String) {
    eprintln!("[stdb] {msg}");
}

/// A stable UUID derived from a player's stable key — used as a render-side key
/// and avatar colour seed for a remote player (stable across reconnects).
pub fn key_uuid(key: &str) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes())
}
