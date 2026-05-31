use macroquad::math::Vec2;

use crate::game::avatar::Facing;
use crate::game::avatar_system::{PLANE_H, PLANE_W};
use crate::game::species::{self, SpeciesId};
use crate::game::structure_kind::{self, StructureKindId};

// ── Dialog ───────────────────────────────────────────────────────────────────

pub struct DialogScript {
    pub greeting: &'static str,
}

// ── Vendor inventory (ShopKeeper component) ──────────────────────────────────

#[derive(Clone, Copy)]
pub enum ShopListingKind {
    Animal(SpeciesId),
    Structure(StructureKindId),
    NestSlot,
}

pub struct ShopListing {
    pub kind: ShopListingKind,
}

pub struct VendorInventory {
    pub listings: Vec<ShopListing>,
}

impl VendorInventory {
    fn default_shop() -> Self {
        let mut animal_ids: Vec<SpeciesId> = species::all_purchasable().map(|d| d.id).collect();
        animal_ids.sort_by_key(|&id| species::get(id).purchase_cost);

        let mut struct_ids: Vec<StructureKindId> =
            structure_kind::all().map(|d| d.id).collect();
        struct_ids.sort_by_key(|&id| structure_kind::get(id).purchase_cost);

        let mut listings: Vec<ShopListing> = animal_ids
            .into_iter()
            .map(|id| ShopListing { kind: ShopListingKind::Animal(id) })
            .chain(struct_ids.into_iter().map(|id| ShopListing {
                kind: ShopListingKind::Structure(id),
            }))
            .collect();
        listings.push(ShopListing { kind: ShopListingKind::NestSlot });
        Self { listings }
    }
}

// ── NPC role (closed sum type) ───────────────────────────────────────────────

pub enum NpcRole {
    ShopKeeper {
        inventory: VendorInventory,
        dialog: DialogScript,
    },
    Breeder {
        dialog: DialogScript,
    },
}

// ── Behavior trait (open for future wandering, schedules, etc.) ──────────────

pub trait NpcBehavior {
    fn tick(&self, pos: &mut Vec2, dt: f32);
}

struct StaticNpcBehavior;

impl NpcBehavior for StaticNpcBehavior {
    fn tick(&self, _pos: &mut Vec2, _dt: f32) {}
}

// ── Composed NPC entity ──────────────────────────────────────────────────────

pub struct Npc {
    pub name: &'static str,
    pub pos: Vec2,
    pub facing: Facing,
    pub interact_radius: f32,
    pub role: NpcRole,
    behaviors: Vec<Box<dyn NpcBehavior>>,
}

impl Npc {
    pub fn tick(&mut self, dt: f32) {
        for b in &self.behaviors {
            b.tick(&mut self.pos, dt);
        }
    }

    pub fn in_range(&self, other: Vec2) -> bool {
        (self.pos - other).length() <= self.interact_radius
    }

    pub fn dialog(&self) -> &DialogScript {
        match &self.role {
            NpcRole::ShopKeeper { dialog, .. } | NpcRole::Breeder { dialog } => dialog,
        }
    }

    fn shopkeeper(name: &'static str, pos: Vec2) -> Self {
        Self {
            name,
            pos,
            facing: Facing::S,
            interact_radius: 140.0,
            role: NpcRole::ShopKeeper {
                inventory: VendorInventory::default_shop(),
                dialog: DialogScript {
                    greeting: "Welcome to my shop! What'll it be?",
                },
            },
            behaviors: vec![Box::new(StaticNpcBehavior)],
        }
    }

    fn breeder(name: &'static str, pos: Vec2) -> Self {
        Self {
            name,
            pos,
            facing: Facing::S,
            interact_radius: 140.0,
            role: NpcRole::Breeder {
                dialog: DialogScript {
                    greeting: "I know all the crossbreed secrets. Pick a pair!",
                },
            },
            behaviors: vec![Box::new(StaticNpcBehavior)],
        }
    }
}

/// Fixed NPC roster for the world. Called once in `GameApp::new`.
pub fn default_npcs() -> Vec<Npc> {
    vec![
        Npc::shopkeeper("Zara the Merchant", Vec2::new(PLANE_W * 0.18, PLANE_H * 0.5)),
        Npc::breeder("Rex the Breeder", Vec2::new(PLANE_W * 0.82, PLANE_H * 0.5)),
    ]
}
