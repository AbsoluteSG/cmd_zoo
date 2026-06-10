# `game/` — the domain model

This folder is the actual game: what an animal is, how coins accrue, how
breeding and catching work, how the world is laid out. Everything here is **pure
rules** — no rendering, no file I/O, no networking. The client (`src/render`,
`src/app`) draws this state; the persistence layer saves it; a future
SpacetimeDB reducer will run it server-side. (See the
[crate README](../../README.md) for what "headless" and "reducer" mean.)

## The center of gravity: `Zoo`

`zoo.rs` defines `Zoo`, the **aggregate root** — one big struct that owns all of
a player's persistent state: their wallet (coins, food, DNA Helix), their
animals, habitats, structures, pedestals, discovered breeding recipes, world
seed, visitors, and so on. Almost every rule is a method on `Zoo` (e.g.
`buy_animal`, `start_zoo_upgrade`, `claim_completed_breeding`).

If you want to understand the game, **start in `zoo.rs`** and follow the methods
out into the smaller modules.

> Rust note: you'll see most methods take `&mut self` (they change the zoo) and
> a `now: DateTime<Utc>` parameter. The rule code never asks the OS for the time
> — the caller passes it in. This keeps the logic deterministic and lets a server
> control the clock during online play.

## How the files group together

### Catalogs (static data, looked up by string id)
- **`species.rs`** — the master list of every animal: its habitat theme, base
  income, rarity/tier, breeding eligibility, plus the big crossbreeding recipe
  table (which two parents make which hybrid). `SpeciesId` is a `&'static str`
  (a string baked into the binary), so animals refer to their species by name.
- **`structure_kind.rs`** — definitions for food-producing structures (Hay Bale,
  Insectary, …): production rate, cap, cost.
- **`rank.rs`** — per-species Rank (Regular → Silver → Gold → … → Neon). Catching
  a duplicate of something you own ranks it up instead of adding a second copy;
  thresholds grow ×3 each step.

### Entities (the things a zoo contains)
- **`animal.rs`** — `Animal` plus `AnimalState` (`Idle` or `Breeding { ends_at }`).
  Income is *derived on demand* from when it was last collected, not ticked.
- **`habitat.rs`** — themed enclosures, the isometric grid dimensions, placement
  and overlap rules, upgrade costs/durations.
- **`structure.rs`** — placed food generators (the entity; its kind is in
  `structure_kind.rs`).
- **`pedestal.rs`** — the first *moveable* zoo object: place it on any free tile,
  dedicate one animal to it, and its income auto-sweeps to your wallet.
- **`player.rs`** — the player's id and display name.
- **`visitor.rs`** — saved state for someone who has visited your zoo in co-op
  (their name, last position, gift inbox, granted permissions).

### Time & money
- **`economy.rs`** — the per-tick hook. It's intentionally a **no-op today**
  (income is computed on demand, breeding is claimed by clicking), but it's kept
  as the obvious home for any future timed system. Callers still call
  `economy::advance(...)` every frame.
- **`exotic_shop.rs`** — a time-windowed rotating shop (4h open / 45m closed)
  whose stock is *derived from the wall-clock*, so every client agrees on what's
  for sale without storing anything.

### The catching loop (Phase 3 — the current focus)
- **`catch.rs`** — the new core verb: target a wild animal, then deplete its
  "catch-resistance" bar via a stat-driven roll, gear abilities, and timed
  skill-check moments.
- **`gear.rs`** — equippable catch tools and loadouts; their stats plus
  owned-collection bonuses are your two sources of catch power (there's
  deliberately no XP/skill tree).
- **`biome_instance.rs`** — a bounded, seed-generated "expedition" map of a single
  biome that you launch from the hub, hunt in, and return from. This is replacing
  the older infinite streamed world below.

### The (legacy) open world
- **`biome.rs`** — procedural world generation: Voronoi biome regions + value
  noise + Poisson-disk placement + weighted spawn tables.
- **`world_chunks.rs`** — streaming the infinite world in chunks as the player
  moves (load radius / cull radius with hysteresis).
- **`wild_animal.rs`** — per-species movement & escape AI (zigzaggers, bursters,
  circlers, vanishers, charging bashers) as small state machines.

### The avatar (your in-world character)
- **`intent.rs`** — `ControllerIntent` / `ActionFlags`: a device-independent
  description of "what the avatar wants to do this frame" (move here, dash, etc.).
- **`avatar.rs`** — the avatar's position/velocity state in world units.
- **`avatar_system.rs`** — updates the avatar each tick as an ordered chain of
  `Behavior` steps, so adding a dash or knockback is *additive* (insert a step)
  rather than a rewrite.

### Multiplayer seams
- **`action.rs`** — `Action`, the single enum of "things a player can do to the
  host's zoo." The host applies its own actions directly; a visitor sends the
  same `Action` over the network and the host applies it on their behalf. One
  vocabulary = one source of truth.
- **`npc.rs` / `merchant.rs` / `vendor.rs`** — placed NPCs and the merchant that
  sells structures. `vendor.rs` is a **stub** for a planned per-biome merchant
  system (only the data seam exists today).

### Utility
- **`rng.rs`** — a tiny deterministic xorshift RNG so procedural placement and
  jitter work without the graphics engine's random.

## Reading order for newcomers

1. `species.rs` — see what an animal *is*.
2. `zoo.rs` — see the state that holds everything and the methods that change it.
3. `animal.rs` + `economy.rs` — see how income works (it's derived, not ticked —
   a small surprise worth understanding).
4. `catch.rs` + `gear.rs` + `biome_instance.rs` — the current gameplay direction.
5. `action.rs` — how all of the above gets driven uniformly in co-op.

## A few Rust idioms you'll see a lot here

- **String ids instead of an enum of every species.** `type SpeciesId =
  &'static str;` — animals are identified by a baked-in string and looked up in a
  `HashMap`. `species::try_get(id)` returns `Option<&SpeciesDef>` so unknown ids
  fail gracefully (important for loading old saves).
- **`once_cell::Lazy`** — the catalogs are built once on first access and cached
  (Rust has no "static initializer" that runs arbitrary code at startup, so this
  is the idiom for a lazily-built global table).
- **Methods return `Result<_, String>` for "the player can't do that"** (not
  enough coins, habitat full). The UI shows the `Err` string as a status message.
