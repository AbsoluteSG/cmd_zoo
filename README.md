# cmd_zoo

An idle zoo-management game built in Rust with [macroquad](https://github.com/not-fl3/macroquad).

This project is a hands-on learning endeavor: a place to practice game development
and systems programming in Rust by building a complete, self-contained game rather
than following a tutorial. The goal is breadth — touching rendering, game state,
persistence, procedural content, and networking — while keeping the code readable
and well-documented.

## What it does

- **Idle economy** — animals accrue coins and DNA Helix over time. Click a critter
  to collect its income once it's full; below-cap animals play a poke sound instead.
- **An open world to explore** — a chunk-streamed wilderness surrounds your home
  zoo plot, with biome-driven procedural animal spawns. Scroll to zoom; use fast-travel
  waypoints to mark and return to points of interest.
- **Catching** — enter catch mode (`C`) and hover over a wild animal to fill the
  capture ring. Wild animals have per-species evasion behaviors: zigzaggers, bursters,
  circlers, vanishers, and charging bashers that can knock back and interrupt your catch.
  Rarer species require multiple successful catches before they're tamed.
- **Breeding & crossbreeding** — walk animals to a nest with the follow chain (`E` to
  inspect → Follow), then deposit them into a nest to start gestation. Pairs can
  produce hybrids from a large recipe table (100+ species, including themed exotics
  and concept hybrids).
- **Food structures** — unlockable structures (Hay Bale, Insectary, Feed Mill,
  Aquaculture) produce food as a third resource alongside coins and DNA Helix.
- **Follow chain** — up to 10 owned animals can trail your avatar in a physics-based
  elastic chain. Use the spotlight deposit view to place them into nests.
- **Visual effects** — five fullscreen post-process shaders (Scanlines, Pixelate,
  Grayscale, Sepia, None) applied to the world scene; menus blur the background.
  Hit impacts trigger camera shake, hitstop, and a venom vignette.
- **Local + online play** — a session/avatar layer supports visiting a friend's
  zoo over a Steam relay transport (optional feature) or a loopback transport
  for single-player. Visitors can send gift animals to the host.

## Controls

| Key / Input | Action |
|-------------|--------|
| WASD | Move avatar |
| Shift | Sprint |
| Space | Dash |
| C | Toggle catch mode |
| E | Interact — inspect nearest animal, or open a nest / food-structure panel |
| Left click | Collect income from a critter (or select in deposit mode) |
| Mouse wheel | Zoom in / out |
| 1 | Shop |
| 3 | Settings / online join panel |
| 4 | Waypoints |
| Escape | Close menu or inspect panel |

## Building

```sh
cargo run            # play single-player
cargo test           # run the test suite
cargo build --release
```

Online play requires the Steamworks SDK and the `steam` feature:

```sh
cargo run --features steam
```

Plain `cargo build` works without the SDK — the Steam transport is gated behind
the optional feature so the project always builds out of the box.

The save file is written to the platform's standard app-data directory
(`directories` crate). A fresh save seeds one blue frog to get you started.

## Learning focus

Each module is a focused study in a different area of building a game in Rust:

| Area | Where | What it explores |
|------|-------|------------------|
| Rendering | `src/render/` | Camera/projection, billboard sprites, depth sorting, HUD, post-process shaders, texture caching |
| World streaming | `src/game/world_chunks.rs` | Chunk load/cull with hysteresis, entity migration, spatial culling |
| Procedural content | `src/game/biome.rs` | Noise sampling, Poisson-disk placement, weighted spawn tables |
| Game AI | `src/game/wild_animal.rs` | State machines for per-species movement and catch behaviors |
| Domain modeling | `src/game/` | Species catalog, economy, breeding recipes, structures |
| Persistence | `src/persistence/` | Versioned save schema with forward migrations, atomic file writes |
| Networking | `src/net/` | Transport abstraction, session/avatar sync, loopback vs. Steam relay |
| Misc systems | `src/audio.rs`, `src/share/` | Sound playback, shareable QR codes for zoo snapshots |

Most modules carry doc comments explaining the design decisions, and core logic
is covered by unit tests (`cargo test`).

## Status

A work in progress and a personal learning project. Expect rough edges,
placeholder art for newer species, and systems that are still evolving.
See `docs/GAMEPLAY_BRAINSTORM.md` for the longer-term design direction.
