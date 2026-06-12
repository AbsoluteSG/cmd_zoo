//! Dedicated skill system: the slottable **active skills** a player fires mid-
//! catch (the `1`/`2`/`3` hotkeys today). Skills are decoupled from hotbar slots
//! — a [`Loadout`] maps each slot to a [`Skill`], so players can re-bind freely
//! later without touching the effect logic.
//!
//! Each [`Skill`] resolves (via [`Skill::def`]) to a [`SkillDef`] carrying its
//! display name, slot icon, cooldown, and a data-only [`SkillEffect`]. The catch
//! engagement applies the effect in [`crate::game::catch::CatchEngagement::use_skill`],
//! so adding/tuning a skill is a one-stop edit here.

/// One slottable active skill. Add a variant + a [`Skill::def`] arm to introduce
/// a new skill; everything else (cooldown UI, hotkeys, effect application) reads
/// through the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Skill {
    /// Instant flat damage, no channel.
    Net,
    /// Suppresses the target's resistance regen for a few seconds.
    Lure,
    /// A short damage-over-time that procs each catch tick.
    Trap,
}

/// The data-only effect a skill applies to a live engagement. Amounts that scale
/// with the target's `tier` keep skills relevant on deep, high-tier bars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkillEffect {
    /// Instant, un-channelled depletion of `base + per_tier * tier`, then scaled
    /// by the engager's `debuff_power` so gear matters.
    FlatDamage { base: f32, per_tier: f32 },
    /// Reduce the target's per-tick resistance regen by `frac` (0..1) for `secs`.
    ReduceRegen { frac: f32, secs: f32 },
    /// Damage-over-time: deplete `base + per_tier * tier` (× `debuff_power`) on
    /// each catch tick for `secs` seconds.
    Dot { base: f32, per_tier: f32, secs: f32 },
}

/// The static definition of a skill: how it presents and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillDef {
    pub skill: Skill,
    /// Short display name (nameplates / tooltips).
    pub name: &'static str,
    /// Slot-icon sprite stem, looked up in `assets/ui/` (no `.png`). **Assign a
    /// skill's icon here.** A missing file just draws the empty slot frame.
    pub icon: &'static str,
    /// Cooldown (seconds) before the skill can be fired again.
    pub cooldown: f32,
    /// What firing the skill does to the engagement.
    pub effect: SkillEffect,
}

impl Skill {
    /// This skill's static definition (name, icon, cooldown, effect). **The one
    /// place to tune skills + assign icons.**
    pub fn def(self) -> SkillDef {
        match self {
            Skill::Net => SkillDef {
                skill: Skill::Net,
                name: "Net",
                icon: "net_skill_icon",
                cooldown: 6.75,
                effect: SkillEffect::FlatDamage { base: 5.0, per_tier: 1.0 },
            },
            Skill::Lure => SkillDef {
                skill: Skill::Lure,
                name: "Lure",
                icon: "lure_skill_icon",
                cooldown: 15.0,
                effect: SkillEffect::ReduceRegen { frac: 0.10, secs: 5.0 },
            },
            Skill::Trap => SkillDef {
                skill: Skill::Trap,
                name: "Trap",
                icon: "trap_skill_icon",
                cooldown: 10.0,
                effect: SkillEffect::Dot { base: 4.0, per_tier: 1.5, secs: 5.0 },
            },
        }
    }
}

/// The default slot loadout, indexed by hotbar slot (`0` = key `1`, …). Swap the
/// entries to re-bind, or replace with a player-chosen loadout later.
pub const DEFAULT_LOADOUT: [Skill; 3] = [Skill::Net, Skill::Lure, Skill::Trap];
