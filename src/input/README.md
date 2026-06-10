# `input/` — turning devices into avatar intent

This module answers one question every frame: **"what does the avatar want to do
right now?"** — and answers it in a way that doesn't care *where* the request
came from (keyboard today; gamepad, network, or AI later).

## The key idea: a Controller produces an Intent

```
real device (keyboard)  ──►  Controller  ──►  ControllerIntent  ──►  avatar_system
                                                (move NE, dash)        (in core)
```

A **`ControllerIntent`** is a device-independent description of the frame's wishes:
a movement direction plus an `ActionFlags` bitset (dash, interact, …). The avatar
movement simulation (`game::avatar_system`, in the headless core) consumes only
intents — it never reads the keyboard. So you can swap or add controllers without
touching the avatar pipeline.

> The intent types (`ControllerIntent`, `ActionFlags`) actually *live in the
> core* (`cmd_zoo_core::game::intent`) so the server can understand them too.
> `controller.rs` just re-exports them under `crate::input::…` so client code has
> a stable path.

## The files

| File | Role |
|------|------|
| `mod.rs` | Declares the controllers and re-exports the shared types. |
| `controller.rs` | The `AvatarController` trait (the "produce an intent" interface) plus `ControllerCtx`, the read-only world view a controller may inspect when sampling. Re-exports the core intent types. |
| `keyboard.rs` | `KeyboardController` — WASD/arrow-key control. Stateless: it polls held keys each sample. |
| `remote.rs` | `RemoteController` — fed by network `Intent` packets via a per-peer queue. To the avatar pipeline it's identical to the keyboard controller; the host runs one per connected visitor and pokes in the latest received intent each frame. |

## Why a trait here? (Rust note)

`AvatarController` is a **trait** — Rust's version of an interface. Anything that
implements it (`KeyboardController`, `RemoteController`, a future `GamepadController`
or `AiController`) can drive an avatar. The session/app code holds controllers as
`Box<dyn AvatarController>` (a "trait object" — dynamic dispatch), so it can mix
local and remote players in one list without knowing their concrete types.

This is the seam that makes co-op work cleanly: a visitor walking around your zoo
is *the same machinery* as you walking around it — their keystrokes just arrive
over the network and get replayed through a `RemoteController` instead of being
read off your local keyboard.

## Adding a new input source

1. Make a struct that implements `AvatarController`.
2. In its sample method, read whatever it reads (a gamepad, an AI policy, a
   replay file) and return a `ControllerIntent`.
3. Register it wherever controllers are created (see `net/session.rs` for how
   per-peer controllers are wired). The avatar pipeline needs no changes.
