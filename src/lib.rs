//! `cmd_zoo` library crate root.
//!
//! Exposes everything `main.rs` (native) and (eventually) `web.rs` (wasm) need
//! to bootstrap the same `EguiApp` against different storage backends.

pub mod app;
pub mod game;
pub mod persistence;
pub mod share;
pub mod ui;
