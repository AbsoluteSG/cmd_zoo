//! `cmd_zoo_core` — the engine-free simulation core.
//!
//! Contains the entire persistent domain (`game`), its serialization
//! (`persistence`), and the share-code codec (`share`). It has **no** dependency
//! on macroquad or any renderer, so it compiles headless and can run both in the
//! desktop client (Solo/offline) and, in a later phase, inside the SpacetimeDB
//! module as the online authority.

pub mod game;
pub mod level;
pub mod persistence;
pub mod share;
