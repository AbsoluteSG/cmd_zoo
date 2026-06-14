//! Collections (Tiny-Zoo-style sets) — own every species in a named set, then
//! **claim once** for a reward: currency, or an exclusive animal that's granted
//! at **max rank (Neon)** and obtainable *no other way* (see
//! [`crate::game::species::COLLECTION_ONLY_IDS`]).
//!
//! Pure data + lookup; the claim logic + persistence live on [`crate::game::zoo`]
//! and flow through `apply_action`, so the same rules run solo and online.

use crate::game::species::SpeciesId;

/// What completing a collection grants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reward {
    /// A coin payout.
    Coins(u64),
    /// A DNA Helix payout.
    Dna(u64),
    /// An exclusive animal (granted at max rank).
    Animal(SpeciesId),
}

/// A named collection: own all of `required` to claim its `reward`.
#[derive(Clone, Copy, Debug)]
pub struct Collection {
    pub id: &'static str,
    pub name: &'static str,
    pub required: &'static [SpeciesId],
    pub reward: Reward,
}

/// **The collection roster.** Edit here to add/retune collections. The first five
/// use existing animals; the last five introduce new obtainable starters.
pub static COLLECTIONS: &[Collection] = &[
    // ── Built from existing animals ───────────────────────────────────────────
    Collection {
        id: "barnyard_bunch",
        name: "Barnyard Bunch",
        required: &["rabbit", "chicken", "sheep", "goat", "cow"],
        reward: Reward::Animal("golden_goose"),
    },
    Collection {
        id: "woodland_watch",
        name: "Woodland Watch",
        required: &["red_fox", "badger", "raccoon", "squirrel", "robin"],
        reward: Reward::Coins(4_000),
    },
    Collection {
        id: "frozen_few",
        name: "Frozen Few",
        required: &["penguin", "snowyOwl", "arctic_fox", "seal", "polar_bear"],
        reward: Reward::Animal("aurora_bear"),
    },
    Collection {
        id: "tide_pool",
        name: "Tide Pool",
        required: &["otter", "dolphin", "octopus", "crab", "pufferfish"],
        reward: Reward::Dna(30),
    },
    Collection {
        id: "savanna_kings",
        name: "Savanna Kings",
        required: &["lion", "zebra", "giraffe", "elephant", "cheetah"],
        reward: Reward::Animal("sunmane_lion"),
    },
    // ── Built from new animals ────────────────────────────────────────────────
    Collection {
        id: "mission_of_love",
        name: "Mission of Love",
        required: &["wedding_dove", "flamingo", "ruby_rabbit"],
        reward: Reward::Animal("cupid_swan"),
    },
    Collection {
        id: "tiny_tots",
        name: "Tiny Tots",
        required: &["ducklet", "lamblet", "piglet"],
        reward: Reward::Animal("golden_chick"),
    },
    Collection {
        id: "garden_sprites",
        name: "Garden Sprites",
        required: &["petal_deer", "glow_moth", "dew_sprite"],
        reward: Reward::Animal("bloomcat"),
    },
    Collection {
        id: "big_top",
        name: "Big Top",
        required: &["dapper_seal", "acro_monkey", "tophat_bear"],
        reward: Reward::Animal("ringmaster_lion"),
    },
    Collection {
        id: "starlight_trio",
        name: "Starlight Trio",
        required: &["comet_cat", "lunar_hare", "solar_finch"],
        reward: Reward::Animal("astral_owl"),
    },
];

/// All collections, in display order.
pub fn all() -> &'static [Collection] {
    COLLECTIONS
}

/// Look up a collection by id.
pub fn get(id: &str) -> Option<&'static Collection> {
    COLLECTIONS.iter().find(|c| c.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::species;

    #[test]
    fn ids_are_unique_and_species_resolve() {
        let mut seen = std::collections::HashSet::new();
        for c in COLLECTIONS {
            assert!(seen.insert(c.id), "duplicate collection id: {}", c.id);
            for s in c.required {
                assert!(species::try_get(s).is_some(), "{}: unknown required species {s}", c.id);
            }
            if let Reward::Animal(sp) = c.reward {
                assert!(species::try_get(sp).is_some(), "{}: unknown reward species {sp}", c.id);
                assert!(
                    species::is_collection_only(sp),
                    "{}: reward {sp} should be in COLLECTION_ONLY_IDS",
                    c.id
                );
            }
        }
    }

    #[test]
    fn get_resolves() {
        assert_eq!(get("frozen_few").unwrap().name, "Frozen Few");
        assert!(get("nope").is_none());
    }
}
