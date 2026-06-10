//! Entity-id generation seam.
//!
//! Every persistent entity (animal, habitat, pedestal, structure, nest, player,
//! wild animal, gift) gets a `Uuid`. Where that id *comes from* differs by build:
//!
//! - **Client / tests** (`std-ids` feature, on by default) — a random v4 UUID,
//!   exactly as before. Strong global uniqueness for saves, gifts, and the wire.
//! - **SpacetimeDB module** (`std-ids` off) — a **host-seeded deterministic**
//!   stream. `Uuid::new_v4()` is both unavailable on `wasm32-unknown-unknown`
//!   (no OS randomness) and *wrong* inside a reducer (reducers must be
//!   reproducible). Instead the reducer seeds this stream once per call from the
//!   host (timestamp ⊕ sender ⊕ a per-call nonce) and ids are drawn from it, so
//!   the same reducer invocation always yields the same ids.
//!
//! All domain constructors call [`new_id`] instead of `Uuid::new_v4()` directly,
//! so the two builds share one code path. Mirrors the thread-local determinism
//! of [`crate::game::rng`].

use uuid::Uuid;

#[cfg(feature = "std-ids")]
pub use std_ids::{new_id, seed_ids};

#[cfg(not(feature = "std-ids"))]
pub use det_ids::{new_id, seed_ids};

#[cfg(feature = "std-ids")]
mod std_ids {
    use super::Uuid;

    /// Fresh random entity id (v4 UUID).
    pub fn new_id() -> Uuid {
        Uuid::new_v4()
    }

    /// No-op on the client: v4 ids carry their own entropy. Present so module
    /// and client share the same call sites.
    pub fn seed_ids(_seed: u64) {}
}

#[cfg(not(feature = "std-ids"))]
mod det_ids {
    use super::Uuid;
    use std::cell::Cell;

    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0x2545_F491_4F6C_DD1D) };
    }

    /// Seed the deterministic id stream for this reducer invocation. Call once at
    /// the top of each reducer with a host-derived, per-call-unique seed so ids
    /// are reproducible yet don't collide across invocations.
    pub fn seed_ids(seed: u64) {
        STATE.with(|s| s.set(if seed == 0 { 0x2545_F491_4F6C_DD1D } else { seed }));
    }

    /// Next deterministic entity id, advancing the thread-local stream.
    pub fn new_id() -> Uuid {
        Uuid::from_u64_pair(next(), next())
    }

    /// splitmix64 — fast, decent-quality, fully deterministic from the seed.
    fn next() -> u64 {
        STATE.with(|s| {
            let z = s.get().wrapping_add(0x9E37_79B9_7F4A_7C15);
            s.set(z);
            let mut z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_across_calls() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b, "consecutive ids must differ");
    }
}
