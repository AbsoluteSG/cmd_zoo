//! Animal sprite cache.
//!
//! At build time, `build.rs` scans `assets/animals/*.png` and emits a static
//! `(&str, &[u8])` slice (the species id → PNG bytes table). At runtime we
//! lazily decode + upload each PNG into an `egui::TextureHandle` on first
//! request, then cache by species id. Missing species fall back to a
//! name-initial placeholder at the call site, so art can be added piecemeal.
//!
//! Special id `_mystery` is used for the cross-species outcome preview in the
//! breeding picker (the recipe identity stays hidden until completion).

use std::collections::HashMap;

use egui::{Context, TextureHandle, TextureOptions};

/// Generated at build time. Each entry is `(species_id, png_bytes)`. Sorted
/// by id for deterministic iteration; we look up via linear scan because the
/// catalog is tiny (≤ ~20 entries) and the hit rate is high after warm-up.
static PNG_TABLE: &[(&str, &[u8])] =
    include!(concat!(env!("OUT_DIR"), "/animal_icons_table.rs"));

pub struct AnimalIcons {
    cache: HashMap<&'static str, TextureHandle>,
}

impl Default for AnimalIcons {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimalIcons {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Return the texture handle for `species_id` if a bundled PNG exists.
    /// Decodes + uploads on the first request, then serves from cache.
    pub fn texture(&mut self, ctx: &Context, species_id: &'static str) -> Option<TextureHandle> {
        if let Some(handle) = self.cache.get(species_id) {
            return Some(handle.clone());
        }
        let bytes = bundled_png(species_id)?;
        let handle = decode_to_texture(ctx, species_id, bytes)?;
        self.cache.insert(species_id, handle.clone());
        Some(handle)
    }

    /// The cross-species outcome preview icon, if a `_mystery.png` was bundled.
    /// Callers fall back to rendering a "?" character when this returns None.
    pub fn mystery(&mut self, ctx: &Context) -> Option<TextureHandle> {
        self.texture(ctx, "_mystery")
    }
}

fn bundled_png(species_id: &str) -> Option<&'static [u8]> {
    // Match exactly first, then fall back to a normalized comparison so a
    // snake_case filename (`king_cobra.png`) lines up with a camelCase
    // species id (`kingCobra`) — and vice versa. Normalization lowercases
    // and drops `_`/`-` separators, so `king_cobra`, `kingCobra`, and
    // `King-Cobra` all collapse to the same key.
    if let Some(bytes) = PNG_TABLE
        .iter()
        .find_map(|(id, bytes)| if *id == species_id { Some(*bytes) } else { None })
    {
        return Some(bytes);
    }
    let target = normalize_id(species_id);
    PNG_TABLE.iter().find_map(|(id, bytes)| {
        if normalize_id(id) == target {
            Some(*bytes)
        } else {
            None
        }
    })
}

/// Canonical form for fuzzy filename↔id matching: lowercase, with `_` and
/// `-` separators removed. Leaves the special `_mystery` id matchable since
/// the bundled `_mystery.png` normalizes the same way.
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
        assert_eq!(normalize_id("_mystery"), "mystery");
    }
}

fn decode_to_texture(
    ctx: &Context,
    species_id: &str,
    png_bytes: &[u8],
) -> Option<TextureHandle> {
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let color = egui::ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw());
    Some(ctx.load_texture(
        format!("animal_{species_id}"),
        color,
        TextureOptions::LINEAR,
    ))
}
