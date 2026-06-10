//! Placed, interactive NPCs and their animation state.
//!
//! An [`Npc`] is a fixed-position character (today just the structure merchant)
//! with a base sprite and a little life of its own:
//! - an **idle vertical bob** so it reads as alive while standing around,
//! - a **scale-pop** whenever it starts/stops talking, smoothing the swap,
//! - a **`{id}_speaking` sprite** shown while its interaction panel is open.
//!
//! Interaction state changes surface as [`NpcEvent`]s rather than side effects,
//! so the app layer owns the feedback policy (pop is applied here; sound effects
//! hang off [`crate::app::GameApp::on_npc_event`] — see the TODOs there). New
//! NPCs register in [`default_npcs`] and inherit all of this for free.

use glam::{Vec2, vec2};

/// Interaction "pop" animation duration (seconds). Kept in the core so the
/// NPC sim is engine-free; the renderer has its own matching `view::POP_DURATION`.
const POP_DURATION: f32 = 0.3;

/// Idle bob speed (radians/sec).
const BOB_RATE: f32 = 2.2;

/// What kind of NPC this is — the app maps each kind to the interaction screen it
/// opens (and, in reverse, to the speaking-sprite swap). Add a variant here when
/// introducing a new placed NPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpcKind {
    /// Sells placeable structures (pedestals) into the hotbar.
    StructureMerchant,
    /// The time-windowed exotic-animal shop.
    ExoticMerchant,
    /// The expedition board — launches a bounded biome expedition (Phase 3).
    ExpeditionBoard,
}

/// A placed, interactive NPC.
#[derive(Clone)]
pub struct Npc {
    /// What this NPC does (drives which screen its interaction opens).
    pub kind: NpcKind,
    /// Stable id. Also the base sprite id in `assets/npcs/` (`{id}.png`), with an
    /// optional talking variant `{id}_speaking.png`.
    pub id: &'static str,
    /// Interaction-prompt noun shown over the NPC (e.g. "Structures").
    pub label: &'static str,
    /// Fixed world-space position (the NPC's feet).
    pub world: Vec2,
    /// Ever-advancing idle bob phase (radians).
    pub bob_phase: f32,
    /// Seconds of scale-pop animation left; >0 = popping.
    pub pop: f32,
    /// True while this NPC's interaction panel is open (drives the speaking sprite).
    pub speaking: bool,
}

/// A discrete change in an NPC's interaction state, emitted by [`Npc::set_speaking`].
/// The app maps these to feedback (scale-pop already applied; sfx to come).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpcEvent {
    /// The player just opened this NPC's panel (idle → speaking).
    StartSpeaking,
    /// The player just closed this NPC's panel (speaking → idle).
    StopSpeaking,
}

impl Npc {
    pub fn new(kind: NpcKind, id: &'static str, label: &'static str, world: Vec2) -> Self {
        // Desync each NPC's idle bob so a future crowd doesn't bob in lockstep.
        let bob_phase = (id.bytes().map(|b| b as u32).sum::<u32>() % 628) as f32 / 100.0;
        Self { kind, id, label, world, bob_phase, pop: 0.0, speaking: false }
    }

    /// Advance idle bob and decay the pop timer. Pure animation — call once/frame.
    pub fn animate(&mut self, dt: f32) {
        self.bob_phase = (self.bob_phase + dt * BOB_RATE) % std::f32::consts::TAU;
        if self.pop > 0.0 {
            self.pop = (self.pop - dt).max(0.0);
        }
    }

    /// Update the speaking state. On a change, kick the scale-pop (so the sprite
    /// swap reads as a deliberate beat) and return the [`NpcEvent`] so the caller
    /// can layer on more feedback (sfx). No change → `None`.
    pub fn set_speaking(&mut self, speaking: bool) -> Option<NpcEvent> {
        if speaking == self.speaking {
            return None;
        }
        self.speaking = speaking;
        self.pop = POP_DURATION;
        Some(if speaking { NpcEvent::StartSpeaking } else { NpcEvent::StopSpeaking })
    }

    /// The sprite id to render this frame: `{id}_speaking` while talking (the
    /// renderer falls back to the base `{id}` if that art isn't bundled), else
    /// the base `{id}`.
    pub fn sprite_id(&self) -> String {
        if self.speaking {
            format!("{}_speaking", self.id)
        } else {
            self.id.to_string()
        }
    }

    /// Current idle bob offset in pre-zoom screen px (negative = lifted up).
    pub fn bob_offset(&self) -> f32 {
        self.bob_phase.sin() * BOB_AMP
    }
}

/// Idle bob amplitude in pre-zoom screen px.
pub const BOB_AMP: f32 = 3.0;

/// Sprite id (and `assets/npcs/` filename) for the exotic merchant.
pub const EXOTIC_MERCHANT_SPRITE_ID: &str = "exotic_merchant";

/// Sprite id for the expedition board. Falls back to a primitive if no art is
/// bundled (the renderer handles missing sprites).
pub const EXPEDITION_BOARD_SPRITE_ID: &str = "expedition_board";

/// Fixed world position of the exotic merchant — inset near the top-right of the
/// plot, mirroring the structure merchant on the left.
pub fn exotic_merchant_pos(center: Vec2, half: f32) -> Vec2 {
    vec2(center.x + half * 0.55, center.y - half * 0.30)
}

/// Fixed world position of the expedition board — just outside the plot's
/// right edge, the gateway out to the biomes. Relative to the plot `center`.
pub fn expedition_board_pos(center: Vec2, half: f32) -> Vec2 {
    vec2(center.x + half * 1.25, center.y)
}

/// The NPCs placed in every world: the structure merchant and the exotic
/// merchant. Add new placed characters here and they inherit bob / pop /
/// speaking-swap automatically.
/// `center` / `half` describe the plot these NPCs belong to (a player's home
/// plot on the hub), so the roster re-bases cleanly onto any plot origin.
pub fn default_npcs(center: Vec2, half: f32) -> Vec<Npc> {
    vec![
        Npc::new(
            NpcKind::StructureMerchant,
            crate::game::merchant::MERCHANT_SPRITE_ID,
            "Structures",
            crate::game::merchant::merchant_pos(center, half),
        ),
        Npc::new(
            NpcKind::ExoticMerchant,
            EXOTIC_MERCHANT_SPRITE_ID,
            "Exotics",
            exotic_merchant_pos(center, half),
        ),
        Npc::new(
            NpcKind::ExpeditionBoard,
            EXPEDITION_BOARD_SPRITE_ID,
            "Expedition",
            expedition_board_pos(center, half),
        ),
    ]
}
