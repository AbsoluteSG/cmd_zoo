//! In-memory transport pair. Used by tests and by the "local online demo"
//! affordance in Settings: spawn a paired host+visitor in the same process
//! so the multi-avatar / session / RemoteController pipeline can be
//! exercised end-to-end without Steam.
//!
//! Construction returns `(host, visitor)`. The host can host arbitrarily
//! many visitors by chaining more `pair()` calls and passing each returned
//! visitor handle to a `Session::join`.

use std::cell::RefCell;
use std::rc::Rc;

use super::protocol::{NetMessage, PeerId};
use super::transport::{NetEvent, NetTransport};

/// Shared mailbox between two endpoints.
struct Mailbox {
    /// Host-bound events (peer connected, peer messages).
    host_inbox: Vec<NetEvent>,
    /// Visitor-bound events (welcomed, deltas).
    visitor_inbox: Vec<NetEvent>,
    /// `true` until either side calls `shutdown` / `disconnect`.
    connected: bool,
    /// PeerId that the host sees the visitor as.
    visitor_peer_id: PeerId,
    /// PeerId the visitor uses to address the host.
    host_peer_id: PeerId,
}

/// The host's view of a single visitor connection.
pub struct LoopbackHost {
    box_: Rc<RefCell<Mailbox>>,
    handshake_sent: bool,
}

/// The visitor's view of its connection to the host.
pub struct LoopbackVisitor {
    box_: Rc<RefCell<Mailbox>>,
    handshake_sent: bool,
}

/// Build a connected pair. The host sees the visitor at `visitor_peer_id`;
/// the visitor addresses the host as `host_peer_id`.
pub fn pair(host_peer_id: PeerId, visitor_peer_id: PeerId) -> (LoopbackHost, LoopbackVisitor) {
    let mb = Rc::new(RefCell::new(Mailbox {
        host_inbox: Vec::new(),
        visitor_inbox: Vec::new(),
        connected: true,
        visitor_peer_id,
        host_peer_id,
    }));
    (
        LoopbackHost {
            box_: mb.clone(),
            handshake_sent: false,
        },
        LoopbackVisitor {
            box_: mb,
            handshake_sent: false,
        },
    )
}

impl NetTransport for LoopbackHost {
    fn poll(&mut self) -> Vec<NetEvent> {
        let mut mb = self.box_.borrow_mut();
        if !self.handshake_sent {
            self.handshake_sent = true;
            let peer = mb.visitor_peer_id;
            mb.host_inbox.insert(0, NetEvent::PeerConnected(peer));
        }
        std::mem::take(&mut mb.host_inbox)
    }
    fn send(&mut self, to: PeerId, msg: NetMessage) {
        let mut mb = self.box_.borrow_mut();
        if !mb.connected {
            return;
        }
        // Host has one visitor in a pair; ignore `to` mismatch silently in
        // tests rather than panicking.
        if to == mb.visitor_peer_id {
            let from = mb.host_peer_id;
            mb.visitor_inbox.push(NetEvent::Message { from, msg });
        }
    }
    fn broadcast(&mut self, msg: NetMessage, except: Option<PeerId>) {
        let mut mb = self.box_.borrow_mut();
        if !mb.connected {
            return;
        }
        if except != Some(mb.visitor_peer_id) {
            let from = mb.host_peer_id;
            mb.visitor_inbox.push(NetEvent::Message { from, msg });
        }
    }
    fn disconnect(&mut self, _peer: PeerId) {
        let mut mb = self.box_.borrow_mut();
        if mb.connected {
            mb.connected = false;
            let visitor_peer = mb.visitor_peer_id;
            let host_peer = mb.host_peer_id;
            mb.visitor_inbox.push(NetEvent::PeerDisconnected(host_peer));
            mb.host_inbox.push(NetEvent::PeerDisconnected(visitor_peer));
        }
    }
    fn shutdown(&mut self) {
        let peer_id = self.box_.borrow().visitor_peer_id;
        self.disconnect(peer_id);
    }
}

impl NetTransport for LoopbackVisitor {
    fn poll(&mut self) -> Vec<NetEvent> {
        let mut mb = self.box_.borrow_mut();
        if !self.handshake_sent {
            self.handshake_sent = true;
            let peer = mb.host_peer_id;
            mb.visitor_inbox.insert(0, NetEvent::PeerConnected(peer));
        }
        std::mem::take(&mut mb.visitor_inbox)
    }
    fn send(&mut self, to: PeerId, msg: NetMessage) {
        let mut mb = self.box_.borrow_mut();
        if !mb.connected {
            return;
        }
        if to == mb.host_peer_id {
            let from = mb.visitor_peer_id;
            mb.host_inbox.push(NetEvent::Message { from, msg });
        }
    }
    fn broadcast(&mut self, msg: NetMessage, except: Option<PeerId>) {
        let mut mb = self.box_.borrow_mut();
        if !mb.connected {
            return;
        }
        if except != Some(mb.host_peer_id) {
            let from = mb.visitor_peer_id;
            mb.host_inbox.push(NetEvent::Message { from, msg });
        }
    }
    fn disconnect(&mut self, _peer: PeerId) {
        let mut mb = self.box_.borrow_mut();
        if mb.connected {
            mb.connected = false;
            let visitor_peer = mb.visitor_peer_id;
            let host_peer = mb.host_peer_id;
            mb.host_inbox.push(NetEvent::PeerDisconnected(visitor_peer));
            mb.visitor_inbox.push(NetEvent::PeerDisconnected(host_peer));
        }
    }
    fn shutdown(&mut self) {
        let peer_id = self.box_.borrow().host_peer_id;
        self.disconnect(peer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::protocol::NetMessage;
    use uuid::Uuid;

    #[test]
    fn pair_delivers_messages_both_ways() {
        let (mut host, mut visitor) = pair(PeerId(1), PeerId(2));

        // First poll surfaces the connection events on both sides.
        let host_events = host.poll();
        assert!(matches!(host_events.as_slice(), [NetEvent::PeerConnected(PeerId(2))]));
        let visitor_events = visitor.poll();
        assert!(matches!(
            visitor_events.as_slice(),
            [NetEvent::PeerConnected(PeerId(1))]
        ));

        // Visitor → host.
        visitor.send(
            PeerId(1),
            NetMessage::Hello {
                player_id: Uuid::nil(),
                display_name: "Buddy".to_string(),
            },
        );
        let evs = host.poll();
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            NetEvent::Message {
                from: PeerId(2),
                msg: NetMessage::Hello { display_name, .. },
            } => assert_eq!(display_name, "Buddy"),
            other => panic!("unexpected event: {other:?}"),
        }

        // Host → visitor via broadcast.
        host.broadcast(
            NetMessage::Welcome {
                your_player_id: Uuid::nil(),
                host_name: "Alex".to_string(),
                spawn_x: 0.0,
                spawn_y: 0.0,
                snapshot: crate::persistence::snapshot_from_zoo(&crate::game::Zoo::new(
                    chrono::Utc::now(),
                )),
            },
            None,
        );
        let evs = visitor.poll();
        assert_eq!(evs.len(), 1);
        assert!(matches!(
            evs[0],
            NetEvent::Message { from: PeerId(1), msg: NetMessage::Welcome { .. } }
        ));
    }

    #[test]
    fn disconnect_surfaces_to_both_sides() {
        let (mut host, mut visitor) = pair(PeerId(1), PeerId(2));
        let _ = host.poll();
        let _ = visitor.poll();
        host.disconnect(PeerId(2));
        let h = host.poll();
        let v = visitor.poll();
        assert!(matches!(h.as_slice(), [NetEvent::PeerDisconnected(PeerId(2))]));
        assert!(matches!(v.as_slice(), [NetEvent::PeerDisconnected(PeerId(1))]));
    }
}
