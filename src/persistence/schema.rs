use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 12;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ZooSnapshot {
    pub schema_version: u32,
    pub player: PlayerDto,
    pub last_saved_at: DateTime<Utc>,
    pub coins: u64,
    pub food: u64,
    /// Secondary currency added in v9. Older saves migrate with `dna_helix: 0`;
    /// `serde(default)` keeps hand-rolled v8-ish test JSON loadable too.
    #[serde(default)]
    pub dna_helix: u64,
    pub habitats: Vec<HabitatDto>,
    pub animals: Vec<AnimalDto>,
    pub structures: Vec<StructureDto>,
    pub claimed_gifts: Vec<Uuid>,
    /// Hybrid species ids the player has unlocked. Empty on fresh saves.
    pub discovered_recipes: Vec<String>,
    /// How many concurrent breedings the player can run (1..=4).
    pub nest_count: u8,
    /// New in v10. Index of an exotic-shop window the player paid to open
    /// early; `None` normally. `serde(default)` keeps older test JSON loadable.
    #[serde(default)]
    pub exotic_skip_window: Option<i64>,
    /// New in v12. Persisted state for non-owner players who have visited
    /// this zoo. Empty in pure single-player saves. `serde(default)` keeps
    /// pre-v12 JSON loadable through the migrator.
    #[serde(default)]
    pub visitors: Vec<VisitorDto>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VisitorDto {
    pub player_id: Uuid,
    pub display_name: String,
    pub first_visited_at: DateTime<Utc>,
    pub last_visited_at: DateTime<Utc>,
    pub last_pos_x: f32,
    pub last_pos_y: f32,
    #[serde(default)]
    pub gift_inbox: Vec<GiftRecordDto>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GiftRecordDto {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub sender_name: String,
    pub species: String,
    pub level: u8,
    pub dropped_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerDto {
    pub id: Uuid,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HabitatDto {
    pub id: Uuid,
    pub theme: String,
    pub level: u8,
    pub animal_ids: Vec<Uuid>,
    /// New in v9. When `Some`, a level-up to `level+1` is in flight; finishes
    /// at this instant. None when idle (or fresh from v8 migration).
    #[serde(default)]
    pub upgrade_finishes_at: Option<DateTime<Utc>>,
    /// New in v11. Anchor tile (grid coords) of this habitat's footprint on
    /// the isometric world grid. `serde(default)` → (0,0) for pre-v11 JSON;
    /// the v10→v11 migration assigns non-overlapping tiles.
    #[serde(default)]
    pub tile_x: i32,
    #[serde(default)]
    pub tile_y: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AnimalDto {
    pub id: Uuid,
    pub species: String,
    pub level: u8,
    pub last_collected_at: DateTime<Utc>,
    pub state: AnimalStateDto,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind")]
pub enum AnimalStateDto {
    Idle,
    /// `destination` was removed in v8 — redeem-on-click made it dead state.
    Breeding {
        partner_id: Uuid,
        ends_at: DateTime<Utc>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StructureDto {
    pub id: Uuid,
    pub kind: String,
    pub level: u8,
    pub last_collected_at: DateTime<Utc>,
}
