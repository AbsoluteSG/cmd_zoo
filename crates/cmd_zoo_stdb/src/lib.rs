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

use spacetimedb::{Identity, ReducerContext, ScheduleAt, Table, TimeDuration, Timestamp};

use cmd_zoo_core::game::{
    action::Action, biome_instance::BiomeInstance, ids, plot, power, species::HabitatTheme,
    zoo::Zoo,
};
use cmd_zoo_core::persistence::{parse_snapshot, snapshot_from_zoo, zoo_from_snapshot};

// ── Tables ──────────────────────────────────────────────────────────────────────

/// One row per known player, keyed by their **stable** `player_key` (derived
/// from their SteamID on the client — `"steam:<id>"`, or `"local:<uuid>"` in
/// non-Steam builds). This is what makes a rejoin *reuse* the same account/zoo/
/// slot instead of spawning a duplicate: the ephemeral SpacetimeDB [`Identity`]
/// changes every connect, so we store the *current* one in `identity` (rebound
/// on each `join_hub`) and map `ctx.sender()` → `player_key` through it.
/// `slot` is the hub plot slot assigned at first join (see [`plot::hub_plot_origin`]).
#[spacetimedb::table(accessor = account, public)]
pub struct Account {
    #[primary_key]
    player_key: String,
    #[unique]
    identity: Identity,
    slot: u64,
    name: String,
    joined_at: Timestamp,
}

/// A player's persistent zoo, keyed by their stable `owner_key`. Stored coarse
/// for the slice: a few queryable scalars plus the full `ZooSnapshot` as JSON,
/// so the client round-trips the same DTOs it already uses. (Normalized
/// animal/structure/… tables come later.)
#[spacetimedb::table(accessor = zoo, public)]
pub struct ZooRow {
    #[primary_key]
    owner_key: String,
    slot: u64,
    plot_x: f32,
    plot_y: f32,
    coins: u64,
    /// Derived progression metric (see `cmd_zoo_core::game::power`). Denormalized
    /// here so other clients can read a player's Power for party panels /
    /// nameplates without deserializing the snapshot. Recomputed on every write.
    power_score: u64,
    snapshot_json: String,
}

/// Live avatar position on the hub (presence), keyed by stable `owner_key`.
/// Updated at a throttled rate by the client via [`move_avatar`]; deleted when
/// the player disconnects so stale avatars don't linger.
#[spacetimedb::table(accessor = avatar_pose, public)]
pub struct AvatarPose {
    #[primary_key]
    owner_key: String,
    x: f32,
    y: f32,
    updated_at: Timestamp,
}

/// A party: a loose social link between players. Membership lives in
/// [`PartyMember`]; this row anchors the party id + who leads it. Parties don't
/// force members together — they only **route** members who independently enter
/// the *same* biome into the *same* instance (see Stage 2's `enter_expedition`).
#[spacetimedb::table(accessor = party, public)]
pub struct Party {
    #[primary_key]
    #[auto_inc]
    party_id: u64,
    leader_key: String,
    created_at: Timestamp,
}

/// One row per player currently in a party, keyed by stable `member_key`. A
/// player is in **at most one** party.
#[spacetimedb::table(accessor = party_member, public)]
pub struct PartyMember {
    #[primary_key]
    member_key: String,
    party_id: u64,
}

/// A pending party invite (`from_key` → `to_key`). Proximity-gated at send time;
/// the target accepts via [`accept_party_invite`] or declines via
/// [`decline_party_invite`].
#[spacetimedb::table(accessor = party_invite, public)]
pub struct PartyInvite {
    #[primary_key]
    #[auto_inc]
    invite_id: u64,
    party_id: u64,
    from_key: String,
    to_key: String,
    created_at: Timestamp,
}

/// A live (or recently-vacated) biome expedition instance. The routing key is
/// `(scope_key, theme)`: everyone sharing a scope who picks the same biome lands
/// in the same instance. `scope_key` is `"party:<id>"` for a party member or
/// `"solo:<player_key>"` otherwise — so partymates are routed together while
/// soloists get a private instance. When the last member leaves, `empty_since`
/// is set and the instance is kept for a rejoin window (POE-style), then reaped
/// by [`close_empty_instances`].
#[spacetimedb::table(accessor = instance, public)]
pub struct Instance {
    #[primary_key]
    #[auto_inc]
    instance_id: u64,
    scope_key: String,
    theme: String,
    seed: u64,
    created_at: Timestamp,
    /// Micros-since-epoch when the instance went empty; `0` while occupied.
    empty_since_micros: i64,
}

/// One row per player currently inside an expedition instance (at most one).
#[spacetimedb::table(accessor = instance_member, public)]
pub struct InstanceMember {
    #[primary_key]
    member_key: String,
    instance_id: u64,
}

/// One catchable wild animal in an instance — the **authoritative** population,
/// shared by every member. Spawned from the instance seed (so positions/species
/// match the client's deterministic [`BiomeInstance`]); removed by
/// [`capture_animal`] so two partymates can't both catch the same one. Keyed by
/// the seed-derived UUID string the client also generates.
#[spacetimedb::table(accessor = instance_animal, public)]
pub struct InstanceAnimal {
    #[primary_key]
    animal_uuid: String,
    instance_id: u64,
    species: String,
    x: f32,
    y: f32,
}

/// A co-op engagement: a player is actively working to catch a particular animal.
/// Many party members can engage the **same** animal at once; when it's caught,
/// every current participant is credited (see [`capture_animal`]). Keyed by
/// `"<animal_uuid>|<member_key>"` so each (player, animal) pair is one row.
#[spacetimedb::table(accessor = instance_engagement, public)]
pub struct InstanceEngagement {
    #[primary_key]
    engage_key: String,
    animal_uuid: String,
    member_key: String,
    instance_id: u64,
}

/// A transient "someone in your party caught something" notification, one row per
/// recipient. The client turns new rows addressed to it into a toast; the reaper
/// prunes them after a short TTL so the table stays small.
#[spacetimedb::table(accessor = capture_event, public)]
pub struct CaptureEvent {
    #[primary_key]
    #[auto_inc]
    event_id: u64,
    recipient_key: String,
    catcher_name: String,
    species: String,
    instance_id: u64,
    created_micros: i64,
}

/// Internal scheduled-reducer timer: fires [`close_empty_instances`] on a loop to
/// reap instances that have stayed empty past the rejoin window.
#[spacetimedb::table(accessor = reaper_schedule, scheduled(close_empty_instances))]
pub struct ReaperSchedule {
    #[primary_key]
    #[auto_inc]
    scheduled_id: u64,
    scheduled_at: ScheduleAt,
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

/// Max hub distance for a party invite — shared with the client via core.
use plot::PARTY_INVITE_RANGE;

/// How long an instance is kept after going empty before it's reaped — the
/// rejoin window (POE-style). Members who return within this window resume the
/// same instance (same seed/state).
const INSTANCE_REJOIN_WINDOW_MICROS: i64 = 5 * 60 * 1_000_000; // 5 minutes
/// How often the reaper checks for expired-empty instances.
const REAPER_INTERVAL_SECS: u64 = 60;
/// How long a party capture-toast row lives before the reaper prunes it.
const CAPTURE_EVENT_TTL_MICROS: i64 = 60 * 1_000_000; // 1 minute

/// A player's display name (from their account), or empty if unknown.
fn account_name(ctx: &ReducerContext, key: &str) -> String {
    ctx.db.account().player_key().find(key.to_string()).map(|a| a.name).unwrap_or_default()
}

/// Resolve the caller's **stable** player key from their live connection
/// identity. `None` if they haven't `join_hub`'d yet.
fn caller_key(ctx: &ReducerContext) -> Option<String> {
    ctx.db.account().identity().find(ctx.sender()).map(|a| a.player_key)
}

/// The expedition instance a player is currently inside, if any. (Used by the
/// catch/membership layer.)
#[allow(dead_code)]
fn instance_of(ctx: &ReducerContext, key: &str) -> Option<u64> {
    ctx.db.instance_member().member_key().find(key.to_string()).map(|m| m.instance_id)
}

/// Number of players currently inside `instance_id`.
fn instance_population(ctx: &ReducerContext, instance_id: u64) -> usize {
    ctx.db.instance_member().iter().filter(|m| m.instance_id == instance_id).count()
}

/// Remove a player from whatever instance they're in. If that empties the
/// instance, stamp `empty_since` so the reaper can close it after the rejoin
/// window. Safe to call when not in an instance.
fn remove_from_instance(ctx: &ReducerContext, key: &str) {
    let Some(member) = ctx.db.instance_member().member_key().find(key.to_string()) else {
        return;
    };
    let instance_id = member.instance_id;
    ctx.db.instance_member().member_key().delete(key.to_string());
    // They're no longer participating in any catch here.
    clear_engagements(ctx, |e| e.member_key == key);

    if instance_population(ctx, instance_id) == 0 {
        if let Some(mut inst) = ctx.db.instance().instance_id().find(instance_id) {
            inst.empty_since_micros = ctx.timestamp.to_micros_since_unix_epoch();
            ctx.db.instance().instance_id().update(inst);
        }
    }
}

/// Populate an instance's authoritative animal set from its seed, running the
/// **same** deterministic generation the client uses ([`BiomeInstance`]) so rows
/// line up with the client's local arrangement by UUID.
fn spawn_instance_animals(ctx: &ReducerContext, instance_id: u64, theme: HabitatTheme, seed: u64) {
    let inst = BiomeInstance::new(theme, seed);
    for a in inst.animals {
        ctx.db.instance_animal().insert(InstanceAnimal {
            animal_uuid: a.id.to_string(),
            instance_id,
            species: a.species.to_string(),
            x: a.pos.x,
            y: a.pos.y,
        });
    }
}

/// Composite key for one (player, animal) co-op engagement row.
fn engage_key(animal_uuid: &str, member_key: &str) -> String {
    format!("{animal_uuid}|{member_key}")
}

/// Drop every engagement row matching a predicate (used to clear an animal's or a
/// member's engagements).
fn clear_engagements(ctx: &ReducerContext, pred: impl Fn(&InstanceEngagement) -> bool) {
    let keys: Vec<String> = ctx
        .db
        .instance_engagement()
        .iter()
        .filter(|e| pred(e))
        .map(|e| e.engage_key)
        .collect();
    for k in keys {
        ctx.db.instance_engagement().engage_key().delete(k);
    }
}

/// Grant one freeform animal of `species` to `owner_key`'s server zoo, via the
/// shared core rules. `Err` (e.g. zoo at capacity) is the caller's to handle —
/// for co-op grants we treat it as best-effort per participant.
fn grant_species_to(ctx: &ReducerContext, owner_key: &str, species: &str) -> Result<(), String> {
    let mut row = ctx
        .db
        .zoo()
        .owner_key()
        .find(owner_key.to_string())
        .ok_or("no zoo")?;
    let snap = parse_snapshot(row.snapshot_json.as_bytes()).map_err(|e| e.to_string())?;
    let mut zoo = zoo_from_snapshot(snap).map_err(|e| e.to_string())?.zoo;
    ids::seed_ids(id_seed(ctx));
    cmd_zoo_core::game::action::apply_action(
        &mut zoo,
        Action::SpawnFreeform { species: species.to_string(), level: 1 },
        now_utc(ctx.timestamp),
    )
    .map_err(|e| format!("{e:?}"))?;
    row.coins = zoo.coins;
    row.power_score = power::power_score(&zoo);
    row.snapshot_json = serde_json::to_string(&snapshot_from_zoo(&zoo)).map_err(|e| e.to_string())?;
    ctx.db.zoo().owner_key().update(row);
    Ok(())
}

/// Delete an instance and all its animals + engagements (used by the reaper).
fn delete_instance(ctx: &ReducerContext, instance_id: u64) {
    let animals: Vec<String> = ctx
        .db
        .instance_animal()
        .iter()
        .filter(|a| a.instance_id == instance_id)
        .map(|a| a.animal_uuid)
        .collect();
    for au in animals {
        ctx.db.instance_animal().animal_uuid().delete(au);
    }
    clear_engagements(ctx, |e| e.instance_id == instance_id);
    ctx.db.instance().instance_id().delete(instance_id);
}

/// A deterministic-ish per-instance seed from its routing key + creation time.
fn instance_seed(scope_key: &str, theme: &str, ts: Timestamp) -> u64 {
    let mut h: u64 = 1469598103934665603; // FNV-1a offset
    for b in scope_key.bytes().chain(theme.bytes()) {
        h = (h ^ b as u64).wrapping_mul(1099511628211);
    }
    h ^ (ts.to_micros_since_unix_epoch() as u64).wrapping_mul(0x9E3779B97F4A7C15)
}

/// The party a player currently belongs to, if any.
fn party_of(ctx: &ReducerContext, key: &str) -> Option<u64> {
    ctx.db.party_member().member_key().find(key.to_string()).map(|m| m.party_id)
}

/// Remove a player from their party, dissolving the party if it empties or
/// handing leadership to a remaining member if the leader left. Also clears any
/// invites the player sent or received. Safe to call when not in a party.
fn remove_from_party(ctx: &ReducerContext, key: &str) {
    // Drop invites involving this player either way (they're leaving the graph).
    let stale: Vec<u64> = ctx
        .db
        .party_invite()
        .iter()
        .filter(|inv| inv.from_key == key || inv.to_key == key)
        .map(|inv| inv.invite_id)
        .collect();
    for invite_id in stale {
        ctx.db.party_invite().invite_id().delete(invite_id);
    }

    let Some(member) = ctx.db.party_member().member_key().find(key.to_string()) else {
        return;
    };
    let party_id = member.party_id;
    ctx.db.party_member().member_key().delete(key.to_string());

    let mut remaining = ctx.db.party_member().iter().filter(|m| m.party_id == party_id);
    match remaining.next() {
        None => {
            // Party is empty — dissolve it.
            ctx.db.party().party_id().delete(party_id);
        }
        Some(other) => {
            // If the leader left, promote the first remaining member.
            if let Some(mut party) = ctx.db.party().party_id().find(party_id) {
                if party.leader_key == key {
                    party.leader_key = other.member_key;
                    ctx.db.party().party_id().update(party);
                }
            }
        }
    }
}



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
pub fn init(ctx: &ReducerContext) {
    // Start the empty-instance reaper loop.
    ctx.db.reaper_schedule().insert(ReaperSchedule {
        scheduled_id: 0,
        scheduled_at: TimeDuration::from_micros(REAPER_INTERVAL_SECS as i64 * 1_000_000).into(),
    });
    log::info!("cmd_zoo_stdb module initialized (reaper every {REAPER_INTERVAL_SECS}s)");
}

#[spacetimedb::reducer(client_connected)]
pub fn identity_connected(ctx: &ReducerContext) {
    log::info!("client connected: {:?}", ctx.sender());
}

#[spacetimedb::reducer(client_disconnected)]
pub fn identity_disconnected(ctx: &ReducerContext) {
    let id = ctx.sender();
    // Clear the player's live presence so they disappear from everyone else's
    // hub immediately. We deliberately keep their `account` (stable key + slot)
    // and `zoo` (their save) — only the ephemeral pose + party state go. On
    // reconnect `join_hub` rebinds the same account (no duplicate).
    if let Some(key) = caller_key(ctx) {
        if ctx.db.avatar_pose().owner_key().find(&key).is_some() {
            ctx.db.avatar_pose().owner_key().delete(&key);
        }
        remove_from_party(ctx, &key);
        // Leave any expedition instance (stamps it empty for the rejoin window —
        // they can reconnect and resume it within 5 minutes).
        remove_from_instance(ctx, &key);
    }
    log::info!("client disconnected: {:?}", id);
}

// ── Reducers ────────────────────────────────────────────────────────────────────

/// Place the caller on the hub: assign the next free plot slot, create a fresh
/// zoo (the same `Zoo::new` the client uses) anchored at that slot's origin, and
/// seed their presence. Idempotent — a second call from an existing player is a
/// no-op.
#[spacetimedb::reducer]
pub fn join_hub(ctx: &ReducerContext, player_key: String, name: String) -> Result<(), String> {
    let id = ctx.sender();
    if player_key.is_empty() {
        return Err("missing player key".into());
    }

    // Returning player: rebind their account to this new live identity, refresh
    // the display name, and re-seed presence. Their slot + zoo are untouched —
    // this is exactly what stops the duplicate-account/stale-avatar problem.
    if let Some(mut acct) = ctx.db.account().player_key().find(&player_key) {
        // Releasing the old identity (now stale) keeps the `identity` unique
        // index satisfied before we claim it for the new connection.
        acct.identity = id;
        if !name.is_empty() {
            acct.name = name.clone();
        }
        ctx.db.account().player_key().update(acct);

        // Re-create presence at their plot if it was cleared on disconnect.
        if ctx.db.avatar_pose().owner_key().find(&player_key).is_none() {
            let (x, y) = ctx
                .db
                .zoo()
                .owner_key()
                .find(&player_key)
                .map(|z| (z.plot_x, z.plot_y))
                .unwrap_or((plot::hub_spawn().x, plot::hub_spawn().y));
            ctx.db.avatar_pose().insert(AvatarPose {
                owner_key: player_key.clone(),
                x,
                y,
                updated_at: ctx.timestamp,
            });
        }
        log::info!("{player_key} rejoined the hub (slot {})", id.to_string());
        return Ok(());
    }

    // Brand-new player: allocate the next slot + create their zoo.
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
        player_key: player_key.clone(),
        identity: id,
        slot,
        name,
        joined_at: ctx.timestamp,
    });
    ctx.db.zoo().insert(ZooRow {
        owner_key: player_key.clone(),
        slot,
        plot_x: origin.x,
        plot_y: origin.y,
        coins: zoo.coins,
        power_score: power::power_score(&zoo),
        snapshot_json: json,
    });
    ctx.db.avatar_pose().insert(AvatarPose {
        owner_key: player_key.clone(),
        x: origin.x,
        y: origin.y,
        updated_at: ctx.timestamp,
    });
    log::info!("{player_key} joined the hub on slot {slot}");
    Ok(())
}

/// Update the caller's avatar position (presence). The only high-rate reducer.
#[spacetimedb::reducer]
pub fn move_avatar(ctx: &ReducerContext, x: f32, y: f32) {
    let Some(key) = caller_key(ctx) else {
        return; // not joined yet
    };
    if let Some(mut pose) = ctx.db.avatar_pose().owner_key().find(&key) {
        pose.x = x;
        pose.y = y;
        pose.updated_at = ctx.timestamp;
        ctx.db.avatar_pose().owner_key().update(pose);
    } else {
        ctx.db.avatar_pose().insert(AvatarPose {
            owner_key: key,
            x,
            y,
            updated_at: ctx.timestamp,
        });
    }
}

// ── Party reducers ────────────────────────────────────────────────────────────

/// Invite a nearby player to a party. Proximity-gated: both avatars must have a
/// live pose within [`PARTY_INVITE_RANGE`]. If the caller isn't already in a
/// party, one is created with the caller as leader. The target accepts via
/// [`accept_party_invite`].
#[spacetimedb::reducer]
pub fn invite_to_party(ctx: &ReducerContext, target_key: String) -> Result<(), String> {
    let me = caller_key(ctx).ok_or("you're not on the hub")?;
    if target_key == me {
        return Err("can't invite yourself".into());
    }
    let mine = ctx.db.avatar_pose().owner_key().find(&me).ok_or("you're not on the hub")?;
    let theirs = ctx
        .db
        .avatar_pose()
        .owner_key()
        .find(&target_key)
        .ok_or("that player isn't on the hub")?;
    let dist = ((mine.x - theirs.x).powi(2) + (mine.y - theirs.y).powi(2)).sqrt();
    if dist > PARTY_INVITE_RANGE {
        return Err("that player is too far away".into());
    }
    if party_of(ctx, &target_key).is_some() {
        return Err("that player is already in a party".into());
    }

    // Ensure the caller has a party to invite into.
    let party_id = match party_of(ctx, &me) {
        Some(pid) => pid,
        None => {
            let party = ctx.db.party().insert(Party {
                party_id: 0, // auto_inc
                leader_key: me.clone(),
                created_at: ctx.timestamp,
            });
            ctx.db.party_member().insert(PartyMember {
                member_key: me.clone(),
                party_id: party.party_id,
            });
            party.party_id
        }
    };

    // Replace any existing pending invite from us to this target.
    let dup: Vec<u64> = ctx
        .db
        .party_invite()
        .iter()
        .filter(|inv| inv.from_key == me && inv.to_key == target_key)
        .map(|inv| inv.invite_id)
        .collect();
    for invite_id in dup {
        ctx.db.party_invite().invite_id().delete(invite_id);
    }

    ctx.db.party_invite().insert(PartyInvite {
        invite_id: 0, // auto_inc
        party_id,
        from_key: me.clone(),
        to_key: target_key.clone(),
        created_at: ctx.timestamp,
    });
    log::info!("{me} invited {target_key} to party {party_id}");
    Ok(())
}

/// Accept a pending party invite addressed to the caller. Leaves any current
/// party first (a player is in at most one).
#[spacetimedb::reducer]
pub fn accept_party_invite(ctx: &ReducerContext, invite_id: u64) -> Result<(), String> {
    let me = caller_key(ctx).ok_or("you're not on the hub")?;
    let invite = ctx
        .db
        .party_invite()
        .invite_id()
        .find(invite_id)
        .ok_or("invite no longer exists")?;
    if invite.to_key != me {
        return Err("that invite isn't for you".into());
    }
    // The party may have dissolved between invite and accept.
    if ctx.db.party().party_id().find(invite.party_id).is_none() {
        ctx.db.party_invite().invite_id().delete(invite_id);
        return Err("that party no longer exists".into());
    }

    remove_from_party(ctx, &me); // also clears the invite we're accepting
    ctx.db.party_member().insert(PartyMember {
        member_key: me.clone(),
        party_id: invite.party_id,
    });
    log::info!("{me} joined party {}", invite.party_id);
    Ok(())
}

/// Decline a pending party invite addressed to the caller.
#[spacetimedb::reducer]
pub fn decline_party_invite(ctx: &ReducerContext, invite_id: u64) -> Result<(), String> {
    let Some(me) = caller_key(ctx) else {
        return Ok(());
    };
    if let Some(invite) = ctx.db.party_invite().invite_id().find(invite_id) {
        if invite.to_key == me {
            ctx.db.party_invite().invite_id().delete(invite_id);
        }
    }
    Ok(())
}

/// Leave the caller's current party (dissolving it if empty, reassigning the
/// leader if needed). No-op if not in a party.
#[spacetimedb::reducer]
pub fn leave_party(ctx: &ReducerContext) {
    if let Some(me) = caller_key(ctx) {
        remove_from_party(ctx, &me);
    }
}

// ── Expedition instance lifecycle ──────────────────────────────────────────────

/// Enter (or resume) an expedition instance for `theme`. Routing: party members
/// share one instance per biome; soloists get a private one. If a matching
/// instance already exists — including one inside its rejoin window — the caller
/// joins it (and its `empty_since` is cleared); otherwise a fresh one is created.
/// The caller leaves any instance they were already in first.
#[spacetimedb::reducer]
pub fn enter_expedition(ctx: &ReducerContext, theme: String) -> Result<(), String> {
    let me = caller_key(ctx).ok_or("call join_hub first")?;
    let theme_enum = HabitatTheme::from_str(&theme).ok_or("unknown biome theme")?;

    // Access gate (authoritative): the caller's Power Score must meet the region's
    // requirement. Checked *before* vacating their current instance so a rejected
    // enter doesn't kick them out of where they are.
    {
        let row = ctx
            .db
            .zoo()
            .owner_key()
            .find(&me)
            .ok_or("no zoo for caller — call join_hub first")?;
        let snap = parse_snapshot(row.snapshot_json.as_bytes()).map_err(|e| e.to_string())?;
        let zoo = zoo_from_snapshot(snap).map_err(|e| e.to_string())?.zoo;
        if let Some(msg) = power::gate_error(power::power_score(&zoo), theme_enum) {
            return Err(msg);
        }
    }

    // Routing scope: the party (shared) or this player alone (private).
    let scope_key = match party_of(ctx, &me) {
        Some(pid) => format!("party:{pid}"),
        None => format!("solo:{me}"),
    };

    // Leaving first means re-entering the same theme is a no-op rejoin, and
    // switching themes correctly vacates the old instance.
    remove_from_instance(ctx, &me);

    // Find an existing instance for this scope+theme (occupied or within window).
    let existing = ctx
        .db
        .instance()
        .iter()
        .find(|i| i.scope_key == scope_key && i.theme == theme)
        .map(|i| i.instance_id);

    let instance_id = match existing {
        Some(id) => {
            // Resume it — clear any pending reap.
            if let Some(mut inst) = ctx.db.instance().instance_id().find(id) {
                inst.empty_since_micros = 0;
                ctx.db.instance().instance_id().update(inst);
            }
            id
        }
        None => {
            let seed = instance_seed(&scope_key, &theme, ctx.timestamp);
            let inst = ctx.db.instance().insert(Instance {
                instance_id: 0, // auto_inc
                scope_key: scope_key.clone(),
                theme: theme.clone(),
                seed,
                created_at: ctx.timestamp,
                empty_since_micros: 0,
            });
            // Spawn the authoritative, shared animal population for this instance.
            spawn_instance_animals(ctx, inst.instance_id, theme_enum, seed);
            inst.instance_id
        }
    };

    ctx.db.instance_member().insert(InstanceMember {
        member_key: me.clone(),
        instance_id,
    });
    log::info!("{me} entered expedition '{theme}' (instance {instance_id}, scope {scope_key})");
    Ok(())
}

/// Leave the caller's current expedition instance (back to the hub). The instance
/// is kept for the rejoin window if it empties. No-op if not in one.
#[spacetimedb::reducer]
pub fn leave_expedition(ctx: &ReducerContext) {
    if let Some(me) = caller_key(ctx) {
        remove_from_instance(ctx, &me);
    }
}

/// Scheduled reaper: delete instances that have stayed empty past the rejoin
/// window. Runs on the [`ReaperSchedule`] loop.
#[spacetimedb::reducer]
pub fn close_empty_instances(ctx: &ReducerContext, _row: ReaperSchedule) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let expired: Vec<u64> = ctx
        .db
        .instance()
        .iter()
        .filter(|i| i.empty_since_micros != 0 && now - i.empty_since_micros >= INSTANCE_REJOIN_WINDOW_MICROS)
        .map(|i| i.instance_id)
        .collect();
    for id in expired {
        delete_instance(ctx, id);
        log::info!("reaped empty expedition instance {id}");
    }

    // Prune delivered/old party capture toasts.
    let stale: Vec<u64> = ctx
        .db
        .capture_event()
        .iter()
        .filter(|e| now - e.created_micros >= CAPTURE_EVENT_TTL_MICROS)
        .map(|e| e.event_id)
        .collect();
    for id in stale {
        ctx.db.capture_event().event_id().delete(id);
    }
}

/// Begin (or re-affirm) a co-op engagement against an animal in the caller's
/// instance. Party members may all engage the **same** animal — there's no lock.
/// Recording the engagement is what makes them a credited participant when it's
/// caught. Idempotent.
#[spacetimedb::reducer]
pub fn engage_animal(ctx: &ReducerContext, animal_uuid: String) -> Result<(), String> {
    let me = caller_key(ctx).ok_or("call join_hub first")?;
    let my_instance = instance_of(ctx, &me).ok_or("you're not in an expedition")?;
    let animal = ctx
        .db
        .instance_animal()
        .animal_uuid()
        .find(&animal_uuid)
        .ok_or("that animal is gone")?;
    if animal.instance_id != my_instance {
        return Err("that animal isn't in your expedition".into());
    }
    let key = engage_key(&animal_uuid, &me);
    if ctx.db.instance_engagement().engage_key().find(&key).is_none() {
        ctx.db.instance_engagement().insert(InstanceEngagement {
            engage_key: key,
            animal_uuid,
            member_key: me,
            instance_id: my_instance,
        });
    }
    Ok(())
}

/// Stop engaging an animal (the caller switched targets or backed off), dropping
/// their participation so they're not credited if it's later caught.
#[spacetimedb::reducer]
pub fn release_animal(ctx: &ReducerContext, animal_uuid: String) {
    if let Some(me) = caller_key(ctx) {
        ctx.db.instance_engagement().engage_key().delete(engage_key(&animal_uuid, &me));
    }
}

/// Catch an animal in the caller's instance. Co-op: the catch is **dispatched to
/// every current participant** (each engager — including the finisher — gets the
/// species granted to their zoo, best-effort per-zoo so one being at capacity
/// doesn't deny the others). The animal is then removed from the shared instance.
/// (Bonus loot — coins etc. — will later be split randomly among participants.)
#[spacetimedb::reducer]
pub fn capture_animal(ctx: &ReducerContext, animal_uuid: String) -> Result<(), String> {
    let me = caller_key(ctx).ok_or("call join_hub first")?;
    let my_instance = instance_of(ctx, &me).ok_or("you're not in an expedition")?;

    let animal = ctx
        .db
        .instance_animal()
        .animal_uuid()
        .find(&animal_uuid)
        .ok_or("that animal is already caught")?;
    if animal.instance_id != my_instance {
        return Err("that animal isn't in your expedition".into());
    }
    let species = animal.species.clone();

    // Everyone engaging this animal is credited; the finisher counts even if their
    // engagement row somehow didn't land.
    let mut participants: Vec<String> = ctx
        .db
        .instance_engagement()
        .iter()
        .filter(|e| e.animal_uuid == animal_uuid)
        .map(|e| e.member_key)
        .collect();
    if !participants.contains(&me) {
        participants.push(me.clone());
    }

    for owner in &participants {
        match grant_species_to(ctx, owner, &species) {
            Ok(()) => log::info!("{owner} was credited {species} (instance {my_instance})"),
            Err(e) => log::info!("skipped crediting {owner} {species}: {e}"),
        }
    }

    // Remove the animal + all its engagements from the shared instance.
    ctx.db.instance_animal().animal_uuid().delete(&animal_uuid);
    clear_engagements(ctx, |e| e.animal_uuid == animal_uuid);

    // Toast the whole party (everyone but the catcher, who has local feedback).
    if let Some(pid) = party_of(ctx, &me) {
        let catcher_name = account_name(ctx, &me);
        let recipients: Vec<String> = ctx
            .db
            .party_member()
            .iter()
            .filter(|m| m.party_id == pid && m.member_key != me)
            .map(|m| m.member_key)
            .collect();
        for r in recipients {
            ctx.db.capture_event().insert(CaptureEvent {
                event_id: 0, // auto_inc
                recipient_key: r,
                catcher_name: catcher_name.clone(),
                species: species.clone(),
                instance_id: my_instance,
                created_micros: ctx.timestamp.to_micros_since_unix_epoch(),
            });
        }
    }

    log::info!("{me} landed {species} for {} participant(s)", participants.len());
    Ok(())
}

/// Apply a serialized [`Action`] to the caller's authoritative zoo by running the
/// shared core rules ([`cmd_zoo_core::game::action::apply_action`]) and writing
/// the result back. This is the proof that the same rules run server-side.
#[spacetimedb::reducer]
pub fn apply_action(ctx: &ReducerContext, action_json: String) -> Result<(), String> {
    let key = caller_key(ctx).ok_or("call join_hub first")?;
    let mut row = ctx
        .db
        .zoo()
        .owner_key()
        .find(&key)
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
    row.power_score = power::power_score(&zoo);
    row.snapshot_json = serde_json::to_string(&snapshot_from_zoo(&zoo)).map_err(|e| e.to_string())?;
    ctx.db.zoo().owner_key().update(row);
    Ok(())
}
