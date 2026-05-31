//! `NetTransport` abstracts the byte-level transport — Steam relay in
//! production, an in-memory queue in tests. The session layer drives polling
//! and dispatch; transports only deliver bytes (or `NetMessage` directly, to
//! sidestep serialization in tests).

use super::protocol::{NetMessage, PeerId};

/// Events surfaced to the session layer each poll. Connection lifecycle plus
/// inbound messages.
#[derive(Debug)]
pub enum NetEvent {
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
    Message { from: PeerId, msg: NetMessage },
}

pub trait NetTransport {
    /// Drain pending events. Non-blocking; called once per frame.
    fn poll(&mut self) -> Vec<NetEvent>;
    /// Send to a specific peer. Best-effort; transport handles ordering /
    /// reliability per its own guarantees.
    fn send(&mut self, to: PeerId, msg: NetMessage);
    /// Send to every connected peer except `except`.
    fn broadcast(&mut self, msg: NetMessage, except: Option<PeerId>);
    /// Disconnect a peer (host-side kick or visitor-side leave).
    fn disconnect(&mut self, peer: PeerId);
    /// Implementation-specific shutdown (close sockets, leave lobby).
    fn shutdown(&mut self);
}
