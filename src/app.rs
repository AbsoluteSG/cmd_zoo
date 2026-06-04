//! Macroquad app shell. Owns the `Zoo`, the file repository, the camera, the
//! texture cache, and the roaming critters; drives the per-frame
//! load/advance/save loop and translates input into camera moves.
//!
//! All game rules live in `crate::game`; this is glue + presentation.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Context;
use chrono::{DateTime, Utc};
use macroquad::prelude::*;
use uuid::Uuid;

use std::collections::HashMap;

use crate::audio::Sounds;
use crate::catching::CatchState;
use crate::game::avatar_system::{self, Behavior};
use crate::game::{Zoo, economy, species};
use crate::game::wild_animal::AiHit;
use crate::game::world_chunks::{WorldChunks, WORLD_W, WORLD_H};
use crate::input::{AvatarController, ControllerCtx, KeyboardController, RemoteController};
use crate::net::Session;
use crate::persistence::json_file::JsonFileRepository;
use crate::render::particles::Particles;
use crate::render::textures::Textures;
use crate::render::view::{self, Camera, CRITTER_H, POP_DURATION};
use crate::render::{menus, world};

/// Which screen owns the input/overlay this frame. `World` is the live critter
/// scene; the others are menu overlays.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    World,
    Shop,
    Settings,
    Waypoints,
    /// Physical breeding-nest panel; the target nest is in `GameApp::active_nest`.
    Nest,
    /// Food-structure panel; the target structure is in `GameApp::active_structure`.
    Structure,
}

/// An interactive ground pad reachable with E: a breeding nest (top row) or a
/// food structure (bottom row), identified by its slot index.
#[derive(Clone, Copy)]
enum Pad {
    Nest(usize),
    Structure(usize),
}

/// Full-screen post-process filter applied to the world (UI stays crisp).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PostEffect {
    None,
    Scanline,
    Pixelate,
    Grayscale,
    Sepia,
}

impl PostEffect {
    pub const ALL: [PostEffect; 5] = [
        PostEffect::None,
        PostEffect::Scanline,
        PostEffect::Pixelate,
        PostEffect::Grayscale,
        PostEffect::Sepia,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PostEffect::None => "None",
            PostEffect::Scanline => "Scanlines",
            PostEffect::Pixelate => "Pixelate",
            PostEffect::Grayscale => "Grayscale",
            PostEffect::Sepia => "Sepia",
        }
    }

    /// Shader selector passed as a float uniform.
    fn code(self) -> f32 {
        match self {
            PostEffect::None => 0.0,
            PostEffect::Scanline => 1.0,
            PostEffect::Pixelate => 2.0,
            PostEffect::Grayscale => 3.0,
            PostEffect::Sepia => 4.0,
        }
    }
}

/// The on-screen presence of a domain `Animal`: it wanders freely across the
/// ground plane (position is render-only, in flat world units) while all of its
/// economy — income rate, storage cap, accrued amount, currency — lives in the
/// backing `Zoo` animal, keyed by `animal_id`.
pub struct Critter {
    pub animal_id: Uuid,
    pub species: &'static str,
    pub pos: Vec2,
    pub target: Vec2,
    pub speed: f32,
    /// Last nonzero movement direction. The art faces **left** by default, so
    /// the renderer mirrors X when moving right.
    pub dir: Vec2,
    /// Seconds remaining standing still (between/within walks). >0 = idling.
    pub idle_timer: f32,
    /// Seconds remaining of the redeem scale-pop animation. >0 = popping.
    pub pop: f32,
    /// Carried velocity, used only while being dragged on the follow "chain"
    /// so the pull reads as weighty momentum rather than rigid tracking.
    pub vel: Vec2,
}

impl Critter {
    fn new(animal_id: Uuid, species: &'static str, pos: Vec2) -> Self {
        Self {
            animal_id,
            species,
            pos,
            target: random_zoo_point(),
            speed: 80.0,
            dir: vec2(-1.0, 1.0),
            idle_timer: rand::gen_range(0.0, 2.0),
            pop: 0.0,
            vel: vec2(0.0, 0.0),
        }
    }

    /// Wander with random pauses; retarget (and idle) on arrival. Income accrual
    /// is derived from the domain animal's `last_collected_at`, not tracked here.
    fn update(&mut self, dt: f32) {
        if self.pop > 0.0 {
            self.pop = (self.pop - dt).max(0.0);
        }
        if self.idle_timer > 0.0 {
            self.idle_timer = (self.idle_timer - dt).max(0.0);
            return; // standing still
        }
        let to = self.target - self.pos;
        let dist = to.length();
        if dist < 4.0 {
            // Arrived: pause a moment, then head somewhere new (within zoo).
            self.target = random_zoo_point();
            self.idle_timer = rand::gen_range(0.4, 2.5);
            return;
        }
        // Occasional spontaneous pause mid-walk.
        if rand::gen_range(0.0, 1.0) < 0.004 {
            self.idle_timer = rand::gen_range(0.4, 1.8);
            return;
        }
        let dir = to / dist;
        self.dir = dir;
        self.pos += dir * self.speed * dt;
    }

    /// Steer straight toward `dest` (used while parked at a nest). Stops cleanly
    /// once close to avoid jitter.
    fn move_toward(&mut self, dest: Vec2, dt: f32) {
        if self.pop > 0.0 {
            self.pop = (self.pop - dt).max(0.0);
        }
        let to = dest - self.pos;
        let dist = to.length();
        if dist < 4.0 {
            return;
        }
        let dir = to / dist;
        self.dir = dir;
        self.pos += dir * (self.speed * 1.2).min(dist / dt) * dt;
    }

    /// Get dragged toward `anchor` as if on an elastic chain. There's slack
    /// (no pull when close), the pull ramps up the more taut the chain gets,
    /// and carried momentum + damping make it feel like hauling weight rather
    /// than a sprite locked to the avatar.
    fn follow_pull(&mut self, anchor: Vec2, dt: f32) {
        if self.pop > 0.0 {
            self.pop = (self.pop - dt).max(0.0);
        }
        const SLACK: f32 = 38.0; // chain rest length — no pull within this
        const STIFFNESS: f32 = 11.0; // accel per unit of tautness
        const MAX_TAUT: f32 = 240.0; // clamp so a big yank doesn't explode
        const MAX_SPEED: f32 = 360.0;

        let to = anchor - self.pos;
        let dist = to.length();
        if dist > SLACK {
            let dir = to / dist;
            let taut = (dist - SLACK).min(MAX_TAUT);
            self.vel += dir * (taut * STIFFNESS) * dt;
            self.dir = dir;
        }
        // Velocity damping: settles the bob when you stop, keeps it from
        // orbiting the anchor. Frame-rate independent.
        let damp = (-7.0 * dt).exp();
        self.vel *= damp;
        let speed = self.vel.length();
        if speed > MAX_SPEED {
            self.vel *= MAX_SPEED / speed;
        }
        self.pos += self.vel * dt;
    }
}

/// Build a critter for every animal in the zoo, scattered within the zoo zone.
fn critters_from_zoo(zoo: &Zoo) -> Vec<Critter> {
    zoo.animals
        .values()
        .map(|a| Critter::new(a.id, a.species, random_zoo_point()))
        .collect()
}

/// A uniformly random point inside the enclosed 9×9 zoo plot, inset slightly
/// from the fence. Tame critters wander here; wild animals never spawn here.
fn random_zoo_point() -> Vec2 {
    let c = crate::game::world_chunks::zoo_center();
    let half = (crate::game::world_chunks::zoo_half_extent() - 48.0).max(0.0);
    c + vec2(rand::gen_range(-half, half), rand::gen_range(-half, half))
}

/// Which texture a notification shows on its left edge.
#[derive(Clone, Copy)]
pub enum NotifIcon {
    /// A currency icon from the icon table ("coin", "dna_helix").
    Currency(&'static str),
    /// A species sprite, looked up in the animal table.
    Animal(&'static str),
}

/// A transient toast shown on the right edge of the screen when the player
/// obtains or collects something. Fades in, holds, then fades out.
pub struct Notification {
    /// Primary label (e.g. "Coins", an animal's display name).
    pub title: String,
    /// Secondary label (e.g. "+250", "Captured!").
    pub amount: String,
    pub icon: NotifIcon,
    /// `get_time()` when the toast was created — drives the fade tween.
    pub created_at: f64,
}

/// Total lifetime of a notification toast, in seconds (fade-in + hold + fade-out).
pub const NOTIF_LIFETIME: f64 = 3.6;
/// Fade-in duration (seconds).
pub const NOTIF_FADE_IN: f64 = 0.30;
/// Fade-out duration (seconds), applied at the end of the lifetime.
pub const NOTIF_FADE_OUT: f64 = 0.6;

/// A timed throw-impact telegraph dropped by a Thrower: a shrinking red circle
/// on the ground that staggers + knocks back the player if they're still inside
/// it when it lands.
pub struct DangerZone {
    /// World-space impact centre (the player's position at throw time).
    pub center: Vec2,
    /// Total fuse time (seconds) — drives the shrink animation.
    pub fuse: f32,
    /// Seconds remaining until impact.
    pub remaining: f32,
    /// World-units impact radius.
    pub radius: f32,
}

/// Open inspect-panel state: which owned animal is being inspected, which side
/// the panel slides in from, and the slide-in progress.
pub struct InspectState {
    pub animal_id: Uuid,
    /// True → panel slides from the right edge (player is on the left), else left.
    pub from_right: bool,
    /// Slide-in progress 0→1, eased each frame.
    pub t: f32,
}

/// World-units reach within which pressing E inspects the nearest owned animal.
pub const INTERACT_RANGE: f32 = 170.0;

/// Most animals that can trail the avatar on the follow chain at once.
pub const MAX_FOLLOWERS: usize = 10;

pub struct GameApp {
    pub zoo: Zoo,
    pub repo: Arc<JsonFileRepository>,
    pub last_modtime: SystemTime,
    pub camera: Camera,
    pub textures: Textures,
    pub sounds: Sounds,
    pub critters: Vec<Critter>,
    /// Chunk-streamed wild world.  Owns all wild animal state and manages
    /// load / cull / cache based on the player's position.
    pub world: WorldChunks,
    /// Catch-mode state (C key toggle, fill progress, hover target).
    pub catch_state: CatchState,
    /// Active overlay screen (World / Shop / Breeding).
    pub screen: Screen,
    /// Menu open/close animation (0 = closed, 1 = open), eased each frame.
    pub menu_t: f32,
    /// The menu being rendered — lags `screen` during the close tween.
    pub shown_menu: Screen,
    /// Offscreen target the world scene is rendered into for post-processing
    /// (menu blur and/or fullscreen effects). Recreated on resize.
    scene_rt: Option<RenderTarget>,
    /// Active fullscreen filter applied to the world.
    pub effect: PostEffect,
    /// Post-process material (built once); None if shader compilation failed.
    post: Option<Material>,
    /// Transient status line: (text, set-at via `get_time()`), cleared after 4s.
    pub status: Option<(String, f64)>,
    /// Persistent error log: red messages shown at the bottom-left, expire after 8s.
    pub errors: VecDeque<(String, f64)>,
    /// All in-world avatars (host + visitors) and the active net transport.
    /// In M2 single-player this is `Session::solo`; toggling "Open to online"
    /// promotes it to `Host`; joining a friend's zoo replaces it with `Visit`.
    pub session: Session,
    /// Local controller (keyboard). Boxed for symmetry with remote
    /// controllers; future gamepad support drops in here.
    controller: Box<dyn AvatarController>,
    /// Per-visitor controllers, keyed by the visitor's stable `player_id`.
    /// The host pushes inbound `WireIntent`s into the matching entry each
    /// frame; each visitor's avatar samples from its own controller.
    remotes: HashMap<Uuid, RemoteController>,
    /// Ordered behavior chain applied per tick. Add `DashBehavior` etc. by
    /// extending this list — no movement rewrite required.
    behaviors: Vec<Box<dyn Behavior>>,
    /// Seconds since the host last broadcast a full ZooSnapshot. Throttled
    /// to ~`SNAPSHOT_BROADCAST_INTERVAL` so visitors see currency/animal
    /// changes without flooding the wire on every frame.
    snapshot_broadcast_t: f32,
    /// Currently-being-typed join code in the Settings join-friend field.
    /// 6 chars max; uppercase Crockford base32 (matches `JoinCode::random`).
    pub join_code_buffer: String,
    /// Right-edge toast queue for obtain/collect events. Oldest first.
    pub notifications: Vec<Notification>,
    /// Remaining hitstop (seconds): gameplay motion freezes while >0. Triggered
    /// when a Basher connects a charge with the player.
    hitstop: f32,
    /// Remaining camera-shake (seconds), decaying. Drives a per-frame jitter
    /// added to the camera offset after the follow lerp.
    camera_shake: f32,
    /// Pooled, texture-free pixel particles for game-feel bursts (income,
    /// captures, hits, births). Drawn in the world scene layer.
    pub particles: Particles,
    /// Active throw-impact telegraphs from Throwers, drawn + resolved each frame.
    pub danger_zones: Vec<DangerZone>,
    /// Remaining venom screen-effect time (seconds): red vignette + brief blur.
    pub venom_fx: f32,
    /// Active animal-inspect side panel, if any. The inspected critter is frozen
    /// in place while this is open.
    pub inspect: Option<InspectState>,
    /// Owned animals currently following the avatar (toggled from the inspect
    /// panel's Follow button). Order is the chain order — the first trails the
    /// avatar, each subsequent one trails the animal ahead of it. Capped at
    /// [`MAX_FOLLOWERS`]. An animal leaves the chain once deposited or released.
    pub following: Vec<Uuid>,
    /// Nest whose panel (`Screen::Nest`) is open, if any.
    pub active_nest: Option<Uuid>,
    /// When `Some(nest_id)`, the spotlight deposit view is active: the world is
    /// dimmed and the player clicks one of their following animals to drop it
    /// into that nest. Escape exits.
    pub depositing: Option<Uuid>,
    /// Food structure whose panel (`Screen::Structure`) is open, if any.
    pub active_structure: Option<Uuid>,
    /// Biome-preview debug mode: free-fly camera that paints only the biome
    /// colour field (no critters, plot, structures, or HUD chrome) and allows
    /// zooming far past the gameplay limit. Toggled with F3. Used to record
    /// clean biome-layout showcases.
    pub debug_biome: bool,
}

/// Far-out zoom floor allowed only in biome-debug mode, so the whole 500k
/// world can fit on screen (the gameplay floor is 0.3).
const DEBUG_ZOOM_MIN: f32 = 0.0012;

/// Hitstop duration applied when a Basher lands a hit.
const BASH_HITSTOP: f32 = 0.12;
/// Camera-shake duration applied when a Basher lands a hit.
const BASH_SHAKE: f32 = 0.35;

/// Fuse (seconds) on a Thrower's object before it lands.
const THROW_FUSE: f32 = 1.1;
/// Impact radius (world units) of a thrown object — stand outside this to dodge.
const THROW_RADIUS: f32 = 95.0;
/// Camera-shake duration applied when a throw connects.
const THROW_SHAKE: f32 = 0.3;
/// Random-vector knockback impulse (world units/s) added to the avatar on a
/// throw hit; the avatar's accel-damped motion bleeds it off over ~0.4s.
const KNOCKBACK_SPEED: f32 = 520.0;
/// Duration (seconds) of the venom screen effect (red vignette + brief blur).
const VENOM_FX_DURATION: f32 = 0.55;
/// Peak camera-shake amplitude in screen pixels.
const SHAKE_AMPLITUDE: f32 = 14.0;

/// How often (seconds) the host re-broadcasts a full ZooSnapshot to visitors.
/// Two seconds is fast enough that purchases feel live without saturating
/// the loopback / Steam relay bandwidth budget.
const SNAPSHOT_BROADCAST_INTERVAL: f32 = 2.0;

impl GameApp {
    pub fn new(zoo: Zoo, repo: Arc<JsonFileRepository>, last_modtime: SystemTime) -> Self {
        let critters = critters_from_zoo(&zoo);
        let spawn = vec2(WORLD_W * 0.5, WORLD_H * 0.5);
        let session = Session::solo(zoo.player.id, spawn);
        let mut camera = default_camera();
        camera.snap_to(spawn, vec2(screen_width(), screen_height()));
        // Seed the wild world from the save (regenerated on the fly; only the
        // chunk deltas come from disk).
        let world = WorldChunks::new(zoo.world_seed, zoo.chunk_deltas.clone());
        Self {
            zoo,
            repo,
            last_modtime,
            camera,
            textures: Textures::new(),
            sounds: Sounds::default(),
            critters,
            world,
            catch_state: CatchState::default(),
            screen: Screen::World,
            menu_t: 0.0,
            shown_menu: Screen::World,
            scene_rt: None,
            effect: PostEffect::None,
            post: build_post_material(),
            status: None,
            errors: VecDeque::new(),
            session,
            controller: Box::new(KeyboardController::default()),
            remotes: HashMap::new(),
            behaviors: avatar_system::default_behaviors(),
            snapshot_broadcast_t: 0.0,
            join_code_buffer: String::new(),
            notifications: Vec::new(),
            hitstop: 0.0,
            camera_shake: 0.0,
            particles: Particles::new(),
            danger_zones: Vec::new(),
            venom_fx: 0.0,
            inspect: None,
            following: Vec::new(),
            active_nest: None,
            depositing: None,
            active_structure: None,
            debug_biome: false,
        }
    }

    /// Attempt to join a friend's hosted zoo by `code`. Today this requires
    /// the `steam` feature; without it we surface an actionable error in the
    /// status line and leave the UI state untouched. Returns true on success.
    pub fn try_join_by_code(&mut self, code: &str) -> bool {
        let code = code.trim();
        if code.len() != 6 {
            self.set_status("join code must be 6 chars");
            return false;
        }
        #[cfg(feature = "steam")]
        {
            use crate::net::steam::SteamTransport;
            match SteamTransport::join(code) {
                Ok(t) => {
                    // Wholesale replace the session as a visitor. Local zoo
                    // becomes a scratch view that the Welcome snapshot will
                    // overwrite shortly. Our local save on disk is untouched.
                    let local_pid = self.zoo.player.id;
                    self.session = crate::net::Session::visit(
                        local_pid,
                        Box::new(t),
                        crate::net::protocol::PeerId(0), // host peer set on first event
                    );
                    self.remotes.clear();
                    self.set_status(format!("joining {code}…"));
                    true
                }
                Err(e) => {
                    self.set_status(format!("join failed: {e}"));
                    false
                }
            }
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = code;
            self.set_status("join needs --features steam (relay transport not built)");
            false
        }
    }

    /// Visitor-side: send a gift of `species` at `level` to the host. No-op
    /// when not in a Visiting session.
    pub fn drop_gift(&mut self, species: &str, level: u8) {
        let crate::net::SessionRole::Visiting { host_peer, .. } = self.session.role else {
            return;
        };
        let Some(t) = self.session.transport.as_mut() else {
            return;
        };
        t.send(
            host_peer,
            crate::net::NetMessage::DropGift {
                species: species.to_string(),
                level,
            },
        );
        self.set_status(format!("gifted a {species} to the host"));
    }

    /// Host-side: claim a gift sitting in a visitor's inbox into our own
    /// zoo. Spawns a freeform animal of the gifted species and removes the
    /// inbox entry. Triggers a snapshot rebroadcast so the visitor sees it
    /// removed.
    pub fn claim_gift(&mut self, visitor_id: Uuid, gift_id: Uuid, now: DateTime<Utc>) {
        let Some(rec) = self.zoo.visitors.get_mut(&visitor_id) else {
            return;
        };
        let Some(pos) = rec.gift_inbox.iter().position(|g| g.id == gift_id) else {
            return;
        };
        let gift = rec.gift_inbox.remove(pos);
        // Spawn the animal in the host's zoo. spawn_animal_freeform places
        // it loose in the world; same path used by the exotic shop.
        match self.zoo.spawn_animal_freeform(gift.species, gift.level, now) {
            Ok(_) => {
                self.sync_critters();
                self.save_under_lock(now);
                let name = species::get(gift.species).display_name;
                self.push_notification(
                    name,
                    format!("Gift · L{}", gift.level),
                    NotifIcon::Animal(gift.species),
                );
                // Force a snapshot push so visitors see the updated inbox.
                self.snapshot_broadcast_t = SNAPSHOT_BROADCAST_INTERVAL;
            }
            Err(e) => {
                // Put it back if we couldn't claim it.
                if let Some(rec) = self.zoo.visitors.get_mut(&visitor_id) {
                    rec.gift_inbox.insert(pos, gift);
                }
                self.set_status(format!("can't claim: {e}"));
            }
        }
    }

    /// Drain any chars typed this frame into `join_code_buffer`. Filters to
    /// the Crockford base32 alphabet (uppercased) and caps at 6. Backspace
    /// removes the last character. Called from the Settings panel each frame.
    pub fn pump_join_code_input(&mut self) {
        while let Some(c) = macroquad::input::get_char_pressed() {
            if is_key_pressed(KeyCode::Backspace) {
                // Backspace surfaces as both a char and a key; we handle below.
            }
            let up = c.to_ascii_uppercase();
            if "0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(up)
                && self.join_code_buffer.len() < 6
            {
                self.join_code_buffer.push(up);
            }
        }
        if is_key_pressed(KeyCode::Backspace) {
            self.join_code_buffer.pop();
        }
    }

    /// True when the session is hosting (Settings UI uses this for the toggle).
    pub fn is_hosting(&self) -> bool {
        matches!(self.session.role, crate::net::SessionRole::Host { .. })
    }

    /// Open the zoo for online visitors via the Steam relay (app ID 480).
    /// No-ops when already hosting or when the `steam` feature is not compiled in.
    pub fn start_hosting(&mut self) {
        if self.is_hosting() { return; }
        #[cfg(feature = "steam")]
        {
            use crate::net::steam::SteamTransport;
            use crate::net::protocol::JoinCode;
            let code = JoinCode::random();
            match SteamTransport::host(code.as_str()) {
                Ok(t) => {
                    self.session.become_host(Box::new(t), code);
                    self.set_status("Zoo open — share your 6-char code with a friend");
                }
                Err(e) => {
                    self.set_status(format!("Steam hosting failed: {e}"));
                }
            }
        }
        #[cfg(not(feature = "steam"))]
        {
            self.set_status("Build with --features steam to host online");
        }
    }

    /// Tear down hosting and return to solo.
    pub fn stop_hosting(&mut self) {
        self.session.end_hosting();
        self.remotes.clear();
        self.set_status("Hosting stopped");
    }

    /// Reconcile `critters` against `zoo.animals`: add a critter for any new
    /// animal, drop critters whose animal is gone. Existing critter positions
    /// are preserved (unlike a full rebuild).
    pub fn sync_critters(&mut self) {
        self.critters
            .retain(|c| self.zoo.animals.contains_key(&c.animal_id));
        let known: std::collections::HashSet<Uuid> =
            self.critters.iter().map(|c| c.animal_id).collect();
        for a in self.zoo.animals.values() {
            if !known.contains(&a.id) {
                self.critters
                    .push(Critter::new(a.id, a.species, random_zoo_point()));
            }
        }
    }

    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some((text.into(), get_time()));
    }

    /// Queue a right-edge toast for an obtain/collect event.
    pub fn push_notification(
        &mut self,
        title: impl Into<String>,
        amount: impl Into<String>,
        icon: NotifIcon,
    ) {
        self.notifications.push(Notification {
            title: title.into(),
            amount: amount.into(),
            icon,
            created_at: get_time(),
        });
        // Bound the queue so a flurry of events can't grow it without limit.
        while self.notifications.len() > 6 {
            self.notifications.remove(0);
        }
    }

    /// Push a red error line to the persistent error log (max 5 visible at once).
    pub fn log_error(&mut self, text: impl Into<String>) {
        self.errors.push_back((text.into(), get_time()));
        while self.errors.len() > 5 {
            self.errors.pop_front();
        }
    }

    fn clear_stale_status(&mut self) {
        if let Some((_, at)) = &self.status {
            if get_time() - *at >= 4.0 {
                self.status = None;
            }
        }
        let now = get_time();
        self.errors.retain(|(_, t)| now - *t < 8.0);
        self.notifications
            .retain(|n| now - n.created_at < NOTIF_LIFETIME);
    }

    /// One simulation step: pick up external writes, advance breeding, persist
    /// completions. Ported from the egui `EguiApp::tick` critical section.
    pub fn tick(&mut self, now: DateTime<Utc>) {
        self.clear_stale_status();
        let repo = self.repo.clone();
        let access = match repo.lock().context("tick lock") {
            Ok(a) => a,
            Err(_) => return,
        };
        if let Ok(Some((zoo, mtime, warnings))) = access.load_if_newer(self.last_modtime) {
            self.zoo = zoo;
            self.last_modtime = mtime;
            // An external write may have added/removed animals — reconcile the
            // on-screen critters, keeping existing positions stable.
            self.sync_critters();
            if let Some(first) = warnings.first() {
                for w in &warnings {
                    eprintln!("reload warning: {w}");
                }
                self.set_status(first.clone());
            }
        }
        economy::advance(&mut self.zoo, now);

        // Auto-complete any nests whose gestation finished: parents are released
        // and an offspring is left in the nest for the player to collect.
        let hatched = self.zoo.advance_nests(now);
        if !hatched.is_empty() {
            self.sync_critters();
            self.sync_world_to_zoo();
            for (idx, _species) in &hatched {
                // Birth sparkle at the nest where it hatched.
                self.particles.birth(crate::game::zoo::Zoo::nest_pos(*idx));
            }
            self.set_status(if hatched.len() == 1 {
                "An egg hatched — collect it from the nest!".to_string()
            } else {
                format!("{} eggs hatched — collect them!", hatched.len())
            });
            self.zoo.last_saved_at = now;
            if let Ok(mt) = access.save(&self.zoo) {
                self.last_modtime = mt;
            }
        }
    }


    /// Menu toggles + (when no menu is open) camera input + click-to-redeem,
    /// plus critter wandering (which continues behind menus).
    pub fn handle_input(&mut self, now: DateTime<Utc>) {
        // F3 toggles the biome-preview debug mode (free-fly, biomes only).
        if is_key_pressed(KeyCode::F3) {
            self.debug_biome = !self.debug_biome;
            if self.debug_biome {
                self.set_status("Biome debug — R reseed · wheel zoom · WASD pan · F3 exit");
            } else {
                // Restore a gameplay zoom and recenter on the avatar.
                self.camera.zoom = self.camera.zoom.max(0.3);
                let p = self.session.my_avatar().pos;
                self.camera.snap_to(p, vec2(screen_width(), screen_height()));
                self.set_status("Biome debug off");
            }
        }
        if self.debug_biome {
            self.handle_debug_input();
            return;
        }

        let mp = mouse_position();
        let mouse = vec2(mp.0, mp.1);

        // Spotlight deposit view is fully modal: only hover/click-to-select an
        // animal and Escape work; everything else is suppressed.
        let deposit_mode = self.depositing.is_some();
        if deposit_mode {
            if is_key_pressed(KeyCode::Escape) {
                self.depositing = None;
            } else if is_mouse_button_pressed(MouseButton::Left) {
                self.try_deposit_select(now);
            }
        }

        // Menu toggles: 1 = Shop, Esc = close. Breeding lives in physical nests.
        if !deposit_mode && is_key_pressed(KeyCode::Key1) {
            self.toggle_screen(Screen::Shop);
        }
        if !deposit_mode && is_key_pressed(KeyCode::Key3) {
            self.toggle_screen(Screen::Settings);
        }
        if !deposit_mode && is_key_pressed(KeyCode::Key4) {
            self.toggle_screen(Screen::Waypoints);
        }
        if !deposit_mode && is_key_pressed(KeyCode::Escape) {
            self.set_screen(Screen::World);
            self.inspect = None;
        }

        // E opens the nearest pad's panel (breeding nest along the top, food
        // structure along the bottom), otherwise inspects the nearest owned
        // animal (only when no menu is open).
        if !deposit_mode && is_key_pressed(KeyCode::E) && self.menu_t < 0.02 {
            let pad = if self.inspect.is_none() { self.nearest_pad() } else { None };
            match pad {
                Some(Pad::Nest(i)) if i < self.zoo.nest_count as usize => {
                    self.active_nest = Some(self.zoo.nests[i].id);
                    self.set_screen(Screen::Nest);
                }
                Some(Pad::Nest(i)) if i == self.zoo.nest_count as usize => self.try_buy_nest(now),
                Some(Pad::Nest(_)) => self.set_status("unlock the nearer nests first"),
                Some(Pad::Structure(i)) if i < self.zoo.structures.len() => {
                    self.active_structure = Some(self.zoo.structures[i].id);
                    self.set_screen(Screen::Structure);
                }
                Some(Pad::Structure(i)) if i == self.zoo.structures.len() => {
                    self.try_buy_food_structure(now)
                }
                Some(Pad::Structure(_)) => self.set_status("unlock the nearer ones first"),
                None => self.toggle_inspect(),
            }
        }

        // C toggles catch mode (only when no menu is open).
        if !deposit_mode && is_key_pressed(KeyCode::C) && self.menu_t < 0.02 {
            self.catch_state.toggle();
            if self.catch_state.active {
                self.set_status("Catch mode — hover over a wild animal");
            } else {
                self.set_status("Catch mode off");
            }
        }

        // Ease the open/close animation; remember which menu to keep drawing
        // while it tweens closed.
        if self.screen != Screen::World {
            self.shown_menu = self.screen;
        }
        let target = if self.screen == Screen::World { 0.0 } else { 1.0 };
        self.menu_t += (target - self.menu_t) * (get_frame_time() * 14.0).min(1.0);
        if self.menu_t < 0.001 {
            self.menu_t = 0.0;
        }

        let dt = get_frame_time();
        // Treat the deposit spotlight like an open menu: it freezes avatar
        // movement, click-to-redeem, and zoom.
        let menu_open = self.menu_t >= 0.02 || deposit_mode;

        // Advance cosmetic particles every frame — they keep animating behind
        // menus and through hitstop, like the wandering critters do.
        self.particles.update(dt);

        // Inspect panel: close if its animal is gone; otherwise ease the slide-in.
        if let Some(ins) = &self.inspect {
            if !self.zoo.animals.contains_key(&ins.animal_id) {
                self.inspect = None;
            }
        }
        if let Some(ins) = &mut self.inspect {
            ins.t += (1.0 - ins.t) * (dt * 14.0).min(1.0);
        }

        // Hitstop: a brief gameplay freeze after a Basher connects. Motion
        // (avatars + wild AI + catch progress) pauses while this ticks down;
        // streaming, net, and rendering keep running.
        if self.hitstop > 0.0 {
            self.hitstop -= dt;
        }
        let frozen = self.hitstop > 0.0;

        if !menu_open {
            // Left-click redeems a critter's accrued income (mouse stays the
            // UI / world-targeting pointer; the avatar is the in-world presence).
            if is_mouse_button_pressed(MouseButton::Left) {
                self.try_redeem_at(mouse, now);
            }
            // Cursor-centric zoom; the follow-lerp re-centers on the avatar.
            let (_, wheel_y) = mouse_wheel();
            if wheel_y != 0.0 {
                let old = self.camera.zoom;
                let factor = if wheel_y > 0.0 { 1.1 } else { 1.0 / 1.1 };
                let new = (old * factor).clamp(0.3, 3.0);
                if new != old {
                    self.camera.offset = mouse - (mouse - self.camera.offset) * (new / old);
                    self.camera.zoom = new;
                }
            }
        }

        // 0. Stream world chunks around the player's current position.
        let player_pos = self.session.my_avatar().pos;
        self.world.update(player_pos);

        // 1. Pump the net transport (no-op in Solo). Inbound visitor intents
        //    are surfaced keyed by their player_id; push them into the matching
        //    RemoteController so the avatar pipeline reads identical shape
        //    regardless of source.
        let inbound = self.session.pump(&mut self.zoo);
        for (pid, wi) in inbound {
            self.remotes.entry(pid).or_default().set_intent(wi);
        }
        // Drop controllers for peers that disconnected (their avatar is gone).
        let live_avatar_ids: std::collections::HashSet<Uuid> =
            self.session.avatars.keys().copied().collect();
        self.remotes.retain(|id, _| live_avatar_ids.contains(id));

        // 2. Sample the local controller. Visitor side ships this intent
        //    upstream; host side just runs it locally.
        let local_intent = {
            let ctx = ControllerCtx {
                dt,
                avatar: self.session.my_avatar(),
                menu_open,
            };
            self.controller.sample(&ctx)
        };

        // 3. If we're visiting, ship the local intent to the host.
        if let crate::net::SessionRole::Visiting { host_peer, .. } = self.session.role {
            if let Some(t) = self.session.transport.as_mut() {
                self.session.tick = self.session.tick.wrapping_add(1);
                t.send(
                    host_peer,
                    crate::net::NetMessage::Intent(crate::net::protocol::WireIntent {
                        tick: self.session.tick,
                        move_x: local_intent.move_dir.x,
                        move_y: local_intent.move_dir.y,
                        actions: local_intent.actions.0,
                    }),
                );
            }
        }

        // 4. Step every avatar in the session with the appropriate intent.
        //    Local avatar uses local_intent; visitor avatars use whatever
        //    their RemoteController last received. We collect (id, intent)
        //    first so we can mutably iterate avatars after.
        let local_id = self.session.local_player_id;
        let world = avatar_system::World {
            habitats: &self.zoo.habitats,
        };
        let mut intents: Vec<(Uuid, crate::input::ControllerIntent)> = Vec::new();
        for id in self.session.avatars.keys().copied().collect::<Vec<_>>() {
            if id == local_id {
                intents.push((id, local_intent));
            } else if let Some(rc) = self.remotes.get_mut(&id) {
                let ctx = ControllerCtx {
                    dt,
                    avatar: &self.session.avatars[&id],
                    menu_open: false, // remote intents are not menu-gated locally
                };
                intents.push((id, rc.sample(&ctx)));
            } else {
                intents.push((id, crate::input::ControllerIntent::default()));
            }
        }
        for (id, intent) in intents {
            if let Some(a) = self.session.avatars.get_mut(&id) {
                // During hitstop the local avatar is staggered and holds still;
                // remote avatars keep stepping so the session stays in sync.
                if frozen && id == local_id {
                    continue;
                }
                avatar_system::step(a, &intent, &world, dt, &self.behaviors);
            }
        }

        // Sprint dust: trail behind the local avatar while sprinting and moving
        // (but not mid-dash, which has its own after-image trail).
        if !frozen
            && local_intent.actions.contains(crate::input::ActionFlags::SPRINT)
            && local_intent.move_dir.length_squared() > 1e-4
        {
            let me = self.session.my_avatar();
            if !me.is_dashing() && me.vel.length_squared() > 100.0 {
                self.particles.sprint_trail(me.pos);
            }
        }

        // 5. Host: broadcast new poses each frame, full snapshot on cadence.
        if matches!(self.session.role, crate::net::SessionRole::Host { .. }) {
            self.session.broadcast_avatars();
            self.snapshot_broadcast_t += dt;
            if self.snapshot_broadcast_t >= SNAPSHOT_BROADCAST_INTERVAL {
                self.snapshot_broadcast_t = 0.0;
                self.session.broadcast_world_snapshot(&self.zoo);
            }
        }

        // Visitor: a recent snapshot may have replaced our local zoo —
        // reconcile critters so render matches state.
        if matches!(self.session.role, crate::net::SessionRole::Visiting { .. }) {
            self.sync_critters();
        }

        // 6. Camera tracks the local avatar.
        self.camera.follow(
            self.session.my_avatar().pos,
            vec2(screen_width(), screen_height()),
            dt,
            8.0,
        );

        // Camera shake: decay and add a per-frame jitter scaled by how much
        // shake remains, so it tapers off smoothly.
        if self.camera_shake > 0.0 {
            self.camera_shake = (self.camera_shake - dt).max(0.0);
            let mag = SHAKE_AMPLITUDE * (self.camera_shake / BASH_SHAKE);
            self.camera.offset += vec2(
                rand::gen_range(-mag, mag),
                rand::gen_range(-mag, mag),
            );
        }

        // Critters wander, except: the inspected one is frozen in place, nested
        // ones are parked at their nest, and followers trail in a chain.
        let locked_id = self.inspect.as_ref().map(|i| i.animal_id);
        let parked = self.zoo.nested_animal_positions();
        let avatar_pos = self.session.my_avatar().pos;
        // First pass: everything that isn't currently following the avatar.
        for c in &mut self.critters {
            if Some(c.animal_id) == locked_id || self.following.contains(&c.animal_id) {
                continue;
            }
            if let Some(&dest) = parked.get(&c.animal_id) {
                c.move_toward(dest, dt);
            } else {
                c.update(dt);
            }
        }
        // Chain pass: each follower is dragged on an elastic leash anchored to
        // the one ahead of it (the head trails the avatar), so the line lags
        // and whips around with real momentum rather than tracking rigidly.
        // Paused during deposit mode, where followers are held in a laid-out row.
        let mut anchor = avatar_pos;
        for fid in if self.depositing.is_some() { Vec::new() } else { self.following.clone() } {
            if Some(fid) == locked_id {
                // Frozen for inspection; keep the chain anchored at its spot.
                if let Some(c) = self.critters.iter().find(|c| c.animal_id == fid) {
                    anchor = c.pos;
                }
                continue;
            }
            if let Some(c) = self.critters.iter_mut().find(|c| c.animal_id == fid) {
                c.follow_pull(anchor, dt);
                anchor = c.pos;
            }
        }

        // ── Wild animal AI + catch resolution ─────────────────────────────
        // Skipped during hitstop so the bash freeze actually reads as a pause.
        if !frozen {
            let cursor_world = view::screen_to_world(mouse, &self.camera);
            let avatar_pos = self.session.my_avatar().pos;
            let hits =
                self.world
                    .update_animal_ai(dt, cursor_world, avatar_pos, self.catch_state.active);

            for hit in hits {
                match hit {
                    // A connecting Basher staggers the player and resets the timer.
                    AiHit::Bash => {
                        self.hitstop = BASH_HITSTOP;
                        self.camera_shake = BASH_SHAKE;
                        self.catch_state.fill = 0.0;
                        self.particles.impact(avatar_pos, Vec2::ZERO);
                        self.sounds.play("poke_lion_sfx");
                    }
                    // A Venomous lunge poisons: hitstop + red vignette + blur.
                    AiHit::Venom(pos) => {
                        self.hitstop = BASH_HITSTOP;
                        self.venom_fx = VENOM_FX_DURATION;
                        self.camera_shake = BASH_SHAKE * 0.6;
                        self.catch_state.fill = 0.0;
                        self.particles.venom(pos);
                        self.sounds.play("poke_lion_sfx");
                    }
                    // A Thrower release drops a timed danger zone (resolved below).
                    AiHit::Throw(target) => {
                        self.danger_zones.push(DangerZone {
                            center: target,
                            fuse: THROW_FUSE,
                            remaining: THROW_FUSE,
                            radius: THROW_RADIUS,
                        });
                        // Bound the queue so a swarm can't grow it without limit.
                        while self.danger_zones.len() > 16 {
                            self.danger_zones.remove(0);
                        }
                    }
                }
            }

            // Collect active animals into owned refs, run catch, then drop the
            // borrow on self.world before calling resolve_catch (which mutates it).
            // Fill speed scales with avatar→animal distance, so pass the live pos.
            let caught_id: Option<uuid::Uuid> = {
                let active = self.world.active_animals();
                self.catch_state.update(mouse, avatar_pos, &active, &self.camera, dt)
            };
            if let Some(id) = caught_id {
                self.resolve_catch(id, now);
            }
        }

        // Venom screen-effect timer (red vignette + blur) decays independently
        // of hitstop so the flash plays out smoothly after the freeze ends.
        if self.venom_fx > 0.0 {
            self.venom_fx = (self.venom_fx - dt).max(0.0);
        }

        // Thrown-object zones tick + resolve every frame — even during a venom
        // hitstop — so a telegraphed throw always lands on schedule.
        self.update_danger_zones(dt);
    }

    /// Tick each active throw-impact zone; on landing, kick up dust and, if the
    /// avatar is inside the radius, stagger them (shake + catch reset + a random
    /// knockback impulse).
    fn update_danger_zones(&mut self, dt: f32) {
        if self.danger_zones.is_empty() {
            return;
        }
        let avatar_pos = self.session.my_avatar().pos;
        let mut landed_hit = false;
        let mut i = 0;
        while i < self.danger_zones.len() {
            self.danger_zones[i].remaining -= dt;
            if self.danger_zones[i].remaining <= 0.0 {
                let z = self.danger_zones.remove(i);
                self.particles.dust(z.center);
                if (avatar_pos - z.center).length() < z.radius {
                    landed_hit = true;
                }
            } else {
                i += 1;
            }
        }
        if landed_hit {
            self.camera_shake = THROW_SHAKE;
            self.catch_state.fill = 0.0;
            // Random-vector knockback added straight to the avatar velocity.
            let ang = rand::gen_range(0.0f32, std::f32::consts::TAU);
            let knock = vec2(ang.cos(), ang.sin()) * KNOCKBACK_SPEED;
            let id = self.session.local_player_id;
            if let Some(a) = self.session.avatars.get_mut(&id) {
                a.vel += knock;
            }
            self.sounds.play("poke_lion_sfx");
        }
    }

    /// Biome-preview debug controls: free pan (WASD/arrows), cursor-centric
    /// zoom with an extended far-out floor, and R to reroll the world seed
    /// (regenerating the biome map + wild world). Suppresses all normal
    /// gameplay input while active.
    fn handle_debug_input(&mut self) {
        let dt = get_frame_time();

        // R rerolls the world seed → a brand-new biome layout.
        if is_key_pressed(KeyCode::R) {
            let new_seed = ((rand::rand() as u64) << 32) | rand::rand() as u64;
            self.zoo.world_seed = new_seed;
            self.world = WorldChunks::new(new_seed, HashMap::new());
            self.zoo.chunk_deltas.clear();
            self.set_status(format!("reseeded · {new_seed:#018x}"));
        }

        // Free pan: shift the screen-space camera offset (no avatar follow).
        let mut pan = vec2(0.0, 0.0);
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left)  { pan.x += 1.0; }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) { pan.x -= 1.0; }
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up)    { pan.y += 1.0; }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down)  { pan.y -= 1.0; }
        self.camera.offset += pan * (900.0 * dt);

        // Cursor-centric zoom, allowed to pull far past the gameplay floor.
        let (_, wheel_y) = mouse_wheel();
        if wheel_y != 0.0 {
            let mp = mouse_position();
            let mouse = vec2(mp.0, mp.1);
            let old = self.camera.zoom;
            let factor = if wheel_y > 0.0 { 1.1 } else { 1.0 / 1.1 };
            let new = (old * factor).clamp(DEBUG_ZOOM_MIN, 3.0);
            if new != old {
                self.camera.offset = mouse - (mouse - self.camera.offset) * (new / old);
                self.camera.zoom = new;
            }
        }
    }

    fn toggle_screen(&mut self, screen: Screen) {
        self.set_screen(if self.screen == screen {
            Screen::World
        } else {
            screen
        });
    }

    /// Switch overlay screens.
    pub fn set_screen(&mut self, screen: Screen) {
        self.screen = screen;
    }

    /// Instantly move the local avatar to `pos`: snap the camera, stream the
    /// destination chunks, and close any open menu. Used by waypoint teleport.
    pub fn teleport_to(&mut self, pos: Vec2) {
        {
            let a = self.session.my_avatar_mut();
            a.pos = pos;
            a.vel = vec2(0.0, 0.0);
        }
        self.camera
            .snap_to(pos, vec2(screen_width(), screen_height()));
        self.world.update(pos);
        self.set_screen(Screen::World);
    }

    /// Toggle the animal-inspect panel. If open, close it; otherwise find the
    /// nearest owned animal within `INTERACT_RANGE` and open a panel for it,
    /// freezing that critter in place. The panel slides in from the screen edge
    /// opposite the player.
    fn toggle_inspect(&mut self) {
        if self.inspect.is_some() {
            self.inspect = None;
            return;
        }
        let apos = self.session.my_avatar().pos;
        let mut best: Option<(f32, Uuid)> = None;
        for c in &self.critters {
            let d = (c.pos - apos).length_squared();
            if d <= INTERACT_RANGE * INTERACT_RANGE
                && best.map_or(true, |(bd, _)| d < bd)
            {
                best = Some((d, c.animal_id));
            }
        }
        match best {
            Some((_, id)) => {
                let screen = view::world_to_screen(apos, &self.camera);
                // Player on the left half → panel from the right, and vice versa.
                let from_right = screen.x < screen_width() * 0.5;
                self.inspect = Some(InspectState { animal_id: id, from_right, t: 0.0 });
            }
            None => self.set_status("Nothing to inspect nearby"),
        }
    }

    /// Slot index (0..`MAX_NESTS`) of the nearest nest pad — owned or still
    /// locked — within `INTERACT_RANGE` of the local avatar.
    pub fn nearest_nest_slot(&self) -> Option<usize> {
        let apos = self.session.my_avatar().pos;
        let mut best: Option<(f32, usize)> = None;
        for i in 0..crate::game::zoo::MAX_NESTS as usize {
            let d = (Zoo::nest_pos(i) - apos).length_squared();
            if d <= INTERACT_RANGE * INTERACT_RANGE && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// Try to unlock the next locked nest, paying its coin/DNA cost.
    pub fn try_buy_nest(&mut self, now: DateTime<Utc>) {
        match self.zoo.buy_nest() {
            Ok(n) => {
                self.save_under_lock(now);
                self.set_status(format!("unlocked nest {n}"));
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Slot index (0..`MAX_FOOD_STRUCTURES`) of the nearest food-structure pad —
    /// owned or locked — within `INTERACT_RANGE` of the local avatar.
    pub fn nearest_structure_slot(&self) -> Option<usize> {
        let apos = self.session.my_avatar().pos;
        let mut best: Option<(f32, usize)> = None;
        for i in 0..crate::game::structure::MAX_FOOD_STRUCTURES {
            let d = (Zoo::food_structure_pos(i) - apos).length_squared();
            if d <= INTERACT_RANGE * INTERACT_RANGE && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// The nearest interactive pad (breeding nest or food structure) within
    /// reach, whichever is closest.
    fn nearest_pad(&self) -> Option<Pad> {
        let apos = self.session.my_avatar().pos;
        let nest = self
            .nearest_nest_slot()
            .map(|i| ((Zoo::nest_pos(i) - apos).length_squared(), Pad::Nest(i)));
        let structure = self
            .nearest_structure_slot()
            .map(|i| ((Zoo::food_structure_pos(i) - apos).length_squared(), Pad::Structure(i)));
        match (nest, structure) {
            (Some((dn, pn)), Some((ds, ps))) => Some(if dn <= ds { pn } else { ps }),
            (Some((_, p)), None) | (None, Some((_, p))) => Some(p),
            (None, None) => None,
        }
    }

    /// Try to unlock the next locked food structure, paying its coin cost.
    pub fn try_buy_food_structure(&mut self, now: DateTime<Utc>) {
        match self.zoo.buy_food_structure(now) {
            Ok(n) => {
                self.save_under_lock(now);
                self.set_status(format!("built food structure {n}"));
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Add `animal_id` to the follow chain (from the inspect panel). Refused if
    /// it's already nested or the chain is full ([`MAX_FOLLOWERS`]).
    pub fn start_following(&mut self, animal_id: Uuid) {
        if self.zoo.animal_in_any_nest(animal_id) {
            self.set_status("that animal is already in a nest");
            return;
        }
        if self.following.contains(&animal_id) {
            return;
        }
        if self.following.len() >= MAX_FOLLOWERS {
            self.set_status(format!("can't follow more than {MAX_FOLLOWERS} at once"));
            return;
        }
        self.following.push(animal_id);
        self.inspect = None;
        self.set_status("following you — press E at a nest to deposit");
    }

    /// Remove `animal_id` from the follow chain, if present.
    pub fn stop_following(&mut self, animal_id: Uuid) {
        self.following.retain(|id| *id != animal_id);
    }

    /// Enter the spotlight deposit view for `nest_id`: dim the world and let the
    /// player click one of their following animals to drop it in. Closes any
    /// open menu instantly, and spreads the chain out into a horizontal row so
    /// the animals don't overlap (the chain physics is paused while depositing).
    pub fn enter_deposit_mode(&mut self, nest_id: Uuid) {
        self.depositing = Some(nest_id);
        self.set_screen(Screen::World);
        self.menu_t = 0.0;
        self.lay_out_followers();
    }

    /// Place each following animal in an evenly spaced horizontal row centered
    /// on the avatar, so they read as a tidy line-up rather than a clump.
    fn lay_out_followers(&mut self) {
        let center = self.session.my_avatar().pos;
        // Aim for ~130px of breathing room between sprites at the current zoom.
        let spacing = (130.0 / self.camera.zoom).max(60.0);
        let n = self.following.len();
        for (i, fid) in self.following.clone().iter().enumerate() {
            let offset = (i as f32 - (n as f32 - 1.0) * 0.5) * spacing;
            if let Some(c) = self.critters.iter_mut().find(|c| c.animal_id == *fid) {
                c.pos = vec2(center.x + offset, center.y);
                c.vel = vec2(0.0, 0.0);
                c.dir = vec2(-1.0, 1.0);
            }
        }
    }

    /// Species of the lone animal already in the active deposit nest, if exactly
    /// one slot is filled — used to dim non-crossbreedable second picks.
    pub fn deposit_partner_species(&self) -> Option<&'static str> {
        let nest_id = self.depositing?;
        let nest = self.zoo.nests.iter().find(|n| n.id == nest_id)?;
        let occ = nest.occupants();
        if occ.len() == 1 {
            self.zoo.animals.get(&occ[0]).map(|a| a.species)
        } else {
            None
        }
    }

    /// The following animal whose sprite is under the cursor right now, if any —
    /// used both to highlight it in the overlay and to resolve a click. Animals
    /// that can't crossbreed with an already-deposited partner are skipped.
    pub fn deposit_hovered(&self) -> Option<Uuid> {
        let mp = mouse_position();
        let mouse = vec2(mp.0, mp.1);
        let cam = self.camera;
        let h = CRITTER_H * cam.zoom;
        let hw = h * 0.4;
        let partner = self.deposit_partner_species();
        // Prefer the front-most (largest screen-Y) match when sprites overlap.
        let mut best: Option<(f32, Uuid)> = None;
        for fid in &self.following {
            let Some(c) = self.critters.iter().find(|c| c.animal_id == *fid) else { continue };
            // Invalid crossbreed second-picks aren't selectable.
            if let Some(p) = partner {
                if species::crossbreed_pool(p, c.species).is_none() {
                    continue;
                }
            }
            let feet = view::world_to_screen(c.pos, &cam);
            let hit = mouse.x >= feet.x - hw
                && mouse.x <= feet.x + hw
                && mouse.y >= feet.y - h
                && mouse.y <= feet.y;
            if hit && best.map_or(true, |(by, _)| feet.y > by) {
                best = Some((feet.y, *fid));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Resolve a click in the deposit spotlight: deposit the hovered animal into
    /// the active nest. Stays in the view (so you can fill the second slot)
    /// until the nest is full or nothing is left to deposit.
    fn try_deposit_select(&mut self, now: DateTime<Utc>) {
        let Some(nest_id) = self.depositing else { return };
        let Some(fid) = self.deposit_hovered() else { return };
        match self.zoo.deposit_in_nest(nest_id, fid) {
            Ok(()) => {
                self.stop_following(fid);
                self.sync_critters();
                self.save_under_lock(now);
                self.set_status("deposited in nest");
                // Auto-exit once the nest is full or the chain is empty.
                let nest_full = self
                    .zoo
                    .nests
                    .iter()
                    .find(|n| n.id == nest_id)
                    .map_or(true, |n| n.free_slot().is_none());
                if nest_full || self.following.is_empty() {
                    self.depositing = None;
                } else {
                    // Re-center the remaining animals so no gap is left behind.
                    self.lay_out_followers();
                }
            }
            Err(e) => self.set_status(format!("{e}")),
        }
    }

    /// Drop a fast-travel waypoint at the local avatar's current position.
    pub fn add_waypoint_here(&mut self, now: DateTime<Utc>) {
        let pos = self.session.my_avatar().pos;
        let name = self.zoo.next_waypoint_name();
        match self.zoo.add_waypoint(name.clone(), pos) {
            Some(_) => {
                self.save_under_lock(now);
                // No dedicated icon yet → falls back to the glow token badge.
                self.push_notification(name, "Waypoint set", NotifIcon::Currency("waypoint"));
            }
            None => self.set_status(format!(
                "Waypoint limit reached ({})",
                crate::game::zoo::Zoo::MAX_WAYPOINTS
            )),
        }
    }

    /// Remove a waypoint by id and persist.
    pub fn remove_waypoint(&mut self, id: Uuid, now: DateTime<Utc>) {
        if self.zoo.remove_waypoint(id) {
            self.save_under_lock(now);
        }
    }

    /// If `mouse` is over a critter, collect its backing animal's income via
    /// the domain's at-cap-only `collect_animal` rule.
    fn try_redeem_at(&mut self, mouse: Vec2, now: DateTime<Utc>) {
        let cam = self.camera;
        let h = CRITTER_H * cam.zoom;
        let hw = h * 0.4; // generous half-width hit box around the sprite
        let idx = self.critters.iter().position(|c| {
            let feet = view::world_to_screen(c.pos, &cam);
            mouse.x >= feet.x - hw
                && mouse.x <= feet.x + hw
                && mouse.y >= feet.y - h
                && mouse.y <= feet.y
        });
        let Some(idx) = idx else { return };
        let id = self.critters[idx].animal_id;
        let species = self.critters[idx].species;

        // Below-cap animals report whether they're collectable at all.
        let at_cap = self
            .zoo
            .animals
            .get(&id)
            .map(|a| a.is_at_cap(now))
            .unwrap_or(false);
        let pos = self.critters[idx].pos;
        let res = self.zoo.collect_animal(id, now);
        if res.total() > 0 {
            // Collected income → income sound + scale-pop + pixel burst.
            self.sounds.play("income_sfx");
            self.critters[idx].pop = POP_DURATION;
            self.save_under_lock(now);
            if res.coins > 0 {
                self.particles.coins(pos);
                self.push_notification(
                    "Coins",
                    format!("+{}", res.coins),
                    NotifIcon::Currency("coin"),
                );
            }
            if res.dna > 0 {
                self.particles.dna(pos);
                self.push_notification(
                    "DNA Helix",
                    format!("+{}", res.dna),
                    NotifIcon::Currency("dna_helix"),
                );
            }
        } else {
            // Poked a critter that isn't ready → per-species poke sound.
            self.sounds.play(&format!("poke_{species}_sfx"));
            if !at_cap {
                self.set_status("not full yet");
            }
        }
    }

    /// Called when the catch circle completes for a wild animal.
    /// Removes it from the world chunk, adds a tame L1 copy to the zoo.
    fn resolve_catch(&mut self, id: uuid::Uuid, now: DateTime<Utc>) {
        // Record the catch on the animal instance; rarer species must be caught
        // multiple times before they're actually captured.
        let Some((species, count)) = self.world.register_catch(id) else { return };
        let name = species::get(species).display_name;
        let required = species::captures_required(species);

        // Not enough catches yet → the animal stays in the world. Reset the
        // fill so the player has to fill the ring again for the next catch.
        if count < required {
            self.catch_state.fill = 0.0;
            self.catch_state.target = None;
            self.sounds.play("income_sfx");
            self.set_status(format!(
                "Caught {}! Needs {} more to capture ({}/{})",
                name,
                required - count,
                count,
                required
            ));
            return;
        }

        // Threshold met → remove it from the world and tame it into the zoo.
        // Grab its world position first so the capture burst fires where it was.
        let catch_pos = self
            .world
            .active_animals()
            .into_iter()
            .find(|a| a.id == id)
            .map(|a| a.pos);
        self.world.remove_animal(id);
        match self.zoo.spawn_animal_freeform(species, 1, now) {
            Ok(_) => {
                self.sounds.play("income_sfx");
                if let Some(pos) = catch_pos {
                    self.particles.capture(pos);
                }
                self.sync_critters();
                self.save_under_lock(now);
                self.push_notification(name, "Captured!", NotifIcon::Animal(species));
            }
            Err(e) => {
                self.set_status(format!("Capture failed: {e}"));
            }
        }
    }

    /// Mirror the live wild-world state (seed + accumulated chunk deltas) into
    /// the zoo so it gets serialized on the next save.
    fn sync_world_to_zoo(&mut self) {
        self.zoo.world_seed = self.world.world_seed();
        self.zoo.chunk_deltas = self.world.export_deltas();
    }

    /// Lock, save, update modtime. Call after any user-driven mutation.
    pub fn save_under_lock(&mut self, now: DateTime<Utc>) {
        self.sync_world_to_zoo();
        self.zoo.last_saved_at = now;
        let repo = self.repo.clone();
        if let Ok(access) = repo.lock() {
            if let Ok(mt) = access.save(&self.zoo) {
                self.last_modtime = mt;
            }
        }
    }

    pub fn draw(&mut self, now: DateTime<Utc>) {
        // Biome-preview debug mode short-circuits the whole gameplay render:
        // just the biome colour field + a minimal HUD, then the cursor.
        if self.debug_biome {
            world::draw_biome_debug(self);
            self.draw_cursor();
            return;
        }

        let menu = self.menu_t > 0.001;
        let effect_on = self.effect != PostEffect::None;
        let venom = self.venom_fx > 0.0;

        if menu || effect_on || venom {
            // Render the scene (no text!) into the offscreen target, then
            // composite it to the screen. Text — HUD and menus — is drawn
            // afterward on the default framebuffer so the font atlas is safe.
            let rt = self.render_scene_to_target(now);
            clear_background(color_u8!(14, 15, 18, 255));
            if menu {
                self.composite_blur(&rt, 1.0);
                draw_rectangle(
                    0.0,
                    0.0,
                    screen_width(),
                    screen_height(),
                    Color::new(0.0, 0.0, 0.0, 0.5 * self.menu_t),
                );
            } else if venom {
                // Venom hit: blur that tapers off + a fading red vignette.
                let intensity = (self.venom_fx / VENOM_FX_DURATION).clamp(0.0, 1.0);
                self.composite_blur(&rt, intensity);
                world::draw_red_vignette(intensity);
            } else {
                self.composite_effect(&rt);
            }
            if !menu {
                world::draw_hud(self, now);
            }
            menus::draw(self, now);
        } else {
            world::draw(self, now);
        }
        if self.depositing.is_some() {
            world::draw_deposit_overlay(self);
        }
        self.draw_cursor();
    }

    /// Render the world scene into `scene_rt` (recreated on resize) and return
    /// a clone of the target.
    fn render_scene_to_target(&mut self, now: DateTime<Utc>) -> RenderTarget {
        let (w, h) = (screen_width(), screen_height());
        let stale = match &self.scene_rt {
            Some(rt) => rt.texture.width() != w || rt.texture.height() != h,
            None => true,
        };
        if stale {
            let rt = render_target(w as u32, h as u32);
            rt.texture.set_filter(FilterMode::Linear);
            self.scene_rt = Some(rt);
        }
        let rt = self.scene_rt.clone().unwrap();
        let mut cam = Camera2D::from_display_rect(Rect::new(0.0, 0.0, w, h));
        cam.render_target = Some(rt.clone());
        set_camera(&cam);
        world::draw_scene(self, now);
        set_default_camera();
        rt
    }

    /// Cheap multi-tap blur (one opaque center pass + 8 soft offset passes).
    /// `intensity` (0–1) scales the offset-pass strength so callers can fade
    /// the blur in/out; 1.0 is the full menu blur.
    fn composite_blur(&self, rt: &RenderTarget, intensity: f32) {
        let (w, h) = (screen_width(), screen_height());
        let r = 2.5 * intensity.clamp(0.0, 1.0);
        blit(&rt.texture, 0.0, 0.0, w, h, 1.0);
        for (dx, dy) in [
            (-r, -r), (0.0, -r), (r, -r),
            (-r, 0.0), (r, 0.0),
            (-r, r), (0.0, r), (r, r),
        ] {
            blit(&rt.texture, dx, dy, w, h, 0.45 * intensity.clamp(0.0, 1.0));
        }
    }

    /// Draw the scene through the active post-process material.
    fn composite_effect(&self, rt: &RenderTarget) {
        let (w, h) = (screen_width(), screen_height());
        match &self.post {
            Some(mat) => {
                mat.set_uniform("resolution", vec2(w, h));
                mat.set_uniform("effect", self.effect.code());
                mat.set_uniform("time", get_time() as f32);
                gl_use_material(mat);
                blit(&rt.texture, 0.0, 0.0, w, h, 1.0);
                gl_use_default_material();
            }
            None => blit(&rt.texture, 0.0, 0.0, w, h, 1.0),
        }
    }

    /// Draw the custom cursor sprite (id "cursor") on top of everything; falls
    /// back to a small ring if no art is bundled. Hotspot at the image top-left.
    fn draw_cursor(&mut self) {
        let (mx, my) = mouse_position();
        match self.textures.icon("cursor") {
            Some(t) => {
                let s = 32.0;
                draw_texture_ex(
                    &t,
                    mx,
                    my,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(s, s)),
                        ..Default::default()
                    },
                );
            }
            None => {
                draw_circle_lines(mx, my, 7.0, 2.0, color_u8!(20, 20, 20, 220));
                draw_circle_lines(mx, my, 7.0, 1.0, WHITE);
            }
        }
    }
}

/// Standard passthrough vertex shader (mirrors macroquad's default).
const POST_VERT: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying lowp vec2 uv;
varying lowp vec4 color;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;

/// Fragment shader: one of several fullscreen filters selected by `effect`.
const POST_FRAG: &str = r#"#version 100
precision mediump float;
varying vec2 uv;
varying vec4 color;
uniform sampler2D Texture;
uniform vec2 resolution;
uniform float effect;
uniform float time;
void main() {
    vec2 u = uv;
    // Pixelate: snap uv to a coarse grid before sampling.
    if (effect > 1.5 && effect < 2.5) {
        float px = 5.0;
        vec2 grid = resolution / px;
        u = (floor(u * grid) + 0.5) / grid;
    }
    vec4 c = texture2D(Texture, u);
    if (effect > 0.5 && effect < 1.5) {
        // Scanlines (slowly rolling).
        float line = sin((uv.y * resolution.y * 3.14159) - time * 6.0);
        c.rgb *= 0.82 + 0.18 * (0.5 + 0.5 * line);
    } else if (effect > 2.5 && effect < 3.5) {
        // Grayscale.
        float g = dot(c.rgb, vec3(0.299, 0.587, 0.114));
        c.rgb = vec3(g);
    } else if (effect > 3.5) {
        // Sepia.
        float g = dot(c.rgb, vec3(0.299, 0.587, 0.114));
        c.rgb = vec3(g) * vec3(1.07, 0.85, 0.63);
    }
    gl_FragColor = c * color;
}"#;

/// Build the post-process material; `None` if shader compilation fails (effects
/// then silently no-op).
fn build_post_material() -> Option<Material> {
    let params = MaterialParams {
        uniforms: vec![
            UniformDesc::new("resolution", UniformType::Float2),
            UniformDesc::new("effect", UniformType::Float1),
            UniformDesc::new("time", UniformType::Float1),
        ],
        ..Default::default()
    };
    match load_material(
        ShaderSource::Glsl {
            vertex: POST_VERT,
            fragment: POST_FRAG,
        },
        params,
    ) {
        Ok(m) => Some(m),
        Err(e) => {
            eprintln!("post-process shader failed to compile: {e}");
            None
        }
    }
}

/// Draw a render-target texture to the screen at `(x,y)` sized `w×h`. Render
/// targets are stored bottom-up, so flip vertically.
fn blit(tex: &Texture2D, x: f32, y: f32, w: f32, h: f32, alpha: f32) {
    draw_texture_ex(
        tex,
        x,
        y,
        Color::new(1.0, 1.0, 1.0, alpha),
        DrawTextureParams {
            dest_size: Some(vec2(w, h)),
            flip_y: true,
            ..Default::default()
        },
    );
}

/// Default camera: zoom 0.7, centred — `snap_to` will reposition on the
/// player spawn immediately after construction.
fn default_camera() -> Camera {
    Camera { offset: vec2(0.0, 0.0), zoom: 0.7 }
}
