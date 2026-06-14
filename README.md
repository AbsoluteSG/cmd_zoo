# cmd_zoo (working title: **Critter Cove**)

A cozy co-op zoo builder — explore a procedural wilderness, catch the critters that roam
it, and raise them into an idle-earning zoo, solo or with friends. Built in Rust with
[macroquad](https://github.com/not-fl3/macroquad).

This started as a hands-on learning endeavor — a place to practice game development and
systems programming in Rust by building a complete, self-contained game — and has grown
into a full cozy/idle game with real-time online co-op. The code stays readable and
heavily documented, and core logic is unit-tested.

## What it does

- **Idle economy** — animals accrue coins and DNA Helix over time. Collect a critter's
  income once it's full (click it, or press interact nearby); below-cap animals just
  play a poke sound. Level animals up and rank up duplicates for bigger payouts, and
  place animals on **pedestals** to bank rewards while away.
- **A vast procedural world** — your home plot sits in a seamless, seed-generated
  wilderness with no loading screens: forest, arctic, savanna, jungle, desert, ocean,
  and stranger biomes, each with its own critters. Mark fast-travel waypoints, zoom, and
  dash across the map.
- **Expeditions** — launch into bounded, seed-generated biome arenas from the hub, hunt
  their wild animals, and bring catches home.
- **Catching that fights back** — target a wild animal and *engage*: deplete its
  catch-resistance bar via a stat-driven roll, three active **skills**, and a DBD-style
  **compass skill check** (a needle sweeps a ring — hit the green band, miss and the bar
  claws back). The target also slowly regenerates, so stalling loses ground. Skills:
  **Net** (flat instant damage), **Lure** (suppresses the target's regen), **Trap** (a
  short damage-over-time). Catch power comes from equipped gear plus owned-collection
  bonuses — no XP or skill tree.
- **Breeding & crossbreeding** — lead animals to a nest on a physics-based follow chain,
  pair them, and wait. A 100+ species catalog yields crossbreeds and themed exotics.
- **Food structures** — unlockable structures produce food, a third resource alongside
  coins and DNA Helix.
- **Online co-op (SpacetimeDB)** — go online to a shared, server-authoritative hub:
  meet other players on the central plaza, form parties, catch animals together (every
  participant is credited), and warp between the plaza and your own zoo plot. A separate
  Steam-relay/loopback session layer also supports visiting a friend's zoo and sending
  gift animals.
- **Controller support** — full gamepad play (keyboard/mouse and pad both work, and the
  game auto-switches to whichever you touch): stick movement, focus-based menu
  navigation, and pad bindings for every gameplay action. Steam Deck friendly.
- **Visual effects** — fullscreen post-process filters (Scanlines, Pixelate, Grayscale,
  Sepia), blurred menu backdrops, swaying grass, collect-pops, and camera shake / hitstop
  / venom vignette on impacts.

## Controls

The game auto-detects whether you're on keyboard/mouse or a gamepad and adapts the UI.

### Keyboard & mouse

| Input | Action |
|-------|--------|
| WASD / arrows | Move avatar |
| Shift | Sprint |
| Space | Dash (hub) · hit skill check (expedition) |
| E | Interact — collect a ready animal, open a nest / structure / pedestal / NPC, or inspect |
| Left click | Collect a critter's income · target+engage a wild animal · confirm in menus/modals |
| 1–5 | Select hotbar slot (1–3 also fire skills in an expedition) |
| Mouse wheel | Cycle hotbar · **Ctrl**+wheel zooms |
| U / O / M | Upgrades · Settings (online join) · Waypoints |
| F6 / F7 | Launch / leave an expedition · connect / leave the online hub |
| G / Y / N / P | Online party: invite · accept · decline · leave |
| H | Online: warp between the shared plaza and your zoo |
| Escape | Close a menu / cancel a modal |

### Gamepad (Xbox / PlayStation layout)

| Input | Action |
|-------|--------|
| Left stick | Move |
| LT (hold) / LB | Sprint / Dash |
| West (□ / X) | Interact · collect a ready animal · engage a wild animal |
| South (✕ / A) | Hit skill check · confirm in menus |
| East (○ / B) | Cancel · close menu |
| D-pad | Fire skills 1/2/3 (expedition) · cycle hotbar (hub) · move menu focus |

## Building

```sh
cargo run            # play single-player
cargo test           # run the test suite
cargo build --release
```

Online play over the Steam relay requires the Steamworks SDK and the `steam` feature:

```sh
cargo run --features steam
```

Plain `cargo build` works without the SDK — the Steam transport is gated behind the
optional feature, and the SpacetimeDB online hub uses its own client SDK. The save file
is written to the platform's standard app-data directory; a fresh save seeds one starter
critter.

## Layout

| Area | Where | What it explores |
|------|-------|------------------|
| Rendering | `src/render/` | Camera/projection, billboard sprites, depth sorting, HUD, focus-nav menus, post-process shaders, texture caching |
| Input | `src/input/` | Device-independent controller/intent abstraction; keyboard, gamepad (gilrs), and remote controllers; active-device tracking |
| Procedural content | `crates/cmd_zoo_core/src/game/biome.rs`, `biome_instance.rs` | Noise sampling, placement, weighted spawn tables; bounded expedition arenas |
| Catching & skills | `crates/cmd_zoo_core/src/game/catch.rs`, `skill.rs`, `gear.rs` | Resistance bar, compass skill check, capture-regen, active skills, gear + collection catch power |
| Domain modeling | `crates/cmd_zoo_core/src/game/` | Species catalog, economy, breeding recipes, nests, structures, pedestals |
| Persistence | `crates/cmd_zoo_core/src/persistence/` | Versioned save schema with forward migrations, atomic writes |
| Networking | `src/net/`, `src/stdb/` | Steam-relay/loopback session sync; SpacetimeDB online hub client |
| Audio | `src/audio.rs` | Embedded sound-effect playback |

Most modules carry doc comments explaining the design decisions.

## Status

An actively evolving project. Expect rough edges, placeholder art for newer species, and
systems still in flux. See `docs/` for design direction.
