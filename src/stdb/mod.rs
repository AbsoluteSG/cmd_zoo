//! SpacetimeDB online backend integration (Milestone B, client side).
//!
//! `bindings` is the auto-generated client module (tables, reducers, the
//! `DbConnection` type) produced by `spacetime generate --lang rust -p
//! crates/cmd_zoo_stdb`. Regenerate it whenever the module schema changes; do
//! not edit it by hand.
//!
//! The hand-written adapter that drives a connection, subscribes to the hub
//! tables, and routes player actions through reducer calls will live alongside
//! it here.

pub mod bindings;
pub mod client;
