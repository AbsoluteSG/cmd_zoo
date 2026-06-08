//! Structure merchant — a fixed-position NPC that sells placeable structures.
//!
//! Today it stocks pedestals; the catalog is a static list so new structures
//! drop in by adding an [`StructureOffer`]. Bought items land in the hotbar
//! inventory (e.g. `Zoo::unplaced_pedestals`) and are placed from there.

use macroquad::math::{Vec2, vec2};

use crate::game::world_chunks::{zoo_center, zoo_half_extent};

/// A kind of placeable structure the merchant can sell.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StructureItemKind {
    Pedestal,
}

/// A single shop line.
pub struct StructureOffer {
    pub kind: StructureItemKind,
    pub name: &'static str,
    pub blurb: &'static str,
}

static OFFERS: &[StructureOffer] = &[StructureOffer {
    kind: StructureItemKind::Pedestal,
    name: "Pedestal",
    blurb: "Dedicate an animal — auto-collects its income (10x offline).",
}];

/// Everything the merchant currently stocks.
pub fn offers() -> &'static [StructureOffer] {
    OFFERS
}

/// Display name shown on the world label and the shop panel.
pub const MERCHANT_NAME: &str = "Structure Merchant";

/// Sprite id looked up in `assets/npcs/` (e.g. `structure_merchant.png`). When
/// the art is missing the renderer falls back to the placeholder vector figure.
pub const MERCHANT_SPRITE_ID: &str = "structure_merchant";

/// The merchant's fixed world position — inset near the top-left of the plot,
/// clear of the nest row and the food-structure row.
pub fn merchant_pos() -> Vec2 {
    let c = zoo_center();
    let half = zoo_half_extent();
    vec2(c.x - half * 0.55, c.y - half * 0.30)
}
