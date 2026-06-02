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

/// Compact constructor for coin-economy species (regular or hybrid). Keeps the
/// large generated roster readable: every such species both costs and pays out
/// coins, so only the distinguishing stats need to be passed.
fn coin_species(
    id: SpeciesId,
    display_name: &'static str,
    theme: HabitatTheme,
    base_rate_per_sec: f64,
    base_storage_cap: u64,
    purchase_cost: u64,
    gestation_seconds: u64,
    scaling: (f64, f64),
    hybrid: bool,
) -> SpeciesDef {
    SpeciesDef {
        id,
        display_name,
        theme,
        base_rate_per_sec,
        base_storage_cap,
        purchase_cost,
        gestation_seconds,
        level_rate_bonus: scaling.0,
        level_cap_bonus: scaling.1,
        hybrid,
        exotic: false,
        income_kind: IncomeKind::Coin,
        purchase_currency: IncomeKind::Coin,
    }
}

/// Compact constructor for premium, coin-economy *exotic* species (the unique
/// themed one-offs). Like `coin_species` but flagged `exotic` so they surface
/// only in the time-windowed exotic shop and never as crossbreed drops.
fn exotic_species(
    id: SpeciesId,
    display_name: &'static str,
    theme: HabitatTheme,
    base_rate_per_sec: f64,
    base_storage_cap: u64,
    purchase_cost: u64,
    gestation_seconds: u64,
    scaling: (f64, f64),
) -> SpeciesDef {
    SpeciesDef {
        id,
        display_name,
        theme,
        base_rate_per_sec,
        base_storage_cap,
        purchase_cost,
        gestation_seconds,
        level_rate_bonus: scaling.0,
        level_cap_bonus: scaling.1,
        hybrid: false,
        exotic: true,
        income_kind: IncomeKind::Coin,
        purchase_currency: IncomeKind::Coin,
    }
}

static CATALOG: Lazy<HashMap<SpeciesId, SpeciesDef>> = Lazy::new(|| {
    let mut entries = vec![
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

    // ─── New base species (catch/shop fodder; many are crossbreed parents) ───
    // Built with the compact `coin_species` constructor. Art may not exist yet;
    // missing PNGs fall back to placeholder drawing.
    use scaling::{BALANCED, SPRINTER, TANK};
    entries.extend([
        // Farmland
        coin_species("rabbit", "Rabbit", HabitatTheme::Farmland, 0.6, 70, 30, 70, SPRINTER, false),
        coin_species("chicken", "Chicken", HabitatTheme::Farmland, 0.7, 80, 35, 75, BALANCED, false),
        coin_species("sheep", "Sheep", HabitatTheme::Farmland, 0.9, 160, 90, 140, TANK, false),
        coin_species("goat", "Goat", HabitatTheme::Farmland, 1.1, 140, 110, 150, BALANCED, false),
        // Wetland
        coin_species("duck", "Duck", HabitatTheme::Wetland, 0.8, 110, 60, 110, BALANCED, false),
        coin_species("beaver", "Beaver", HabitatTheme::Wetland, 1.3, 260, 180, 200, TANK, false),
        coin_species("heron", "Heron", HabitatTheme::Wetland, 1.4, 150, 160, 180, SPRINTER, false),
        coin_species("salamander", "Salamander", HabitatTheme::Wetland, 0.9, 120, 80, 130, BALANCED, false),
        // Forest
        coin_species("hedgehog", "Hedgehog", HabitatTheme::Forest, 0.6, 90, 45, 90, BALANCED, false),
        coin_species("badger", "Badger", HabitatTheme::Forest, 1.2, 220, 150, 170, TANK, false),
        coin_species("raccoon", "Raccoon", HabitatTheme::Forest, 1.3, 200, 170, 160, SPRINTER, false),
        coin_species("squirrel", "Squirrel", HabitatTheme::Forest, 0.7, 90, 40, 80, SPRINTER, false),
        coin_species("wolf", "Grey Wolf", HabitatTheme::Forest, 2.2, 420, 420, 260, BALANCED, false),
        coin_species("boar", "Wild Boar", HabitatTheme::Forest, 1.8, 360, 300, 240, TANK, false),
        coin_species("robin", "Robin", HabitatTheme::Forest, 0.8, 90, 45, 80, SPRINTER, false),
        coin_species("mole", "Mole", HabitatTheme::Forest, 0.7, 200, 70, 130, TANK, false),
        coin_species("lynx", "Lynx", HabitatTheme::Forest, 1.9, 300, 340, 240, SPRINTER, false),
        // Arctic
        coin_species("arctic_fox", "Arctic Fox", HabitatTheme::Arctic, 1.4, 240, 220, 200, SPRINTER, false),
        coin_species("seal", "Harbor Seal", HabitatTheme::Arctic, 1.5, 320, 260, 220, BALANCED, false),
        coin_species("walrus", "Walrus", HabitatTheme::Arctic, 2.0, 700, 560, 360, TANK, false),
        coin_species("reindeer", "Reindeer", HabitatTheme::Arctic, 1.7, 380, 340, 260, BALANCED, false),
        // Savanna
        coin_species("cheetah", "Cheetah", HabitatTheme::Savanna, 3.0, 420, 700, 300, SPRINTER, false),
        coin_species("giraffe", "Giraffe", HabitatTheme::Savanna, 2.4, 780, 640, 360, TANK, false),
        coin_species("elephant", "Elephant", HabitatTheme::Savanna, 3.2, 1500, 1200, 480, TANK, false),
        coin_species("rhino", "Rhino", HabitatTheme::Savanna, 3.0, 1200, 1000, 440, TANK, false),
        coin_species("meerkat", "Meerkat", HabitatTheme::Savanna, 1.0, 160, 120, 150, SPRINTER, false),
        coin_species("ostrich", "Ostrich", HabitatTheme::Savanna, 2.2, 360, 420, 260, SPRINTER, false),
        // Jungle
        coin_species("parrot", "Parrot", HabitatTheme::Jungle, 1.3, 180, 160, 170, SPRINTER, false),
        coin_species("jaguar", "Jaguar", HabitatTheme::Jungle, 3.2, 520, 820, 320, SPRINTER, false),
        coin_species("sloth", "Sloth", HabitatTheme::Jungle, 0.5, 600, 200, 360, TANK, false),
        coin_species("chameleon", "Chameleon", HabitatTheme::Jungle, 1.1, 150, 140, 160, BALANCED, false),
        // Ocean
        coin_species("dolphin", "Dolphin", HabitatTheme::Ocean, 2.6, 460, 560, 300, SPRINTER, false),
        coin_species("octopus", "Octopus", HabitatTheme::Ocean, 2.2, 400, 480, 280, BALANCED, false),
        coin_species("pufferfish", "Pufferfish", HabitatTheme::Ocean, 1.2, 160, 140, 160, BALANCED, false),
        coin_species("crab", "Crab", HabitatTheme::Ocean, 0.9, 200, 90, 150, TANK, false),
    ]);

    // ─── New hybrid species (crossbreed-only; see RECIPES below) ─────────────
    entries.extend([
        coin_species("chickbit", "Chickbit", HabitatTheme::Farmland, 2.5, 500, 800, 240, SPRINTER, true),
        coin_species("shoat", "Shoat", HabitatTheme::Farmland, 2.8, 900, 900, 300, TANK, true),
        coin_species("woolcow", "Wool Cow", HabitatTheme::Farmland, 3.0, 2200, 1100, 420, TANK, true),
        coin_species("duckver", "Duckver", HabitatTheme::Wetland, 2.6, 560, 820, 260, BALANCED, true),
        coin_species("heronder", "Heronder", HabitatTheme::Wetland, 3.1, 520, 950, 280, SPRINTER, true),
        coin_species("frock", "Frock", HabitatTheme::Wetland, 2.4, 480, 760, 240, BALANCED, true),
        coin_species("hedger", "Hedger", HabitatTheme::Forest, 2.5, 520, 800, 250, BALANCED, true),
        coin_species("rascurrel", "Rascurrel", HabitatTheme::Forest, 2.9, 500, 860, 260, SPRINTER, true),
        coin_species("wolboar", "Wolboar", HabitatTheme::Forest, 3.4, 820, 1200, 340, TANK, true),
        coin_species("direfox", "Direfox", HabitatTheme::Forest, 3.6, 640, 1300, 320, SPRINTER, true),
        coin_species("prickmouse", "Prickmouse", HabitatTheme::Forest, 2.2, 460, 700, 220, SPRINTER, true),
        coin_species("sealfox", "Sealfox", HabitatTheme::Arctic, 2.8, 620, 900, 280, BALANCED, true),
        coin_species("walrideer", "Walrideer", HabitatTheme::Arctic, 3.2, 1100, 1300, 420, TANK, true),
        coin_species("penseal", "Penseal", HabitatTheme::Arctic, 2.7, 720, 880, 300, BALANCED, true),
        coin_species("tuskbear", "Tusk Bear", HabitatTheme::Arctic, 4.2, 1600, 1900, 460, TANK, true),
        coin_species("cheeraffe", "Cheeraffe", HabitatTheme::Savanna, 3.6, 900, 1500, 360, SPRINTER, true),
        coin_species("elephino", "Elephino", HabitatTheme::Savanna, 4.0, 2200, 2200, 520, TANK, true),
        coin_species("meerich", "Meerich", HabitatTheme::Savanna, 3.0, 520, 1100, 280, SPRINTER, true),
        coin_species("liotah", "Liotah", HabitatTheme::Savanna, 4.4, 820, 2000, 400, SPRINTER, true),
        coin_species("zebraffe", "Zebraffe", HabitatTheme::Savanna, 3.4, 1000, 1400, 380, BALANCED, true),
        coin_species("parrojag", "Parrojag", HabitatTheme::Jungle, 3.8, 640, 1500, 340, SPRINTER, true),
        coin_species("slowmeleon", "Slowmeleon", HabitatTheme::Jungle, 2.0, 900, 900, 400, TANK, true),
        coin_species("monrot", "Monrot", HabitatTheme::Jungle, 3.2, 560, 1100, 300, SPRINTER, true),
        coin_species("torrot", "Torrot", HabitatTheme::Jungle, 2.8, 520, 980, 280, BALANCED, true),
        coin_species("goldsloth", "Gold Sloth", HabitatTheme::Jungle, 1.0, 1400, 1000, 460, TANK, true),
        coin_species("doctopus", "Doctopus", HabitatTheme::Ocean, 3.4, 720, 1300, 340, BALANCED, true),
        coin_species("puffcrab", "Puffcrab", HabitatTheme::Ocean, 2.4, 560, 820, 260, TANK, true),
        coin_species("seadolph", "Seadolph", HabitatTheme::Ocean, 3.6, 700, 1400, 340, SPRINTER, true),
        coin_species("goldpuff", "Goldpuff", HabitatTheme::Ocean, 3.0, 640, 1200, 320, BALANCED, true),
        coin_species("ottaver", "Ottaver", HabitatTheme::Wetland, 2.9, 640, 1000, 300, BALANCED, true),
        coin_species("owlfox", "Owlfox", HabitatTheme::Arctic, 3.0, 560, 1100, 300, SPRINTER, true),
        coin_species("tortdeer", "Tortdeer", HabitatTheme::Arctic, 2.2, 1200, 1000, 440, TANK, true),
        coin_species("ghostdeer", "Ghost Deer", HabitatTheme::Forest, 3.2, 720, 1300, 360, BALANCED, true),
        // Early-game forest hybrids built from the starter species so the
        // opening hours have rewarding, reachable crossbreeds.
        coin_species("scamp", "Scamp", HabitatTheme::Forest, 1.6, 360, 360, 180, SPRINTER, true),
        coin_species("lilyleap", "Lilyleap", HabitatTheme::Forest, 1.8, 300, 320, 170, SPRINTER, true),
        coin_species("marshmask", "Marsh Mask", HabitatTheme::Forest, 2.0, 420, 420, 200, BALANCED, true),
        coin_species("embermane", "Embermane", HabitatTheme::Forest, 2.6, 520, 620, 240, BALANCED, true),
        coin_species("bogmane", "Bog Mane", HabitatTheme::Forest, 2.4, 560, 560, 240, TANK, true),
        coin_species("pridelet", "Pridelet", HabitatTheme::Forest, 2.2, 460, 480, 210, SPRINTER, true),
        coin_species("burrowkin", "Burrowkin", HabitatTheme::Forest, 1.5, 520, 360, 200, TANK, true),
        coin_species("stagstalker", "Stagstalker", HabitatTheme::Forest, 2.8, 600, 700, 260, BALANCED, true),
    ]);

    // ─── Unique themed exotics (concept critters; exotic-shop only) ──────────
    entries.extend([
        exotic_species("candy_dove", "Candy Dove", HabitatTheme::Farmland, 4.0, 9000, 24000, 28800, SPRINTER),
        exotic_species("robot_cat", "Robot Cat", HabitatTheme::Forest, 12.0, 6000, 48000, 36000, SPRINTER),
        exotic_species("moophin", "Moophin", HabitatTheme::Ocean, 6.0, 22000, 60000, 43200, TANK),
        exotic_species("zombie_dog", "Zombie Dog", HabitatTheme::Forest, 5.0, 13000, 30000, 32400, BALANCED),
        exotic_species("lava_lynx", "Lava Lynx", HabitatTheme::Savanna, 18.0, 5000, 90000, 50400, SPRINTER),
        exotic_species("crystal_stag", "Crystal Stag", HabitatTheme::Arctic, 7.0, 26000, 75000, 54000, TANK),
        exotic_species("origami_crane", "Origami Crane", HabitatTheme::Wetland, 5.5, 8000, 27000, 25200, BALANCED),
        exotic_species("clockwork_owl", "Clockwork Owl", HabitatTheme::Arctic, 9.0, 11000, 52000, 39600, BALANCED),
        exotic_species("galaxy_whale", "Galaxy Whale", HabitatTheme::Ocean, 8.0, 60000, 150000, 86400, TANK),
        exotic_species("mushroom_toad", "Mushroom Toad", HabitatTheme::Wetland, 4.5, 14000, 21000, 23400, TANK),
        exotic_species("shadow_panther", "Shadow Panther", HabitatTheme::Jungle, 20.0, 7000, 110000, 57600, SPRINTER),
        exotic_species("plush_bear", "Plush Bear", HabitatTheme::Farmland, 3.5, 30000, 18000, 28800, TANK),
        exotic_species("neon_gecko", "Neon Gecko", HabitatTheme::Jungle, 10.0, 5500, 40000, 30600, SPRINTER),
    ]);

    // ─── Concept hybrids — exotics crossed with the wider roster. Names are
    // coined to evoke the blend rather than mash two words together. ─────────
    entries.extend([
        coin_species("gumdrop", "Gumdrop", HabitatTheme::Wetland, 6.0, 12000, 38000, 32400, SPRINTER, true),
        coin_species("glitchpaw", "Glitchpaw", HabitatTheme::Forest, 14.0, 7000, 55000, 39600, SPRINTER, true),
        coin_species("tuxtide", "Tuxtide", HabitatTheme::Ocean, 7.0, 24000, 70000, 46800, TANK, true),
        coin_species("hopocalypse", "Hopocalypse", HabitatTheme::Forest, 6.0, 15000, 42000, 36000, BALANCED, true),
        coin_species("cinderfrost", "Cinderfrost", HabitatTheme::Arctic, 16.0, 9000, 95000, 50400, BALANCED, true),
        coin_species("prismhart", "Prismhart", HabitatTheme::Arctic, 8.0, 28000, 88000, 54000, TANK, true),
        coin_species("foldfeather", "Foldfeather", HabitatTheme::Wetland, 6.5, 9000, 33000, 28800, BALANCED, true),
        coin_species("ticktalon", "Ticktalon", HabitatTheme::Arctic, 10.0, 12000, 60000, 41400, SPRINTER, true),
        coin_species("stardive", "Stardive", HabitatTheme::Ocean, 9.0, 62000, 165000, 86400, TANK, true),
        coin_species("sporehop", "Sporehop", HabitatTheme::Wetland, 5.0, 16000, 26000, 25200, TANK, true),
        coin_species("nightmaw", "Nightmaw", HabitatTheme::Jungle, 22.0, 8000, 125000, 60000, SPRINTER, true),
        coin_species("snugfang", "Snugfang", HabitatTheme::Arctic, 5.5, 34000, 64000, 50400, TANK, true),
        coin_species("voltscale", "Voltscale", HabitatTheme::Jungle, 12.0, 6500, 50000, 34200, SPRINTER, true),
        coin_species("frostingmane", "Frostingmane", HabitatTheme::Farmland, 6.0, 13000, 46000, 37800, BALANCED, true),
        coin_species("mechabyss", "Mechabyss", HabitatTheme::Ocean, 15.0, 40000, 180000, 90000, TANK, true),
        coin_species("gravestalker", "Gravestalker", HabitatTheme::Jungle, 18.0, 12000, 135000, 64800, SPRINTER, true),
    ]);

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

    // Bulk recipes in compact (parentA, parentB, hybrid) form. Each gets the
    // standard 45/45/10 pool (most pairings hand back a parent; the hybrid is
    // the rare drop). Chosen so that nearly every base species is a parent in
    // at least one recipe and every hybrid is some recipe's offspring.
    let simple: &[(SpeciesId, SpeciesId, SpeciesId)] = &[
        ("rabbit", "chicken", "chickbit"),
        ("sheep", "goat", "shoat"),
        ("cow", "sheep", "woolcow"),
        ("duck", "beaver", "duckver"),
        ("heron", "salamander", "heronder"),
        ("blue_frog", "duck", "frock"),
        ("hedgehog", "badger", "hedger"),
        ("raccoon", "squirrel", "rascurrel"),
        ("wolf", "boar", "wolboar"),
        ("fox", "wolf", "direfox"),
        ("field_mouse", "hedgehog", "prickmouse"),
        ("arctic_fox", "seal", "sealfox"),
        ("walrus", "reindeer", "walrideer"),
        ("penguin", "seal", "penseal"),
        ("polar_bear", "walrus", "tuskbear"),
        ("cheetah", "giraffe", "cheeraffe"),
        ("elephant", "rhino", "elephino"),
        ("meerkat", "ostrich", "meerich"),
        ("lion", "cheetah", "liotah"),
        ("zebra", "giraffe", "zebraffe"),
        ("parrot", "jaguar", "parrojag"),
        ("sloth", "chameleon", "slowmeleon"),
        ("monkey", "parrot", "monrot"),
        ("toucan", "parrot", "torrot"),
        ("goldenToucan", "sloth", "goldsloth"),
        ("dolphin", "octopus", "doctopus"),
        ("pufferfish", "crab", "puffcrab"),
        ("reefSeahorse", "dolphin", "seadolph"),
        ("goldenCarp", "pufferfish", "goldpuff"),
        ("otter", "beaver", "ottaver"),
        ("snowyOwl", "arctic_fox", "owlfox"),
        ("giantTortoise", "reindeer", "tortdeer"),
        ("albinoDeer", "reindeer", "ghostdeer"),
        // Early-game forest hybrids from the starter species — reachable quickly
        // so the opening hours aren't boring.
        ("fox", "field_mouse", "scamp"),
        ("field_mouse", "blue_frog", "lilyleap"),
        ("blue_frog", "fox", "marshmask"),
        ("fox", "lion", "embermane"),
        ("blue_frog", "lion", "bogmane"),
        ("field_mouse", "lion", "pridelet"),
        ("mole", "field_mouse", "burrowkin"),
        ("lynx", "albinoDeer", "stagstalker"),
        // Concept hybrids: a themed exotic crossed with something from the roster.
        ("candy_dove", "blue_frog", "gumdrop"),
        ("robot_cat", "field_mouse", "glitchpaw"),
        ("moophin", "penguin", "tuxtide"),
        ("zombie_dog", "rabbit", "hopocalypse"),
        ("lava_lynx", "arctic_fox", "cinderfrost"),
        ("crystal_stag", "albinoDeer", "prismhart"),
        ("origami_crane", "heron", "foldfeather"),
        ("clockwork_owl", "snowyOwl", "ticktalon"),
        ("galaxy_whale", "dolphin", "stardive"),
        ("mushroom_toad", "treeFrog", "sporehop"),
        ("shadow_panther", "jaguar", "nightmaw"),
        ("plush_bear", "polar_bear", "snugfang"),
        ("neon_gecko", "chameleon", "voltscale"),
        ("candy_dove", "birthday_horse", "frostingmane"),
        ("robot_cat", "galaxy_whale", "mechabyss"),
        ("zombie_dog", "shadow_panther", "gravestalker"),
    ];
    for (a, b, hyb) in simple {
        let pool = vec![
            PoolEntry { species: *a, weight: 45 },
            PoolEntry { species: *b, weight: 45 },
            PoolEntry { species: *hyb, weight: 10 },
        ];
        out.insert((*a, *b), pool.clone());
        out.insert((*b, *a), pool);
    }

    out
});

/// How many successful wild captures of a species are required before one is
/// actually tamed into the zoo. Rarity is approximated by `purchase_cost`:
/// cheap/common species join on the first catch, while rare and premium
/// species demand several successful captures. Unknown ids fall back to 1.
pub fn captures_required(species: SpeciesId) -> u32 {
    match try_get(species).map(|d| d.purchase_cost).unwrap_or(0) {
        0..=99 => 1,
        100..=299 => 2,
        300..=799 => 3,
        800..=2_499 => 4,
        _ => 5,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every species referenced by any recipe pool must exist in the catalog —
    /// guards against typos in the bulk recipe table.
    #[test]
    fn all_recipe_species_exist() {
        for ((a, b), pool) in RECIPES.iter() {
            assert!(try_get(a).is_some(), "recipe parent '{a}' not in catalog");
            assert!(try_get(b).is_some(), "recipe parent '{b}' not in catalog");
            for e in pool {
                assert!(
                    try_get(e.species).is_some(),
                    "recipe outcome '{}' not in catalog",
                    e.species
                );
            }
        }
    }

    /// At least 90% of all species must participate in some recipe — either as
    /// a parent or as the hybrid outcome.
    #[test]
    fn most_species_belong_to_a_recipe() {
        let mut in_recipe: HashSet<SpeciesId> = HashSet::new();
        for ((a, b), pool) in RECIPES.iter() {
            in_recipe.insert(*a);
            in_recipe.insert(*b);
            for e in pool {
                in_recipe.insert(e.species);
            }
        }
        let total = CATALOG.len();
        let covered = CATALOG.keys().filter(|id| in_recipe.contains(*id)).count();
        let pct = covered as f32 / total as f32;
        assert!(
            pct >= 0.90,
            "only {covered}/{total} ({:.1}%) species belong to a recipe",
            pct * 100.0
        );
    }

    /// Sanity-check the roster sizes the design calls for.
    #[test]
    fn roster_has_enough_hybrids() {
        let hybrids = CATALOG.values().filter(|d| d.hybrid).count();
        assert!(hybrids >= 30, "expected >=30 hybrids, found {hybrids}");
        let recipe_pairs = all_recipes().len();
        assert!(recipe_pairs >= 30, "expected >=30 recipes, found {recipe_pairs}");
    }
}

