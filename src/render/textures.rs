//! Macroquad texture cache. Replaces egui's `AnimalIcons`.
//!
//! `build.rs` embeds three `(&str, &[u8])` PNG tables (animals, tiles,
//! habitats). Each texture is decoded into a `Texture2D` on first request and
//! cached by id. Lookup is fuzzy: a snake_case filename matches a camelCase id
//! and vice versa (ported from the old `ui::images::normalize_id`). Missing
//! ids return `None` so callers can fall back to placeholder drawing.

use std::collections::HashMap;

use macroquad::prelude::*;

static ANIMAL_TABLE: &[(&str, &[u8])] =
    include!(concat!(env!("OUT_DIR"), "/animal_icons_table.rs"));
static TILE_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/tile_table.rs"));
static HABITAT_TABLE: &[(&str, &[u8])] =
    include!(concat!(env!("OUT_DIR"), "/habitat_table.rs"));
static ICON_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/icon_table.rs"));

/// Which embedded table to look an id up in.
#[derive(Clone, Copy)]
enum Kind {
    Animal,
    Tile,
    Habitat,
    Icon,
}

impl Kind {
    fn table(self) -> &'static [(&'static str, &'static [u8])] {
        match self {
            Kind::Animal => ANIMAL_TABLE,
            Kind::Tile => TILE_TABLE,
            Kind::Habitat => HABITAT_TABLE,
            Kind::Icon => ICON_TABLE,
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Kind::Animal => "a:",
            Kind::Tile => "t:",
            Kind::Habitat => "h:",
            Kind::Icon => "i:",
        }
    }
}

#[derive(Default)]
pub struct Textures {
    /// Cache keyed by "<prefix><id>". Value is `None` when no PNG is bundled,
    /// so we don't rescan the table on every frame for missing art.
    cache: HashMap<String, Option<Texture2D>>,
}

impl Textures {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn animal(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Animal, id)
    }

    pub fn tile(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Tile, id)
    }

    pub fn habitat(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Habitat, id)
    }

    /// Income-currency icon (e.g. "coin", "dna_helix"), from `assets/icons/`.
    pub fn icon(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Icon, id)
    }

    /// Any bundled ground tile (first by sorted id), for auto-detecting the
    /// world tile size. `None` until tile art is dropped into `assets/tiles/`.
    pub fn any_tile(&mut self) -> Option<Texture2D> {
        let id = TILE_TABLE.first()?.0;
        self.get(Kind::Tile, id)
    }

    fn get(&mut self, kind: Kind, id: &str) -> Option<Texture2D> {
        let key = format!("{}{}", kind.prefix(), id);
        if let Some(slot) = self.cache.get(&key) {
            return slot.clone();
        }
        let decoded = lookup_bytes(kind.table(), id).map(|bytes| {
            let tex = Texture2D::from_file_with_format(bytes, Some(ImageFormat::Png));
            tex.set_filter(FilterMode::Nearest);
            tex
        });
        self.cache.insert(key, decoded.clone());
        decoded
    }
}

/// Exact match first, then a normalized comparison so snake_case filenames and
/// camelCase ids line up (`king_cobra.png` ↔ `kingCobra`).
fn lookup_bytes(table: &'static [(&'static str, &'static [u8])], id: &str) -> Option<&'static [u8]> {
    if let Some((_, bytes)) = table.iter().find(|(k, _)| *k == id) {
        return Some(bytes);
    }
    let target = normalize_id(id);
    table
        .iter()
        .find(|(k, _)| normalize_id(k) == target)
        .map(|(_, bytes)| *bytes)
}

/// Canonical form for fuzzy id matching: lowercase, with `_`/`-` removed.
fn normalize_id(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::normalize_id;

    #[test]
    fn snake_camel_and_kebab_normalize_alike() {
        assert_eq!(normalize_id("king_cobra"), "kingcobra");
        assert_eq!(normalize_id("kingCobra"), "kingcobra");
        assert_eq!(normalize_id("King-Cobra"), "kingcobra");
    }
}
