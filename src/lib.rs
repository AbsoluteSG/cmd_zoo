//! `cmd_zoo` library crate root.
//!
//! Exposes everything `main.rs` needs to bootstrap the Macroquad app against
//! the file-backed repository. `game`, `persistence`, and `share` are
//! renderer-agnostic; `render` + `app` are the Macroquad presentation layer.

// The engine-free simulation core lives in its own crate. Re-export its modules
// under the original paths so client code keeps using `crate::game`,
// `crate::persistence`, and `crate::share` unchanged.
pub use cmd_zoo_core::{game, persistence, share};

pub mod app;
pub mod audio;
pub mod expedition;
pub mod input;
pub mod net;
pub mod render;
