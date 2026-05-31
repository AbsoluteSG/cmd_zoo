//! Input abstraction. A `Controller` produces a `ControllerIntent` each tick;
//! `game::avatar_system` consumes intents through a Behavior chain. New
//! controllers (gamepad, remote/network in M2, AI) plug in without touching
//! the avatar pipeline.

pub mod controller;
pub mod keyboard;
pub mod remote;

pub use controller::{ActionFlags, AvatarController, ControllerCtx, ControllerIntent};
pub use keyboard::KeyboardController;
pub use remote::RemoteController;
