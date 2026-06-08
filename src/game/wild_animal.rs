//! Wild animals roaming the world outside the zoo — the catch targets for the
//! player-skill acquisition loop (§34 / §35 of the design doc).
//!
//! Each `WildAnimal` carries its own `Moveset` state. When the player enters
//! catch mode and moves the cursor within `ALERT_RADIUS` world-units, the
//! animal detects the threat and switches from lazy wandering to its
//! species-specific escape pattern.  The caller drives `update` every frame
//! and reads `fill_speed` to know how fast the capture circle should progress.

use glam::{Vec2, vec2};
use uuid::Uuid;

use crate::game::rng as rand;

use crate::game::species::SpeciesId;
use crate::game::world_chunks::{resolve_zoo_collision, WORLD_W, WORLD_H};

// ── Tuning constants ──────────────────────────────────────────────────────────

/// World-units radius at which an animal notices the cursor and starts fleeing.
pub const ALERT_RADIUS: f32 = 200.0;

/// Base screen-space radius of the catch circle (pixels at zoom 1×).
/// Scaled by `cam.zoom` in the renderer and hover-detection code so they
/// always agree.
pub const CATCH_SCREEN_RADIUS_BASE: f32 = 85.0;

/// Orbit radius for the Circler moveset in world units.
const CIRCLER_ORBIT_RADIUS: f32 = 190.0;

const WANDER_SPEED: f32 = 55.0;
const FLEE_SPEED: f32 = 220.0;

// ── Basher tuning ──────────────────────────────────────────────────────────────

/// Charge speed of a Basher barrelling toward the player.
const BASH_SPEED: f32 = FLEE_SPEED * 2.4;
/// World-units from the player at which a charge counts as a hit.
const BASH_HIT_DIST: f32 = 52.0;
/// Max seconds a single charge runs before it's declared a miss.
const BASH_CHARGE_TIMEOUT: f32 = 1.4;
/// Recovery (stun) seconds after a successful hit — short, the reward is the hit.
const BASH_HIT_RECOVERY: f32 = 1.0;
/// Recovery seconds after a whiffed charge — longer, the window to catch it.
const BASH_MISS_RECOVERY: f32 = 2.5;

// ── Thrower tuning ───────────────────────────────────────────────────────────────

/// Speed (fraction of flee speed) a Thrower backs away from the player while
/// it lobs objects — slow enough that the cursor can stay on it.
const THROWER_RETREAT_MULT: f32 = 0.45;
/// Seconds between throws once a Thrower is engaged (re-rolled each throw).
const THROW_COOLDOWN_MIN: f32 = 1.4;
const THROW_COOLDOWN_MAX: f32 = 2.2;

// ── Venomous tuning ──────────────────────────────────────────────────────────────

/// World-units within which a Venomous stalks the player (even outside catch
/// mode); beyond this it wanders calmly.
const VENOM_STALK_RADIUS: f32 = 700.0;
/// Slow creep speed while closing the distance.
const VENOM_STALK_SPEED: f32 = WANDER_SPEED * 1.3;
/// Distance at which a stalking Venomous commits to a lunge.
const VENOM_LUNGE_TRIGGER: f32 = 170.0;
/// Lunge dash speed.
const VENOM_LUNGE_SPEED: f32 = FLEE_SPEED * 2.2;
/// Distance from the player at which a lunge counts as a hit.
const VENOM_HIT_DIST: f32 = 50.0;
/// Max seconds a single lunge runs before it's declared a miss.
const VENOM_LUNGE_TIMEOUT: f32 = 0.5;
/// Recovery seconds after a connecting lunge.
const VENOM_HIT_RECOVERY: f32 = 1.2;
/// Recovery seconds after a whiffed lunge.
const VENOM_MISS_RECOVERY: f32 = 1.8;

/// An aggression event surfaced by `WildAnimal::update` for the app layer to
/// turn into juice (hitstop, shake, screen effects, particles).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AiHit {
    /// A Basher connected a charge with the player.
    Bash,
    /// A Venomous landed a lunge — carries the animal's world position so the
    /// caller can spawn the venom puff where it struck.
    Venom(Vec2),
    /// A Thrower released an object aimed at this world position (the player's
    /// location at throw time). Resolves into a timed danger zone.
    Throw(Vec2),
}

// ── Moveset ───────────────────────────────────────────────────────────────────

/// Evasion behaviour used when the animal detects the player in catch mode.
/// Each variant embeds its own per-instance state (timers, angles …) so
/// multiple animals of the same species each run independent AI.
#[derive(Clone, Debug)]
pub enum Moveset {
    /// Sharp random direction changes — unpredictable but subtly telegraphed.
    Zigzagger {
        /// Seconds until the next random direction deviation.
        dir_timer: f32,
        /// Current flee direction (unit vector).
        current_dir: Vec2,
    },

    /// Holds completely still for a moment, then bursts in a straight line.
    Burster {
        /// Seconds remaining in the current phase (hold or dash).
        phase_timer: f32,
        /// True while dashing; false while winding up / holding still.
        dashing: bool,
        /// Direction of the current or upcoming dash.
        dash_dir: Vec2,
    },

    /// Orbits the cursor at a fixed radius — the player must intercept the arc.
    Circler {
        /// Current angle (radians) of the orbit.
        angle: f32,
        /// Counter-clockwise (+1) or clockwise (−1).
        spin: f32,
    },

    /// Moves *toward* the cursor — the player must hold their ground.
    Aggressor,

    /// Periodically vanishes and reappears at an offset position.
    /// The `hidden` flag is mirrored onto `WildAnimal::hidden` so the
    /// renderer can skip drawing the sprite during the teleport.
    Vanisher {
        /// Countdown to the next vanish / appear transition.
        timer: f32,
        /// True while the animal is invisible (mid-teleport).
        hidden: bool,
    },

    /// Moves at a fraction of normal flee speed; fill speed is also reduced
    /// so the catch is a patience test rather than a free win.
    Freezer,

    /// High initial speed that drains quickly to near-zero; rewards patience.
    Panicker {
        /// 1.0 = full stamina (fast); 0.0 = exhausted (slow).
        stamina: f32,
    },

    /// Charges *into* the player instead of fleeing. A connecting bash staggers
    /// the player (hitstop + camera shake) and resets the capture timer, then
    /// the animal recovers briefly; a missed charge leaves it winded for longer,
    /// opening the real catch window.
    Basher {
        /// Recovery countdown — while >0 the animal is winded and holds still
        /// (the moment to catch it). 0 means ready to charge again.
        recovery: f32,
        /// Seconds elapsed in the current charge; a charge that exceeds
        /// `BASH_CHARGE_TIMEOUT` without connecting counts as a miss.
        charge_time: f32,
        /// Locked-in charge direction, aimed at the player when the charge began.
        charge_dir: Vec2,
        /// True while a charge is in progress.
        charging: bool,
    },

    /// Backs slowly away while lobbing objects at the player's position. Each
    /// throw drops a timed danger zone (handled by the app); standing in it
    /// when it lands staggers and knocks back the player.
    Thrower {
        /// Seconds until the next throw while engaged. Re-rolled per throw.
        cooldown: f32,
    },

    /// Stalks the player slowly even outside catch mode, then lunges at close
    /// range. A connecting lunge poisons the player (hitstop + red vignette +
    /// brief blur); then the animal recovers, opening the catch window.
    Venomous {
        /// True while a lunge dash is in progress.
        lunging: bool,
        /// Seconds elapsed in the current lunge (miss after `VENOM_LUNGE_TIMEOUT`).
        lunge_time: f32,
        /// Recovery countdown — while >0 the animal holds still (catch window).
        recovery: f32,
        /// Locked-in lunge direction, aimed at the player when the lunge began.
        lunge_dir: Vec2,
    },
}

impl Moveset {
    pub fn zigzagger() -> Self {
        Self::Zigzagger { dir_timer: 0.0, current_dir: vec2(-1.0, 0.0) }
    }
    pub fn burster() -> Self {
        Self::Burster {
            phase_timer: rand::gen_range(0.8f32, 1.5),
            dashing: false,
            dash_dir: vec2(1.0, 0.0),
        }
    }
    pub fn circler() -> Self {
        Self::Circler {
            angle: rand::gen_range(0.0f32, std::f32::consts::TAU),
            spin: if rand::gen_range(0, 2) == 0 { 1.0 } else { -1.0 },
        }
    }
    pub fn aggressor() -> Self { Self::Aggressor }
    pub fn vanisher() -> Self {
        Self::Vanisher { timer: rand::gen_range(3.0f32, 5.0), hidden: false }
    }
    pub fn freezer() -> Self { Self::Freezer }
    pub fn panicker() -> Self { Self::Panicker { stamina: 1.0 } }
    pub fn basher() -> Self {
        Self::Basher {
            recovery: 0.0,
            charge_time: 0.0,
            charge_dir: vec2(1.0, 0.0),
            charging: false,
        }
    }
    pub fn thrower() -> Self {
        Self::Thrower {
            cooldown: rand::gen_range(0.8f32, 1.5),
        }
    }
    pub fn venomous() -> Self {
        Self::Venomous {
            lunging: false,
            lunge_time: 0.0,
            recovery: 0.0,
            lunge_dir: vec2(1.0, 0.0),
        }
    }
}

// ── WildAnimal ────────────────────────────────────────────────────────────────

pub struct WildAnimal {
    /// Stable identity used by the catch system to track the target across frames.
    pub id: Uuid,
    pub species: SpeciesId,
    pub pos: Vec2,
    pub vel: Vec2,
    pub moveset: Moveset,
    /// Lazy wander target used when the animal is calm.
    wander_target: Vec2,
    /// True while using the escape moveset.
    pub is_fleeing: bool,
    /// Grace period (seconds) after the cursor leaves alert range before the
    /// animal calms back down to wandering.
    flee_cooldown: f32,
    /// True while a Vanisher is mid-teleport (invisible). Mirrored from the
    /// Vanisher state to avoid coupling the renderer to the enum internals.
    pub hidden: bool,
    /// How many times this exact animal has been successfully caught. Rarer
    /// species must be caught `species::captures_required` times before they're
    /// actually captured into the zoo; until then each successful catch only
    /// bumps this counter and the animal stays in the world.
    pub catches: u32,
    /// The chunk this animal was procedurally spawned in. Stable across
    /// migration and chunk regeneration — used as the key for persisted deltas.
    pub origin_chunk: (i32, i32),
    /// This animal's deterministic spawn index within its origin chunk. Together
    /// with `origin_chunk` it forms the animal's persistent identity, so capture
    /// deltas survive eviction + regeneration from the world seed.
    pub spawn_index: u16,
}

impl WildAnimal {
    pub fn new(
        species: SpeciesId,
        pos: Vec2,
        moveset: Moveset,
        origin_chunk: (i32, i32),
        spawn_index: u16,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            species,
            pos,
            vel: vec2(0.0, 0.0),
            moveset,
            wander_target: local_wander_point(pos),
            is_fleeing: false,
            flee_cooldown: 0.0,
            hidden: false,
            catches: 0,
            origin_chunk,
            spawn_index,
        }
    }

    /// Advance AI and position by `dt` seconds.
    ///
    /// `cursor_world` is the cursor position in world space and `player_pos` the
    /// local avatar's world position — only meaningful when `catch_mode` is
    /// true, ignored otherwise.
    ///
    /// Returns `Some(AiHit)` on the frame this animal lands an aggressive action
    /// (Basher charge, Venomous lunge, or Thrower release), so the caller can
    /// apply hitstop / camera shake / screen effects and reset the catch timer.
    pub fn update(
        &mut self,
        dt: f32,
        cursor_world: Vec2,
        player_pos: Vec2,
        catch_mode: bool,
    ) -> Option<AiHit> {
        let to_cursor = cursor_world - self.pos;
        let dist_to_cursor = to_cursor.length();

        // ── Flee / calm toggle ────────────────────────────────────────────
        if catch_mode && dist_to_cursor < ALERT_RADIUS {
            self.is_fleeing = true;
            self.flee_cooldown = 1.8;
        } else if self.flee_cooldown > 0.0 {
            self.flee_cooldown -= dt;
            if self.flee_cooldown <= 0.0 {
                self.is_fleeing = false;
            }
        }

        // Aggressive movesets integrate their own motion (toward the player,
        // not away) and may emit a hit event, so handle them separately.
        if matches!(self.moveset, Moveset::Basher { .. }) {
            return self.update_basher(dt, player_pos, catch_mode).then_some(AiHit::Bash);
        }
        if matches!(self.moveset, Moveset::Thrower { .. }) {
            return self.update_thrower(dt, player_pos, catch_mode);
        }
        if matches!(self.moveset, Moveset::Venomous { .. }) {
            return self.update_venomous(dt, player_pos);
        }

        // ── Vanisher teleport (handled separately to avoid borrow conflicts) ─
        self.update_vanisher(dt, catch_mode);

        // ── snapshot fleeing / hidden state before the mutable match ────────
        let currently_hidden = self.hidden;
        let currently_fleeing = self.is_fleeing && catch_mode;

        let target_vel = if currently_fleeing {
            self.flee_velocity(dt, cursor_world, to_cursor, dist_to_cursor, currently_hidden)
        } else {
            self.wander_velocity(dt)
        };

        // Exponential approach to target velocity — snappy without being instant.
        let t = (750.0 * dt).min(1.0);
        self.vel = self.vel + (target_vel - self.vel) * t;

        self.pos += self.vel * dt;
        self.pos.x = self.pos.x.clamp(0.0, WORLD_W);
        self.pos.y = self.pos.y.clamp(0.0, WORLD_H);
        // Wild animals are barred from the home zoo plot, even mid-chase.
        self.pos = resolve_zoo_collision(self.pos);
        None
    }

    /// Basher state machine: charge the player when engaged, stagger them on a
    /// hit, then recover. Returns `true` the frame a charge connects.
    fn update_basher(&mut self, dt: f32, player_pos: Vec2, catch_mode: bool) -> bool {
        let to_player = player_pos - self.pos;
        let dist = to_player.length();
        // Engaged once the cursor has alerted it (mirrors the flee toggle above).
        let engaged = catch_mode && self.is_fleeing;

        let mut hit = false;
        let mut recovering = false;
        let mut charging_now = false;
        let mut charge_velocity = vec2(0.0, 0.0);

        if let Moveset::Basher { recovery, charge_time, charge_dir, charging } = &mut self.moveset {
            if *recovery > 0.0 {
                // Winded — hold still and tick down. This is the catch window.
                *recovery -= dt;
                *charging = false;
                recovering = true;
            } else if engaged {
                if !*charging {
                    // Begin a fresh charge, locking aim at the player.
                    *charging = true;
                    *charge_time = 0.0;
                    *charge_dir = if dist > 0.1 { to_player / dist } else { vec2(1.0, 0.0) };
                }
                *charge_time += dt;
                if dist < BASH_HIT_DIST {
                    // Connected: stagger the player, brief recovery.
                    hit = true;
                    *recovery = BASH_HIT_RECOVERY;
                    *charging = false;
                } else if *charge_time > BASH_CHARGE_TIMEOUT {
                    // Whiffed: longer recovery opens the catch window.
                    *recovery = BASH_MISS_RECOVERY;
                    *charging = false;
                } else {
                    charging_now = true;
                    charge_velocity = *charge_dir * BASH_SPEED;
                }
            } else {
                *charging = false;
            }
        }

        let target_vel = if recovering || (engaged && !charging_now) {
            vec2(0.0, 0.0)
        } else if charging_now {
            charge_velocity
        } else {
            // Not engaged → behave like any calm wanderer.
            self.wander_velocity(dt)
        };

        let t = (750.0 * dt).min(1.0);
        self.vel = self.vel + (target_vel - self.vel) * t;
        self.pos += self.vel * dt;
        self.pos.x = self.pos.x.clamp(0.0, WORLD_W);
        self.pos.y = self.pos.y.clamp(0.0, WORLD_H);
        self.pos = resolve_zoo_collision(self.pos);
        hit
    }

    /// Thrower state machine: while engaged, slowly back away from the player
    /// and lob an object on a cooldown. Returns `Some(AiHit::Throw(player_pos))`
    /// on the frame a throw is released; the app turns it into a danger zone.
    fn update_thrower(&mut self, dt: f32, player_pos: Vec2, catch_mode: bool) -> Option<AiHit> {
        let to_player = player_pos - self.pos;
        let dist = to_player.length();
        let engaged = catch_mode && self.is_fleeing;

        let mut threw = false;
        if let Moveset::Thrower { cooldown } = &mut self.moveset
            && engaged
        {
            *cooldown -= dt;
            if *cooldown <= 0.0 {
                *cooldown = rand::gen_range(THROW_COOLDOWN_MIN, THROW_COOLDOWN_MAX);
                threw = true;
            }
        }

        let target_vel = if engaged {
            // Back slowly away from the player so the cursor can stay on it.
            let away = if dist > 0.1 { -to_player / dist } else { vec2(1.0, 0.0) };
            away * (FLEE_SPEED * THROWER_RETREAT_MULT)
        } else {
            self.wander_velocity(dt)
        };

        let t = (750.0 * dt).min(1.0);
        self.vel = self.vel + (target_vel - self.vel) * t;
        self.pos += self.vel * dt;
        self.pos.x = self.pos.x.clamp(0.0, WORLD_W);
        self.pos.y = self.pos.y.clamp(0.0, WORLD_H);
        self.pos = resolve_zoo_collision(self.pos);

        threw.then_some(AiHit::Throw(player_pos))
    }

    /// Venomous state machine: stalk the player slowly whenever they're within
    /// `VENOM_STALK_RADIUS` (independent of catch mode), lunge at close range,
    /// then recover. Returns `Some(AiHit::Venom(pos))` the frame a lunge lands.
    fn update_venomous(&mut self, dt: f32, player_pos: Vec2) -> Option<AiHit> {
        let to_player = player_pos - self.pos;
        let dist = to_player.length();
        let in_range = dist < VENOM_STALK_RADIUS;
        let dir = if dist > 0.1 { to_player / dist } else { vec2(1.0, 0.0) };

        let mut hit = false;
        let mut recovering = false;
        let mut lunging_now = false;
        let mut lunge_velocity = vec2(0.0, 0.0);

        if let Moveset::Venomous { lunging, lunge_time, recovery, lunge_dir } = &mut self.moveset {
            if *recovery > 0.0 {
                // Winded — hold still and tick down. This is the catch window.
                *recovery -= dt;
                *lunging = false;
                recovering = true;
            } else if in_range {
                if *lunging {
                    *lunge_time += dt;
                    if dist < VENOM_HIT_DIST {
                        hit = true;
                        *recovery = VENOM_HIT_RECOVERY;
                        *lunging = false;
                    } else if *lunge_time > VENOM_LUNGE_TIMEOUT {
                        *recovery = VENOM_MISS_RECOVERY;
                        *lunging = false;
                    } else {
                        lunging_now = true;
                        lunge_velocity = *lunge_dir * VENOM_LUNGE_SPEED;
                    }
                } else if dist < VENOM_LUNGE_TRIGGER {
                    // Commit to a lunge, locking aim at the player.
                    *lunging = true;
                    *lunge_time = 0.0;
                    *lunge_dir = dir;
                    lunging_now = true;
                    lunge_velocity = dir * VENOM_LUNGE_SPEED;
                }
            } else {
                *lunging = false;
            }
        }

        let target_vel = if recovering {
            vec2(0.0, 0.0)
        } else if lunging_now {
            lunge_velocity
        } else if in_range {
            // Creep toward the player.
            dir * VENOM_STALK_SPEED
        } else {
            self.wander_velocity(dt)
        };

        let t = (750.0 * dt).min(1.0);
        self.vel = self.vel + (target_vel - self.vel) * t;
        self.pos += self.vel * dt;
        self.pos.x = self.pos.x.clamp(0.0, WORLD_W);
        self.pos.y = self.pos.y.clamp(0.0, WORLD_H);
        self.pos = resolve_zoo_collision(self.pos);

        hit.then_some(AiHit::Venom(self.pos))
    }

    /// How fast (progress / second, range 0–1) the capture circle fills.
    /// Slower movers also have slower fill so no moveset is trivially easy.
    pub fn fill_speed(&self) -> f32 {
        match &self.moveset {
            Moveset::Freezer         => 0.18,
            Moveset::Aggressor       => 0.24,
            Moveset::Vanisher { .. } => 0.28,
            Moveset::Burster { .. }  => 0.33,
            Moveset::Circler { .. }  => 0.36,
            Moveset::Basher { .. }   => 0.30,
            Moveset::Thrower { .. }  => 0.30,
            Moveset::Venomous { .. } => 0.30,
            _                        => 0.42,
        }
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Advance the Vanisher timer and apply teleportation when it fires.
    /// Split from `flee_velocity` so the `self.pos` write happens outside the
    /// mutable borrow on `self.moveset`.
    fn update_vanisher(&mut self, dt: f32, catch_mode: bool) {
        // Phase 1 — tick the timer and record what transition (if any) fired.
        // The borrow on self.moveset is released at the end of this block.
        let teleport_delta: Option<(f32, f32)> = {
            if let Moveset::Vanisher { ref mut timer, ref mut hidden } = self.moveset {
                if catch_mode {
                    *timer -= dt;
                }
                if catch_mode && *timer <= 0.0 {
                    if *hidden {
                        // Reappear: become visible, schedule next vanish.
                        *hidden = false;
                        *timer = rand::gen_range(2.5f32, 4.5);
                        let dx = rand::gen_range(-220.0f32, 220.0);
                        let dy = rand::gen_range(-220.0f32, 220.0);
                        Some((dx, dy))
                    } else {
                        // Vanish: become invisible.
                        *hidden = true;
                        *timer = 0.55;
                        None
                    }
                } else {
                    if !catch_mode && *hidden {
                        // Calm down → reappear.
                        *hidden = false;
                        *timer = rand::gen_range(2.5f32, 4.5);
                    }
                    None
                }
            } else {
                return; // not a Vanisher
            }
        };

        // Phase 2 — sync self.hidden and apply the teleport offset.
        // self.moveset is no longer borrowed here.
        if let Moveset::Vanisher { hidden, .. } = &self.moveset {
            self.hidden = *hidden;
        }
        if let Some((dx, dy)) = teleport_delta {
            self.pos.x = (self.pos.x + dx).clamp(0.0, WORLD_W);
            self.pos.y = (self.pos.y + dy).clamp(0.0, WORLD_H);
            self.pos = resolve_zoo_collision(self.pos);
        }
    }

    /// Compute target velocity while fleeing.
    /// `currently_hidden` is a pre-snapshotted copy of `self.hidden` so we
    /// don't need a second field borrow inside the match.
    fn flee_velocity(
        &mut self,
        dt: f32,
        cursor_world: Vec2,
        to_cursor: Vec2,
        dist: f32,
        currently_hidden: bool,
    ) -> Vec2 {
        let away = if dist > 0.1 { -to_cursor / dist } else { vec2(1.0, 0.0) };

        match &mut self.moveset {
            Moveset::Zigzagger { dir_timer, current_dir } => {
                *dir_timer -= dt;
                if *dir_timer <= 0.0 {
                    *dir_timer = rand::gen_range(0.30f32, 0.65);
                    // Deviate up to ±90° from straight-away.
                    let dev = rand::gen_range(-1.0f32, 1.0) * std::f32::consts::FRAC_PI_2;
                    *current_dir = safe_normalize(rotate(away, dev));
                }
                *current_dir * FLEE_SPEED
            }

            Moveset::Burster { phase_timer, dashing, dash_dir } => {
                *phase_timer -= dt;
                if *dashing {
                    if *phase_timer <= 0.0 {
                        *dashing = false;
                        *phase_timer = rand::gen_range(0.7f32, 1.4);
                    }
                    *dash_dir * (FLEE_SPEED * 2.6) // fast dash
                } else {
                    if *phase_timer <= 0.0 {
                        // Wind-up complete — launch the dash.
                        *dashing = true;
                        *phase_timer = rand::gen_range(0.28f32, 0.50);
                        let dev = rand::gen_range(-0.4f32, 0.4);
                        *dash_dir = safe_normalize(rotate(away, dev));
                    }
                    vec2(0.0, 0.0) // holding still before dash
                }
            }

            Moveset::Circler { angle, spin } => {
                *angle += *spin * 2.8 * dt;
                let orbit_pos = cursor_world
                    + vec2(angle.cos(), angle.sin()) * CIRCLER_ORBIT_RADIUS;
                let to_orbit = orbit_pos - self.pos;
                let d = to_orbit.length();
                if d > 1.0 { safe_normalize(to_orbit) * (FLEE_SPEED * 1.3) }
                else { vec2(0.0, 0.0) }
            }

            Moveset::Aggressor => {
                // Towards the cursor — the player must hold their nerve.
                let toward = if dist > 0.1 { to_cursor / dist } else { vec2(0.0, 0.0) };
                toward * (FLEE_SPEED * 0.80)
            }

            Moveset::Vanisher { .. } => {
                // Velocity is 0 while hidden; run away when visible.
                // Uses the pre-snapshotted flag to avoid a nested field borrow.
                if currently_hidden {
                    vec2(0.0, 0.0)
                } else {
                    away * (FLEE_SPEED * 1.05)
                }
            }

            Moveset::Freezer => away * (FLEE_SPEED * 0.22),

            Moveset::Panicker { stamina } => {
                *stamina = (*stamina - dt * 0.22).max(0.0);
                // Speed from 2.5× (fresh) down to 0.45× (exhausted).
                let speed_mult = 0.45 + *stamina * 2.05;
                // Jitter at high stamina for the "panicking" feel.
                let jitter = vec2(
                    rand::gen_range(-1.0f32, 1.0) * *stamina * 0.35,
                    rand::gen_range(-1.0f32, 1.0) * *stamina * 0.35,
                );
                safe_normalize(away + jitter) * (FLEE_SPEED * speed_mult)
            }

            // These integrate their own motion in dedicated update fns and
            // never reach the generic flee path.
            Moveset::Basher { .. } | Moveset::Thrower { .. } | Moveset::Venomous { .. } => {
                vec2(0.0, 0.0)
            }
        }
    }

    /// Compute target velocity while calmly wandering.
    fn wander_velocity(&mut self, dt: f32) -> Vec2 {
        // Restore per-moveset calm state.
        match &mut self.moveset {
            Moveset::Panicker { stamina } => {
                *stamina = (*stamina + dt * 0.12).min(1.0);
            }
            Moveset::Burster { dashing, phase_timer, .. } => {
                if *dashing {
                    // Reset so the next encounter starts from a hold phase.
                    *dashing = false;
                    *phase_timer = rand::gen_range(0.8f32, 1.5);
                }
            }
            _ => {}
        }
        // borrow of self.moveset is released here

        let to = self.wander_target - self.pos;
        let dist = to.length();
        if dist < 12.0 {
            self.wander_target = local_wander_point(self.pos);
            return vec2(0.0, 0.0);
        }
        // Occasional spontaneous pause mid-wander.
        if rand::gen_range(0.0f32, 1.0) < 0.003 {
            return vec2(0.0, 0.0);
        }
        safe_normalize(to) * WANDER_SPEED
    }
}

// ── Free helpers ──────────────────────────────────────────────────────────────

/// Rotate a 2D vector `v` by `angle` radians counter-clockwise.
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    vec2(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Normalize `v`; returns the zero vector when the length is negligible.
fn safe_normalize(v: Vec2) -> Vec2 {
    let len = v.length();
    if len < 0.001 { vec2(0.0, 0.0) } else { v / len }
}

/// Radius (world units) within which a calm wild animal picks its next wander
/// target. Keeps animals roaming their local neighborhood instead of striking
/// out across the (now enormous) world.
const WANDER_RADIUS: f32 = 600.0;

/// A random wander target within `WANDER_RADIUS` of `from`, clamped to the world.
pub fn local_wander_point(from: Vec2) -> Vec2 {
    let dx = rand::gen_range(-WANDER_RADIUS, WANDER_RADIUS);
    let dy = rand::gen_range(-WANDER_RADIUS, WANDER_RADIUS);
    vec2(
        (from.x + dx).clamp(0.0, WORLD_W),
        (from.y + dy).clamp(0.0, WORLD_H),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn thrower_lobs_on_cooldown_and_backs_away() {
        // Animal at (1000,1000); player off to +x so "away" is -x. Cursor sits
        // on the animal so it stays engaged (catch mode on).
        let start = vec2(1000.0, 1000.0);
        let player = vec2(2000.0, 1000.0);
        let mut a = WildAnimal::new("monkey", start, Moveset::thrower(), (0, 0), 0);

        let mut throws = 0;
        let mut consecutive = 0;
        let mut max_consecutive = 0;
        for _ in 0..300 {
            match a.update(DT, start, player, true) {
                Some(AiHit::Throw(_)) => {
                    throws += 1;
                    consecutive += 1;
                    max_consecutive = max_consecutive.max(consecutive);
                }
                _ => consecutive = 0,
            }
        }
        assert!(throws >= 2, "engaged Thrower should lob multiple times, got {throws}");
        assert_eq!(max_consecutive, 1, "throws must be spaced by a cooldown, not every frame");
        assert!(a.pos.x < start.x, "Thrower should back away from the player (-x), got {}", a.pos.x);
    }

    #[test]
    fn thrower_idle_when_not_engaged() {
        let start = vec2(1000.0, 1000.0);
        let player = vec2(2000.0, 1000.0);
        let mut a = WildAnimal::new("monkey", start, Moveset::thrower(), (0, 0), 0);
        // catch_mode off → never engaged → never throws.
        for _ in 0..300 {
            assert!(a.update(DT, start, player, false).is_none());
        }
    }

    #[test]
    fn venomous_stalks_toward_player_outside_catch_mode() {
        let start = vec2(1000.0, 1000.0);
        let player = vec2(1400.0, 1000.0); // dist 400: in stalk range, beyond lunge trigger
        let mut a = WildAnimal::new("treeFrog", start, Moveset::venomous(), (0, 0), 0);
        for _ in 0..30 {
            // catch_mode false — Venomous stalks regardless.
            a.update(DT, vec2(0.0, 0.0), player, false);
        }
        assert!(a.pos.x > start.x, "Venomous should creep toward the player (+x), got {}", a.pos.x);
    }

    #[test]
    fn venomous_lunge_lands_then_recovers() {
        let player = vec2(1000.0, 1000.0);
        // Place it just inside the lunge trigger so it commits immediately.
        let start = vec2(1000.0, 1160.0);
        let mut a = WildAnimal::new("treeFrog", start, Moveset::venomous(), (0, 0), 0);

        let mut hit_frame = None;
        for f in 0..60 {
            if let Some(AiHit::Venom(_)) = a.update(DT, vec2(0.0, 0.0), player, false) {
                hit_frame = Some(f);
                break;
            }
        }
        assert!(hit_frame.is_some(), "lunge should connect within timeout");

        // After a hit the animal recovers (holds still, emits nothing).
        let rest = a.pos;
        for _ in 0..3 {
            assert!(a.update(DT, vec2(0.0, 0.0), player, false).is_none());
        }
        assert!((a.pos - rest).length() < 5.0, "should hold still during recovery");
    }
}
