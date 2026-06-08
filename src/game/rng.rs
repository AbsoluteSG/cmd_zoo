//! Tiny headless RNG shim mirroring the slice of `macroquad::rand` the domain
//! uses (`gen_range`), so the simulation can run without a graphics engine.
//!
//! Behaviour parity with macroquad's generator is **not** required — this only
//! needs to produce reasonable pseudo-random values for procedural placement and
//! wild-animal jitter. A per-thread xorshift64 keeps it allocation-free and
//! deterministic per thread.

use std::cell::Cell;

thread_local! {
    // Nonzero seed; advances on every draw.
    static STATE: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) };
}

fn next_u64() -> u64 {
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

/// Uniform float in [0, 1).
fn next_f64() -> f64 {
    // 53-bit mantissa worth of randomness.
    (next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// Re-seed the thread-local generator (parity with `macroquad::rand::srand`).
#[allow(dead_code)]
pub fn srand(seed: u64) {
    STATE.with(|s| s.set(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed }));
}

/// A value type that `gen_range` can sample over a half-open range.
pub trait RangeSample {
    fn sample(low: Self, high: Self) -> Self;
}

impl RangeSample for f32 {
    fn sample(low: Self, high: Self) -> Self {
        if high <= low {
            return low;
        }
        low + (high - low) * next_f64() as f32
    }
}

impl RangeSample for f64 {
    fn sample(low: Self, high: Self) -> Self {
        if high <= low {
            return low;
        }
        low + (high - low) * next_f64()
    }
}

impl RangeSample for i32 {
    fn sample(low: Self, high: Self) -> Self {
        if high <= low {
            return low;
        }
        let span = (high - low) as u64;
        low + (next_u64() % span) as i32
    }
}

/// Random value in `[low, high)` — drop-in for `macroquad::rand::gen_range`.
pub fn gen_range<T: RangeSample>(low: T, high: T) -> T {
    T::sample(low, high)
}
