# cmd_zoo

An idle zoo-management game built in Rust with [macroquad](https://github.com/not-fl3/macroquad).

This project is a hands-on learning endeavor: a place to practice game development
and systems programming in Rust by building a complete, self-contained game rather
than following a tutorial. The goal is breadth — touching rendering, game state,
persistence, procedural content, and networking — while keeping the code readable
and well-documented.

## What it does

- **Idle economy** — animals live in habitats, accrue income over time, and are
  collected for coins or DNA Helix.
- **An open world to explore** — a chunk-streamed wilderness surrounds your home
  zoo plot, with biome-driven procedural animal spawns.
- **Catching** — wild animals roam with per-species evasion behaviors (zigzag,
  burst, vanish, circle, and a charging "basher"). You catch them with a
  hover-to-fill capture ring; rarer animals take repeated catches.
- **Breeding & crossbreeding** — pair animals to discover hybrids from a large
  recipe table (100+ species, including themed exotics and concept hybrids).
- **Local + online play** — a session/avatar layer supports visiting a friend's
  zoo over a Steam relay transport (optional feature) or a loopback transport
  for single-player.

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

## Status

A work in progress and a personal learning project. Expect rough edges,
placeholder art for newer species, and systems that are still evolving.
