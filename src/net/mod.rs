//! Networking layer for M2 co-op. Transport-agnostic by design: a `NetTransport`
//! trait handles bytes, the protocol layer handles message framing, and the
//! session layer ties peers to in-world avatars.
//!
//! The concrete transports are:
//!   - `loopback`: in-memory paired endpoints for tests + local demo.
//!   - `steam`:    Steam relay / NetworkingSockets (added in a follow-up spike).
//!
//! Solo play does not touch this module at all — `Session::solo()` is a
//! pure-local construction with no transport.

pub mod demo_bot;
pub mod loopback;
pub mod protocol;
pub mod session;
pub mod transport;

#[cfg(feature = "steam")]
pub mod steam;

pub use protocol::{JoinCode, NetMessage, PeerId};
pub use session::{Session, SessionRole};
pub use transport::{NetEvent, NetTransport};
