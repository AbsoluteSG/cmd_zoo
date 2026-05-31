//! In-process "fake visitor" used by the Settings → "Local co-op demo"
//! affordance. Holds the visitor side of a `loopback::pair`, sends Hello,
//! then wanders by emitting `Intent` packets on a slow timer. Lets us
//! exercise the whole host pipeline (peer accept → RemoteController →
//! multi-avatar render → broadcast) without Steam.

use macroquad::rand;
use uuid::Uuid;

use super::protocol::{NetMessage, PeerId, WireIntent};
use super::transport::{NetEvent, NetTransport};

pub struct DemoBot {
    pub player_id: Uuid,
    pub display_name: String,
    transport: Box<dyn NetTransport>,
    /// Peer-id we use to address the host (set after first PeerConnected).
    host_peer: Option<PeerId>,
    sent_hello: bool,
    /// Seconds left on the current direction; rerolled when it hits 0.
    dir_timer: f32,
    move_x: f32,
    move_y: f32,
    tick: u32,
}

impl DemoBot {
    pub fn new(transport: Box<dyn NetTransport>, display_name: impl Into<String>) -> Self {
        Self {
            player_id: Uuid::new_v4(),
            display_name: display_name.into(),
            transport,
            host_peer: None,
            sent_hello: false,
            dir_timer: 0.0,
            move_x: 0.0,
            move_y: 0.0,
            tick: 0,
        }
    }

    /// Drive the bot one frame: poll, learn the host peer, send Hello once,
    /// reroll wander direction, ship an Intent.
    pub fn tick(&mut self, dt: f32) {
        for ev in self.transport.poll() {
            match ev {
                NetEvent::PeerConnected(p) => {
                    self.host_peer = Some(p);
                }
                NetEvent::PeerDisconnected(_) => {
                    self.host_peer = None;
                }
                NetEvent::Message { .. } => { /* ignore inbound, this is a movement-only bot */ }
            }
        }
        let Some(host) = self.host_peer else { return };
        if !self.sent_hello {
            self.sent_hello = true;
            self.transport.send(
                host,
                NetMessage::Hello {
                    player_id: self.player_id,
                    display_name: self.display_name.clone(),
                },
            );
        }
        self.dir_timer -= dt;
        if self.dir_timer <= 0.0 {
            self.dir_timer = rand::gen_range(1.5, 3.5);
            // Random unit-ish vector, occasionally idle.
            if rand::gen_range(0u32, 4) == 0 {
                self.move_x = 0.0;
                self.move_y = 0.0;
            } else {
                let a = rand::gen_range(0.0_f32, std::f32::consts::TAU);
                self.move_x = a.cos();
                self.move_y = a.sin();
            }
        }
        self.tick = self.tick.wrapping_add(1);
        self.transport.send(
            host,
            NetMessage::Intent(WireIntent {
                tick: self.tick,
                move_x: self.move_x,
                move_y: self.move_y,
                actions: 0,
            }),
        );
    }

    pub fn shutdown(&mut self) {
        if let Some(host) = self.host_peer {
            self.transport.send(host, NetMessage::Goodbye);
        }
        self.transport.shutdown();
    }
}
