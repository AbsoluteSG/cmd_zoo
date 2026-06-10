# `cmd_zoo_core` — the headless simulation core

This crate is the **brain** of the game. It holds all the rules — animals,
economy, breeding, catching, biomes, save format — and **nothing about
graphics, windows, or input devices**. It compiles and runs without a screen.

If you're new to Rust: a *crate* is Rust's unit of compilation and distribution
(think "a library" or "a package"). This project is a **workspace** of two
crates: this headless `cmd_zoo_core` and the desktop client in the repo's
top-level `src/`. The client depends on this crate; this crate depends on
nothing game-engine-related.

```
cmd_zoo (client, top-level src/) ──depends on──► cmd_zoo_core (this crate)
   macroquad: windows, textures,                  pure rules: zoo state,
   audio, input, rendering                        economy, breeding, saves
```

## Why split the rules out from the renderer?

Two reasons, and the second is the important one for this project's roadmap:

1. **Testability.** Because there's no engine here, every rule can be unit-tested
   with plain `cargo test` — no window, no GPU. Look for `#[cfg(test)]` blocks at
   the bottom of most files.

2. **Online authority (SpacetimeDB).** The long-term plan is multiplayer where a
   server holds the one true game state. The same rule code needs to run in two
   places: on the player's machine for solo/offline play, **and** inside a
   server module that all players connect to. Keeping the rules engine-free means
   the *exact same functions* can be compiled into that server.

### What is SpacetimeDB (in one paragraph)?

[SpacetimeDB](https://spacetimedb.com) is a database that also runs your
server-side logic. Instead of a database + a separate backend service, you upload
a **module** (a WebAssembly program — which can be compiled from Rust) that owns
the tables *and* the functions that change them. Those functions are called
**reducers**: a reducer is the only way to mutate the database, it runs to
completion atomically (all-or-nothing, like a transaction), and clients trigger
them by sending a request. Clients then *subscribe* to tables and get live
updates pushed to them. So in our case: this crate's pure functions (e.g.
"apply this catch", "advance the economy") are designed to be callable from
inside a reducer, with the `Zoo` state living in SpacetimeDB tables. You don't
need to know SpacetimeDB to read this crate today — just know that "pure,
deterministic, no I/O" is a deliberate constraint so the migration is possible.

> Status note: the SpacetimeDB module itself is not in this repo yet. This crate
> is the *preparation* for it — the code is being shaped so it can drop into a
> reducer later. References to "online authority" and "reducers" in doc comments
> describe that destination.

## Layout

| Folder | What lives there | README |
|--------|------------------|--------|
| `src/game/` | The entire domain: species catalog, the `Zoo` aggregate, economy, breeding, catching, biomes, avatar movement, co-op actions | [`src/game/README.md`](src/game/README.md) |
| `src/persistence/` | Turning a `Zoo` into JSON on disk and back, with versioned save migrations | [`src/persistence/README.md`](src/persistence/README.md) |
| `src/share/` | The `czoo1:` share-code codec (compress a zoo snapshot into a copy-paste string / QR code) | [`src/share/README.md`](src/share/README.md) |
| `src/lib.rs` | The crate root — just declares the three modules above | — |

## Design rules this crate follows

These constraints are what make the future server migration possible. If you add
code here, keep them:

- **No engine dependencies.** No `macroquad`, no window/audio/GPU. Math uses
  `glam` (a small linear-algebra library), pinned to the same version the client
  uses so `Vec2` is literally the same type on both sides of the boundary.
- **Deterministic.** Given the same inputs, a function must produce the same
  output everywhere (your machine and the server must agree). That's why there's
  a tiny custom RNG (`game/rng.rs`) instead of pulling in the engine's random,
  and why procedural generation is seeded.
- **Time is passed in, never read.** Functions take a `now: DateTime<Utc>`
  argument instead of calling `Utc::now()` themselves. The caller decides what
  "now" is — which is essential when a server, not the client, owns the clock.
- **Mutations go through data, not ad-hoc methods.** Player actions are modeled
  as an `Action` enum (`game/action.rs`) so the host can apply the *same* action
  whether it came from the local player or arrived over the network.

## Rust glossary for this crate

A few terms you'll meet immediately:

- **`struct` / `enum`** — a struct groups fields together (like a record/class
  with no inheritance); an enum is a "one of these variants" type, and each
  variant can carry its own data (e.g. `AnimalState::Breeding { ends_at, .. }`).
- **`Option<T>`** — either `Some(value)` or `None`. Rust has no `null`; absence is
  this explicit type.
- **`Result<T, E>`** — either `Ok(value)` or `Err(error)`. Fallible functions
  return this; the `?` operator early-returns the error.
- **`trait`** — like an interface: a set of methods a type can implement
  (`ZooRepository` in `persistence` is one).
- **Ownership / `&` / `&mut`** — every value has one owner; `&zoo` is a read-only
  borrow, `&mut zoo` is an exclusive mutable borrow. Most rule functions take
  `&mut Zoo` (they change the world) or `&Zoo` (they only read it).

## Running the tests

```sh
cargo test -p cmd_zoo_core
```

That runs only this crate's tests — a fast way to check the rules without
building the graphical client.
