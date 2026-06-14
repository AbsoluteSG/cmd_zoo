//! Loads hand-authored [`Level`] blueprints and serves them to the renderer.
//!
//! Shipped half of the level system (the editor is dev-only, behind the `editor`
//! feature). Levels are **embedded** into the binary at build time via the
//! `build.rs` `level_table.rs` table; in dev builds we additionally overlay any
//! on-disk `assets/levels/*.json` so the editor's saves show up without a
//! rebuild. A missing/garbage file is skipped (logged), so a location always
//! falls back to procedural generation.

use std::collections::HashMap;

use cmd_zoo_core::level::{Level, parse_level};

/// Build-embedded level documents: `(file_stem, json_bytes)`.
static LEVEL_TABLE: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/level_table.rs"));

/// Repo `assets/levels` dir (dev only). `CARGO_MANIFEST_DIR` is the client crate
/// root, which is the repo root — the editor only ever runs from there.
#[cfg(any(debug_assertions, feature = "editor"))]
pub fn levels_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("levels")
}

/// In-memory map of location key → authored level.
#[derive(Default)]
pub struct LevelStore {
    map: HashMap<String, Level>,
}

impl LevelStore {
    /// Load every embedded level, then (in dev) overlay on-disk edits.
    pub fn load_all() -> Self {
        let mut map = HashMap::new();
        for (stem, bytes) in LEVEL_TABLE {
            match parse_level(bytes) {
                Ok(level) => {
                    let key = if level.key.is_empty() { (*stem).to_string() } else { level.key.clone() };
                    map.insert(key, level);
                }
                Err(e) => eprintln!("[level] skipping embedded '{stem}': {e}"),
            }
        }
        #[cfg(any(debug_assertions, feature = "editor"))]
        Self::overlay_disk(&mut map);
        Self { map }
    }

    /// Overlay any `assets/levels/*.json` from disk over the embedded set, so the
    /// editor's saves take effect on the next run without rebuilding.
    #[cfg(any(debug_assertions, feature = "editor"))]
    fn overlay_disk(map: &mut HashMap<String, Level>) {
        let dir = levels_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read(&path).map_err(anyhow::Error::from).and_then(|b| parse_level(&b)) {
                Ok(level) if !level.key.is_empty() => {
                    map.insert(level.key.clone(), level);
                }
                Ok(_) => eprintln!("[level] skipping {}: empty key", path.display()),
                Err(e) => eprintln!("[level] skipping {}: {e}", path.display()),
            }
        }
    }

    /// The authored level for `key`, if one exists.
    pub fn get(&self, key: &str) -> Option<&Level> {
        self.map.get(key)
    }

    /// Insert/replace a level (used by the editor after a save so the live game
    /// reflects it immediately).
    #[cfg(feature = "editor")]
    pub fn insert(&mut self, level: Level) {
        self.map.insert(level.key.clone(), level);
    }
}

/// Write a level blueprint to `assets/levels/<key>.json` (atomic temp+rename).
/// Editor-only.
#[cfg(feature = "editor")]
pub fn save_level(level: &Level) -> anyhow::Result<()> {
    let dir = levels_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", level.key));
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, cmd_zoo_core::level::to_json(level)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}
