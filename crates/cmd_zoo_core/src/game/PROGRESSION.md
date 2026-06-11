# Power Score & region access

Instead of XP/levels, progression is a **Power Score** derived from a player's
*owned animals*. It gates which regions/expeditions they can enter, so a fresh
player can't walk into an end-game region. **Gear is separate** — it boosts catch
performance (`CatchStats` via `gear.rs`) but does **not** feed Power Score.

All of this lives in **[`power.rs`](power.rs)** (pure core, shared by client +
the SpacetimeDB module).

## The formula (tune in `power.rs`)

Per owned animal:
```
base = PER_ANIMAL                       // flat breadth reward
     + TIER_POWER[catch_tier(species)]  // rarity, NON-LINEAR (spikes at tier 4→5)
     + PER_LEVEL * (level - 1)
power = base * rank_multiplier(stage)   // rank scales the whole animal
        * (EXOTIC_MULT if exotic)       // super-rares spike further
        * (HYBRID_MULT if hybrid)
```
`power_score(zoo)` sums this over `zoo.animals`. It's **balanced** (breadth, level,
rank all matter) but **top-tier rares dominate**. Consts: `PER_ANIMAL`,
`PER_LEVEL`, `TIER_POWER`, `EXOTIC_MULT`, `HYBRID_MULT`.

## Region requirements (tune in `power.rs`)

`required_power(theme)` maps each `HabitatTheme` to a minimum Power Score
(starters = 0 so new players always have somewhere to go, up to Void = 3500).
`can_enter` / `gate_error` are the check + the user-facing message.

## Where it's enforced & shown

- **Server (authoritative):** `enter_expedition` reducer rejects under-powered
  players (`cmd_zoo_stdb/src/lib.rs`).
- **Client:** `GameApp::power_score()`; the solo/UX gate in `launch_expedition`;
  the **expedition board** disables locked regions and shows their requirement;
  a **HUD chip** (icon id `"power"` — drop `assets/icons/power.png`, falls back to
  a coloured circle).
- **Visible to others:** denormalized `ZooRow.power_score` column (recomputed on
  every snapshot write) drives the **party panel** and **avatar nameplates**.

## Deploy note

`power.rs` is authoritative shared core. After changing the formula/requirements
or the server gate, rebuild + `spacetime publish` so the module agrees (the
`power_score` column addition is additive — normal publish, no `-c`). Client-only
UI (HUD/board) needs no republish.
