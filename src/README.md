# `src/` — the desktop client (Macroquad)

This is the **client crate**: the thing you actually run with `cargo run`. It
opens a window, draws the world, plays sounds, reads the keyboard/mouse, and
talks to the network. All the *game rules* live in the separate headless
[`cmd_zoo_core`](../crates/cmd_zoo_core/README.md) crate; this crate is the
**presentation + I/O layer** wrapped around those rules.

```
src/  (cmd_zoo binary)  ── uses ──►  cmd_zoo_core  (rules: Zoo, economy, catch…)
  macroquad window/GPU/audio/input         pure, headless, testable
```

> Rust note: the top-level `Cargo.toml` is the binary crate; `crates/cmd_zoo_core`
> is a library crate it depends on. `src/lib.rs` re-exports the core's modules
> under `crate::game`, `crate::persistence`, `crate::share` so client code can
> write `crate::game::Zoo` as if it were local.

## Engine: what is Macroquad?

[Macroquad](https://github.com/not-fl3/macroquad) is a small, batteries-included
Rust game framework: it gives you a window, a 2D/3D draw API, texture loading,
audio, and input polling, all behind one `use macroquad::prelude::*;`. It runs an
**async main loop** — note the `#[macroquad::main(...)]` attribute on `main()` and
the `next_frame().await` at the end of each loop iteration. That `.await` is how
you hand control back to the engine to actually present the frame.

The loop is dead simple (see `main.rs`):

```rust
loop {
    let now = Utc::now();
    app.tick(now);          // advance time-driven state (offline catch-up, etc.)
    app.handle_input(now);  // turn key/mouse into game actions
    app.draw(now);          // render the world + HUD + menus
    next_frame().await;     // present, then come back next frame
}
```

## The files in this folder

| File | What it does |
|------|--------------|
| `main.rs` | Entry point. `boot()` opens the save file, loads the `Zoo`, applies offline progress, seeds a starter frog on a fresh save, then runs the frame loop above. |
| `lib.rs` | Crate root. Declares the client modules and re-exports the core's modules. |
| `app.rs` | **`GameApp`** — the central shell. Owns the `Zoo`, the file repository, the camera, the texture cache, the roaming critters, menus, and per-frame save. This is the glue between rules, input, and rendering. Start here to see how a frame is assembled. |
| `audio.rs` | Sound-effect cache. Sound files in `assets/sfx/` are embedded by `build.rs`, decoded once, and played by id (fuzzy lookup; missing ids are a silent no-op). |
| `expedition.rs` | Client controller for the Phase-3 catch loop: owns the current biome instance + live catch engagement and translates player commands into calls on the headless `biome_instance` + `catch` core. Deliberately macroquad-free so it can be unit-tested. |

## The subfolders (each has its own README)

| Folder | Responsibility | README |
|--------|----------------|--------|
| `render/` | All drawing: 2.5D projection, texture cache, world scene, HUD, menus, particles, grass, terrain props, shaders | [`render/README.md`](render/README.md) |
| `input/` | Turning real devices (keyboard now; gamepad/network later) into device-independent avatar `ControllerIntent`s | [`input/README.md`](input/README.md) |
| `net/` | Co-op: a transport abstraction (loopback for tests, Steam relay in production), the wire protocol, and the session that ties peers to avatars | [`net/README.md`](net/README.md) |

## How a frame flows through the crate

1. **`input/`** samples the keyboard → a `ControllerIntent` ("move NE, dash").
2. **`cmd_zoo_core`** (`avatar_system`, `catch`, `economy`, `Zoo` methods)
   applies that intent and advances the rules.
3. **`net/`** (if online) syncs the authoritative state between host and visitors.
4. **`render/`** draws the resulting `Zoo` + avatars + effects, plus the HUD.

The important mental model: **`src/` never decides game outcomes.** It samples
input, calls into the rules, and draws whatever the rules produced. That
separation is what lets the same rules eventually run on a server.

## Building & running

```sh
cargo run                  # play single-player
cargo run --features steam # enable the Steam relay transport (needs the SDK)
cargo test                 # run client + core tests
```

See the repository [top-level README](../README.md) for controls and the full
feature tour.
