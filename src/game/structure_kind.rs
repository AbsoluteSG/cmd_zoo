use once_cell::sync::Lazy;
use std::collections::HashMap;

pub type StructureKindId = &'static str;

pub struct StructureKindDef {
    pub id: StructureKindId,
    pub display_name: &'static str,
    pub glyph: &'static str,
    pub base_food_per_sec: f64,
    pub base_food_cap: u64,
    pub purchase_cost: u64,
}

static CATALOG: Lazy<HashMap<StructureKindId, StructureKindDef>> = Lazy::new(|| {
    let entries = [
        StructureKindDef {
            id: "hay_bale",
            display_name: "Hay Bale",
            glyph: "≋",
            base_food_per_sec: 0.5,
            base_food_cap: 80,
            purchase_cost: 250,
        },
        StructureKindDef {
            id: "insectary",
            display_name: "Insectary",
            glyph: "✺",
            base_food_per_sec: 1.2,
            base_food_cap: 240,
            purchase_cost: 500,
        },
        StructureKindDef {
            id: "feed_mill",
            display_name: "Feed Mill",
            glyph: "⌬",
            base_food_per_sec: 2.5,
            base_food_cap: 600,
            purchase_cost: 900,
        },
        StructureKindDef {
            id: "aquaculture",
            display_name: "Aquaculture",
            glyph: "≈",
            base_food_per_sec: 4.0,
            base_food_cap: 1200,
            purchase_cost: 1600,
        },
    ];
    entries.into_iter().map(|d| (d.id, d)).collect()
});

pub fn get(id: StructureKindId) -> &'static StructureKindDef {
    CATALOG
        .get(id)
        .unwrap_or_else(|| panic!("unknown structure kind: {id}"))
}

pub fn try_get(id: &str) -> Option<&'static StructureKindDef> {
    CATALOG.get(id)
}

pub fn all() -> impl Iterator<Item = &'static StructureKindDef> {
    CATALOG.values()
}
