# Catching — how it works & where to tune it

The catch loop: you **target** a wild animal, then **engage** it. The target has a
**catch-resistance bar** ("catch HP") that you deplete to capture it. Depletion
comes from three sources:

1. a **stat-roll tick** — your catch power vs the target's resistance, stepped
   every `CATCH_TICK_SECS`;
2. **abilities** (Net / Lure / Trap, keys 1/2/3) — instant depletion and/or
   debuffs;
3. **skill checks** — timed `[SPACE]` windows; hitting one lands bonus depletion,
   **missing one refills part of the bar** (the lose-condition).

Each catch tick also spends **stamina** (∝ the target's bar depth); run out and
the attempt ends. All of this is **pure, deterministic logic** in
[`catch.rs`](catch.rs) — the same code runs solo on the client and inside the
SpacetimeDB reducer online.

> **Authoritative values:** the numbers in `catch.rs` are shared by solo *and*
> server. After changing them, run `spacetime publish` so the online module
> agrees (see [`SPACETIME.md`](../../../../SPACETIME.md)). The client-only knobs
> in `src/app.rs` (cooldowns, regen cadence, camera) don't need a republish.

---

## Per-animal / per-class tuning (the main knobs)

All in **`catch.rs`**. A [`CatchClass`] bundles the per-animal catch feel;
[`catch_config`] maps each species to its class; [`TargetProfile::for_species`]
resolves the final numbers from `(class, tier)`.

### `CatchClass` fields

| Field | Meaning |
|---|---|
| `resistance_base` | Bar depth at tier 0 (deeper = longer catch) |
| `resistance_per_tier` | Added bar depth per rarity tier |
| `skill_freq_base` | Skill checks per second at tier 0 |
| `skill_freq_per_tier` | Added skill-check frequency per tier |
| `miss_refill_frac` | Fraction of a missed check's reward **refilled** onto the bar (the lose-con; higher = harsher misses) |
| `skill_reward_mult` | Multiplier on how much a **landed** check depletes |

The real values are `resistance_max = resistance_base + resistance_per_tier *
tier` and `skill_check_frequency = skill_freq_base + skill_freq_per_tier * tier`,
where `tier` (1–5) comes from `catch_tier(species)` (species rarity).

### Built-in presets
- **`NORMAL`** — the baseline (reproduces the original tier-only tuning).
- **`SKITTISH`** — jumpy prey: lots of checks, harsh miss penalty, big hit reward.
  High-variance, twitchy.
- **`STUBBORN`** — bruiser: deep bar, few checks. A slow grind that leans on raw
  catch power / stamina.

### Three ways to tune

| You want to change… | Edit | How |
|---|---|---|
| **One specific animal** | `catch_config` | add a `match` arm: `"king_cobra" => CatchClass::SKITTISH,` (or an inline `CatchClass { .. }`) |
| **A whole biome** | `class_for_theme` | map the `HabitatTheme` to a class |
| **An archetype everywhere** | a `CatchClass` preset | edit `NORMAL` / `SKITTISH` / `STUBBORN`, or add a new `pub const` preset |

```rust
// catch.rs — the override point. Per-species first, then a per-theme default.
pub fn catch_config(species: SpeciesId) -> CatchClass {
    match species {
        // "king_cobra" => CatchClass::SKITTISH,
        // "polar_bear" => CatchClass::STUBBORN,
        _ => class_for_theme(species::get(species).theme),
    }
}
```

Current per-theme defaults (`class_for_theme`): Forest / Farmland / Wetland →
`SKITTISH`; Arctic / Tundra / Volcanic / Ocean → `STUBBORN`; everything else →
`NORMAL`.

---

## Other catch constants (`catch.rs`)

These aren't per-class (yet) — they're global to the whole catch loop:

| Constant | Meaning | Default |
|---|---|---|
| `CATCH_TICK_SECS` | Stat-roll cadence ("attack speed") — lower = faster | `0.5` |
| `SKILL_CHECK_BASE_REWARD` | Base depletion from a landed check (before tier/class) | `18.0` |
| `SKILL_CHECK_WINDOW` | How long a check stays hittable | `1.1s` |
| `STAMINA_PER_TICK_PER_RESISTANCE` | Stamina cost per tick = this × bar depth | `0.1` |
| `CatchStats::default` | Bare-handed engager stats: `catch_power`, `skill_bonus`, `debuff_power`, `max_stamina` | `14 / 1 / 1 / 200` |
| `CatchEngagement::use_ability` | Net / Lure / Trap magnitudes (per-arm constants) | — |
| `roll_next_check` | Skill-check cadence jitter | `±40%` |

Player-side catch power is summed in [`gear.rs`](gear.rs) from the equipped
loadout + owned-collection bonuses (there's no XP/skill tree by design).

---

## Player stats & modifiers (items / gear / buffs)

The engager's stats are a **`CatchStats`** (`catch.rs`) — the *resolved* numbers an
engagement consumes. Every source contributes **`CatchMods`** (additive deltas),
and [`gear::catch_stats`] folds them all over `CatchStats::base()`:

```
CatchStats::base()
  + collection_mods(distinct, rank_sum)   // owned-collection bonus
  + each equipped GearItem.mods           // gear
  + each extra CatchMods                  // temporary buffs / event effects
  → .sanitized()                          // clamp to sane ranges
```

### Modifiable stats (`CatchStats` / `CatchMods` fields)

| Stat | Effect | Identity |
|---|---|---|
| `catch_power` | Baseline depletion/sec | — |
| `skill_bonus` | Landed-skill-check payoff × | — |
| `debuff_power` | Ability debuff strength × | — |
| `max_stamina` | Stamina pool | — |
| `stamina_regen_per_tick` | Out-of-combat regen per tick | `10` |
| `target_resist_mult` | **Target** resistance × (`<1` = easier) | `1.0` |
| `crit_chance` | Chance a stat tick crits `[0,1]` | `0.0` |
| `crit_mult` | Crit damage × | `1.5` |

In `CatchMods` the multiplicative-style stats are **signed deltas from identity**:
`target_resist_mult: -0.2` = −20 % target resistance; `crit_chance: 0.1` = +10 %.

### Adding the kind of effects you described

- **Pendant that boosts stamina regen** → a `GearItem` whose `mods` has
  `stamina_regen_per_tick: 5.0`.
- **Collection-milestone buff that softens targets** → add `target_resist_mult:
  -0.15` to `collection_mods` (or push a `CatchMods` into the active-buffs list).
- **Crit-on-capture effect** → any source with `crit_chance` / `crit_mult`. Crits
  are surfaced to the floating "damage" numbers (`CatchEngagement.last_hit_crit`).

Active (non-gear) buffs are folded via the `extra: &[CatchMods]` argument to
`catch_stats`; the client passes `GameApp.catch_buffs` there — **that `Vec` is the
hook to push temporary/event/collection buffs into.**

### Adding a brand-new modifiable stat

1. add the field to `CatchStats` **and** `CatchMods`,
2. fold it in `CatchStats::apply` (and clamp in `sanitized` if needed),
3. read it where it applies (`CatchEngagement::tick`, or the client for
   client-only stats like regen).

---

## Client-only feel knobs (`src/app.rs`)

These live in the binary crate (rendering/input), not the headless core, so they
**don't** need a module republish:

| Constant | Meaning | Default |
|---|---|---|
| `ABILITY_COOLDOWNS` | Per-slot cooldowns `[Net, Lure, Trap]` (seconds) | `[6, 8, 10]` |
| `STAMINA_REGEN_PER_TICK` | Flat stamina granted each regen tick (out of combat) | `50` |
| `STAMINA_REGEN_TICK_SECS` | Seconds between regen ticks | `1.25` |
| `ENGAGE_ZOOM_MULT` | Camera zoom-in factor while engaging | `1.6` |
| `CATCH_NUMBER_LIFE` | Lifetime of floating "damage" numbers | `0.85s` |
| `CATCH_TICK_SFX` | SFX id played per damage number (`assets/sfx/<id>.wav`) | `"catch_tick"` |

Abilities only fire while a target is engaged; using one starts its cooldown
(shown as the radial sweep on the skill slots).
