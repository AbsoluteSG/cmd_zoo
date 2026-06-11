//! Target-based catching (Phase 3) — the new core verb that replaces the old
//! hover-to-fill mechanic.
//!
//! The loop, MMORPG-style: the player clicks a wild animal to *target* it, then
//! *engages* (catches). The target carries a **catch-resistance bar** ("catch
//! health"); depleting it captures the animal. Depletion comes from three
//! sources, exactly as the roadmap specifies:
//!
//! 1. a **stat-driven roll loop** ticking the bar down over time (your catch
//!    power vs the target's resistance);
//! 2. **input abilities** (from equipped gear) that deplete the bar faster
//!    and/or **debuff** the target (weaken its resistance, prevent it fleeing);
//! 3. **random skill-check moments** (DBD-style timed inputs) whose frequency
//!    scales with the animal's **tier** — hitting them lands bonus depletion.
//!
//! This module is **pure logic over a seeded [`LcgRng`]** — no rendering, no
//! input polling, no wall-clock. The client (and, later, a SpacetimeDB reducer)
//! drives it by feeding `dt` and discrete events, so the very same engagement
//! can run client-side for Solo and server-authoritative online. Per-engager
//! `contribution` is tracked here as the hook the Phase 5 co-op/lock-battle
//! model builds on.

use crate::game::biome::LcgRng;
use crate::game::species::{self, SpeciesId};

/// Catch tier 1–5 — the difficulty/scarcity band of a target. Reuses the same
/// `purchase_cost`-derived rarity proxy as [`species::captures_required`] so the
/// catch difficulty and the codex rarity stay in step. A higher tier means a
/// deeper resistance bar *and* more frequent skill checks.
pub fn catch_tier(species: SpeciesId) -> u8 {
    species::captures_required(species).clamp(1, 5) as u8
}

/// ════════════════════════════════════════════════════════════════════════════
/// CATCH TUNING — per-class config + per-species overrides.
///
/// **This is the one place to tune how hard each animal is to catch.** A
/// [`CatchClass`] bundles the catch knobs; [`catch_config`] maps a species to its
/// class (per-species overrides first, then a per-class default). [`TargetProfile`]
/// resolves the final numbers from `(class, tier)`.
///
/// To tune one animal: add an arm to [`catch_config`]'s override `match`.
/// To tune a whole archetype: edit a [`CatchClass`] preset (or add a new one).
/// ════════════════════════════════════════════════════════════════════════════
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CatchClass {
    /// Resistance-bar depth at tier 0; the real bar is `base + per_tier * tier`.
    pub resistance_base: f32,
    pub resistance_per_tier: f32,
    /// Skill checks per second: `freq_base + freq_per_tier * tier`.
    pub skill_freq_base: f32,
    pub skill_freq_per_tier: f32,
    /// Fraction of a missed skill-check's reward refilled onto the bar (the
    /// lose-condition). Higher = punishing misses harder.
    pub miss_refill_frac: f32,
    /// Multiplier on how much a *landed* skill check depletes (on top of tier
    /// scaling). >1 rewards twitch play; <1 makes checks matter less.
    pub skill_reward_mult: f32,
}

impl CatchClass {
    /// Baseline archetype — reproduces the original tier-only tuning.
    pub const NORMAL: Self = Self {
        resistance_base: 60.0,
        resistance_per_tier: 40.0,
        skill_freq_base: 0.15,
        skill_freq_per_tier: 0.12,
        miss_refill_frac: 0.75,
        skill_reward_mult: 1.0,
    };
    /// Jumpy prey: lots of skill checks, harsh miss penalty, but each hit helps a
    /// lot. A twitchy, high-variance catch.
    pub const SKITTISH: Self = Self {
        resistance_base: 50.0,
        resistance_per_tier: 30.0,
        skill_freq_base: 0.30,
        skill_freq_per_tier: 0.20,
        miss_refill_frac: 1.0,
        skill_reward_mult: 1.3,
    };
    /// Stubborn bruiser: a deep bar and few skill checks — a slow grind that
    /// leans on raw catch power/stamina rather than reflexes.
    pub const STUBBORN: Self = Self {
        resistance_base: 90.0,
        resistance_per_tier: 70.0,
        skill_freq_base: 0.10,
        skill_freq_per_tier: 0.06,
        miss_refill_frac: 0.5,
        skill_reward_mult: 0.8,
    };
}

/// The catch class for a species. **Edit this to tune catching.** Per-species
/// overrides take priority; otherwise a per-habitat-theme default is used (and
/// most themes currently fall back to [`CatchClass::NORMAL`]).
pub fn catch_config(species: SpeciesId) -> CatchClass {
    // 1) Per-species overrides — add specific animals here.
    match species {
        // e.g. "king_cobra" => CatchClass::SKITTISH,
        // e.g. "polar_bear" => CatchClass::STUBBORN,
        _ => class_for_theme(species::get(species).theme),
    }
}

/// Per-class default by habitat theme. **Edit this to tune a whole biome's feel.**
fn class_for_theme(theme: species::HabitatTheme) -> CatchClass {
    use species::HabitatTheme::*;
    match theme {
        // Skittish biomes (small, jumpy fauna).
        Forest | Farmland | Wetland => CatchClass::SKITTISH,
        // Stubborn biomes (big, hardy fauna).
        Arctic | Tundra | Volcanic | Ocean => CatchClass::STUBBORN,
        // Everything else uses the baseline.
        _ => CatchClass::NORMAL,
    }
}

/// The catch-relevant profile of a target, derived from its species (or built
/// directly in tests). Everything the engagement needs to know about *what is
/// being caught*; the engager's side lives in [`CatchStats`].
#[derive(Clone, Debug, PartialEq)]
pub struct TargetProfile {
    pub species: SpeciesId,
    pub tier: u8,
    /// Full depth of the catch-resistance bar ("catch health").
    pub resistance_max: f32,
    /// Expected skill-check moments per second while engaged. Scales with tier.
    pub skill_check_frequency: f32,
    /// The resolved catch class (carries miss-refill + skill-reward tuning the
    /// engagement reads each tick).
    pub class: CatchClass,
}

impl TargetProfile {
    /// Derive a target's catch profile from its species, via its [`catch_config`]
    /// class scaled by tier.
    pub fn for_species(species: SpeciesId) -> Self {
        let tier = catch_tier(species);
        let class = catch_config(species);
        Self {
            species,
            tier,
            resistance_max: class.resistance_base + class.resistance_per_tier * tier as f32,
            skill_check_frequency: class.skill_freq_base + class.skill_freq_per_tier * tier as f32,
            class,
        }
    }
}

/// The engager's **resolved** catch stats — the final numbers an engagement
/// consumes, after folding the bare-handed baseline with every modifier source
/// (gear, owned-collection bonuses, temporary buffs). Build these with
/// [`crate::game::gear::catch_stats`]; never mutate fields ad-hoc — add a
/// [`CatchMods`] source instead so the system stays composable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CatchStats {
    /// Baseline resistance depleted per second by the stat-roll loop.
    pub catch_power: f32,
    /// Multiplier on the bonus depletion from a landed skill check.
    pub skill_bonus: f32,
    /// Multiplier on how strongly abilities debuff the target.
    pub debuff_power: f32,
    /// Maximum catch **stamina**. Each catch tick spends stamina in proportion
    /// to the target's resistance, so a bigger pool (earned through progression)
    /// is what lets a player sustain catching deeper-bar, rarer animals — the
    /// difficulty-scaling gate. Drained on the engager's side, not here.
    pub max_stamina: f32,
    /// Flat stamina restored per regen tick (out of combat). Identity-additive:
    /// items/buffs add to this.
    pub stamina_regen_per_tick: f32,
    /// Multiplier on the *target's* effective catch-resistance. `1.0` = normal;
    /// **below 1.0 means the target is easier** (a "reduce enemy resistance"
    /// buff). Folded from `1.0` by [`CatchMods::target_resist_mult`] deltas.
    pub target_resist_mult: f32,
    /// Chance `[0,1]` that a stat-roll depletion tick is a **critical hit**.
    pub crit_chance: f32,
    /// Damage multiplier applied to a critical depletion tick (`>= 1.0`).
    pub crit_mult: f32,
}

impl CatchStats {
    /// The bare-handed baseline (no gear, empty collection). Multiplicative-style
    /// stats sit at their identity here (`target_resist_mult = 1`, `crit_mult`
    /// the crit payoff, `crit_chance = 0`); [`CatchMods`] deltas move them.
    pub const fn base() -> Self {
        Self {
            catch_power: 5.0,
            skill_bonus: 1.0,
            debuff_power: 1.0,
            max_stamina: 200.0,
            stamina_regen_per_tick: 10.0,
            target_resist_mult: 1.0,
            crit_chance: 0.0,
            crit_mult: 1.5,
        }
    }

    /// Fold one modifier source onto these stats (field-wise add of its deltas).
    pub fn apply(&mut self, m: &CatchMods) {
        self.catch_power += m.catch_power;
        self.skill_bonus += m.skill_bonus;
        self.debuff_power += m.debuff_power;
        self.max_stamina += m.max_stamina;
        self.stamina_regen_per_tick += m.stamina_regen_per_tick;
        self.target_resist_mult += m.target_resist_mult;
        self.crit_chance += m.crit_chance;
        self.crit_mult += m.crit_mult;
    }

    /// Clamp resolved stats to sane ranges after all sources are folded.
    pub fn sanitized(mut self) -> Self {
        self.max_stamina = self.max_stamina.max(1.0);
        self.stamina_regen_per_tick = self.stamina_regen_per_tick.max(0.0);
        self.target_resist_mult = self.target_resist_mult.max(0.1); // never free / div-0
        self.crit_chance = self.crit_chance.clamp(0.0, 1.0);
        self.crit_mult = self.crit_mult.max(1.0);
        self
    }
}

impl Default for CatchStats {
    /// Bare-handed baseline — see [`CatchStats::base`].
    fn default() -> Self {
        Self::base()
    }
}

/// **Additive deltas** to [`CatchStats`] contributed by one modifier source — a
/// gear item, a collection bonus, or a temporary buff. Everything is a delta over
/// the baseline, so sources compose by simple summation (see [`CatchStats::apply`]).
///
/// For the multiplicative-style stats the delta is signed change from identity:
/// e.g. `target_resist_mult: -0.2` means *"−20% target resistance"*, and
/// `crit_chance: 0.1` means *"+10% crit"*. Add new modifiable stats by adding a
/// field here, to [`CatchStats`], and to [`CatchStats::apply`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CatchMods {
    pub catch_power: f32,
    pub skill_bonus: f32,
    pub debuff_power: f32,
    pub max_stamina: f32,
    pub stamina_regen_per_tick: f32,
    /// Signed delta to `target_resist_mult` (negative = target resistance reduced).
    pub target_resist_mult: f32,
    /// Added crit chance `[0,1]`.
    pub crit_chance: f32,
    /// Added crit multiplier.
    pub crit_mult: f32,
}

impl CatchMods {
    /// A no-op modifier (all zero) — handy as a `..` base for partial literals.
    pub const NONE: Self = Self {
        catch_power: 0.0,
        skill_bonus: 0.0,
        debuff_power: 0.0,
        max_stamina: 0.0,
        stamina_regen_per_tick: 0.0,
        target_resist_mult: 0.0,
        crit_chance: 0.0,
        crit_mult: 0.0,
    };
}

/// Live debuffs an engagement has stacked on its target via abilities. All
/// decay over time; the engagement applies their effects each tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Debuffs {
    /// Fraction (0..1) added to effective depletion — a "softened" target.
    /// Decays linearly back to 0 over its remaining duration.
    pub weaken: f32,
    /// Seconds remaining during which the target cannot flee (lure/tether).
    pub flee_lock_secs: f32,
    /// Seconds remaining on the current `weaken` stack.
    pub weaken_secs: f32,
}

/// A DBD-style timed skill-check window. Surfaced on the engagement for the
/// client to render; the player calls [`CatchEngagement::hit_skill_check`]
/// while it is live to claim the bonus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillCheck {
    /// Seconds left to hit before the window lapses.
    pub remaining: f32,
    /// Total window length (for rendering a shrinking gauge).
    pub window: f32,
    /// Resistance depleted if hit (already tier-scaled; `skill_bonus` applies
    /// on top at hit time).
    pub reward: f32,
}

/// An equippable ability the player triggers mid-engagement. Sourced from gear;
/// the engagement resolves each into instant depletion and/or a debuff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbilityKind {
    /// A net: a big burst of instant catch-resistance depletion.
    Net,
    /// A lure/tether: locks the target in place so it can't flee (a support
    /// role in a group); minor depletion.
    Lure,
    /// A trap: weakens the target's resistance over time (a depletion debuff).
    Trap,
}

/// How an engagement resolved this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngagementOutcome {
    /// Still in progress.
    Ongoing,
    /// The bar emptied — the animal is captured.
    Captured,
    /// The target fled (reserved for the flee model; not yet emitted by `tick`).
    Fled,
    /// The engager ran out of stamina mid-catch — the attempt ends without a
    /// capture (the animal is left in the world to try again later).
    Exhausted,
}

/// A single in-flight catch engagement against one target. Owns the resistance
/// bar, active debuffs, the current skill-check window, and its own RNG stream
/// so the whole thing is deterministic and reproducible from `(target, seed)`.
#[derive(Clone, Debug)]
pub struct CatchEngagement {
    pub target: TargetProfile,
    /// Current catch-resistance remaining; capture fires at `<= 0`.
    pub resistance: f32,
    pub debuffs: Debuffs,
    /// The live skill-check window, if one is currently up.
    pub skill_check: Option<SkillCheck>,
    /// Total resistance this engager has depleted — the per-engager
    /// **contribution** the Phase 5 co-op/ownership model keys on.
    pub contribution: f32,
    /// Whether the most recent stat-roll depletion was a **critical hit** (set
    /// each tick) — surfaced so the UI can punch up the damage number.
    pub last_hit_crit: bool,
    /// Seconds until the next skill-check window may spawn.
    next_check_in: f32,
    /// Accumulated time toward the next discrete depletion tick.
    tick_accum: f32,
    rng: LcgRng,
}

/// Cadence of the stat-roll depletion. The bar steps down once per tick rather
/// than draining continuously, so progress reads as increments — and it mirrors
/// an authoritative server tick when this runs inside a SpacetimeDB reducer.
pub const CATCH_TICK_SECS: f32 = 1.25;

/// Stamina spent **per catch tick, per point of the target's `resistance_max`**.
/// Cost-per-tick = this × resistance, so rarer/deeper-bar animals burn the pool
/// faster: a player whose `max_stamina` can't cover the whole catch is exhausted
/// before the bar empties. Tuned with [`CatchStats::default`]'s pool so a fresh
/// player handles low tiers and must progress to sustain the high ones.
pub const STAMINA_PER_TICK_PER_RESISTANCE: f32 = 0.1;

/// Base resistance a landed skill check removes, before tier, `skill_bonus`, and
/// the per-class `skill_reward_mult`. Tuned so a well-timed check is a meaningful
/// chunk of a low-tier bar.
const SKILL_CHECK_BASE_REWARD: f32 = 18.0;
/// How long a skill-check window stays hittable.
const SKILL_CHECK_WINDOW: f32 = 1.1;

impl CatchEngagement {
    /// Begin engaging `target`. `seed` makes the skill-check cadence
    /// deterministic (derive it from the target's instance id online).
    pub fn new(target: TargetProfile, seed: u64) -> Self {
        let resistance = target.resistance_max;
        let mut rng = LcgRng::new(seed);
        let next_check_in = Self::roll_next_check(&target, &mut rng);
        Self {
            target,
            resistance,
            debuffs: Debuffs::default(),
            skill_check: None,
            contribution: 0.0,
            last_hit_crit: false,
            next_check_in,
            tick_accum: 0.0,
            rng,
        }
    }

    /// Fraction of the bar already depleted, 0..1 — for rendering the bar.
    pub fn progress(&self) -> f32 {
        if self.target.resistance_max <= 0.0 {
            return 1.0;
        }
        (1.0 - self.resistance / self.target.resistance_max).clamp(0.0, 1.0)
    }

    /// Advance the engagement by `dt` seconds under the engager's `stats`,
    /// spending from the engager's `stamina` pool. Runs the stepped stat-roll
    /// depletion, decays debuffs, and spawns/expires skill-check windows. Each
    /// catch tick costs stamina proportional to the target's resistance; if the
    /// pool can't cover a tick the attempt ends [`EngagementOutcome::Exhausted`].
    pub fn tick(&mut self, dt: f32, stats: &CatchStats, stamina: &mut f32) -> EngagementOutcome {
        if dt <= 0.0 {
            return self.outcome();
        }
        self.decay_debuffs(dt);

        // Stat-roll loop, stepped: accumulate time and deplete one increment per
        // discrete tick, so the bar drops in chunks rather than draining smooth.
        // Each tick also spends stamina ∝ the target's resistance.
        //
        // Modifiers fold in here: `target_resist_mult` below 1 makes every hit
        // land harder (the target's effective resistance is reduced), and
        // `crit_chance`/`crit_mult` roll a bigger hit. All sourced from `stats`
        // (gear/collection/buffs) so new items just change the numbers.
        let resist_factor = 1.0 / stats.target_resist_mult.max(0.1);
        let per_tick = stats.catch_power * (1.0 + self.debuffs.weaken) * resist_factor * CATCH_TICK_SECS;
        let stamina_cost = STAMINA_PER_TICK_PER_RESISTANCE * self.target.resistance_max;
        self.tick_accum += dt;
        while self.tick_accum >= CATCH_TICK_SECS {
            self.tick_accum -= CATCH_TICK_SECS;
            if *stamina < stamina_cost {
                return EngagementOutcome::Exhausted;
            }
            *stamina -= stamina_cost;
            // Critical hit roll on this depletion tick.
            self.last_hit_crit = stats.crit_chance > 0.0 && self.rng.next_f32() < stats.crit_chance;
            let amount = if self.last_hit_crit { per_tick * stats.crit_mult } else { per_tick };
            self.deplete(amount);
            if self.is_captured() {
                break;
            }
        }

        // Skill-check lifecycle: expire a live window, or count down to the next.
        if let Some(sc) = &mut self.skill_check {
            sc.remaining -= dt;
            if sc.remaining <= 0.0 {
                // Missed it — the target claws back part of the bar (lose-con).
                let refill = sc.reward * self.target.class.miss_refill_frac;
                self.skill_check = None;
                self.refill(refill);
                self.next_check_in = Self::roll_next_check(&self.target, &mut self.rng);
            }
        } else {
            self.next_check_in -= dt;
            if self.next_check_in <= 0.0 {
                self.spawn_skill_check();
            }
        }
        self.outcome()
    }

    /// Trigger an equipped ability. Resolves to instant depletion and/or a
    /// debuff, scaled by the engager's stats. Safe to call any time.
    pub fn use_ability(&mut self, ability: AbilityKind, stats: &CatchStats) {
        match ability {
            AbilityKind::Net => {
                // A burst proportional to baseline power — the bread-and-butter
                // active. Scales with tier so it stays relevant on deep bars.
                self.deplete(stats.catch_power * 2.5 + 8.0 * self.target.tier as f32);
            }
            AbilityKind::Lure => {
                self.deplete(stats.catch_power * 0.5);
                self.debuffs.flee_lock_secs = self.debuffs.flee_lock_secs.max(4.0);
            }
            AbilityKind::Trap => {
                // Stack a weaken debuff that speeds all depletion for a while.
                self.debuffs.weaken = (self.debuffs.weaken + 0.4 * stats.debuff_power).min(1.5);
                self.debuffs.weaken_secs = self.debuffs.weaken_secs.max(5.0);
            }
        }
    }

    /// The player hit the live skill check. Applies its (skill-bonus-scaled)
    /// reward and clears the window. Returns `true` if a window was live.
    pub fn hit_skill_check(&mut self, stats: &CatchStats) -> bool {
        let Some(sc) = self.skill_check.take() else {
            return false;
        };
        self.deplete(sc.reward * stats.skill_bonus);
        // The next window cadence resumes from here.
        self.next_check_in = Self::roll_next_check(&self.target, &mut self.rng);
        true
    }

    /// The player let the skill check lapse (or deliberately skipped it). Refills
    /// part of the bar (the miss penalty) and resumes the cadence.
    pub fn miss_skill_check(&mut self) {
        if let Some(sc) = self.skill_check.take() {
            self.refill(sc.reward * self.target.class.miss_refill_frac);
            self.next_check_in = Self::roll_next_check(&self.target, &mut self.rng);
        }
    }

    /// True once the catch-resistance bar is empty.
    pub fn is_captured(&self) -> bool {
        self.resistance <= 0.0
    }

    // ── internals ─────────────────────────────────────────────────────────────

    fn outcome(&self) -> EngagementOutcome {
        if self.is_captured() {
            EngagementOutcome::Captured
        } else {
            EngagementOutcome::Ongoing
        }
    }

    /// Remove `amount` from the bar (clamped at 0) and bank it as contribution.
    fn deplete(&mut self, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        let applied = amount.min(self.resistance.max(0.0));
        self.resistance -= amount;
        self.contribution += applied;
    }

    /// Add `amount` back onto the bar (the skill-check miss penalty), clamped to
    /// the target's full resistance. Walks back banked contribution by the same
    /// amount so it still reflects *net* progress.
    fn refill(&mut self, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        let headroom = (self.target.resistance_max - self.resistance).max(0.0);
        let applied = amount.min(headroom);
        self.resistance += applied;
        self.contribution = (self.contribution - applied).max(0.0);
    }

    fn decay_debuffs(&mut self, dt: f32) {
        if self.debuffs.flee_lock_secs > 0.0 {
            self.debuffs.flee_lock_secs = (self.debuffs.flee_lock_secs - dt).max(0.0);
        }
        if self.debuffs.weaken_secs > 0.0 {
            self.debuffs.weaken_secs = (self.debuffs.weaken_secs - dt).max(0.0);
            if self.debuffs.weaken_secs == 0.0 {
                self.debuffs.weaken = 0.0;
            }
        }
    }

    fn spawn_skill_check(&mut self) {
        let reward = SKILL_CHECK_BASE_REWARD
            * (0.6 + 0.2 * self.target.tier as f32)
            * self.target.class.skill_reward_mult;
        self.skill_check = Some(SkillCheck {
            remaining: SKILL_CHECK_WINDOW,
            window: SKILL_CHECK_WINDOW,
            reward,
        });
    }

    /// Roll the gap until the next skill-check window from the target's
    /// frequency, with ±40% jitter so the cadence never feels metronomic.
    fn roll_next_check(target: &TargetProfile, rng: &mut LcgRng) -> f32 {
        let mean = 1.0 / target.skill_check_frequency.max(0.05);
        let jitter = 0.6 + 0.8 * rng.next_f32(); // 0.6..1.4
        (mean * jitter).max(0.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(tier_resistance: f32) -> TargetProfile {
        TargetProfile {
            species: "field_mouse",
            tier: 1,
            resistance_max: tier_resistance,
            skill_check_frequency: 0.5,
            class: CatchClass::NORMAL,
        }
    }

    #[test]
    fn stat_roll_eventually_captures() {
        let mut e = CatchEngagement::new(profile(100.0), 1);
        let stats = CatchStats { catch_power: 50.0, ..Default::default() };
        // 50/s vs 100 → captured after ~2s. Step in small ticks.
        let mut outcome = EngagementOutcome::Ongoing;
        for _ in 0..240 {
            outcome = e.tick(1.0 / 60.0, &stats, &mut 1.0e9_f32);
            if outcome == EngagementOutcome::Captured {
                break;
            }
        }
        assert_eq!(outcome, EngagementOutcome::Captured);
        assert!(e.is_captured());
        // Contribution never exceeds the bar depth (clamped at the kill).
        assert!(e.contribution <= 100.0 + f32::EPSILON);
    }

    #[test]
    fn depletion_is_stepped_not_continuous() {
        let mut e = CatchEngagement::new(profile(1000.0), 11);
        let stats = CatchStats { catch_power: 20.0, ..Default::default() };
        // A small sub-tick step shouldn't move the bar yet.
        e.tick(CATCH_TICK_SECS * 0.4, &stats, &mut 1.0e9_f32);
        assert_eq!(e.resistance, 1000.0, "no depletion within a tick");
        // Crossing the tick boundary applies exactly one increment.
        e.tick(CATCH_TICK_SECS * 0.7, &stats, &mut 1.0e9_f32);
        assert!((e.resistance - (1000.0 - 20.0 * CATCH_TICK_SECS)).abs() < 0.01);
    }

    #[test]
    fn net_ability_bursts_the_bar() {
        let mut e = CatchEngagement::new(profile(100.0), 7);
        let before = e.resistance;
        e.use_ability(AbilityKind::Net, &CatchStats::default());
        assert!(e.resistance < before, "net should deplete the bar");
        assert!(e.contribution > 0.0);
    }

    #[test]
    fn trap_weaken_speeds_depletion() {
        let stats = CatchStats::default();
        // Two identical engagements; one gets a Trap weaken first.
        let mut plain = CatchEngagement::new(profile(500.0), 3);
        let mut trapped = CatchEngagement::new(profile(500.0), 3);
        trapped.use_ability(AbilityKind::Trap, &stats);
        for _ in 0..30 {
            plain.tick(0.1, &stats, &mut 1.0e9_f32);
            trapped.tick(0.1, &stats, &mut 1.0e9_f32);
        }
        assert!(
            trapped.resistance < plain.resistance,
            "weakened target should have less resistance left"
        );
    }

    #[test]
    fn skill_check_spawns_and_hitting_it_helps() {
        let mut e = CatchEngagement::new(profile(1000.0), 42);
        let stats = CatchStats::default();
        // Run until a window appears.
        let mut spawned = false;
        for _ in 0..600 {
            e.tick(0.05, &stats, &mut 1.0e9_f32);
            if e.skill_check.is_some() {
                spawned = true;
                break;
            }
        }
        assert!(spawned, "a skill check should spawn within ~30s");
        let before = e.resistance;
        assert!(e.hit_skill_check(&stats));
        assert!(e.resistance < before, "hitting the check should deplete the bar");
        assert!(e.skill_check.is_none(), "window clears after a hit");
    }

    #[test]
    fn missing_a_skill_check_refills_the_bar() {
        let mut e = CatchEngagement::new(profile(1000.0), 42);
        let stats = CatchStats::default();
        // Run until a window appears, then deplete a bit so there's headroom.
        for _ in 0..600 {
            e.tick(0.05, &stats, &mut 1.0e9_f32);
            if e.skill_check.is_some() {
                break;
            }
        }
        assert!(e.skill_check.is_some(), "a skill check should spawn");
        e.deplete(40.0); // open up headroom so a refill is observable
        let before = e.resistance;
        e.miss_skill_check();
        assert!(e.resistance > before, "missing a check claws the bar back up");
        assert!(e.resistance <= e.target.resistance_max, "refill clamps at full");
    }

    #[test]
    fn lure_locks_flee() {
        let mut e = CatchEngagement::new(profile(100.0), 5);
        e.use_ability(AbilityKind::Lure, &CatchStats::default());
        assert!(e.debuffs.flee_lock_secs > 0.0);
        e.tick(1.0, &CatchStats::default(), &mut 1.0e9_f32);
        assert!(e.debuffs.flee_lock_secs > 0.0, "lock persists a few seconds");
    }

    #[test]
    fn deterministic_from_seed() {
        let run = || {
            let mut e = CatchEngagement::new(TargetProfile::for_species("penguin"), 99);
            let stats = CatchStats::default();
            let mut checks = 0;
            for _ in 0..400 {
                e.tick(0.1, &stats, &mut 1.0e9_f32);
                if e.skill_check.is_some() {
                    checks += 1;
                    e.miss_skill_check();
                }
            }
            (e.resistance, checks)
        };
        assert_eq!(run(), run(), "same seed → same engagement evolution");
    }

    #[test]
    fn tier_scales_resistance() {
        // A cheap common (tier 1) is shallower than a premium (tier 5).
        let mouse = TargetProfile::for_species("field_mouse");
        assert_eq!(mouse.tier, 1);
        // Build a synthetic tier-5 to compare the derivation monotonicity.
        let t5 = TargetProfile { tier: 5, resistance_max: 60.0 + 40.0 * 5.0, ..mouse.clone() };
        assert!(t5.resistance_max > mouse.resistance_max);
    }

    #[test]
    fn running_out_of_stamina_exhausts_the_attempt() {
        // Deep bar (1000) → 100 stamina per tick; a 30-pool can't cover one tick.
        let mut e = CatchEngagement::new(profile(1000.0), 1);
        let stats = CatchStats { catch_power: 20.0, ..Default::default() };
        let mut stamina = 30.0;
        let outcome = e.tick(CATCH_TICK_SECS, &stats, &mut stamina);
        assert_eq!(outcome, EngagementOutcome::Exhausted);
        assert!(!e.is_captured(), "no capture on exhaustion");
        assert_eq!(stamina, 30.0, "an unaffordable tick spends nothing");
    }

    #[test]
    fn stamina_is_spent_proportionally_and_a_big_pool_captures() {
        let mut e = CatchEngagement::new(profile(100.0), 2);
        let stats = CatchStats { catch_power: 50.0, ..Default::default() };
        let mut stamina = 10_000.0;
        let mut outcome = EngagementOutcome::Ongoing;
        for _ in 0..240 {
            outcome = e.tick(1.0 / 60.0, &stats, &mut stamina);
            if outcome == EngagementOutcome::Captured {
                break;
            }
        }
        assert_eq!(outcome, EngagementOutcome::Captured);
        assert!(stamina < 10_000.0, "stamina was spent during the catch");
    }
}
