use once_cell::sync::Lazy;
use std::collections::HashMap;

pub type SpeciesId = &'static str;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HabitatTheme {
    Farmland,
    Forest,
    Arctic,
    Savanna,
    Wetland,
    Jungle,
    Ocean,
}

impl HabitatTheme {
    pub fn name(self) -> &'static str {
        match self {
            HabitatTheme::Farmland => "Farmland",
            HabitatTheme::Forest => "Forest",
            HabitatTheme::Arctic => "Arctic",
            HabitatTheme::Savanna => "Savanna",
            HabitatTheme::Wetland => "Wetland",
            HabitatTheme::Jungle => "Jungle",
            HabitatTheme::Ocean => "Ocean"
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Farmland" => Some(Self::Farmland),
            "Forest" => Some(Self::Forest),
            "Arctic" => Some(Self::Arctic),
            "Savanna" => Some(Self::Savanna),
            "Wetland" => Some(Self::Wetland),
            "Jungle" => Some(Self::Jungle),
            "Ocean" => Some(Self::Ocean),
            _ => None,
        }
    }
}

/// What currency an animal's income drains into when collected at cap.
/// Most species produce coins; the rarest exotic-tier species produce
/// DNA Helix directly, completing the late-game economy loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncomeKind {
    Coin,
    DnaHelix,
}

pub struct SpeciesDef {
    pub id: SpeciesId,
    pub display_name: &'static str,
    pub theme: HabitatTheme,
    pub base_rate_per_sec: f64,
    pub base_storage_cap: u64,
    pub purchase_cost: u64,
    pub gestation_seconds: u64,
    /// Per-level rate multiplier bonus: rate(L) = base_rate * (1 + bonus*(L-1)).
    /// 0.5 = balanced, 0.8+ = "sprinter", 0.2 = "tank".
    pub level_rate_bonus: f64,
    /// Per-level capacity multiplier bonus: cap(L) = base_cap * (1 + bonus*(L-1)).
    /// 1.0 = balanced (cap doubles between L1 and L2), 1.5+ = "tank", 0.5 = "sprinter".
    pub level_cap_bonus: f64,
    /// True for species only obtainable via crossbreeding — kept out of the
    /// shop offerings but valid for `auto_place_animal` and snapshots.
    pub hybrid: bool,
    /// True for premium "exotic" species sold only in the time-windowed
    /// exotic shop. Independent of `hybrid`: an exotic is not a crossbreed
    /// drop, and is excluded from the regular Animals shop tab.
    pub exotic: bool,
    /// Coin (default) or DnaHelix (rare exotics). Used by `collect_habitat`
    /// to route at-cap income into the right currency bucket.
    pub income_kind: IncomeKind,
    /// Which currency `buy_animal` charges `purchase_cost` in. Coin by
    /// default; premium species (e.g. exotics) can be sold for DNA Helix.
    /// Independent of `income_kind` — what an animal *costs* and what it
    /// *produces* are separate decisions.
    pub purchase_currency: IncomeKind,
}

/// One entry in a crossbreed outcome pool: a possible offspring species
/// weighted against the others in its pool. Weights sum to whatever, the
/// roller computes the total per call.
#[derive(Clone, Copy, Debug)]
pub struct PoolEntry {
    pub species: SpeciesId,
    pub weight: u32,
}

/// Convenience constants for the three scaling archetypes. Construction-time
/// only — these are the values used in `CATALOG` entries below.
pub mod scaling {
    /// Even scaling — current default.
    pub const BALANCED: (f64, f64) = (0.5, 1.0);
    /// Trades capacity for raw output. Best for active players who collect often.
    pub const SPRINTER: (f64, f64) = (0.8, 0.4);
    /// Trades rate for storage. Best for idle/offline play.
    pub const TANK: (f64, f64) = (0.2, 1.6);
}

static CATALOG: Lazy<HashMap<SpeciesId, SpeciesDef>> = Lazy::new(|| {
    let entries = [
        SpeciesDef {
            id: "field_mouse",
            display_name: "Field Mouse",            
            theme: HabitatTheme::Forest,
            base_rate_per_sec: 0.5,
            base_storage_cap: 60,
            purchase_cost: 25,
            gestation_seconds: 60,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "blue_frog",
            display_name: "Blue Frog",
            theme: HabitatTheme::Wetland,
            base_rate_per_sec: 2.0,
            base_storage_cap: 120,
            purchase_cost: 50,
            gestation_seconds: 90,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        // Tank: slow trickle, huge storage — leave it overnight.
        SpeciesDef {
            id: "treeFrog",
            display_name: "Tree Frog",            
            theme: HabitatTheme::Wetland,
            base_rate_per_sec: 0.8,
            base_storage_cap: 80,
            purchase_cost: 60,
            gestation_seconds: 90,
            level_rate_bonus: scaling::TANK.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "penguin",
            display_name: "Penguin",            
            theme: HabitatTheme::Arctic,
            base_rate_per_sec: 1.2,
            base_storage_cap: 200,
            purchase_cost: 150,
            gestation_seconds: 180,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "snowyOwl",
            display_name: "Snowy Owl",            
            theme: HabitatTheme::Arctic,
            base_rate_per_sec: 1.5,
            base_storage_cap: 100,
            purchase_cost: 120,
            gestation_seconds: 240,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "giantTortoise",
            display_name: "Giant Tortoise",            
            theme: HabitatTheme::Savanna,
            base_rate_per_sec: 0.3,
            base_storage_cap: 500,
            purchase_cost: 150,
            gestation_seconds: 600,
            level_rate_bonus: scaling::TANK.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        // Sprinter: small storage but earns fast when you check on it.
        SpeciesDef {
            id: "monkey",
            display_name: "Capuchin",            
            theme: HabitatTheme::Jungle,
            base_rate_per_sec: 2.0,
            base_storage_cap: 400,
            purchase_cost: 400,
            gestation_seconds: 240,
            level_rate_bonus: scaling::SPRINTER.0,
            level_cap_bonus: scaling::SPRINTER.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "goldenToucan",
            display_name: "Golden Toucan",            
            theme: HabitatTheme::Jungle,
            base_rate_per_sec: 1.8,
            base_storage_cap: 130,
            purchase_cost: 140,
            gestation_seconds: 220,
            level_rate_bonus: scaling::SPRINTER.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "lion",
            display_name: "Lion",            
            theme: HabitatTheme::Savanna,
            base_rate_per_sec: 3.5,
            base_storage_cap: 800,
            purchase_cost: 1000,
            gestation_seconds: 360,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        // Sprinter: foxes lean toward burst output.
        SpeciesDef {
            id: "fox",
            display_name: "Red Fox",            
            theme: HabitatTheme::Forest,
            base_rate_per_sec: 1.5,
            base_storage_cap: 240,
            purchase_cost: 250,
            gestation_seconds: 150,
            level_rate_bonus: scaling::SPRINTER.0,
            level_cap_bonus: scaling::SPRINTER.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "reefSeahorse",
            display_name: "Reef Seahorse",            
            theme: HabitatTheme::Ocean,
            base_rate_per_sec: 1.0,
            base_storage_cap: 180,
            purchase_cost: 130,
            gestation_seconds: 260,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "otter",
            display_name: "River Otter",            
            theme: HabitatTheme::Wetland,
            base_rate_per_sec: 1.8,
            base_storage_cap: 320,
            purchase_cost: 320,
            gestation_seconds: 200,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        // Tank: the king of overnight idle.
        SpeciesDef {
            id: "polar_bear",
            display_name: "Polar Bear",            
            theme: HabitatTheme::Arctic,
            base_rate_per_sec: 4.0,
            base_storage_cap: 1200,
            purchase_cost: 1500,
            gestation_seconds: 420,
            level_rate_bonus: scaling::TANK.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        // Sprinter: noisy, fast, doesn't store much.
        SpeciesDef {
            id: "toucan",
            display_name: "Toucan",            
            theme: HabitatTheme::Jungle,
            base_rate_per_sec: 1.0,
            base_storage_cap: 180,
            purchase_cost: 180,
            gestation_seconds: 120,
            level_rate_bonus: scaling::SPRINTER.0,
            level_cap_bonus: scaling::SPRINTER.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "albinoDeer",
            display_name: "Albino Deer",            
            theme: HabitatTheme::Forest,
            base_rate_per_sec: 2.0,
            base_storage_cap: 260,
            purchase_cost: 400,
            gestation_seconds: 420,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "zebra",
            display_name: "Zebra",            
            theme: HabitatTheme::Savanna,
            base_rate_per_sec: 2.5,
            base_storage_cap: 600,
            purchase_cost: 650,
            gestation_seconds: 300,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "goldenCarp",
            display_name: "Golden Carp",            
            theme: HabitatTheme::Ocean,
            base_rate_per_sec: 3.2,
            base_storage_cap: 320,
            purchase_cost: 600,
            gestation_seconds: 450,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "cow",
            display_name: "Cow",            
            theme: HabitatTheme::Farmland,
            base_rate_per_sec: 2.0,
            base_storage_cap: 2000,
            purchase_cost: 1000,
            gestation_seconds: 660,
            level_rate_bonus: scaling::TANK.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: false,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        // ─── Hybrids (crossbreeding only; not shown in shop) ─────────────────
        // Hybrids stay out of the shop via `hybrid: true`, NOT via a zero
        // purchase_cost — animal_level_up_cost reads purchase_cost, so a zero
        // here would make hybrid levelling free. These values are the
        // hybrid's level-up baseline, sized to feel premium vs base species.
        SpeciesDef {
            id: "frox",
            display_name: "Frox",            
            theme: HabitatTheme::Forest,
            base_rate_per_sec: 2.5,
            base_storage_cap: 500,
            purchase_cost: 800,
            gestation_seconds: 240,
            level_rate_bonus: scaling::SPRINTER.0,
            level_cap_bonus: scaling::SPRINTER.1,
            hybrid: true,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "prism_seahorse",
            display_name: "Prism Seahorse",            
            theme: HabitatTheme::Ocean,
            base_rate_per_sec: 35.0,
            base_storage_cap: 18000,
            purchase_cost: 800,
            gestation_seconds: 259200,
            level_rate_bonus: scaling::SPRINTER.0,
            level_cap_bonus: scaling::SPRINTER.1,
            hybrid: true,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "butter_horse",
            display_name: "Butter Horse",            
            theme: HabitatTheme::Farmland,
            base_rate_per_sec: 5.0,
            base_storage_cap: 70000,
            purchase_cost: 800,
            gestation_seconds: 185760,
            level_rate_bonus: scaling::TANK.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: true,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "otterfly",
            display_name: "Otterfly",            
            theme: HabitatTheme::Wetland,
            base_rate_per_sec: 3.5,
            base_storage_cap: 800,
            purchase_cost: 1200,
            gestation_seconds: 360,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: true,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "yellow_yellow",
            display_name: "Yellow Yellow",            
            theme: HabitatTheme::Forest,
            base_rate_per_sec: 15.5,
            base_storage_cap: 12000,
            purchase_cost: 1200,
            gestation_seconds: 53100,
            level_rate_bonus: scaling::TANK.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: true,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        // The crown jewel: Snow Lion's income IS DNA Helix, completing the
        // late-game loop. base_rate × base_cap chosen so an L1 at cap pays
        // out a small DNA trickle — collect a couple of these and you can
        // sustain the exotic shop.
        SpeciesDef {
            id: "snowLion",
            display_name: "Snow Lion",
            theme: HabitatTheme::Arctic,
            base_rate_per_sec: 0.02,
            base_storage_cap: 5,
            purchase_cost: 3000,
            gestation_seconds: 720,
            level_rate_bonus: scaling::TANK.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: true,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "monkan",
            display_name: "Monkan",            
            theme: HabitatTheme::Jungle,
            base_rate_per_sec: 10.0,
            base_storage_cap: 500,
            purchase_cost: 1000,
            gestation_seconds: 300,
            level_rate_bonus: scaling::SPRINTER.0,
            level_cap_bonus: scaling::SPRINTER.1,
            hybrid: true,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "zebraLion",
            display_name: "Savanna Striped Zebra",            
            theme: HabitatTheme::Savanna,
            base_rate_per_sec: 5.5,
            base_storage_cap: 1600,
            purchase_cost: 2500,
            gestation_seconds: 660,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: true,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "sun_bear",
            display_name: "Sun Bear",
            theme: HabitatTheme::Forest,
            base_rate_per_sec: 10.0,
            base_storage_cap: 7300,
            purchase_cost: 5000,
            gestation_seconds: 18720,
            level_rate_bonus: scaling::TANK.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: true,
            exotic: false,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        // ─── Exotics (time-windowed exotic shop only) ────────────────────────
        // Not crossbreed drops (hybrid: false) and excluded from the regular
        // Animals tab (exotic: true). Premium-priced; the rarest pay out DNA
        // Helix directly to feed the late-game loop.
        SpeciesDef {
            id: "biggy_cheese",
            display_name: "Biggy Cheese",
            theme: HabitatTheme::Farmland,
            base_rate_per_sec: 32.0,
            base_storage_cap: 12500,
            purchase_cost: 999999,
            gestation_seconds: 67500,
            level_rate_bonus: scaling::SPRINTER.0,
            level_cap_bonus: scaling::SPRINTER.1,
            hybrid: false,
            exotic: true,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "phoenix",
            display_name: "Phoenix",
            theme: HabitatTheme::Savanna,
            base_rate_per_sec: 10.0,
            base_storage_cap: 50000,
            purchase_cost: 5000000,
            gestation_seconds: 86400,
            level_rate_bonus: scaling::SPRINTER.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: true,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "kraken",
            display_name: "Kraken",
            theme: HabitatTheme::Ocean,
            base_rate_per_sec: 0.03,
            base_storage_cap: 5,
            purchase_cost: 999,
            gestation_seconds: 432000,
            level_rate_bonus: scaling::TANK.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: false,
            exotic: true,
            income_kind: IncomeKind::DnaHelix,
            purchase_currency: IncomeKind::DnaHelix,
        },
        SpeciesDef {
            id: "unicorn",
            display_name: "Unicorn",
            theme: HabitatTheme::Forest,
            base_rate_per_sec: 4.5,
            base_storage_cap: 2400,
            purchase_cost: 50505,
            gestation_seconds: 30600,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::TANK.1,
            hybrid: false,
            exotic: true,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::Coin,
        },
        SpeciesDef {
            id: "stained_butterfly",
            display_name: "Stained Butterfly",
            theme: HabitatTheme::Forest,
            base_rate_per_sec: 2.0,
            base_storage_cap: 17000,
            purchase_cost: 39,
            gestation_seconds: 36000,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: true,
            income_kind: IncomeKind::Coin,
            purchase_currency: IncomeKind::DnaHelix,
        },
        SpeciesDef {
            id: "birthday_horse",
            display_name: "Birthday Horse",
            theme: HabitatTheme::Farmland,
            base_rate_per_sec: 0.1,
            base_storage_cap: 3,
            purchase_cost: 199,
            gestation_seconds: 780,
            level_rate_bonus: scaling::BALANCED.0,
            level_cap_bonus: scaling::BALANCED.1,
            hybrid: false,
            exotic: true,
            income_kind: IncomeKind::DnaHelix,
            purchase_currency: IncomeKind::DnaHelix,
        },
    ];
    entries.into_iter().map(|d| (d.id, d)).collect()
});

/// Crossbreed pools. Pair lookup is order-insensitive (we insert both
/// orderings below). Each pool lists the legal offspring species and their
/// relative weights — parents go in at high weights so most pairings give
/// you a parent back; the hybrid is the rare drop.
///
/// Reading the weights: the roller picks a cumulative-weight slot, so for
/// `[(fox, 45), (treeFrog, 45), (frox, 10)]` the hybrid lands ~10% of the
/// time and each parent ~45%. Weights are integers (not percentages) for
/// clarity when tuning — feel free to use any positive scale.
///
/// Adding a new pair: define its pool here. The new entry is automatically
/// picked up by `crossbreed_pool`, `all_recipes` (for the codex), and the
/// breeding-tab candidate filter.
static RECIPES: Lazy<HashMap<(SpeciesId, SpeciesId), Vec<PoolEntry>>> = Lazy::new(|| {
    let raw: &[(SpeciesId, SpeciesId, &[PoolEntry])] = &[
        (
            "fox",
            "treeFrog",
            &[
                PoolEntry { species: "fox", weight: 45 },
                PoolEntry { species: "treeFrog", weight: 45 },
                PoolEntry { species: "frox", weight: 10 },
            ],
        ),
        (
            "otter",
            "toucan",
            &[
                PoolEntry { species: "otter", weight: 45 },
                PoolEntry { species: "toucan", weight: 45 },
                PoolEntry { species: "otterfly", weight: 10 },
            ],
        ),
        (
            "penguin",
            "lion",
            &[
                PoolEntry { species: "penguin", weight: 47 },
                PoolEntry { species: "lion", weight: 47 },
                PoolEntry { species: "snowLion", weight: 6 },
            ],
        ),
        (
            "monkey",
            "toucan",
            &[
                PoolEntry { species: "monkey", weight: 45 },
                PoolEntry { species: "toucan", weight: 45 },
                PoolEntry { species: "monkan", weight: 10 },
            ],
        ),
        (
            "zebra",
            "lion",
            &[
                PoolEntry { species: "zebra", weight: 45 },
                PoolEntry { species: "lion", weight: 45 },
                PoolEntry { species: "zebraLion", weight: 10 },
            ],
        ),
        (
            "biggy_cheese",
            "polar_bear",
            &[
                PoolEntry { species: "biggy_cheese", weight: 5 },
                PoolEntry { species: "polar_bear", weight: 40 },
                PoolEntry { species: "yellow_yellow", weight: 3 },
                PoolEntry { species: "sun_bear", weight: 15 },
            ],
        ),
        (
            "stained_butterfly",
            "reefSeahorse",
            &[
                PoolEntry { species: "reefSeahorse", weight: 50 },
                PoolEntry { species: "stained_butterfly", weight: 10 },
                PoolEntry { species: "prism_seahorse", weight: 5 },
                PoolEntry { species: "butter_horse", weight: 1 },
            ],
        ),
    ];
    let mut out: HashMap<(SpeciesId, SpeciesId), Vec<PoolEntry>> = HashMap::new();
    for (a, b, pool) in raw {
        out.insert((*a, *b), pool.to_vec());
        out.insert((*b, *a), pool.to_vec());
    }
    out
});

pub fn get(id: SpeciesId) -> &'static SpeciesDef {
    CATALOG
        .get(id)
        .unwrap_or_else(|| panic!("unknown species id: {id}"))
}

pub fn try_get(id: &str) -> Option<&'static SpeciesDef> {
    CATALOG.get(id)
}

/// Regular shop species — neither crossbreed offspring nor exotics.
pub fn all_purchasable() -> impl Iterator<Item = &'static SpeciesDef> {
    CATALOG.values().filter(|d| !d.hybrid && !d.exotic)
}

/// Hybrid species. Used by the codex.
pub fn all_hybrids() -> impl Iterator<Item = &'static SpeciesDef> {
    CATALOG.values().filter(|d| d.hybrid)
}

/// Exotic species. Used by the time-windowed exotic shop pool.
pub fn all_exotics() -> impl Iterator<Item = &'static SpeciesDef> {
    CATALOG.values().filter(|d| d.exotic)
}

/// Pool of possible offspring species for the given pair, if a legal pool
/// exists. Order-insensitive. Returns `None` for unknown / illegal pairs —
/// `Zoo::start_breeding` rejects those with `SpeciesMismatch`.
pub fn crossbreed_pool(a: SpeciesId, b: SpeciesId) -> Option<&'static [PoolEntry]> {
    RECIPES.get(&(a, b)).map(|v| v.as_slice())
}

/// All crossbreed pairs that produce a hybrid (non-parent) offspring,
/// deduped to one entry per canonical pair. Used by the codex view to list
/// "what can I discover here?" — the third tuple slot is the hybrid species.
pub fn all_recipes() -> Vec<(SpeciesId, SpeciesId, SpeciesId)> {
    let mut seen: std::collections::HashSet<(SpeciesId, SpeciesId)> = std::collections::HashSet::new();
    let mut out: Vec<(SpeciesId, SpeciesId, SpeciesId)> = Vec::new();
    for ((a, b), pool) in RECIPES.iter() {
        let key = if a <= b { (*a, *b) } else { (*b, *a) };
        if !seen.insert(key) {
            continue;
        }
        // The hybrid in a pool is any entry whose species is neither parent.
        // There's at most one in our current catalog.
        if let Some(hybrid) = pool
            .iter()
            .find(|e| e.species != *a && e.species != *b)
        {
            out.push((key.0, key.1, hybrid.species));
        }
    }
    out.sort_by(|x, y| x.0.cmp(y.0).then(x.1.cmp(y.1)));
    out
}

/// Walk a pool's cumulative weight using `seed` and return the chosen
/// species. Panics if the pool is empty or all weights are zero — the
/// catalog must never define such a pool.
pub fn roll_pool(seed: u64, pool: &[PoolEntry]) -> SpeciesId {
    debug_assert!(!pool.is_empty(), "roll_pool called on empty pool");
    let total: u64 = pool.iter().map(|e| e.weight as u64).sum();
    debug_assert!(total > 0, "roll_pool called on zero-weight pool");
    let mut pick = seed % total;
    for entry in pool {
        let w = entry.weight as u64;
        if pick < w {
            return entry.species;
        }
        pick -= w;
    }
    // Unreachable given `total > 0`, but keep the fallback so a buggy
    // future pool definition can't panic in release.
    pool.last().expect("non-empty").species
}

/// 64-bit xorshift step; deterministic, no-dep PRNG used for the crossbreed
/// pool roll and the exotic-shop window seeding. Avoids the zero fixed-point.
pub fn xorshift64(state: u64) -> u64 {
    let mut x = state.max(1);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

/// Build a stable per-pair, per-claim seed from the two animal UUIDs and
/// the gestation `ends_at`. Same pair + same ends_at always produce the
/// same offspring on claim — important so a reload-on-tick doesn't reroll.
pub fn pool_seed(a_id: uuid::Uuid, b_id: uuid::Uuid, ends_at_secs: i64) -> u64 {
    let (a_lo, a_hi) = a_id.as_u64_pair();
    let (b_lo, b_hi) = b_id.as_u64_pair();
    let mut s = a_lo
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(a_hi);
    s = xorshift64(s).wrapping_add(b_lo);
    s = xorshift64(s).wrapping_add(b_hi);
    s = xorshift64(s).wrapping_add(ends_at_secs as u64);
    xorshift64(s)
}

