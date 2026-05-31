//! `cmd_zoo` library crate root.
//!
//! Exposes everything `main.rs` needs to bootstrap the Macroquad app against
//! the file-backed repository. `game`, `persistence`, and `share` are
//! renderer-agnostic; `render` + `app` are the Macroquad presentation layer.

pub mod app;
pub mod audio;
pub mod game;
pub mod input;
pub mod net;
pub mod persistence;
pub mod render;
pub mod share;
