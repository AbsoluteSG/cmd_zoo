#[cfg(feature = "file-persistence")]
pub mod json_file;
pub mod schema;

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow, bail};
use serde_json::Value;
use uuid::Uuid;

use crate::game::player::{DEFAULT_PLAYER_NAME, Player};
use crate::game::species::SpeciesId;
use crate::game::structure_kind;
use crate::game::visitor::{GiftRecord, VisitorRecord};
use crate::game::zoo::{Waypoint, world_seed_from_player};
use crate::game::{Animal, AnimalState, Habitat, HabitatTheme, Structure, Zoo, species};
use glam::vec2;
use schema::*;

/// Single-instance convenience API. The shared-instance fast path uses
/// `JsonFileRepository::lock()` directly to hold a critical section across
/// load/mutate/save. `load` here is kept for tests and for future repository
/// backends that don't need cross-process coordination.
pub trait ZooRepository {
    #[allow(dead_code)]
    fn load(&self) -> Result<Option<Zoo>>;
    fn save(&self, zoo: &Zoo) -> Result<()>;
}

pub fn snapshot_from_zoo(zoo: &Zoo) -> ZooSnapshot {
    ZooSnapshot {
        schema_version: SCHEMA_VERSION,
        player: PlayerDto {
            id: zoo.player.id,
            name: zoo.player.name.clone(),
        },
        last_saved_at: zoo.last_saved_at,
        coins: zoo.coins,
        food: zoo.food,
        dna_helix: zoo.dna_helix,
        habitats: zoo
            .habitats
            .iter()
            .map(|h| HabitatDto {
                id: h.id,
                theme: h.theme.name().to_string(),
                level: h.level,
                animal_ids: h.animal_ids.clone(),
                upgrade_finishes_at: h.upgrade_finishes_at,
                tile_x: h.tile.0,
                tile_y: h.tile.1,
            })
            .collect(),
        animals: zoo
            .animals
            .values()
            .map(|a| AnimalDto {
                id: a.id,
                species: a.species.to_string(),
                level: a.level,
                last_collected_at: a.last_collected_at,
                state: match a.state {
                    AnimalState::Idle => AnimalStateDto::Idle,
                    AnimalState::Breeding {
                        partner_id,
                        ends_at,
                    } => AnimalStateDto::Breeding {
                        partner_id,
                        ends_at,
                    },
                },
            })
            .collect(),
        structures: zoo
            .structures
            .iter()
            .map(|s| StructureDto {
                id: s.id,
                kind: s.kind.to_string(),
                level: s.level,
                last_collected_at: s.last_collected_at,
            })
            .collect(),
        claimed_gifts: zoo.claimed_gifts.iter().copied().collect(),
        discovered_recipes: zoo
            .discovered_recipes
            .iter()
            .map(|s| s.to_string())
            .collect(),
        nest_count: zoo.nest_count,
        nests: zoo
            .nests
            .iter()
            .map(|n| NestDto { id: n.id, slots: n.slots })
            .collect(),
        exotic_skip_window: zoo.exotic_skip_window,
        visitors: zoo
            .visitors
            .values()
            .map(|v| VisitorDto {
                player_id: v.player_id,
                display_name: v.display_name.clone(),
                first_visited_at: v.first_visited_at,
                last_visited_at: v.last_visited_at,
                last_pos_x: v.last_pos.x,
                last_pos_y: v.last_pos.y,
                gift_inbox: v
                    .gift_inbox
                    .iter()
                    .map(|g| GiftRecordDto {
                        id: g.id,
                        sender_id: g.sender_id,
                        sender_name: g.sender_name.clone(),
                        species: g.species.to_string(),
                        level: g.level,
                        dropped_at: g.dropped_at,
                    })
                    .collect(),
                permissions: v.permissions.0,
            })
            .collect(),
        world_seed: zoo.world_seed,
        waypoints: zoo
            .waypoints
            .iter()
            .map(|w| WaypointDto {
                id: w.id,
                name: w.name.clone(),
                x: w.pos.x,
                y: w.pos.y,
            })
            .collect(),
        species_dupes: zoo
            .species_dupes
            .iter()
            .map(|(s, c)| SpeciesDupeDto {
                species: s.to_string(),
                count: *c,
            })
            .collect(),
        zoo_level: zoo.zoo_level,
        zoo_upgrade_finishes_at: zoo.zoo_upgrade_finishes_at,
        pedestals: zoo
            .pedestals
            .iter()
            .map(|p| PedestalDto {
                id: p.id,
                tile_x: p.tile.0,
                tile_y: p.tile.1,
                animal: p.animal,
                dedicated_at: p.dedicated_at,
                cooldown_until: p.cooldown_until,
            })
            .collect(),
        unplaced_pedestals: zoo.unplaced_pedestals,
    }
}

/// Migration notes captured during `parse_snapshot` that don't fit the
/// typed `ZooSnapshot` (e.g. v8→v9 consolidation breadcrumbs). The caller
/// folds these into the user-visible `LoadedZoo::warnings`.
#[derive(Default, Clone)]
pub struct MigrationNotes {
    pub messages: Vec<String>,
}

/// Parse raw JSON bytes into a current-version `ZooSnapshot`, migrating older schemas in place.
pub fn parse_snapshot(bytes: &[u8]) -> Result<ZooSnapshot> {
    parse_snapshot_with_notes(bytes).map(|(snap, _)| snap)
}

pub fn parse_snapshot_with_notes(bytes: &[u8]) -> Result<(ZooSnapshot, MigrationNotes)> {
    let mut value: Value =
        serde_json::from_slice(bytes).map_err(|e| anyhow!("parsing save: {e}"))?;
    let mut version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("save file missing schema_version"))?;

    while version < SCHEMA_VERSION as u64 {
        match version {
            1 => {
                migrate_v1_to_v2(&mut value);
                version = 2;
            }
            2 => {
                migrate_v2_to_v3(&mut value);
                version = 3;
            }
            3 => {
                migrate_v3_to_v4(&mut value);
                version = 4;
            }
            4 => {
                migrate_v4_to_v5(&mut value);
                version = 5;
            }
            5 => {
                migrate_v5_to_v6(&mut value);
                version = 6;
            }
            6 => {
                migrate_v6_to_v7(&mut value);
                version = 7;
            }
            7 => {
                migrate_v7_to_v8(&mut value);
                version = 8;
            }
            8 => {
                migrate_v8_to_v9(&mut value);
                version = 9;
            }
            9 => {
                migrate_v9_to_v10(&mut value);
                version = 10;
            }
            10 => {
                migrate_v10_to_v11(&mut value);
                version = 11;
            }
            11 => {
                migrate_v11_to_v12(&mut value);
                version = 12;
            }
            12 => {
                migrate_v12_to_v13(&mut value);
                version = 13;
            }
            13 => {
                migrate_v13_to_v14(&mut value);
                version = 14;
            }
            14 => {
                migrate_v14_to_v15(&mut value);
                version = 15;
            }
            15 => {
                migrate_v15_to_v16(&mut value);
                version = 16;
            }
            16 => {
                migrate_v16_to_v17(&mut value);
                version = 17;
            }
            17 => {
                migrate_v17_to_v18(&mut value);
                version = 18;
            }
            18 => {
                migrate_v18_to_v19(&mut value);
                version = 19;
            }
            19 => {
                migrate_v19_to_v20(&mut value);
                version = 20;
            }
            20 => {
                migrate_v20_to_v21(&mut value);
                version = 21;
            }
            v => bail!("no migration path from schema version {v}"),
        }
    }
    if version != SCHEMA_VERSION as u64 {
        bail!("unsupported save schema version: {version}");
    }

    // Pull out and clear the side-channel breadcrumb keys before typed
    // deserialization (serde would reject unknown fields by default — and
    // even with allow we don't want them on the typed DTO).
    let mut notes = MigrationNotes::default();
    if let Value::Object(map) = &mut value {
        if let Some(Value::Array(lines)) = map.remove("_v9_consolidation_notes") {
            for line in lines {
                if let Value::String(s) = line {
                    notes.messages.push(s);
                }
            }
        }
    }

    let snap: ZooSnapshot = serde_json::from_value(value)
        .map_err(|e| anyhow!("parsing migrated snapshot: {e}"))?;
    Ok((snap, notes))
}

fn migrate_v1_to_v2(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(2u64));
        map.entry("player".to_string()).or_insert_with(|| {
            serde_json::json!({
                "id": crate::game::ids::new_id().to_string(),
                "name": DEFAULT_PLAYER_NAME,
            })
        });
    }
}

fn migrate_v2_to_v3(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(3u64));
        map.entry("food".to_string()).or_insert(Value::from(0u64));
        map.entry("structures".to_string())
            .or_insert(Value::Array(Vec::new()));
        map.entry("claimed_gifts".to_string())
            .or_insert(Value::Array(Vec::new()));
    }
}

fn migrate_v3_to_v4(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(4u64));
        map.entry("pending_offspring".to_string())
            .or_insert(Value::Array(Vec::new()));
    }
}

fn migrate_v4_to_v5(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(5u64));
        map.entry("discovered_recipes".to_string())
            .or_insert(Value::Array(Vec::new()));
    }
}

fn migrate_v5_to_v6(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(6u64));
        // Old saves default to one nest; they have to buy the rest.
        map.entry("nest_count".to_string()).or_insert(Value::from(1u64));
    }
}

fn migrate_v6_to_v7(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(7u64));
        // v7 adds the holding pen + per-breeding destination. Empty pen +
        // every existing Breeding state defaults to AutoPlace (current behavior).
        map.entry("holding_pen".to_string())
            .or_insert(Value::Array(Vec::new()));
        if let Some(Value::Array(animals)) = map.get_mut("animals") {
            for a in animals.iter_mut() {
                if let Some(state) = a.get_mut("state").and_then(|s| s.as_object_mut()) {
                    if state.get("kind").and_then(|k| k.as_str()) == Some("Breeding")
                        && !state.contains_key("destination")
                    {
                        state.insert(
                            "destination".to_string(),
                            Value::String("AutoPlace".to_string()),
                        );
                    }
                }
            }
        }
    }
}

/// v8 rips out the redeem queue plumbing: holding pen, pending offspring, and
/// the per-breeding destination field. Held entries are discarded (they were
/// never claimable in v7 once the UI shipped without a holding-pen panel) and
/// any in-flight Breeding states lose the now-unused `destination` field.
fn migrate_v7_to_v8(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(8u64));
        map.remove("holding_pen");
        map.remove("pending_offspring");
        if let Some(Value::Array(animals)) = map.get_mut("animals") {
            for a in animals.iter_mut() {
                if let Some(state) = a.get_mut("state").and_then(|s| s.as_object_mut()) {
                    if state.get("kind").and_then(|k| k.as_str()) == Some("Breeding") {
                        state.remove("destination");
                    }
                }
            }
        }
    }
}

/// v9 introduces:
/// - DNA Helix secondary currency (`dna_helix: u64`, seeded at 0)
/// - Per-habitat timed upgrades (`upgrade_finishes_at`, seeded null)
/// - Single habitat per theme — extras are consolidated, **coins refunded**
///   for their purchase + each level of upgrades, and their animals merged
///   into the survivor (or dropped with a warning if the survivor is full
///   at its current capacity).
///
/// Consolidation rule: keep the highest-level habitat per theme (tiebreak on
/// animal count). For each dropped habitat, refund a flat 500 coins per
/// habitat slot + 200·L² per upgrade level — these mirror the historical
/// `habitat_purchase_cost(0)` and `habitat_upgrade_cost` curves at the time
/// the player paid them.
fn migrate_v8_to_v9(value: &mut Value) {
    let Value::Object(map) = value else { return };
    map.insert("schema_version".into(), Value::from(9u64));
    map.entry("dna_helix".to_string()).or_insert(Value::from(0u64));

    // Stamp every habitat with a null upgrade slot.
    if let Some(Value::Array(habs)) = map.get_mut("habitats") {
        for h in habs.iter_mut() {
            if let Some(obj) = h.as_object_mut() {
                obj.entry("upgrade_finishes_at".to_string()).or_insert(Value::Null);
            }
        }
    }

    // Group habitats by theme. For groups > 1, keep the head (highest level,
    // tiebreak on animal count) and fold the rest into the survivor + refund.
    let mut refund: u64 = 0;
    let mut overflow_animals: usize = 0;
    let mut consolidated_themes: Vec<(String, usize)> = Vec::new();

    if let Some(Value::Array(habs_arr)) = map.get_mut("habitats") {
        let original: Vec<Value> = std::mem::take(habs_arr);
        let mut by_theme: std::collections::BTreeMap<String, Vec<Value>> = Default::default();
        for h in original {
            let theme = h
                .get("theme")
                .and_then(|t| t.as_str())
                .unwrap_or("Forest")
                .to_string();
            by_theme.entry(theme).or_default().push(h);
        }
        let mut kept: Vec<Value> = Vec::new();
        for (theme, mut group) in by_theme {
            if group.len() > 1 {
                consolidated_themes.push((theme.clone(), group.len()));
            }
            // Sort survivors: highest level first, then most animals.
            group.sort_by(|a, b| {
                let la = a.get("level").and_then(|v| v.as_u64()).unwrap_or(1);
                let lb = b.get("level").and_then(|v| v.as_u64()).unwrap_or(1);
                let aa = a
                    .get("animal_ids")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let ab = b
                    .get("animal_ids")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                lb.cmp(&la).then(ab.cmp(&aa))
            });
            let mut iter = group.into_iter();
            let mut survivor = iter.next().expect("group is non-empty");
            for dropped in iter {
                let level = dropped
                    .get("level")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1);
                // Purchase refund + each upgrade level (200·L² historical curve).
                refund = refund.saturating_add(500);
                for l in 1..level {
                    refund = refund.saturating_add(200u64.saturating_mul(l).saturating_mul(l));
                }
                // Survivor's capacity (3 + (L-1)*2) is the post-merge cap.
                let survivor_level = survivor
                    .get("level")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1);
                let survivor_cap = 3 + (survivor_level.saturating_sub(1)) * 2;
                if let (Some(Value::Array(s_ids)), Some(Value::Array(d_ids))) = (
                    survivor.get_mut("animal_ids").map(|v| v),
                    dropped.get("animal_ids"),
                ) {
                    for id in d_ids.iter().cloned() {
                        if (s_ids.len() as u64) < survivor_cap {
                            s_ids.push(id);
                        } else {
                            overflow_animals += 1;
                        }
                    }
                }
            }
            kept.push(survivor);
        }
        *habs_arr = kept;
    }

    // Apply the refund to coins.
    if refund > 0 {
        let current = map
            .get("coins")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        map.insert(
            "coins".to_string(),
            Value::from(current.saturating_add(refund)),
        );
    }

    // Stash a side-channel breadcrumb that `zoo_from_snapshot` will translate
    // into user-visible warnings. Stored under a key that no DTO consumes.
    if !consolidated_themes.is_empty() || overflow_animals > 0 {
        let summary: Vec<String> = consolidated_themes
            .iter()
            .map(|(t, n)| format!("{n} {t} habitats merged"))
            .collect();
        let mut lines = summary;
        if refund > 0 {
            lines.push(format!("refunded {refund} coins from consolidation"));
        }
        if overflow_animals > 0 {
            lines.push(format!("{overflow_animals} animal(s) overflowed and were dropped"));
        }
        map.insert(
            "_v9_consolidation_notes".to_string(),
            Value::Array(lines.into_iter().map(Value::String).collect()),
        );
    }
}

/// v10 adds the exotic-shop early-open override (`exotic_skip_window`),
/// seeded null — old saves have never paid to skip a window.
fn migrate_v9_to_v10(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(10u64));
        map.entry("exotic_skip_window".to_string()).or_insert(Value::Null);
    }
}

/// v12 introduces co-op visitor records. Pre-v12 saves have no `visitors`
/// field; seed it as an empty array — no other state changes.
fn migrate_v11_to_v12(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(12u64));
        map.entry("visitors".to_string())
            .or_insert(Value::Array(Vec::new()));
    }
}

/// v12 → v13 adds the procedural world: a `world_seed` derived from the player
/// id (so existing saves get a stable world) and an empty `chunk_deltas` list.
fn migrate_v12_to_v13(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(13u64));
        let seed = map
            .get("player")
            .and_then(|p| p.get("id"))
            .and_then(|id| id.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(world_seed_from_player)
            .unwrap_or(0);
        map.entry("world_seed".to_string())
            .or_insert(Value::from(seed));
        map.entry("chunk_deltas".to_string())
            .or_insert(Value::Array(Vec::new()));
    }
}

/// v13 → v14 adds player-placed fast-travel waypoints (empty for old saves).
fn migrate_v13_to_v14(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(14u64));
        map.entry("waypoints".to_string())
            .or_insert(Value::Array(Vec::new()));
    }
}

/// v15 introduces one-of-each ownership + per-species Rank. Old saves get an
/// empty `species_dupes`; the loader collapses any existing duplicate animals
/// into Rank progress at restore time.
fn migrate_v14_to_v15(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(15u64));
        map.entry("species_dupes".to_string())
            .or_insert(Value::Array(Vec::new()));
    }
}

/// v16 adds the upgradable zoo: a `zoo_level` (0 = base plot) and an optional
/// in-flight expansion timer. Old saves default to the starting plot.
fn migrate_v15_to_v16(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(16u64));
        map.entry("zoo_level".to_string()).or_insert(Value::from(0u64));
        map.entry("zoo_upgrade_finishes_at".to_string())
            .or_insert(Value::Null);
    }
}

/// v17 adds per-visitor `permissions` (co-op grants like selling). Old visitor
/// records default to no special permissions; `serde(default)` also covers any
/// records without the field, so the bump is effectively a version label.
fn migrate_v16_to_v17(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(17u64));
        if let Some(Value::Array(visitors)) = map.get_mut("visitors") {
            for v in visitors.iter_mut() {
                if let Some(obj) = v.as_object_mut() {
                    obj.entry("permissions".to_string()).or_insert(Value::from(0u64));
                }
            }
        }
    }
}

/// v18 adds placeable pedestals. Old saves have none, so an empty list (also
/// covered by `serde(default)`) — the bump is effectively a version label.
fn migrate_v17_to_v18(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(18u64));
        map.entry("pedestals".to_string())
            .or_insert(Value::Array(Vec::new()));
    }
}

/// v19 adds the unplaced-pedestal hotbar inventory (and per-pedestal lock /
/// cooldown timestamps, handled by `serde(default)`). Old saves default to 0.
fn migrate_v18_to_v19(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(19u64));
        map.entry("unplaced_pedestals".to_string())
            .or_insert(Value::from(0u64));
    }
}

/// v20 retires the infinite procedural open world. `chunk_deltas` (the only
/// persisted wild-world state) is dropped; `world_seed` is kept (it still seeds
/// the cosmetic terrain/grass scatter and expedition arrangement). The field is
/// simply removed from the save — the typed `ZooSnapshot` no longer carries it.
fn migrate_v19_to_v20(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(20u64));
        map.remove("chunk_deltas");
    }
}

/// v20 → v21: nests gained persisted ids/occupants (`nests`). Pre-v21 saves have
/// none — `serde(default)` yields an empty list and the loader rebuilds nests
/// from `nest_count`, so this step only bumps the version.
fn migrate_v20_to_v21(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(21u64));
    }
}

/// v11 introduces isometric grid placement: each habitat gains `tile_x`/`tile_y`.
/// Pre-v11 saves have no coordinates, so auto-layout the habitats onto the grid
/// deterministically — row-major, stepping by the 2×2 footprint so nothing
/// overlaps (8 columns fit in the 16-wide grid).
fn migrate_v10_to_v11(value: &mut Value) {
    if let Value::Object(map) = value {
        map.insert("schema_version".into(), Value::from(11u64));
        if let Some(Value::Array(habs)) = map.get_mut("habitats") {
            const COLS: i64 = 8; // 16-wide grid / 2-wide footprint
            for (i, h) in habs.iter_mut().enumerate() {
                if let Some(obj) = h.as_object_mut() {
                    let i = i as i64;
                    let x = (i % COLS) * 2;
                    let y = (i / COLS) * 2;
                    obj.entry("tile_x".to_string()).or_insert(Value::from(x));
                    obj.entry("tile_y".to_string()).or_insert(Value::from(y));
                }
            }
        }
    }
}

/// Outcome of loading a snapshot. `warnings` carries human-readable messages
/// about entries that were silently dropped (e.g. an animal whose species id
/// has been retired from the catalog in a refactor). Surfacing these as a
/// status banner in the app makes "what happened to my frog?" answerable
/// without making old saves unloadable.
pub struct LoadedZoo {
    pub zoo: Zoo,
    pub warnings: Vec<String>,
}

/// Convert a parsed `ZooSnapshot` into a runtime `Zoo`. Tolerant of entries
/// referencing species / structure kinds that are no longer in the catalogs
/// (e.g. after a rename) — they're dropped and recorded in `warnings`. Hard
/// errors are reserved for things we genuinely can't recover from (schema
/// version mismatch, unknown habitat theme).
pub fn zoo_from_snapshot(s: ZooSnapshot) -> Result<LoadedZoo> {
    if s.schema_version != SCHEMA_VERSION {
        return Err(anyhow!(
            "unsupported save schema version {}; expected {}",
            s.schema_version,
            SCHEMA_VERSION
        ));
    }

    let mut warnings: Vec<String> = Vec::new();

    // Habitat themes are a fixed enum; an unknown theme is a hard error
    // (corrupted save or future-version save). We keep the strict behavior.
    let mut habitats = s
        .habitats
        .into_iter()
        .map(|h| {
            let theme = HabitatTheme::from_str(&h.theme)
                .ok_or_else(|| anyhow!("unknown habitat theme: {}", h.theme))?;
            Ok::<_, anyhow::Error>(Habitat {
                id: h.id,
                theme,
                level: h.level,
                animal_ids: h.animal_ids,
                tile: (h.tile_x, h.tile_y),
                upgrade_finishes_at: h.upgrade_finishes_at,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Lifetime per-species duplicate counts (drives Rank). Unknown ids dropped.
    let mut species_dupes: HashMap<SpeciesId, u32> = HashMap::new();
    for d in s.species_dupes {
        if let Some(def) = species::try_get(&d.species) {
            *species_dupes.entry(def.id).or_insert(0) += d.count;
        }
    }

    let mut animals: HashMap<Uuid, Animal> = HashMap::new();
    let mut owned_species: HashSet<SpeciesId> = HashSet::new();
    let mut dropped_species: std::collections::HashMap<String, usize> = Default::default();
    let mut collapsed = 0usize;
    for a in s.animals {
        let Some(def) = species::try_get(&a.species) else {
            *dropped_species.entry(a.species.clone()).or_default() += 1;
            continue;
        };
        // One-of-each: a second copy of an owned species becomes a duplicate
        // (Rank progress) rather than a second map entry.
        if owned_species.contains(&def.id) {
            *species_dupes.entry(def.id).or_insert(0) += 1;
            collapsed += 1;
            continue;
        }
        let state = match a.state {
            AnimalStateDto::Idle => AnimalState::Idle,
            AnimalStateDto::Breeding {
                partner_id,
                ends_at,
            } => AnimalState::Breeding {
                partner_id,
                ends_at,
            },
        };
        owned_species.insert(def.id);
        animals.insert(
            a.id,
            Animal {
                id: a.id,
                species: def.id,
                level: a.level,
                stage: 0, // set below from species_dupes
                last_collected_at: a.last_collected_at,
                state,
            },
        );
    }
    // Project the final duplicate counts onto each surviving animal's Rank.
    for a in animals.values_mut() {
        a.stage = crate::game::rank::rank_for_dupes(
            species_dupes.get(a.species).copied().unwrap_or(0),
        );
    }
    if collapsed > 0 {
        warnings.push(format!(
            "merged {collapsed} duplicate animal(s) into species Rank progress"
        ));
    }
    for (species_id, n) in dropped_species {
        warnings.push(format!(
            "dropped {n} animal(s) of unknown species '{species_id}'"
        ));
    }
    // Prune dangling animal_ids on habitats so the UI doesn't dereference
    // ids that no longer exist in the animals map.
    for h in habitats.iter_mut() {
        h.animal_ids.retain(|id| animals.contains_key(id));
    }

    let mut structures: Vec<Structure> = Vec::new();
    let mut dropped_kinds: std::collections::HashMap<String, usize> = Default::default();
    for st in s.structures {
        let Some(def) = structure_kind::try_get(&st.kind) else {
            *dropped_kinds.entry(st.kind.clone()).or_default() += 1;
            continue;
        };
        structures.push(Structure {
            id: st.id,
            kind: def.id,
            level: st.level,
            last_collected_at: st.last_collected_at,
        });
    }
    for (kind_id, n) in dropped_kinds {
        warnings.push(format!(
            "dropped {n} structure(s) of unknown kind '{kind_id}'"
        ));
    }

    let mut discovered_recipes: HashSet<SpeciesId> = HashSet::new();
    let mut dropped_recipes: Vec<String> = Vec::new();
    for id in s.discovered_recipes {
        match species::try_get(&id) {
            Some(d) => {
                discovered_recipes.insert(d.id);
            }
            None => dropped_recipes.push(id),
        }
    }
    if !dropped_recipes.is_empty() {
        warnings.push(format!(
            "removed {} unknown hybrid(s) from codex: {}",
            dropped_recipes.len(),
            dropped_recipes.join(", ")
        ));
    }

    let mut visitors: HashMap<Uuid, VisitorRecord> = HashMap::new();
    let mut dropped_gift_species: std::collections::HashMap<String, usize> = Default::default();
    for v in s.visitors {
        let mut inbox: Vec<GiftRecord> = Vec::new();
        for g in v.gift_inbox {
            match species::try_get(&g.species) {
                Some(def) => inbox.push(GiftRecord {
                    id: g.id,
                    sender_id: g.sender_id,
                    sender_name: g.sender_name,
                    species: def.id,
                    level: g.level,
                    dropped_at: g.dropped_at,
                }),
                None => {
                    *dropped_gift_species.entry(g.species).or_default() += 1;
                }
            }
        }
        visitors.insert(
            v.player_id,
            VisitorRecord {
                player_id: v.player_id,
                display_name: v.display_name,
                first_visited_at: v.first_visited_at,
                last_visited_at: v.last_visited_at,
                last_pos: vec2(v.last_pos_x, v.last_pos_y),
                gift_inbox: inbox,
                permissions: crate::game::visitor::PermissionSet(v.permissions),
            },
        );
    }
    for (species_id, n) in dropped_gift_species {
        warnings.push(format!(
            "dropped {n} gift(s) of unknown species '{species_id}'"
        ));
    }

    // Rebuild pedestals, dropping any dedication whose animal no longer exists.
    let pedestals: Vec<crate::game::pedestal::Pedestal> = s
        .pedestals
        .into_iter()
        .map(|p| {
            let animal = p.animal.filter(|aid| animals.contains_key(aid));
            crate::game::pedestal::Pedestal {
                id: p.id,
                tile: (p.tile_x, p.tile_y),
                // Drop the lock if the dedicated animal vanished.
                dedicated_at: animal.and(p.dedicated_at),
                cooldown_until: p.cooldown_until,
                animal,
            }
        })
        .collect();

    let zoo = Zoo {
        player: Player {
            id: s.player.id,
            name: s.player.name,
        },
        visitors,
        coins: s.coins,
        food: s.food,
        // dna_helix loads from the schema field after the v9 bump; until
        // that lands the default-of-0 keeps every save loadable.
        dna_helix: s.dna_helix,
        habitats,
        animals,
        species_dupes,
        structures,
        claimed_gifts: s.claimed_gifts.into_iter().collect::<HashSet<_>>(),
        discovered_recipes,
        nest_count: s.nest_count.min(crate::game::zoo::MAX_NESTS),
        // Restore persisted nests (stable ids + deposited occupants) so an open
        // nest panel survives a snapshot round-trip and partial deposits aren't
        // lost. Pre-v21 saves have no `nests` → rebuild one per owned nest with a
        // fresh id and let `relink_breeding_nests` re-seat in-progress pairs.
        nests: {
            let cap = s.nest_count.min(crate::game::zoo::MAX_NESTS) as usize;
            if s.nests.is_empty() {
                (0..cap).map(|_| crate::game::zoo::Nest::new()).collect()
            } else {
                s.nests
                    .iter()
                    .take(cap)
                    .map(|n| crate::game::zoo::Nest { id: n.id, slots: n.slots, offspring: None })
                    .collect()
            }
        },
        exotic_skip_window: s.exotic_skip_window,
        // A 0 seed means a pre-v13 save that slipped through without derivation;
        // derive a stable seed from the player id so the world is reproducible.
        world_seed: if s.world_seed == 0 {
            world_seed_from_player(s.player.id)
        } else {
            s.world_seed
        },
        waypoints: s
            .waypoints
            .into_iter()
            .map(|w| Waypoint { id: w.id, name: w.name, pos: vec2(w.x, w.y) })
            .collect(),
        // Runtime plot origin defaults to the hub centre; a hub placement
        // reassigns it after load.
        plot_origin: crate::game::plot::world_center(),
        zoo_level: s.zoo_level.min(crate::game::zoo::MAX_ZOO_LEVEL),
        zoo_upgrade_finishes_at: s.zoo_upgrade_finishes_at,
        pedestals,
        unplaced_pedestals: s.unplaced_pedestals,
        last_saved_at: s.last_saved_at,
    };
    let mut zoo = zoo;
    zoo.relink_breeding_nests();

    Ok(LoadedZoo { zoo, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn snapshot_roundtrip_preserves_state() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut zoo = Zoo::new(now);
        zoo.player.rename("Alex");
        zoo.coins = 5000;
        zoo.food = 200;
        zoo.buy_animal("field_mouse", now).unwrap();
        zoo.buy_animal("field_mouse", now).unwrap();
        zoo.claimed_gifts.insert(Uuid::new_v4());
        // Expand the zoo with an in-flight build so both expansion fields persist.
        zoo.coins += 50_000;
        zoo.start_zoo_upgrade(now).unwrap();
        let ready = zoo.zoo_upgrade_finishes_at.unwrap() + chrono::Duration::seconds(1);
        zoo.claim_zoo_upgrade(ready).unwrap();
        zoo.start_zoo_upgrade(ready).unwrap();

        let snap = snapshot_from_zoo(&zoo);
        let json = serde_json::to_vec(&snap).unwrap();
        let snap2 = parse_snapshot(&json).unwrap();
        let loaded = zoo_from_snapshot(snap2).unwrap();
        assert!(loaded.warnings.is_empty(), "clean round-trip should have no warnings");
        let zoo2 = loaded.zoo;

        assert_eq!(zoo2.coins, zoo.coins);
        assert_eq!(zoo2.food, zoo.food);
        assert_eq!(zoo2.player.id, zoo.player.id);
        assert_eq!(zoo2.player.name, "Alex");
        assert_eq!(zoo2.habitats.len(), zoo.habitats.len());
        assert_eq!(zoo2.animals.len(), zoo.animals.len());
        assert_eq!(zoo2.structures.len(), zoo.structures.len());
        assert_eq!(zoo2.claimed_gifts, zoo.claimed_gifts);
        assert_eq!(zoo2.zoo_level, 1);
        assert_eq!(zoo2.zoo_upgrade_finishes_at, zoo.zoo_upgrade_finishes_at);
    }

    #[test]
    fn nest_ids_and_occupants_survive_roundtrip() {
        // Regression: nests used to be rebuilt from `nest_count` with fresh ids on
        // every load, so an open nest panel closed on the next reload/online
        // resync (its `active_nest` id no longer matched). Now ids + deposited
        // occupants round-trip intact.
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut zoo = Zoo::new(now);
        zoo.coins = 1_000_000;
        zoo.buy_nest().unwrap();
        let (_, a) = zoo.buy_animal("field_mouse", now).unwrap();
        zoo.nests[0].slots = [Some(a), None]; // a partial deposit
        let nest_id = zoo.nests[0].id;

        let json = serde_json::to_vec(&snapshot_from_zoo(&zoo)).unwrap();
        let zoo2 = zoo_from_snapshot(parse_snapshot(&json).unwrap()).unwrap().zoo;

        assert_eq!(zoo2.nests.len(), 1);
        assert_eq!(zoo2.nests[0].id, nest_id, "nest id must be stable across a reload");
        assert_eq!(zoo2.nests[0].slots, [Some(a), None], "deposited occupant survives");
    }

    #[test]
    fn v15_save_migrates_to_v16_with_zoo_expansion_defaults() {
        // A minimal v15 save object; the migrator should add v16 expansion
        // fields defaulting to a base, un-upgraded plot.
        let v15 = serde_json::json!({
            "schema_version": 15,
            "player": { "id": Uuid::new_v4(), "name": "Old" },
            "last_saved_at": Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap(),
            "coins": 10, "food": 0, "dna_helix": 0,
            "habitats": [], "animals": [], "structures": [],
            "claimed_gifts": [], "discovered_recipes": [],
            "nest_count": 1, "exotic_skip_window": null,
            "visitors": [], "world_seed": 42, "chunk_deltas": [],
            "waypoints": [], "species_dupes": [],
        });
        let bytes = serde_json::to_vec(&v15).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert_eq!(snap.zoo_level, 0);
        assert_eq!(snap.zoo_upgrade_finishes_at, None);
    }

    #[test]
    fn v18_save_migrates_to_v19_with_empty_hotbar() {
        // A minimal v18 save (with one pre-lock pedestal) should migrate to v19
        // with `unplaced_pedestals` defaulting to 0 and the pedestal's new lock
        // fields defaulting to null.
        let v18 = serde_json::json!({
            "schema_version": 18,
            "player": { "id": Uuid::new_v4(), "name": "Old" },
            "last_saved_at": Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap(),
            "coins": 10, "food": 0, "dna_helix": 0,
            "habitats": [], "animals": [], "structures": [],
            "claimed_gifts": [], "discovered_recipes": [],
            "nest_count": 1, "exotic_skip_window": null,
            "visitors": [], "world_seed": 42, "chunk_deltas": [],
            "waypoints": [], "species_dupes": [],
            "zoo_level": 0, "zoo_upgrade_finishes_at": null,
            "pedestals": [{ "id": Uuid::new_v4(), "tile_x": 1, "tile_y": 2, "animal": null }],
        });
        let bytes = serde_json::to_vec(&v18).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert_eq!(snap.unplaced_pedestals, 0);
        assert_eq!(snap.pedestals.len(), 1);
        assert_eq!(snap.pedestals[0].dedicated_at, None);
        assert_eq!(snap.pedestals[0].cooldown_until, None);
    }

    #[test]
    fn species_dupes_round_trip_and_restore_rank() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut zoo = Zoo::new(now);
        let id = zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        for _ in 0..10 {
            zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        }
        assert_eq!(zoo.animals[&id].stage, 1); // Silver

        let json = serde_json::to_vec(&snapshot_from_zoo(&zoo)).unwrap();
        let zoo2 = zoo_from_snapshot(parse_snapshot(&json).unwrap()).unwrap().zoo;
        assert_eq!(*zoo2.species_dupes.get("field_mouse").unwrap(), 10);
        // Rank is re-derived onto the surviving animal on load.
        let a = zoo2.animals.values().find(|a| a.species == "field_mouse").unwrap();
        assert_eq!(a.stage, 1);
    }

    #[test]
    fn loading_collapses_legacy_duplicate_animals_into_rank() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        // Hand-build a v15 snapshot with three same-species animals (as a
        // pre-one-of-each save would have) and confirm collapse.
        let mut snap = snapshot_from_zoo(&Zoo::new(now));
        for _ in 0..3 {
            snap.animals.push(crate::persistence::schema::AnimalDto {
                id: Uuid::new_v4(),
                species: "field_mouse".to_string(),
                level: 1,
                last_collected_at: now,
                state: crate::persistence::schema::AnimalStateDto::Idle,
            });
        }
        let loaded = zoo_from_snapshot(snap).unwrap();
        let zoo = loaded.zoo;
        assert_eq!(zoo.animals.len(), 1, "duplicates collapse to one");
        // Two extras become duplicate Rank progress.
        assert_eq!(*zoo.species_dupes.get("field_mouse").unwrap(), 2);
        assert!(loaded.warnings.iter().any(|w| w.contains("merged")));
    }

    #[test]
    fn snapshot_roundtrip_preserves_world_seed() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut zoo = Zoo::new(now);
        zoo.world_seed = 0xDEAD_BEEF_1234;

        let snap = snapshot_from_zoo(&zoo);
        let json = serde_json::to_vec(&snap).unwrap();
        let zoo2 = zoo_from_snapshot(parse_snapshot(&json).unwrap()).unwrap().zoo;

        assert_eq!(zoo2.world_seed, zoo.world_seed);
    }

    #[test]
    fn v19_save_migrates_to_v20_dropping_chunk_deltas() {
        let v19 = serde_json::json!({
            "schema_version": 19,
            "player": { "id": Uuid::new_v4(), "name": "Old" },
            "last_saved_at": Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap(),
            "coins": 10, "food": 0, "dna_helix": 0,
            "habitats": [], "animals": [], "structures": [],
            "claimed_gifts": [], "discovered_recipes": [],
            "nest_count": 1, "exotic_skip_window": null,
            "visitors": [], "world_seed": 42,
            "chunk_deltas": [{ "cx": 1, "cy": 2, "removed": [0], "partial": [] }],
            "waypoints": [], "species_dupes": [],
            "zoo_level": 0, "zoo_upgrade_finishes_at": null,
            "pedestals": [], "unplaced_pedestals": 0,
        });
        let bytes = serde_json::to_vec(&v19).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        // world_seed survives the migration; chunk_deltas is gone from the type.
        assert_eq!(snap.world_seed, 42);
    }

    #[test]
    fn snapshot_roundtrip_preserves_pedestals() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut zoo = Zoo::new(now);
        zoo.dna_helix = 100_000;
        zoo.coins = 100_000;
        // One placed pedestal with a dedicated (locked) animal, one placed empty,
        // plus some unplaced stock in the hotbar.
        zoo.unplaced_pedestals = 3;
        let ped = zoo.place_pedestal((1, -2)).unwrap();
        zoo.purchase_animal("field_mouse", now).unwrap();
        let animal = *zoo.animals.keys().next().unwrap();
        zoo.dedicate_animal(ped, animal, now).unwrap();
        zoo.place_pedestal((-3, 1)).unwrap();

        let snap = snapshot_from_zoo(&zoo);
        let json = serde_json::to_vec(&snap).unwrap();
        let zoo2 = zoo_from_snapshot(parse_snapshot(&json).unwrap()).unwrap().zoo;

        assert_eq!(zoo2.pedestals.len(), 2);
        assert_eq!(zoo2.unplaced_pedestals, 1);
        let p = zoo2.pedestals.iter().find(|p| p.id == ped).expect("pedestal present");
        assert_eq!(p.tile, (1, -2));
        assert_eq!(p.animal, Some(animal));
        assert_eq!(p.dedicated_at, Some(now), "lock timestamp round-trips");
        assert!(zoo2.pedestals.iter().any(|p| p.tile == (-3, 1) && p.animal.is_none()));
    }

    #[test]
    fn snapshot_roundtrip_preserves_waypoints() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut zoo = Zoo::new(now);
        let id = zoo
            .add_waypoint("Oasis", glam::vec2(123_456.0, 654_321.0))
            .unwrap();

        let snap = snapshot_from_zoo(&zoo);
        let json = serde_json::to_vec(&snap).unwrap();
        let zoo2 = zoo_from_snapshot(parse_snapshot(&json).unwrap()).unwrap().zoo;

        assert_eq!(zoo2.waypoints.len(), 1);
        let w = &zoo2.waypoints[0];
        assert_eq!(w.id, id);
        assert_eq!(w.name, "Oasis");
        assert_eq!(w.pos.x, 123_456.0);
        assert_eq!(w.pos.y, 654_321.0);
    }

    #[test]
    fn v13_save_migrates_to_v14_with_empty_waypoints() {
        let pid = Uuid::new_v4();
        let v13 = serde_json::json!({
            "schema_version": 13,
            "player": { "id": pid.to_string(), "name": "Alex" },
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 100, "food": 0, "dna_helix": 0,
            "habitats": [], "animals": [], "structures": [],
            "claimed_gifts": [], "discovered_recipes": [],
            "nest_count": 1, "exotic_skip_window": null, "visitors": [],
            "world_seed": 42, "chunk_deltas": []
        });
        let bytes = serde_json::to_vec(&v13).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert!(snap.waypoints.is_empty());
    }

    #[test]
    fn v12_save_migrates_to_v13_with_seed_from_player() {
        let pid = Uuid::new_v4();
        let v12 = serde_json::json!({
            "schema_version": 12,
            "player": { "id": pid.to_string(), "name": "Alex" },
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 100,
            "food": 0,
            "dna_helix": 0,
            "habitats": [],
            "animals": [],
            "structures": [],
            "claimed_gifts": [],
            "discovered_recipes": [],
            "nest_count": 1,
            "exotic_skip_window": null,
            "visitors": []
        });
        let bytes = serde_json::to_vec(&v12).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert_eq!(snap.world_seed, world_seed_from_player(pid));
    }

    #[test]
    fn snapshot_rejects_unsupported_version() {
        let bad = serde_json::json!({"schema_version": 99, "coins": 0});
        let bytes = serde_json::to_vec(&bad).unwrap();
        assert!(parse_snapshot(&bytes).is_err());
    }

    #[test]
    fn v1_save_chains_through_to_current() {
        let v1 = serde_json::json!({
            "schema_version": 1,
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 42,
            "habitats": [{
                "id": Uuid::new_v4().to_string(),
                "theme": "Forest",
                "level": 1,
                "animal_ids": []
            }],
            "animals": []
        });
        let bytes = serde_json::to_vec(&v1).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert_eq!(snap.coins, 42);
        assert_eq!(snap.food, 0);
        assert!(snap.structures.is_empty());
        assert!(snap.claimed_gifts.is_empty());
        assert_eq!(snap.player.name, DEFAULT_PLAYER_NAME);
    }

    #[test]
    fn v2_save_migrates_to_current() {
        let v2 = serde_json::json!({
            "schema_version": 2,
            "player": {"id": Uuid::new_v4().to_string(), "name": "Alex"},
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 100,
            "habitats": [],
            "animals": []
        });
        let bytes = serde_json::to_vec(&v2).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert_eq!(snap.food, 0);
        assert_eq!(snap.player.name, "Alex");
    }

    #[test]
    fn v3_save_migrates_to_current_with_empty_pending_offspring() {
        let v3 = serde_json::json!({
            "schema_version": 3,
            "player": {"id": Uuid::new_v4().to_string(), "name": "Alex"},
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 1,
            "food": 2,
            "habitats": [],
            "animals": [],
            "structures": [],
            "claimed_gifts": []
        });
        let bytes = serde_json::to_vec(&v3).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert!(snap.discovered_recipes.is_empty());
    }

    #[test]
    fn v4_save_migrates_to_v5_with_empty_discovered_recipes() {
        let v4 = serde_json::json!({
            "schema_version": 4,
            "player": {"id": Uuid::new_v4().to_string(), "name": "Alex"},
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 1,
            "food": 2,
            "habitats": [],
            "animals": [],
            "structures": [],
            "claimed_gifts": [],
            "pending_offspring": []
        });
        let bytes = serde_json::to_vec(&v4).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert!(snap.discovered_recipes.is_empty());
        // The v4→v5→v6 chain also seeded a starter nest_count.
        assert_eq!(snap.nest_count, 1);
    }

    #[test]
    fn v5_save_migrates_to_v6_with_starter_nest_count() {
        let v5 = serde_json::json!({
            "schema_version": 5,
            "player": {"id": Uuid::new_v4().to_string(), "name": "Alex"},
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 100,
            "food": 0,
            "habitats": [],
            "animals": [],
            "structures": [],
            "claimed_gifts": [],
            "pending_offspring": [],
            "discovered_recipes": []
        });
        let bytes = serde_json::to_vec(&v5).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert_eq!(snap.nest_count, 1);
    }

    /// v7 saves migrate to v8 by stripping the now-defunct `holding_pen` and
    /// `pending_offspring` fields and the `destination` discriminator from
    /// any in-flight Breeding state. Held offspring (which weren't redeemable
    /// in the final v7 UI anyway) are silently discarded.
    #[test]
    fn v7_save_migrates_to_v8_drops_pen_pending_and_destination() {
        let partner_id = Uuid::new_v4();
        let v7 = serde_json::json!({
            "schema_version": 7,
            "player": {"id": Uuid::new_v4().to_string(), "name": "Alex"},
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 100,
            "food": 0,
            "habitats": [],
            "animals": [{
                "id": Uuid::new_v4().to_string(),
                "species": "field_mouse",
                "level": 1,
                "last_collected_at": "2026-01-01T00:00:00Z",
                "state": {
                    "kind": "Breeding",
                    "partner_id": partner_id.to_string(),
                    "ends_at": "2026-01-01T00:01:00Z",
                    "destination": "HoldingPen"
                }
            }],
            "structures": [],
            "claimed_gifts": [],
            "pending_offspring": [{"species": "field_mouse", "bred_at": "2026-01-01T00:00:00Z"}],
            "discovered_recipes": [],
            "nest_count": 1,
            "holding_pen": [{"species": "field_mouse", "level": 1, "bred_at": "2026-01-01T00:00:00Z"}]
        });
        let bytes = serde_json::to_vec(&v7).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        // Breeding state lost the `destination` field — it parses as plain {partner_id, ends_at}.
        match &snap.animals[0].state {
            AnimalStateDto::Breeding { .. } => {}
            _ => panic!("expected Breeding state on the migrated animal"),
        }
    }

    #[test]
    fn v10_save_migrates_to_v11_with_grid_layout() {
        // Two habitats, no tile coords. v10→v11 must assign non-overlapping
        // anchors (2×2 footprints stepping by 2).
        let v10 = serde_json::json!({
            "schema_version": 10,
            "player": {"id": Uuid::new_v4().to_string(), "name": "Alex"},
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 1,
            "food": 0,
            "dna_helix": 0,
            "habitats": [
                {"id": Uuid::new_v4().to_string(), "theme": "Forest", "level": 1, "animal_ids": [], "upgrade_finishes_at": null},
                {"id": Uuid::new_v4().to_string(), "theme": "Wetland", "level": 1, "animal_ids": [], "upgrade_finishes_at": null}
            ],
            "animals": [],
            "structures": [],
            "claimed_gifts": [],
            "discovered_recipes": [],
            "nest_count": 1,
            "exotic_skip_window": null
        });
        let bytes = serde_json::to_vec(&v10).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert_eq!((snap.habitats[0].tile_x, snap.habitats[0].tile_y), (0, 0));
        assert_eq!((snap.habitats[1].tile_x, snap.habitats[1].tile_y), (2, 0));
        // And the loaded zoo's habitats carry those tiles.
        let loaded = zoo_from_snapshot(snap).unwrap();
        let mut tiles: Vec<_> = loaded.zoo.habitats.iter().map(|h| h.tile).collect();
        tiles.sort();
        assert_eq!(tiles, vec![(0, 0), (2, 0)]);
    }

    #[test]
    fn v11_save_migrates_to_v12_with_empty_visitors() {
        let v11 = serde_json::json!({
            "schema_version": 11,
            "player": {"id": Uuid::new_v4().to_string(), "name": "Alex"},
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 1,
            "food": 0,
            "dna_helix": 0,
            "habitats": [],
            "animals": [],
            "structures": [],
            "claimed_gifts": [],
            "discovered_recipes": [],
            "nest_count": 1,
            "exotic_skip_window": null
        });
        let bytes = serde_json::to_vec(&v11).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert!(snap.visitors.is_empty());
        let loaded = zoo_from_snapshot(snap).unwrap();
        assert!(loaded.zoo.visitors.is_empty());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn v12_round_trips_visitor_records() {
        use crate::game::visitor::{GiftRecord, VisitorRecord};
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut zoo = Zoo::new(now);
        let visitor_id = Uuid::new_v4();
        let mut v = VisitorRecord::new(visitor_id, "Buddy", now);
        v.last_pos = glam::vec2(123.5, 678.25);
        v.gift_inbox.push(GiftRecord {
            id: Uuid::new_v4(),
            sender_id: Uuid::new_v4(),
            sender_name: "Pal".to_string(),
            species: "field_mouse",
            level: 2,
            dropped_at: now,
        });
        zoo.visitors.insert(visitor_id, v);

        let snap = snapshot_from_zoo(&zoo);
        let bytes = serde_json::to_vec(&snap).unwrap();
        let snap2 = parse_snapshot(&bytes).unwrap();
        let loaded = zoo_from_snapshot(snap2).unwrap();
        let restored = loaded.zoo.visitors.get(&visitor_id).expect("visitor preserved");
        assert_eq!(restored.display_name, "Buddy");
        assert_eq!(restored.last_pos.x, 123.5);
        assert_eq!(restored.last_pos.y, 678.25);
        assert_eq!(restored.gift_inbox.len(), 1);
        assert_eq!(restored.gift_inbox[0].species, "field_mouse");
    }

    #[test]
    fn v11_snapshot_round_trips_tiles() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        let wid = zoo.buy_habitat(HabitatTheme::Wetland, (6, 4)).unwrap();
        let snap = snapshot_from_zoo(&zoo);
        let json = serde_json::to_vec(&snap).unwrap();
        let snap2 = parse_snapshot(&json).unwrap();
        let zoo2 = zoo_from_snapshot(snap2).unwrap().zoo;
        let w = zoo2.habitats.iter().find(|h| h.id == wid).unwrap();
        assert_eq!(w.tile, (6, 4));
    }

    /// Regression: an old save that references a species id that's no
    /// longer in the catalog (e.g. after a rename refactor) must load
    /// gracefully — the offending animal is dropped, the dangling
    /// reference in its habitat is cleaned up, and a warning is reported.
    #[test]
    fn load_drops_unknown_species_with_warning() {
        let habitat_id = Uuid::new_v4();
        let lost_animal_id = Uuid::new_v4();
        let live_animal_id = Uuid::new_v4();
        // Hand-rolled current-schema save with one animal of a deliberately
        // retired species id and one valid mouse.
        let payload = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "player": {"id": Uuid::new_v4().to_string(), "name": "Alex"},
            "last_saved_at": "2026-01-01T00:00:00Z",
            "coins": 100,
            "food": 0,
            "dna_helix": 0,
            "habitats": [{
                "id": habitat_id.to_string(),
                "theme": "Forest",
                "level": 1,
                "animal_ids": [lost_animal_id.to_string(), live_animal_id.to_string()],
                "upgrade_finishes_at": null
            }],
            "animals": [
                {
                    "id": lost_animal_id.to_string(),
                    "species": "retiredCritter",
                    "level": 1,
                    "last_collected_at": "2026-01-01T00:00:00Z",
                    "state": {"kind": "Idle"}
                },
                {
                    "id": live_animal_id.to_string(),
                    "species": "field_mouse",
                    "level": 1,
                    "last_collected_at": "2026-01-01T00:00:00Z",
                    "state": {"kind": "Idle"}
                }
            ],
            "structures": [],
            "claimed_gifts": [],
            "discovered_recipes": [],
            "nest_count": 1
        });
        let bytes = serde_json::to_vec(&payload).unwrap();
        let snap = parse_snapshot(&bytes).unwrap();
        let loaded = zoo_from_snapshot(snap).unwrap();
        // The mouse survived; the retired species did not.
        assert_eq!(loaded.zoo.animals.len(), 1);
        assert!(loaded.zoo.animals.contains_key(&live_animal_id));
        // Dangling animal_id was pruned from the habitat.
        let habitat = &loaded.zoo.habitats[0];
        assert_eq!(habitat.animal_ids, vec![live_animal_id]);
        // A warning naming the lost species was recorded.
        assert!(
            loaded
                .warnings
                .iter()
                .any(|w| w.contains("retiredCritter")),
            "expected a warning mentioning 'retiredCritter', got: {:?}",
            loaded.warnings
        );
    }

}

