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

use super::bindings::{
    AvatarPoseTableAccess, DbConnection, ZooTableAccess,
    apply_action, join_hub, move_avatar,
};

/// Maincloud endpoint (the managed SpacetimeDB host).
pub const MAINCLOUD_URI: &str = "https://maincloud.spacetimedb.com";
/// The published module / database name (see `spacetime publish`).
pub const DEFAULT_MODULE: &str = "critter-cove";

/// A flattened view of a subscribed `zoo` row for the renderer/app.
#[derive(Clone, Debug)]
pub struct ZooView {
    pub owner: Identity,
    pub slot: u64,
    pub plot: (f32, f32),
    pub coins: u64,
    pub snapshot_json: String,
}

/// A live connection to the online hub. Dropping it stops the worker thread.
pub struct OnlineClient {
    conn: DbConnection,
    _worker: std::thread::JoinHandle<()>,
}

impl OnlineClient {
    /// Connect to `module` at `uri`, subscribe to the hub tables, and start the
    /// background message pump. Non-blocking: the connection completes
    /// asynchronously (watch the `on_connect`/`on_connect_error` logs).
    pub fn connect(uri: &str, module: &str) -> anyhow::Result<Self> {
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
            ]);

        let worker = conn.run_threaded();
        Ok(Self { conn, _worker: worker })
    }

    /// Connect to the default Maincloud deployment.
    pub fn connect_maincloud() -> anyhow::Result<Self> {
        Self::connect(MAINCLOUD_URI, DEFAULT_MODULE)
    }

    /// Our own identity, once the connection has established it.
    pub fn identity(&self) -> Option<Identity> {
        self.conn.try_identity()
    }

    // ── Reducer calls (mutations) ───────────────────────────────────────────────

    /// Place us on the hub (assigns a plot slot + creates our zoo server-side).
    pub fn join_hub(&self, name: &str) -> anyhow::Result<()> {
        self.conn
            .reducers
            .join_hub(name.to_string())
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

    // ── Subscribed-state snapshots (reads of the client cache) ───────────────────

    /// All known avatar poses `(owner, x, y)`.
    pub fn avatar_poses(&self) -> Vec<(Identity, f32, f32)> {
        self.conn
            .db
            .avatar_pose()
            .iter()
            .map(|p| (p.owner, p.x, p.y))
            .collect()
    }

    /// Other players' avatar targets as `(stable per-identity key, world pos)`,
    /// excluding our own. The key is a deterministic UUID derived from the
    /// SpacetimeDB identity, so it's stable across frames (and drives avatar
    /// colour). Used to drive moving remote avatars on the hub.
    pub fn peer_avatar_targets(&self) -> Vec<(Uuid, Vec2)> {
        let me = self.identity();
        self.conn
            .db
            .avatar_pose()
            .iter()
            .filter(|p| Some(p.owner) != me)
            .map(|p| (identity_uuid(&p.owner), vec2(p.x, p.y)))
            .collect()
    }

    /// All known zoos on the hub.
    pub fn zoos(&self) -> Vec<ZooView> {
        self.conn
            .db
            .zoo()
            .iter()
            .map(|z| ZooView {
                owner: z.owner,
                slot: z.slot,
                plot: (z.plot_x, z.plot_y),
                coins: z.coins,
                snapshot_json: z.snapshot_json,
            })
            .collect()
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

/// A stable UUID derived from a SpacetimeDB [`Identity`] — used as a render-side
/// key and avatar colour seed for a remote player.
pub fn identity_uuid(id: &Identity) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, id.to_string().as_bytes())
}
