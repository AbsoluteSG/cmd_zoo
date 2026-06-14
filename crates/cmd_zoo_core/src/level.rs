//! Hand-authored **level / blueprint** documents — pure data, no engine.
//!
//! A [`Level`] describes the authored contents of a location (the hub, an
//! expedition arena, or a future place): painted ground tiles, terrain props,
//! NPCs, and nest/food positions. It is produced by the dev-only level editor
//! and consumed by the shipped game's loader to **replace** procedural
//! generation for that location (procedural is the fallback when no level
//! exists). This module is serde-only and headless so it can be unit-tested and
//! reused by tooling; disk/embed loading lives in the client crate.

use serde::{Deserialize, Serialize};

/// Bump when the on-disk shape changes; [`migrate`] upgrades older documents.
pub const LEVEL_SCHEMA_VERSION: u32 = 1;

/// A complete authored location.
///
/// Coordinate spaces:
/// - `tiles`, `props`, `npcs` are **world-absolute** (the shared hub/plaza or an
///   expedition arena).
/// - `nest_tiles` / `food_tiles` are **plot-relative** tile coordinates
///   (centre-relative), applied per player plot.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Level {
    #[serde(default)]
    pub schema_version: u32,
    /// Location key: `"hub"`, `"expedition_<theme>"`, or a custom name.
    pub key: String,
    /// Sparse painted ground cells; unpainted cells keep procedural biome color.
    #[serde(default)]
    pub tiles: Vec<TileCell>,
    #[serde(default)]
    pub props: Vec<PropEntry>,
    #[serde(default)]
    pub npcs: Vec<NpcEntry>,
    /// Plot-relative nest tile per nest slot (index = slot).
    #[serde(default)]
    pub nest_tiles: Vec<[i32; 2]>,
    /// Plot-relative food-structure tile per food slot.
    #[serde(default)]
    pub food_tiles: Vec<[i32; 2]>,
}

/// One painted ground cell: a tile texture id at a world-grid coordinate.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TileCell {
    pub tx: i32,
    pub ty: i32,
    pub tile_id: String,
}

fn default_scale() -> f32 {
    1.0
}

/// A placed terrain prop (decorative billboard) in world coordinates.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PropEntry {
    pub x: f32,
    pub y: f32,
    /// Terrain prop id, e.g. `"forest/oak"`.
    pub id: String,
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub flip: bool,
}

/// A placed NPC in world coordinates. `kind` is a [`crate::game::npc::NpcKind`]
/// serialized as a string; `id`/`label` mirror the sprite + prompt.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NpcEntry {
    pub x: f32,
    pub y: f32,
    pub kind: String,
    pub id: String,
    pub label: String,
}

impl Level {
    /// An empty level for `key` at the current schema version.
    pub fn new(key: impl Into<String>) -> Self {
        Self { schema_version: LEVEL_SCHEMA_VERSION, key: key.into(), ..Default::default() }
    }
}

/// Parse a level document from JSON bytes and forward-migrate it.
pub fn parse_level(bytes: &[u8]) -> anyhow::Result<Level> {
    let level: Level = serde_json::from_slice(bytes)?;
    Ok(migrate(level))
}

/// Serialize a level to pretty JSON (what the editor writes to disk).
pub fn to_json(level: &Level) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(level)?)
}

/// Upgrade an older document to [`LEVEL_SCHEMA_VERSION`]. No-op at v1; add arms
/// here as the shape evolves (mirrors `persistence`'s migration chain).
fn migrate(level: Level) -> Level {
    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let mut l = Level::new("hub");
        l.tiles.push(TileCell { tx: 1, ty: -2, tile_id: "grass".into() });
        l.props.push(PropEntry { x: 10.0, y: 20.0, id: "forest/oak".into(), scale: 1.2, flip: true });
        l.npcs.push(NpcEntry { x: 5.0, y: 6.0, kind: "StructureMerchant".into(), id: "structure_merchant".into(), label: "Shop".into() });
        l.nest_tiles.push([-3, -3]);
        l.food_tiles.push([0, 3]);

        let bytes = to_json(&l).unwrap();
        assert_eq!(parse_level(&bytes).unwrap(), l);
    }

    #[test]
    fn minimal_document_parses_with_defaults() {
        let l = parse_level(br#"{"key":"hub"}"#).unwrap();
        assert_eq!(l.key, "hub");
        assert!(l.tiles.is_empty() && l.props.is_empty() && l.npcs.is_empty());
    }

    #[test]
    fn prop_scale_defaults_to_one() {
        let l = parse_level(br#"{"key":"x","props":[{"x":0.0,"y":0.0,"id":"a"}]}"#).unwrap();
        assert_eq!(l.props[0].scale, 1.0);
        assert!(!l.props[0].flip);
    }
}
