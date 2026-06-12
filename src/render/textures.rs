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
static NPC_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/npc_table.rs"));
static HOTBAR_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/hotbar_table.rs"));
/// General UI sprites (e.g. the catch/stamina `bar_container` + `bar_fill`).
static UI_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/ui_table.rs"));
/// Embedded UI fonts (`assets/fonts/*.ttf|otf`), e.g. Bebas Neue.
static FONT_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/font_table.rs"));

/// Load the custom UI font (the first font bundled in `assets/fonts/`). `None`
/// when no font is bundled (callers fall back to the macroquad default font).
pub fn ui_font() -> Option<macroquad::text::Font> {
    let (_, bytes) = FONT_TABLE.first()?;
    macroquad::text::load_ttf_font_from_bytes(bytes).ok()
}
/// Ground-structure sprites (nests, future silos), keyed by id (e.g. `"nest"`).
static STRUCTURE_TABLE: &[(&str, &[u8])] =
    include!(concat!(env!("OUT_DIR"), "/structure_table.rs"));
/// Terrain props, keyed `"<biome>/<name>"` (e.g. `"forest/oak"`).
static TERRAIN_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/terrain_table.rs"));
/// Player character sprite sheets, keyed by stem (e.g. `"player"`), from
/// `assets/player/`. Used by the animated avatar; `None` falls back to the
/// procedural toon-ball.
static PLAYER_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/player_table.rs"));
/// Active-skill icons, keyed by stem (e.g. `"net_skill_icon"`), from
/// `assets/skills/`. A dedicated folder since the skill catalog will grow.
static SKILL_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/skill_table.rs"));

/// Which embedded table to look an id up in.
#[derive(Clone, Copy)]
enum Kind {
    Animal,
    Tile,
    Habitat,
    Icon,
    Npc,
    Hotbar,
    Ui,
    Terrain,
    Structure,
    Player,
    Skill,
}

impl Kind {
    fn table(self) -> &'static [(&'static str, &'static [u8])] {
        match self {
            Kind::Animal => ANIMAL_TABLE,
            Kind::Tile => TILE_TABLE,
            Kind::Habitat => HABITAT_TABLE,
            Kind::Icon => ICON_TABLE,
            Kind::Npc => NPC_TABLE,
            Kind::Hotbar => HOTBAR_TABLE,
            Kind::Ui => UI_TABLE,
            Kind::Terrain => TERRAIN_TABLE,
            Kind::Structure => STRUCTURE_TABLE,
            Kind::Player => PLAYER_TABLE,
            Kind::Skill => SKILL_TABLE,
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Kind::Animal => "a:",
            Kind::Tile => "t:",
            Kind::Habitat => "h:",
            Kind::Icon => "i:",
            Kind::Npc => "n:",
            Kind::Hotbar => "hb:",
            Kind::Ui => "ui:",
            Kind::Terrain => "tp:",
            Kind::Structure => "s:",
            Kind::Player => "pl:",
            Kind::Skill => "sk:",
        }
    }
}

/// All terrain-prop ids bundled for `biome` (the folder name, matched
/// case-insensitively), e.g. `["forest/oak", "forest/rock"]`. Empty if the biome
/// has no props. Used by the world-gen scatter to pick scenery per tile.
pub fn terrain_prop_ids(biome: &str) -> Vec<&'static str> {
    let prefix = format!("{}/", biome.to_ascii_lowercase());
    TERRAIN_TABLE
        .iter()
        .filter(|(k, _)| k.to_ascii_lowercase().starts_with(&prefix))
        .map(|(k, _)| *k)
        .collect()
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

    /// An NPC sprite by id, from `assets/npcs/` (e.g. "structure_merchant").
    /// `None` falls back to placeholder vector art.
    pub fn npc(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Npc, id)
    }

    /// A hotbar-UI sprite by id, from `assets/hotbar/` (e.g. "slot_container",
    /// "slot_container_selected", or an item icon like "pedestal").
    pub fn hotbar(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Hotbar, id)
    }

    /// A general UI sprite by id, from `assets/ui/` (e.g. "bar_container",
    /// "bar_fill"). `None` falls back to primitive drawing.
    pub fn ui(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Ui, id)
    }

    /// A terrain prop by its `"<biome>/<name>"` id, from `assets/terrain/`.
    pub fn terrain(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Terrain, id)
    }

    /// A ground-structure sprite by id, from `assets/structures/` (e.g. "nest").
    /// `None` falls back to placeholder vector art.
    pub fn structure(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Structure, id)
    }

    /// A player-character sprite sheet by id, from `assets/player/` (e.g.
    /// "player"). `None` falls back to the procedural toon-ball avatar.
    pub fn player(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Player, id)
    }

    /// An active-skill icon by id, from `assets/skills/` (e.g. "net_skill_icon").
    /// `None` falls back to the empty slot frame.
    pub fn skill(&mut self, id: &str) -> Option<Texture2D> {
        self.get(Kind::Skill, id)
    }

    /// Any bundled ground tile (first by sorted id), for auto-detecting the
    /// world tile size. `None` until tile art is dropped into `assets/tiles/`.
    pub fn any_tile(&mut self) -> Option<Texture2D> {
        let id = TILE_TABLE.first()?.0;
        self.get(Kind::Tile, id)
    }

    /// The grass tuft atlas (`assets/tiles/grass_atlas.png`), converted so the
    /// blade shape drives **alpha** while RGB is forced white — so the grass
    /// mesh's per-vertex colour fully controls the hue. The source PNG is a
    /// grayscale tuft on black (BinbunGrass's `shape` texture, where the red
    /// channel is the mask), which would otherwise render as opaque black boxes.
    /// Cached; uses Linear filtering for soft tufts. `None` if the asset is
    /// missing.
    pub fn grass_atlas(&mut self) -> Option<Texture2D> {
        let key = "grass_atlas_rgba".to_string();
        if let Some(slot) = self.cache.get(&key) {
            return slot.clone();
        }
        let built = lookup_bytes(TILE_TABLE, "grass_atlas").and_then(|bytes| {
            let mut img = Image::from_file_with_format(bytes, Some(ImageFormat::Png)).ok()?;
            for px in img.bytes.chunks_exact_mut(4) {
                // Linear luminance → alpha keeps the soft feathered edges, so big
                // overlapping translucent tufts blend into a painterly field.
                let lum = px[0].max(px[1]).max(px[2]);
                px[0] = 255;
                px[1] = 255;
                px[2] = 255;
                px[3] = lum;
            }
            let tex = Texture2D::from_image(&img);
            tex.set_filter(FilterMode::Linear);
            Some(tex)
        });
        self.cache.insert(key, built.clone());
        built
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
