//! STUB: per-biome animal vendors.
//!
//! Today every non-hybrid, non-exotic species is sold from a single `SHOP`
//! window (`render::menus::draw_shop`). The plan is to segment animals to
//! distinct NPC merchants — one per biome — each selling only that biome's
//! fauna, placed around the world.
//!
//! This module stubs the data model for that future system: the vendor roster,
//! their flavour names, and per-vendor stock queries. Only the *seam* exists
//! here — no merchant is placed in the world, has dialogue, or moves stock yet.
//!
//! The original seven biomes ship as `open` vendors, so their fauna stay
//! buyable in the current single-window shop. The eleven new biomes are stubbed
//! `coming_soon`: their fauna are catch-only in the wild until the segmented
//! shop is built. Flip `coming_soon` to `false` (and later give the vendor a
//! world position) to "open" a merchant.

use crate::game::species::{self, HabitatTheme, SpeciesDef, SpeciesId};

/// A biome-themed merchant NPC. STUB: no world placement / dialogue yet.
#[derive(Clone, Copy, Debug)]
pub struct BiomeVendor {
    /// The biome whose fauna this merchant sells.
    pub theme: HabitatTheme,
    /// Flavour name shown above the vendor's stock.
    pub npc_name: &'static str,
    /// When true the merchant isn't open for business yet — their biome's fauna
    /// are catch-only until the per-NPC shop system lands.
    pub coming_soon: bool,
}

/// The full vendor roster, one merchant per biome. Original biomes are open;
/// the new-biome merchants are stubbed `coming_soon`.
pub const VENDORS: &[BiomeVendor] = &[
    // ── Open today (sold in the current single-window shop) ─────────────────
    BiomeVendor { theme: HabitatTheme::Farmland, npc_name: "Farmstead Stall",  coming_soon: false },
    BiomeVendor { theme: HabitatTheme::Forest,   npc_name: "Woodland Warden",  coming_soon: false },
    BiomeVendor { theme: HabitatTheme::Arctic,   npc_name: "Glacier Outpost",  coming_soon: false },
    BiomeVendor { theme: HabitatTheme::Savanna,  npc_name: "Safari Post",      coming_soon: false },
    BiomeVendor { theme: HabitatTheme::Wetland,  npc_name: "Marsh Peddler",    coming_soon: false },
    BiomeVendor { theme: HabitatTheme::Jungle,   npc_name: "Canopy Bazaar",    coming_soon: false },
    BiomeVendor { theme: HabitatTheme::Ocean,    npc_name: "Tidewater Dock",   coming_soon: false },
    // ── Coming soon (catch-only until the segmented shop ships) ──────────────
    BiomeVendor { theme: HabitatTheme::Desert,    npc_name: "Caravan Trader",   coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Tundra,    npc_name: "Frostward Nomad",  coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Taiga,     npc_name: "Boreal Trapper",   coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Volcanic,  npc_name: "Emberforge Smith", coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Badlands,  npc_name: "Mesa Drifter",     coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Beach,     npc_name: "Boardwalk Vendor", coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Highlands, npc_name: "Summit Sherpa",    coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Mythical,  npc_name: "Wandering Mystic", coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Void,      npc_name: "The Hollow Broker",coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Festive,   npc_name: "Merry Pedlar",     coming_soon: true },
    BiomeVendor { theme: HabitatTheme::Food,      npc_name: "Pantry Keeper",    coming_soon: true },
];

/// The merchant for `theme`, if one is defined.
pub fn vendor_for(theme: HabitatTheme) -> Option<&'static BiomeVendor> {
    VENDORS.iter().find(|v| v.theme == theme)
}

/// Purchasable (non-hybrid, non-exotic) species belonging to a vendor's biome,
/// cheapest first. The future per-NPC shop will render exactly this list.
pub fn stock(theme: HabitatTheme) -> Vec<&'static SpeciesDef> {
    let mut v: Vec<&'static SpeciesDef> =
        species::all_purchasable().filter(|d| d.theme == theme).collect();
    v.sort_by_key(|d| d.purchase_cost);
    v
}

/// Whether a species is currently buyable: it must belong to an *open* vendor.
/// Catch-only new-biome fauna return false. Unknown themes default to buyable.
pub fn is_for_sale(id: SpeciesId) -> bool {
    match species::try_get(id) {
        Some(d) if !d.hybrid && !d.exotic && !species::is_collection_only(id) => {
            vendor_for(d.theme).map_or(true, |v| !v.coming_soon)
        }
        _ => false,
    }
}

/// Species sold by every currently-open vendor — the stock the single-window
/// shop shows today. Replaces the old flat `species::all_purchasable()` listing.
pub fn open_shop_stock() -> Vec<&'static SpeciesDef> {
    let mut v: Vec<&'static SpeciesDef> = species::all_purchasable()
        .filter(|d| vendor_for(d.theme).map_or(true, |vd| !vd.coming_soon))
        .collect();
    v.sort_by_key(|d| d.purchase_cost);
    v
}

/// Names of the merchants that aren't open yet — shown as a "coming soon"
/// teaser in the current shop.
pub fn coming_soon_vendors() -> impl Iterator<Item = &'static BiomeVendor> {
    VENDORS.iter().filter(|v| v.coming_soon)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every biome theme must have exactly one vendor.
    #[test]
    fn one_vendor_per_theme() {
        use std::collections::HashSet;
        let themes: HashSet<_> = VENDORS.iter().map(|v| v.theme as u8 as usize).collect();
        assert_eq!(themes.len(), VENDORS.len(), "duplicate vendor theme");
        // Spot-check a couple of representative biomes resolve.
        assert!(vendor_for(HabitatTheme::Forest).is_some());
        assert!(vendor_for(HabitatTheme::Void).is_some());
    }

    /// A vendor's stock is non-empty and strictly on-theme.
    #[test]
    fn stock_is_on_theme() {
        for v in VENDORS {
            for d in stock(v.theme) {
                assert_eq!(d.theme, v.theme, "{} stock off-theme: {}", v.npc_name, d.id);
            }
        }
    }

    /// Open-shop stock excludes catch-only new-biome fauna.
    #[test]
    fn open_stock_excludes_coming_soon() {
        for d in open_shop_stock() {
            assert!(is_for_sale(d.id), "{} listed but not for sale", d.id);
        }
        // A new-biome wild species is catch-only, not for sale.
        assert!(!is_for_sale("fennec_fox"));
        // An original-biome species stays for sale.
        assert!(is_for_sale("field_mouse"));
    }
}
