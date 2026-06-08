//! Sound-effect cache. `build.rs` embeds every file in `assets/sfx/` (ogg/wav)
//! into a `(&str, &[u8])` table; sounds are decoded once at startup and played
//! by id. The id is the file stem, lookup is fuzzy (snake/camel/kebab
//! insensitive), and missing ids are a silent no-op so the game runs before any
//! audio art is added.
//!
//! Asset naming conventions (per-animal `{species_id}_poke`, global `*_sfx`) and
//! how to add a clip are documented in [`assets/sfx/README.md`](../../assets/sfx/README.md).

use std::collections::HashMap;

use macroquad::audio::{Sound, load_sound_from_bytes, play_sound_once};

static SFX_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/sfx_table.rs"));

#[derive(Default)]
pub struct Sounds {
    map: HashMap<String, Sound>,
}

impl Sounds {
    /// Decode every embedded sound. Async because macroquad's decoder is —
    /// call once during startup (before the frame loop).
    pub async fn load_all() -> Self {
        let mut map = HashMap::new();
        for (id, bytes) in SFX_TABLE {
            if let Ok(s) = load_sound_from_bytes(bytes).await {
                map.insert(id.to_string(), s);
            }
        }
        Self { map }
    }

    /// Play `id` once if it exists (exact, then fuzzy match); else do nothing.
    pub fn play(&self, id: &str) {
        if let Some(s) = self.get(id) {
            play_sound_once(s);
        }
    }

    fn get(&self, id: &str) -> Option<&Sound> {
        if let Some(s) = self.map.get(id) {
            return Some(s);
        }
        let target = normalize_id(id);
        self.map
            .iter()
            .find(|(k, _)| normalize_id(k) == target)
            .map(|(_, s)| s)
    }
}

/// Canonical form for fuzzy id matching: lowercase, with `_`/`-` removed.
fn normalize_id(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}
