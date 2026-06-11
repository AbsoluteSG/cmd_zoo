//! Catch gear & loadouts (Phase 3) — one of the **two sources** of catch power
//! the roadmap calls for. The other is owned-collection bonuses (also here, as
//! [`collection_bonus`]). There is deliberately **no character level or skill
//! tree**: your catch power is your equipped tools plus what your zoo already
//! holds, so both feed back into the collection loop.
//!
//! A [`GearItem`] grants passive catch stats and (optionally) an active
//! [`AbilityKind`] usable mid-engagement. A [`Loadout`] is the small set of
//! equipped items; its combined stats + abilities flow into a
//! [`CatchEngagement`](crate::game::catch::CatchEngagement) via [`CatchStats`].
//!
//! Pure data + arithmetic, headless and unit-tested — the same derivation runs
//! client-side for Solo and inside a SpacetimeDB reducer online.

use crate::game::catch::{AbilityKind, CatchMods, CatchStats};

/// Stable string id of a gear item (mirrors `SpeciesId`'s `&'static str` style).
pub type GearId = &'static str;

/// Which loadout slot an item occupies. Keeps a build to one primary *tool*
/// (the catch verb) plus one *support* gadget (lure/trap), so equipping is a
/// real choice rather than stacking nets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GearSlot {
    /// The primary catching tool (e.g. a net).
    Tool,
    /// A support gadget (e.g. a lure or trap).
    Support,
}

/// A craftable/buyable catch item. Grants a bundle of passive [`CatchMods`] while
/// equipped and, optionally, an active ability. Because the bonus is a full
/// `CatchMods`, **any** modifiable stat is fair game — a net adds catch power, a
/// pendant could add `stamina_regen_per_tick`, a charm could add `crit_chance` or
/// shave `target_resist_mult` — without touching this struct.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GearItem {
    pub id: GearId,
    pub display_name: &'static str,
    pub slot: GearSlot,
    /// Passive stat modifiers granted while equipped.
    pub mods: CatchMods,
    /// The active ability this item grants, if any.
    pub ability: Option<AbilityKind>,
}

/// The starter catalog. Intentionally tiny for Phase 3: a baseline tool plus one
/// of each support gadget so both ability roles exist. Obtaining/crafting and a
/// wider catalog are an open tuning decision in the roadmap.
pub const STARTER_NET: GearItem = GearItem {
    id: "starter_net",
    display_name: "Starter Net",
    slot: GearSlot::Tool,
    mods: CatchMods { catch_power: 8.0, ..CatchMods::NONE },
    ability: Some(AbilityKind::Net),
};

pub const SNARE_LURE: GearItem = GearItem {
    id: "snare_lure",
    display_name: "Snare Lure",
    slot: GearSlot::Support,
    mods: CatchMods { skill_bonus: 0.15, debuff_power: 0.2, ..CatchMods::NONE },
    ability: Some(AbilityKind::Lure),
};

pub const BOX_TRAP: GearItem = GearItem {
    id: "box_trap",
    display_name: "Box Trap",
    slot: GearSlot::Support,
    mods: CatchMods { catch_power: 2.0, debuff_power: 0.5, ..CatchMods::NONE },
    ability: Some(AbilityKind::Trap),
};

/// Look up a gear item by id.
pub fn get(id: GearId) -> Option<&'static GearItem> {
    match id {
        "starter_net" => Some(&STARTER_NET),
        "snare_lure" => Some(&SNARE_LURE),
        "box_trap" => Some(&BOX_TRAP),
        _ => None,
    }
}

/// Every gear item in the catalog (for shop/loadout UIs).
pub fn all() -> [&'static GearItem; 3] {
    [&STARTER_NET, &SNARE_LURE, &BOX_TRAP]
}

/// The player's equipped catch tools. At most one item per slot — a Tool and a
/// Support — so a loadout reads as a clear build, not a stack.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loadout {
    pub tool: Option<GearId>,
    pub support: Option<GearId>,
}

impl Loadout {
    /// The default starting kit: a net in hand and a lure on the belt.
    pub fn starter() -> Self {
        Self { tool: Some(STARTER_NET.id), support: Some(SNARE_LURE.id) }
    }

    /// Equip `item` into its slot, replacing whatever was there. Returns the id
    /// that was displaced, if any.
    pub fn equip(&mut self, item: &GearItem) -> Option<GearId> {
        let slot = match item.slot {
            GearSlot::Tool => &mut self.tool,
            GearSlot::Support => &mut self.support,
        };
        std::mem::replace(slot, Some(item.id))
    }

    /// The equipped items, resolved from the catalog (skipping unknown ids).
    pub fn items(&self) -> impl Iterator<Item = &'static GearItem> {
        [self.tool, self.support]
            .into_iter()
            .flatten()
            .filter_map(get)
    }

    /// The active abilities this loadout grants, in slot order.
    pub fn abilities(&self) -> Vec<AbilityKind> {
        self.items().filter_map(|g| g.ability).collect()
    }
}

/// Passive catch bonus granted by what the zoo already owns — the second source
/// of catch power. Scales gently with breadth (distinct species owned) and
/// depth (summed Rank stages), so a fuller, higher-Rank collection makes you a
/// better catcher and the loop feeds itself. A [`CatchMods`] like any other
/// source, so a "collection milestone" buff (e.g. crit chance, target-resistance
/// reduction) just adds fields here.
pub fn collection_mods(distinct_species: usize, rank_sum: u32) -> CatchMods {
    CatchMods {
        // +0.5 catch power per distinct species, +0.25 per accumulated Rank.
        catch_power: 0.5 * distinct_species as f32 + 0.25 * rank_sum as f32,
        // Breadth sharpens skill-check payoff a touch (+1% each, capped).
        skill_bonus: (0.01 * distinct_species as f32).min(0.5),
        // Stamina pool grows with breadth + depth — the main difficulty gate, so
        // a fuller, higher-Rank collection lets you sustain catching rarer
        // animals. +15 per distinct species, +12 per accumulated Rank.
        max_stamina: 15.0 * distinct_species as f32 + 12.0 * rank_sum as f32,
        ..CatchMods::NONE
    }
}

/// Resolve the engager's total [`CatchStats`] by folding **every modifier source**
/// over the bare-handed baseline: collection bonuses, equipped gear, and any
/// `extra` mods (temporary buffs, event effects, …). This is the single entry
/// point the engagement is fed — add a source, get it folded; no call-site math.
pub fn catch_stats(
    loadout: &Loadout,
    distinct_species: usize,
    rank_sum: u32,
    extra: &[CatchMods],
) -> CatchStats {
    let mut out = CatchStats::base();
    out.apply(&collection_mods(distinct_species, rank_sum));
    for g in loadout.items() {
        out.apply(&g.mods);
    }
    for m in extra {
        out.apply(m);
    }
    out.sanitized()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_loadout_grants_net_and_lure() {
        let lo = Loadout::starter();
        let abilities = lo.abilities();
        assert!(abilities.contains(&AbilityKind::Net));
        assert!(abilities.contains(&AbilityKind::Lure));
    }

    #[test]
    fn equipping_replaces_within_slot() {
        let mut lo = Loadout::starter();
        // Swapping the support from lure to trap displaces the lure.
        let displaced = lo.equip(&BOX_TRAP);
        assert_eq!(displaced, Some(SNARE_LURE.id));
        assert_eq!(lo.support, Some(BOX_TRAP.id));
        assert!(lo.abilities().contains(&AbilityKind::Trap));
        assert!(!lo.abilities().contains(&AbilityKind::Lure));
    }

    #[test]
    fn gear_raises_catch_power_over_baseline() {
        let bare = catch_stats(&Loadout::default(), 0, 0, &[]);
        let kitted = catch_stats(&Loadout::starter(), 0, 0, &[]);
        assert_eq!(bare.catch_power, CatchStats::default().catch_power);
        assert!(kitted.catch_power > bare.catch_power, "the net adds catch power");
    }

    #[test]
    fn collection_bonus_scales_with_breadth_and_rank() {
        let small = catch_stats(&Loadout::default(), 1, 0, &[]);
        let big = catch_stats(&Loadout::default(), 40, 30, &[]);
        assert!(big.catch_power > small.catch_power);
        assert!(big.skill_bonus >= small.skill_bonus);
    }

    #[test]
    fn extra_mods_fold_in_and_compose() {
        let base = catch_stats(&Loadout::default(), 0, 0, &[]);
        let pendant = CatchMods { stamina_regen_per_tick: 20.0, ..CatchMods::NONE };
        let charm = CatchMods { crit_chance: 0.15, target_resist_mult: -0.2, ..CatchMods::NONE };
        let buffed = catch_stats(&Loadout::default(), 0, 0, &[pendant, charm]);
        assert!((buffed.stamina_regen_per_tick - base.stamina_regen_per_tick - 20.0).abs() < 1e-4);
        assert!((buffed.crit_chance - 0.15).abs() < 1e-4);
        assert!((buffed.target_resist_mult - 0.8).abs() < 1e-4, "−20% target resistance");
    }

    #[test]
    fn get_resolves_static_refs() {
        assert_eq!(get("box_trap").unwrap().id, "box_trap");
        assert!(get("nonexistent").is_none());
    }
}
