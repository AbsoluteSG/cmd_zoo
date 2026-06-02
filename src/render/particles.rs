//! Texture-free, object-pooled particle system. Particles are drawn as raw
//! pixels (small filled rects) — no assets — and live in flat world space so
//! they pan/zoom with the camera and depth-blend with the scene.
//!
//! Pooling: a fixed-capacity `Vec<Particle>` is pre-allocated once; spawning
//! reuses dead slots via a round-robin cursor and never grows the backing
//! store, so a burst-heavy frame can't allocate or grow unbounded. A particle
//! whose `life` has run out is a free slot.
//!
//! Safe to call from `render::world::draw_scene` (which may render into an
//! offscreen target): it draws only primitives, never text.

use macroquad::prelude::*;

use super::view::{self, Camera};

/// One pooled particle. Integrated each frame with simple gravity + drag.
#[derive(Clone, Copy)]
struct Particle {
    /// Flat world-space position (same units as critters).
    pos: Vec2,
    /// World-space velocity (units/s).
    vel: Vec2,
    /// Seconds of life remaining; `<= 0` marks the slot free.
    life: f32,
    /// Lifetime the particle was spawned with — drives the alpha fade.
    max_life: f32,
    /// Base pixel size at zoom 1.0 (scaled by `cam.zoom` when drawn).
    size: f32,
    /// Base colour; alpha is multiplied by the remaining-life fraction.
    color: Color,
    /// Downward (positive world-y) acceleration, units/s².
    gravity: f32,
    /// Per-second velocity damping (0 = none).
    drag: f32,
}

impl Particle {
    const DEAD: Particle = Particle {
        pos: Vec2::ZERO,
        vel: Vec2::ZERO,
        life: 0.0,
        max_life: 1.0,
        size: 1.0,
        color: WHITE,
        gravity: 0.0,
        drag: 0.0,
    };
}

/// Hard cap on live particles. The pool is allocated once at this size and
/// never grows; spawning past it overwrites the oldest slot, which is fine for
/// short-lived cosmetic bursts.
const CAP: usize = 1024;

/// A fixed-capacity pool of pixel particles plus the emitters that seed them.
pub struct Particles {
    pool: Vec<Particle>,
    /// Round-robin write head — where the next spawned particle goes.
    cursor: usize,
}

impl Default for Particles {
    fn default() -> Self {
        Self::new()
    }
}

impl Particles {
    /// Pre-allocate the pool full of dead particles. No further allocation
    /// happens during play.
    pub fn new() -> Self {
        Self {
            pool: vec![Particle::DEAD; CAP],
            cursor: 0,
        }
    }

    /// Number of currently-live particles (for tests / debugging).
    #[cfg(test)]
    fn live(&self) -> usize {
        self.pool.iter().filter(|p| p.life > 0.0).count()
    }

    /// Advance every live particle: integrate gravity, drag, motion, and age.
    pub fn update(&mut self, dt: f32) {
        for p in &mut self.pool {
            if p.life <= 0.0 {
                continue;
            }
            p.vel.y += p.gravity * dt;
            p.vel *= 1.0 / (1.0 + p.drag * dt);
            p.pos += p.vel * dt;
            p.life -= dt;
        }
    }

    /// Project each live particle to the screen and draw it as a pixel square.
    /// Alpha fades with the remaining-life fraction. No textures, no text.
    pub fn draw(&self, cam: &Camera) {
        for p in &self.pool {
            if p.life <= 0.0 {
                continue;
            }
            let t = (p.life / p.max_life).clamp(0.0, 1.0);
            let s = (p.size * cam.zoom).max(1.0);
            let screen = view::world_to_screen(p.pos, cam);
            let c = Color::new(p.color.r, p.color.g, p.color.b, p.color.a * t);
            draw_rectangle(screen.x - s * 0.5, screen.y - s * 0.5, s, s, c);
        }
    }

    /// Spawn one particle into the next pool slot (overwriting the oldest when
    /// full), advancing the round-robin cursor.
    fn spawn(&mut self, p: Particle) {
        self.pool[self.cursor] = p;
        self.cursor = (self.cursor + 1) % CAP;
    }

    /// Generic burst. `dir` is the central launch direction (normalized
    /// internally); a near-zero `dir` emits a full radial spray. `spread` is
    /// the half-angle (radians) around `dir`. Ranges are `(min, max)`.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        pos: Vec2,
        n: usize,
        color: Color,
        speed: (f32, f32),
        dir: Vec2,
        spread: f32,
        gravity: f32,
        life: (f32, f32),
        size: (f32, f32),
    ) {
        let (base, half) = if dir.length_squared() < 1e-6 {
            (0.0, std::f32::consts::PI) // full circle
        } else {
            (dir.y.atan2(dir.x), spread)
        };
        for _ in 0..n {
            let a = base + rand::gen_range(-half, half);
            let spd = rand::gen_range(speed.0, speed.1);
            let l = rand::gen_range(life.0, life.1);
            // Small per-particle brightness jitter so the burst doesn't look flat.
            let j = rand::gen_range(0.85, 1.0);
            self.spawn(Particle {
                pos,
                vel: vec2(a.cos(), a.sin()) * spd,
                life: l,
                max_life: l,
                size: rand::gen_range(size.0, size.1),
                color: Color::new(color.r * j, color.g * j, color.b * j, color.a),
                gravity,
                drag: 1.2,
            });
        }
    }

    // ── Event emitters ──────────────────────────────────────────────────────

    /// Gold coin burst — pops upward then arcs back down. Used on income redeem.
    pub fn coins(&mut self, pos: Vec2) {
        const GOLD: Color = color_u8!(255, 210, 90, 255);
        self.emit(pos, 14, GOLD, (70.0, 170.0), vec2(0.0, -1.0), 0.8, 340.0, (0.4, 0.8), (2.0, 4.0));
    }

    /// Pink DNA burst — same motion as coins, used when DNA income is redeemed.
    pub fn dna(&mut self, pos: Vec2) {
        const PINK: Color = color_u8!(196, 120, 220, 255);
        self.emit(pos, 14, PINK, (70.0, 170.0), vec2(0.0, -1.0), 0.8, 340.0, (0.4, 0.8), (2.0, 4.0));
    }

    /// Celebratory radial burst when a wild animal is finally captured.
    pub fn capture(&mut self, pos: Vec2) {
        const GREEN: Color = color_u8!(180, 255, 110, 255);
        const WHITEISH: Color = color_u8!(235, 255, 220, 255);
        self.emit(pos, 22, GREEN, (90.0, 230.0), Vec2::ZERO, 0.0, 140.0, (0.5, 0.9), (2.0, 4.0));
        self.emit(pos, 10, WHITEISH, (60.0, 160.0), Vec2::ZERO, 0.0, 80.0, (0.4, 0.8), (1.5, 3.0));
    }

    /// Short impact debris when a Basher connects. `dir` biases the spray
    /// (pass `Vec2::ZERO` for a fully radial pop).
    pub fn impact(&mut self, pos: Vec2, dir: Vec2) {
        const SPARK: Color = color_u8!(255, 180, 90, 255);
        self.emit(pos, 16, SPARK, (120.0, 280.0), dir, 1.1, 260.0, (0.2, 0.45), (2.0, 4.0));
    }

    /// Soft sparkle drifting upward when a gestation completes (a birth).
    pub fn birth(&mut self, pos: Vec2) {
        const SOFT: Color = color_u8!(255, 240, 200, 255);
        self.emit(pos, 18, SOFT, (30.0, 90.0), vec2(0.0, -1.0), 1.0, -40.0, (0.8, 1.4), (2.0, 3.5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_never_grows_and_caps_live_count() {
        let mut p = Particles::new();
        let cap_before = p.pool.capacity();
        // Spawn far more than CAP across several bursts.
        for _ in 0..200 {
            p.coins(vec2(0.0, 0.0));
        }
        assert_eq!(p.pool.len(), CAP, "pool length stays fixed at CAP");
        assert_eq!(p.pool.capacity(), cap_before, "pool never reallocates");
        assert!(p.live() <= CAP, "live count can never exceed the pool");
    }

    #[test]
    fn particles_die_after_their_lifetime() {
        let mut p = Particles::new();
        p.capture(vec2(10.0, 10.0));
        assert!(p.live() > 0, "burst seeds live particles");
        // Step well past the longest configured lifetime (<= 0.9s).
        for _ in 0..120 {
            p.update(1.0 / 60.0);
        }
        assert_eq!(p.live(), 0, "all particles expire after their lifetime");
    }
}
