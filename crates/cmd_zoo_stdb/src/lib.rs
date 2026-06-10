//! `cmd_zoo_stdb` — the SpacetimeDB authority module for cmd_zoo.
//!
//! This is the **thin vertical slice** of the online backend (Milestone B): it
//! proves the shape end to end — a player connects, is placed on a hub plot, and
//! mutates their server-persisted zoo by calling a reducer that runs the *exact
//! same* [`cmd_zoo_core`] rules the desktop client runs locally.
//!
//! Tables are the authoritative state; reducers are the only way to change it.
//! The core runs headless here (no filesystem; entity ids are host-seeded and
//! deterministic — see [`cmd_zoo_core::game::ids`]), so reducer replays are
//! reproducible.
//!
//! Scope of the slice: `account`, `zoo` (one serialized snapshot per player), and
//! `avatar_pose` (presence). Reducers: `join_hub`, `move_avatar`, and a generic
//! `apply_action` wrapping [`cmd_zoo_core::game::action::apply_action`]. The full
//! table normalization + the rest of the reducer surface come later.

use spacetimedb::{Identity, ReducerContext, Table, Timestamp};

use cmd_zoo_core::game::{action::Action, ids, plot, zoo::Zoo};
use cmd_zoo_core::persistence::{parse_snapshot, snapshot_from_zoo, zoo_from_snapshot};

// ── Tables ──────────────────────────────────────────────────────────────────────

/// One row per known player, keyed by their SpacetimeDB [`Identity`]. `slot` is
/// the hub plot slot assigned at join (see [`plot::hub_plot_origin`]).
#[spacetimedb::table(accessor = account, public)]
pub struct Account {
    #[primary_key]
    identity: Identity,
    slot: u64,
    name: String,
    joined_at: Timestamp,
}

/// A player's persistent zoo. Stored coarse for the slice: a few queryable
/// scalars plus the full `ZooSnapshot` as JSON, so the client round-trips the
/// same DTOs it already uses. (Normalized animal/structure/… tables come later.)
#[spacetimedb::table(accessor = zoo, public)]
pub struct ZooRow {
    #[primary_key]
    owner: Identity,
    slot: u64,
    plot_x: f32,
    plot_y: f32,
    coins: u64,
    snapshot_json: String,
}

/// Live avatar position on the hub (presence). Updated at a throttled rate by the
/// client via [`move_avatar`].
#[spacetimedb::table(accessor = avatar_pose, public)]
pub struct AvatarPose {
    #[primary_key]
    owner: Identity,
    x: f32,
    y: f32,
    updated_at: Timestamp,
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

/// The reducer's deterministic wall-clock as a `chrono::DateTime<Utc>` (the core
/// takes time as a parameter; the host supplies it via `ctx.timestamp`).
fn now_utc(ts: Timestamp) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_micros(ts.to_micros_since_unix_epoch()).unwrap_or_default()
}

/// A per-call seed for the core's deterministic id stream. Derived from the
/// host's reducer timestamp, so id generation inside this call is reproducible.
fn id_seed(ctx: &ReducerContext) -> u64 {
    ctx.timestamp.to_micros_since_unix_epoch() as u64
}

// ── Lifecycle ───────────────────────────────────────────────────────────────────

#[spacetimedb::reducer(init)]
pub fn init(_ctx: &ReducerContext) {
    log::info!("cmd_zoo_stdb module initialized");
}

#[spacetimedb::reducer(client_connected)]
pub fn identity_connected(ctx: &ReducerContext) {
    log::info!("client connected: {:?}", ctx.sender());
}

#[spacetimedb::reducer(client_disconnected)]
pub fn identity_disconnected(ctx: &ReducerContext) {
    log::info!("client disconnected: {:?}", ctx.sender());
}

// ── Reducers ────────────────────────────────────────────────────────────────────

/// Place the caller on the hub: assign the next free plot slot, create a fresh
/// zoo (the same `Zoo::new` the client uses) anchored at that slot's origin, and
/// seed their presence. Idempotent — a second call from an existing player is a
/// no-op.
#[spacetimedb::reducer]
pub fn join_hub(ctx: &ReducerContext, name: String) -> Result<(), String> {
    let id = ctx.sender();
    if ctx.db.account().identity().find(id).is_some() {
        return Ok(()); // already on the hub
    }

    ids::seed_ids(id_seed(ctx));
    let slot = ctx.db.account().count(); // 0-based; next free slot
    let now = now_utc(ctx.timestamp);

    let mut zoo = Zoo::new(now);
    let origin = plot::hub_plot_origin(slot as u32);
    zoo.plot_origin = origin;
    if !name.is_empty() {
        zoo.player.name = name.clone();
    }

    let json = serde_json::to_string(&snapshot_from_zoo(&zoo)).map_err(|e| e.to_string())?;

    ctx.db.account().insert(Account {
        identity: id,
        slot,
        name,
        joined_at: ctx.timestamp,
    });
    ctx.db.zoo().insert(ZooRow {
        owner: id,
        slot,
        plot_x: origin.x,
        plot_y: origin.y,
        coins: zoo.coins,
        snapshot_json: json,
    });
    ctx.db.avatar_pose().insert(AvatarPose {
        owner: id,
        x: origin.x,
        y: origin.y,
        updated_at: ctx.timestamp,
    });
    log::info!("{:?} joined the hub on slot {slot}", id);
    Ok(())
}

/// Update the caller's avatar position (presence). The only high-rate reducer.
#[spacetimedb::reducer]
pub fn move_avatar(ctx: &ReducerContext, x: f32, y: f32) {
    let id = ctx.sender();
    if let Some(mut pose) = ctx.db.avatar_pose().owner().find(id) {
        pose.x = x;
        pose.y = y;
        pose.updated_at = ctx.timestamp;
        ctx.db.avatar_pose().owner().update(pose);
    } else {
        ctx.db.avatar_pose().insert(AvatarPose {
            owner: id,
            x,
            y,
            updated_at: ctx.timestamp,
        });
    }
}

/// Apply a serialized [`Action`] to the caller's authoritative zoo by running the
/// shared core rules ([`cmd_zoo_core::game::action::apply_action`]) and writing
/// the result back. This is the proof that the same rules run server-side.
#[spacetimedb::reducer]
pub fn apply_action(ctx: &ReducerContext, action_json: String) -> Result<(), String> {
    let id = ctx.sender();
    let mut row = ctx
        .db
        .zoo()
        .owner()
        .find(id)
        .ok_or("no zoo for caller — call join_hub first")?;

    let action: Action =
        serde_json::from_str(&action_json).map_err(|e| format!("bad action json: {e}"))?;

    let snap = parse_snapshot(row.snapshot_json.as_bytes()).map_err(|e| e.to_string())?;
    let mut zoo = zoo_from_snapshot(snap).map_err(|e| e.to_string())?.zoo;

    ids::seed_ids(id_seed(ctx));
    let now = now_utc(ctx.timestamp);
    cmd_zoo_core::game::action::apply_action(&mut zoo, action, now)
        .map_err(|e| format!("action rejected: {e:?}"))?;

    row.coins = zoo.coins;
    row.snapshot_json = serde_json::to_string(&snapshot_from_zoo(&zoo)).map_err(|e| e.to_string())?;
    ctx.db.zoo().owner().update(row);
    Ok(())
}
