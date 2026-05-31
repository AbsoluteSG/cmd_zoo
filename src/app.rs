//! Macroquad app shell. Owns the `Zoo`, the file repository, the camera, the
//! texture cache, and the roaming critters; drives the per-frame
//! load/advance/save loop and translates input into camera moves.
//!
//! All game rules live in `crate::game`; this is glue + presentation.

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Context;
use chrono::{DateTime, Utc};
use macroquad::prelude::*;
use uuid::Uuid;

use std::collections::HashMap;

use crate::audio::Sounds;
use crate::game::avatar_system::{self, Behavior};
use crate::game::{Animal, AnimalState, Zoo, economy, species};
use crate::input::{AvatarController, ControllerCtx, KeyboardController, RemoteController};
use crate::net::demo_bot::DemoBot;
use crate::net::{Session, loopback, protocol::JoinCode};
use crate::persistence::json_file::JsonFileRepository;
use crate::render::textures::Textures;
use crate::render::view::{self, Camera, CRITTER_H, PLANE_H, PLANE_W, POP_DURATION, TILT};
use crate::render::{menus, world};

/// Which screen owns the input/overlay this frame. `World` is the live critter
/// scene; the others are menu overlays.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    World,
    Shop,
    Breeding,
    Settings,
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
}

impl Critter {
    fn new(animal_id: Uuid, species: &'static str, pos: Vec2) -> Self {
        Self {
            animal_id,
            species,
            pos,
            target: random_plane_point(),
            speed: 80.0,
            dir: vec2(-1.0, 1.0),
            idle_timer: rand::gen_range(0.0, 2.0),
            pop: 0.0,
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
            // Arrived: pause a moment, then head somewhere new.
            self.target = random_plane_point();
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
}

/// Build a critter for every animal in the zoo, scattered across the plane.
fn critters_from_zoo(zoo: &Zoo) -> Vec<Critter> {
    zoo.animals
        .values()
        .map(|a| Critter::new(a.id, a.species, random_plane_point()))
        .collect()
}

/// A uniformly random point inside the ground plane.
fn random_plane_point() -> Vec2 {
    vec2(
        rand::gen_range(0.0, PLANE_W),
        rand::gen_range(0.0, PLANE_H),
    )
}

pub struct GameApp {
    pub zoo: Zoo,
    pub repo: Arc<JsonFileRepository>,
    pub last_modtime: SystemTime,
    pub camera: Camera,
    pub textures: Textures,
    pub sounds: Sounds,
    pub critters: Vec<Critter>,
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
    /// Breeding pair staging (animal ids), used by the Breeding menu.
    pub breeding_first_pick: Option<Uuid>,
    pub breeding_second_pick: Option<Uuid>,
    /// Transient status line: (text, set-at via `get_time()`), cleared after 4s.
    pub status: Option<(String, f64)>,
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
    /// Optional in-process demo "visitor" driven by `DemoBot`. Used by the
    /// Settings → "Local co-op demo" affordance to prove the multi-avatar
    /// pipeline works end-to-end without Steam. Spawned alongside the host
    /// transport when the user enables the demo; torn down on disable.
    demo_bot: Option<DemoBot>,
    /// Seconds since the host last broadcast a full ZooSnapshot. Throttled
    /// to ~`SNAPSHOT_BROADCAST_INTERVAL` so visitors see currency/animal
    /// changes without flooding the wire on every frame.
    snapshot_broadcast_t: f32,
    /// Currently-being-typed join code in the Settings join-friend field.
    /// 6 chars max; uppercase Crockford base32 (matches `JoinCode::random`).
    pub join_code_buffer: String,
}

/// How often (seconds) the host re-broadcasts a full ZooSnapshot to visitors.
/// Two seconds is fast enough that purchases feel live without saturating
/// the loopback / Steam relay bandwidth budget.
const SNAPSHOT_BROADCAST_INTERVAL: f32 = 2.0;

impl GameApp {
    pub fn new(zoo: Zoo, repo: Arc<JsonFileRepository>, last_modtime: SystemTime) -> Self {
        let critters = critters_from_zoo(&zoo);
        let spawn = vec2(PLANE_W * 0.5, PLANE_H * 0.5);
        let session = Session::solo(zoo.player.id, spawn);
        let mut camera = default_camera();
        camera.snap_to(spawn, vec2(screen_width(), screen_height()));
        Self {
            zoo,
            repo,
            last_modtime,
            camera,
            textures: Textures::new(),
            sounds: Sounds::default(),
            critters,
            screen: Screen::World,
            menu_t: 0.0,
            shown_menu: Screen::World,
            scene_rt: None,
            effect: PostEffect::None,
            post: build_post_material(),
            breeding_first_pick: None,
            breeding_second_pick: None,
            status: None,
            session,
            controller: Box::new(KeyboardController::default()),
            remotes: HashMap::new(),
            behaviors: avatar_system::default_behaviors(),
            demo_bot: None,
            snapshot_broadcast_t: 0.0,
            join_code_buffer: String::new(),
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
                self.set_status(format!("claimed gift: {} L{}", gift.species, gift.level));
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

    /// Start hosting via the in-process loopback transport + a single demo
    /// visitor bot. Stand-in for "Open Zoo to Online" until the Steam
    /// transport lands.
    pub fn start_local_demo(&mut self) {
        if self.is_hosting() {
            return;
        }
        let (host_t, visitor_t) = loopback::pair(
            crate::net::protocol::PeerId(1),
            crate::net::protocol::PeerId(2),
        );
        self.session
            .become_host(Box::new(host_t), JoinCode::random());
        self.demo_bot = Some(DemoBot::new(Box::new(visitor_t), "Demo Bot"));
        self.set_status("local co-op demo: bot joining…");
    }

    /// Tear down local hosting (demo or otherwise) — return to solo.
    pub fn stop_hosting(&mut self) {
        if let Some(mut bot) = self.demo_bot.take() {
            bot.shutdown();
        }
        self.session.end_hosting();
        self.remotes.clear();
        self.set_status("hosting stopped");
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
                    .push(Critter::new(a.id, a.species, random_plane_point()));
            }
        }
    }

    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some((text.into(), get_time()));
    }

    fn clear_stale_status(&mut self) {
        if let Some((_, at)) = &self.status {
            if get_time() - *at >= 4.0 {
                self.status = None;
            }
        }
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
        let before = self.breeding_pair_count();
        economy::advance(&mut self.zoo, now);
        let after = self.breeding_pair_count();
        if after < before {
            self.set_status(format!("{} gestation(s) completed", before - after));
            self.zoo.last_saved_at = now;
            if let Ok(mt) = access.save(&self.zoo) {
                self.last_modtime = mt;
            }
        }
    }

    fn breeding_pair_count(&self) -> usize {
        use crate::game::AnimalState;
        self.zoo
            .animals
            .values()
            .filter(|a| matches!(a.state, AnimalState::Breeding { .. }))
            .count()
            / 2
    }

    /// Menu toggles + (when no menu is open) camera input + click-to-redeem,
    /// plus critter wandering (which continues behind menus).
    pub fn handle_input(&mut self, now: DateTime<Utc>) {
        let mp = mouse_position();
        let mouse = vec2(mp.0, mp.1);

        // Menu toggles: 1 = Shop, 2 = Breeding, Esc = close.
        if is_key_pressed(KeyCode::Key1) {
            self.toggle_screen(Screen::Shop);
        }
        if is_key_pressed(KeyCode::Key2) {
            self.toggle_screen(Screen::Breeding);
        }
        if is_key_pressed(KeyCode::Key3) {
            self.toggle_screen(Screen::Settings);
        }
        if is_key_pressed(KeyCode::Escape) {
            self.set_screen(Screen::World);
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
        let menu_open = self.menu_t >= 0.02;

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

        // 0. Tick the in-process demo bot (no-op when None). Done before
        //    pumping so its messages are visible this frame.
        if let Some(bot) = self.demo_bot.as_mut() {
            bot.tick(dt);
        }

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
                avatar_system::step(a, &intent, &world, dt, &self.behaviors);
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

        for c in &mut self.critters {
            c.update(dt);
        }
    }

    fn toggle_screen(&mut self, screen: Screen) {
        self.set_screen(if self.screen == screen {
            Screen::World
        } else {
            screen
        });
    }

    /// Switch overlay screens, clearing transient breeding staging.
    pub fn set_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.breeding_first_pick = None;
        self.breeding_second_pick = None;
    }

    /// Idle animals eligible to breed with `first_pick` (different species and
    /// a valid crossbreed pool). With no first pick, all idle animals.
    pub fn breeding_candidates(&self, first_pick: Option<Uuid>) -> Vec<&Animal> {
        let first_species = first_pick
            .and_then(|id| self.zoo.animals.get(&id))
            .map(|a| a.species);
        let mut v: Vec<&Animal> = self
            .zoo
            .animals
            .values()
            .filter(|a| matches!(a.state, AnimalState::Idle))
            .filter(|a| match (first_pick, first_species) {
                (Some(fid), Some(sp)) => {
                    a.id != fid
                        && a.species != sp
                        && species::crossbreed_pool(sp, a.species).is_some()
                }
                _ => true,
            })
            .collect();
        v.sort_by(|a, b| a.species.cmp(b.species).then(a.id.cmp(&b.id)));
        v
    }

    /// Active gestation pairs as (animal_a, animal_b, ends_at), de-duped.
    pub fn active_gestations(&self) -> Vec<(&Animal, &Animal, DateTime<Utc>)> {
        let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        let mut out = Vec::new();
        for a in self.zoo.animals.values() {
            if seen.contains(&a.id) {
                continue;
            }
            if let AnimalState::Breeding { partner_id, ends_at } = a.state {
                if let Some(b) = self.zoo.animals.get(&partner_id) {
                    seen.insert(a.id);
                    seen.insert(partner_id);
                    out.push((a, b, ends_at));
                }
            }
        }
        out.sort_by(|x, y| x.2.cmp(&y.2));
        out
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
        let res = self.zoo.collect_animal(id, now);
        if res.total() > 0 {
            // Collected income → income sound + scale-pop.
            self.sounds.play("income_sfx");
            self.critters[idx].pop = POP_DURATION;
            self.save_under_lock(now);
            let msg = match (res.coins, res.dna) {
                (c, 0) => format!("+{c} coins"),
                (0, d) => format!("+{d} DNA"),
                (c, d) => format!("+{c} coins  +{d} DNA"),
            };
            self.set_status(msg);
        } else {
            // Poked a critter that isn't ready → per-species poke sound.
            self.sounds.play(&format!("poke_{species}_sfx"));
            if !at_cap {
                self.set_status("not full yet");
            }
        }
    }

    /// Lock, save, update modtime. Call after any user-driven mutation.
    pub fn save_under_lock(&mut self, now: DateTime<Utc>) {
        self.zoo.last_saved_at = now;
        let repo = self.repo.clone();
        if let Ok(access) = repo.lock() {
            if let Ok(mt) = access.save(&self.zoo) {
                self.last_modtime = mt;
            }
        }
    }

    pub fn draw(&mut self, now: DateTime<Utc>) {
        let menu = self.menu_t > 0.001;
        let effect_on = self.effect != PostEffect::None;

        if menu || effect_on {
            // Render the scene (no text!) into the offscreen target, then
            // composite it to the screen. Text — HUD and menus — is drawn
            // afterward on the default framebuffer so the font atlas is safe.
            let rt = self.render_scene_to_target(now);
            clear_background(color_u8!(14, 15, 18, 255));
            if menu {
                self.composite_blur(&rt);
                draw_rectangle(
                    0.0,
                    0.0,
                    screen_width(),
                    screen_height(),
                    Color::new(0.0, 0.0, 0.0, 0.5 * self.menu_t),
                );
            } else {
                self.composite_effect(&rt);
            }
            if !menu {
                world::draw_hud(self);
            }
            menus::draw(self, now);
        } else {
            world::draw(self, now);
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
    fn composite_blur(&self, rt: &RenderTarget) {
        let (w, h) = (screen_width(), screen_height());
        let r = 2.5;
        blit(&rt.texture, 0.0, 0.0, w, h, 1.0);
        for (dx, dy) in [
            (-r, -r), (0.0, -r), (r, -r),
            (-r, 0.0), (r, 0.0),
            (-r, r), (0.0, r), (r, r),
        ] {
            blit(&rt.texture, dx, dy, w, h, 0.45);
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

/// A camera that fits the whole ground plane on screen, centered, at startup.
fn default_camera() -> Camera {
    let zoom = ((screen_width() * 0.9) / PLANE_W).clamp(0.3, 1.5);
    let plane_screen = vec2(PLANE_W, PLANE_H * TILT) * zoom;
    let offset = vec2(screen_width() * 0.5, screen_height() * 0.5) - plane_screen * 0.5;
    Camera { offset, zoom }
}
