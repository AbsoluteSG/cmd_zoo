# `net/` — co-op networking

This module lets a second player visit your zoo. It is **host-authoritative**:
one player (the host) owns the one true `Zoo`; visitors send what they *want* to
do and the host decides and broadcasts the result. Solo play never touches any of
this — `Session::solo()` is a pure-local construction with no transport.

## The three layers

The design separates "moving bytes" from "what the bytes mean" from "who's
connected," so each can change independently:

```
        ┌─────────────────────────────────────────────┐
        │ session.rs   — avatars + peers + controllers │  "who is here, drive them"
        ├─────────────────────────────────────────────┤
        │ protocol.rs  — NetMessage, JoinCode, PeerId  │  "what the bytes mean"
        ├─────────────────────────────────────────────┤
        │ transport.rs — NetTransport trait, NetEvent  │  "move bytes; notify me"
        │   loopback.rs (in-memory)   steam.rs (relay) │
        └─────────────────────────────────────────────┘
```

### `transport.rs` — the byte pipe (a trait)
`NetTransport` is an interface for "send these bytes / poll for events." It hides
*how* bytes travel. Two implementations:
- **`loopback.rs`** — an in-memory paired host+visitor in the same process. Used
  by tests and the "local co-op demo," so the whole pipeline can run without
  Steam or a second machine.
- **`steam.rs`** — the real transport over Steam's relay (NetworkingSockets).
  **Gated behind the `steam` Cargo feature** so plain `cargo build` works without
  the Steamworks SDK. Discovery is codeless: the host's share code *is* its
  SteamID, and a visitor's stable `player_id` is derived from their SteamID
  (`UUIDv5`) so reinstalls keep their visitor record.

> Rust note: `NetTransport` being a trait means `Session` holds a
> `Box<dyn NetTransport>` and doesn't care which transport it's driving — the
> exact same session logic runs in a unit test (loopback) and in production
> (Steam).

### `protocol.rs` — the message format
Defines `NetMessage` (the things peers send each other), `JoinCode`, and `PeerId`
(an opaque transport-level peer handle — the session maps it to a game-level
`player_id`). Visitors stream **`Intent`** (movement/action wishes); the host
streams avatar deltas and world changes back. Serialization is **JSON for now** —
small and debuggable, reusing the `serde` derives already on the game DTOs; it can
be swapped for a binary format later if bandwidth matters.

### `session.rs` — the live game session
Owns the set of avatars, the connected peers, and a controller per peer. It drives
the transport, reacts to its events (peer joined/left, message arrived), and routes
each visitor's inbound `Intent` into that visitor's `RemoteController` (see
[`input/`](../input/README.md)). `SessionRole` distinguishes host vs visitor.

## How a visitor's action reaches the host's zoo

This ties together with `game::action::Action` in the core:

1. Visitor presses a key → their client builds an `Intent` / `Action`.
2. It's sent as a `NetMessage` over the transport.
3. The host receives it, feeds movement into that peer's `RemoteController`, and
   applies any `Action` to **its** authoritative `Zoo` on the visitor's behalf.
4. The host broadcasts the resulting state change to everyone.

Because both the host's own input and the visitor's network input become the same
`Action`/`Intent` types, there's a single source of truth and no "visitor edits a
local copy that gets overwritten" bug.

## Helper files
- **`demo_bot.rs`** — an in-process fake visitor (Settings → "Local co-op demo").
  Holds the visitor side of a loopback pair, says Hello, then wanders by emitting
  `Intent`s on a timer. Exercises the entire host pipeline without Steam.
- **`diag.rs`** — an append-only `cmd_zoo_net.log` next to the executable. Windowed
  release builds have no console, so every host/join step is logged here (and to
  stderr under `cargo run`) to make co-op issues debuggable. Best-effort and
  panic-free — logging never affects gameplay.

## Building with Steam

```sh
cargo run --features steam
```

Requires the Steamworks SDK to be discoverable at build time, and on Windows the
`steam_api64.dll` redistributable next to the produced exe at runtime. Without the
feature, the Steam transport is simply not compiled and the rest of the game still
builds and runs.

## Relationship to the SpacetimeDB plan

Today's model is **host-authoritative peer-to-peer** (one player's machine is the
authority). The longer-term multiplayer direction is to move that authority into a
SpacetimeDB server module, where the headless rules in
[`cmd_zoo_core`](../../crates/cmd_zoo_core/README.md) run inside reducers and all
players are clients of the server. This `net/` layer is the current-generation
co-op that the migration will eventually generalize.
