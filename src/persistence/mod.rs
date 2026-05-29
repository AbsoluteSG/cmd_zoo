pub mod json_file;
pub mod schema;

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow, bail};
use serde_json::Value;
use uuid::Uuid;

use crate::game::player::{DEFAULT_PLAYER_NAME, Player};
use crate::game::species::SpeciesId;
use crate::game::structure_kind;
use crate::game::{Animal, AnimalState, Habitat, HabitatTheme, Structure, Zoo, species};
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
        exotic_skip_window: zoo.exotic_skip_window,
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
                "id": Uuid::new_v4().to_string(),
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
                upgrade_finishes_at: h.upgrade_finishes_at,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut animals: HashMap<Uuid, Animal> = HashMap::new();
    let mut dropped_species: std::collections::HashMap<String, usize> = Default::default();
    for a in s.animals {
        let Some(def) = species::try_get(&a.species) else {
            *dropped_species.entry(a.species.clone()).or_default() += 1;
            continue;
        };
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
        animals.insert(
            a.id,
            Animal {
                id: a.id,
                species: def.id,
                level: a.level,
                last_collected_at: a.last_collected_at,
                state,
            },
        );
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

    let zoo = Zoo {
        player: Player {
            id: s.player.id,
            name: s.player.name,
        },
        coins: s.coins,
        food: s.food,
        // dna_helix loads from the schema field after the v9 bump; until
        // that lands the default-of-0 keeps every save loadable.
        dna_helix: s.dna_helix,
        habitats,
        animals,
        structures,
        claimed_gifts: s.claimed_gifts.into_iter().collect::<HashSet<_>>(),
        discovered_recipes,
        nest_count: s.nest_count.clamp(1, crate::game::zoo::MAX_NESTS),
        exotic_skip_window: s.exotic_skip_window,
        last_saved_at: s.last_saved_at,
    };

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
        zoo.buy_animal("fieldMouse", now).unwrap();
        zoo.buy_animal("fieldMouse", now).unwrap();
        zoo.claimed_gifts.insert(Uuid::new_v4());

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
                "species": "fieldMouse",
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
            "pending_offspring": [{"species": "fieldMouse", "bred_at": "2026-01-01T00:00:00Z"}],
            "discovered_recipes": [],
            "nest_count": 1,
            "holding_pen": [{"species": "fieldMouse", "level": 1, "bred_at": "2026-01-01T00:00:00Z"}]
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
                    "species": "fieldMouse",
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

