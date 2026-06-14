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
    // ── Added in the proc-gen overhaul: natural biomes ──────────────────────
    // Cosmetic for now (no spawn-table entries yet → barren land); gameplay
    // habitats can adopt them later.
    Desert,
    Tundra,
    Taiga,
    Volcanic,
    Badlands,
    Beach,
    Highlands,
    // ── Fantastical / rare biomes (scattered special patches) ───────────────
    Mythical,
    Void,
    Festive,
    Food,
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
            HabitatTheme::Ocean => "Ocean",
            HabitatTheme::Desert => "Desert",
            HabitatTheme::Tundra => "Tundra",
            HabitatTheme::Taiga => "Taiga",
            HabitatTheme::Volcanic => "Volcanic",
            HabitatTheme::Badlands => "Badlands",
            HabitatTheme::Beach => "Beach",
            HabitatTheme::Highlands => "Highlands",
            HabitatTheme::Mythical => "Mythical",
            HabitatTheme::Void => "Void",
            HabitatTheme::Festive => "Festive",
            HabitatTheme::Food => "Food",
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
            "Desert" => Some(Self::Desert),
            "Tundra" => Some(Self::Tundra),
            "Taiga" => Some(Self::Taiga),
            "Volcanic" => Some(Self::Volcanic),
            "Badlands" => Some(Self::Badlands),
            "Beach" => Some(Self::Beach),
            "Highlands" => Some(Self::Highlands),
            "Mythical" => Some(Self::Mythical),
            "Void" => Some(Self::Void),
            "Festive" => Some(Self::Festive),
            "Food" => Some(Self::Food),
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
            id: "red_fox",
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
        coin_species("grey_wolf", "Grey Wolf", HabitatTheme::Forest, 2.2, 420, 420, 260, BALANCED, false),
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
        coin_species("glass_fox", "Glass Fox", HabitatTheme::Arctic, 16.0, 9000, 95000, 50400, BALANCED, true),
        coin_species("prismhart", "Prismhart", HabitatTheme::Arctic, 8.0, 28000, 88000, 54000, TANK, true),
        coin_species("foldfeather", "Foldfeather", HabitatTheme::Wetland, 6.5, 9000, 33000, 28800, BALANCED, true),
        coin_species("ticktalon", "Ticktalon", HabitatTheme::Arctic, 10.0, 12000, 60000, 41400, SPRINTER, true),
        coin_species("stardive", "Stardive", HabitatTheme::Ocean, 9.0, 62000, 165000, 86400, TANK, true),
        coin_species("sporehop", "Sporehop", HabitatTheme::Wetland, 5.0, 16000, 26000, 25200, TANK, true),
        coin_species("nightmaw", "Nightmaw", HabitatTheme::Jungle, 22.0, 8000, 125000, 60000, SPRINTER, true),
        coin_species("snugfang", "Snugfang", HabitatTheme::Arctic, 5.5, 34000, 64000, 50400, TANK, true),
        coin_species("voltscale", "Voltscale", HabitatTheme::Jungle, 12.0, 6500, 50000, 34200, SPRINTER, true),
        coin_species("jack_of_all_manes", "Jack of All Manes", HabitatTheme::Farmland, 6.0, 13000, 46000, 37800, BALANCED, true),
        coin_species("mechabyss", "Mechabyss", HabitatTheme::Ocean, 15.0, 40000, 180000, 90000, TANK, true),
        coin_species("gravestalker", "Gravestalker", HabitatTheme::Jungle, 18.0, 12000, 135000, 64800, SPRINTER, true),
    ]);

    // ─── Secondary (rarer) crossbreed hybrids ────────────────────────────────
    // Every recipe can also drop a second, scarcer hybrid (see RECIPES). These
    // are slightly stronger than the primary hybrid as a reward for the lower
    // odds. Themed to match the cross's primary hybrid.
    entries.extend([
        // Secondary drops for the base-roster recipes.
        coin_species("billowool", "Billowool", HabitatTheme::Farmland, 3.0, 950, 950, 310, TANK, true),
        coin_species("hornwool_bovram", "Hornwool Bovram", HabitatTheme::Farmland, 3.2, 2300, 1200, 430, TANK, true),
        coin_species("marshplume", "Marsh Plume", HabitatTheme::Wetland, 3.3, 540, 1000, 290, SPRINTER, true),
        coin_species("bramblesett", "Bramblesett", HabitatTheme::Forest, 2.7, 560, 860, 260, TANK, true),
        coin_species("tuskhowl", "Tuskhowl", HabitatTheme::Forest, 3.6, 860, 1300, 350, TANK, true),
        coin_species("duskrunner", "Duskrunner", HabitatTheme::Forest, 3.8, 680, 1400, 330, SPRINTER, true),
        coin_species("quillsqueak", "Quillsqueak", HabitatTheme::Forest, 2.4, 500, 760, 230, SPRINTER, true),
        coin_species("frostpup", "Frostpup", HabitatTheme::Arctic, 3.0, 660, 950, 290, BALANCED, true),
        coin_species("tundratusk", "Tundratusk", HabitatTheme::Arctic, 3.4, 1150, 1400, 430, TANK, true),
        coin_species("blizzardmaw", "Blizzardmaw", HabitatTheme::Arctic, 4.4, 1700, 2000, 470, TANK, true),
        coin_species("spotspire", "Spotspire", HabitatTheme::Savanna, 3.8, 950, 1600, 370, SPRINTER, true),
        coin_species("pachyhorn", "Pachyhorn", HabitatTheme::Savanna, 4.2, 2300, 2400, 530, TANK, true),
        coin_species("sentryplume", "Sentryplume", HabitatTheme::Savanna, 3.2, 560, 1150, 290, SPRINTER, true),
        coin_species("prideflash", "Prideflash", HabitatTheme::Savanna, 4.6, 860, 2100, 410, SPRINTER, true),
        coin_species("stripespire", "Stripespire", HabitatTheme::Savanna, 3.6, 1050, 1500, 390, BALANCED, true),
        coin_species("plumeprowl", "Plumeprowl", HabitatTheme::Jungle, 4.0, 680, 1600, 350, SPRINTER, true),
        coin_species("mosslimber", "Mosslimber", HabitatTheme::Jungle, 2.1, 950, 950, 410, TANK, true),
        coin_species("chatterperch", "Chatterperch", HabitatTheme::Jungle, 3.4, 600, 1150, 310, SPRINTER, true),
        coin_species("gildedyawn", "Gilded Yawn", HabitatTheme::Jungle, 1.1, 1500, 1100, 470, TANK, true),
        coin_species("inkfin", "Inkfin", HabitatTheme::Ocean, 3.6, 760, 1400, 350, BALANCED, true),
        coin_species("spineshell", "Spineshell", HabitatTheme::Ocean, 2.6, 600, 870, 270, TANK, true),
        coin_species("tidecurl", "Tidecurl", HabitatTheme::Ocean, 3.8, 740, 1500, 350, SPRINTER, true),
        coin_species("gildedspine", "Gilded Spine", HabitatTheme::Ocean, 3.2, 680, 1300, 330, BALANCED, true),
        coin_species("rivergnaw", "Rivergnaw", HabitatTheme::Wetland, 3.1, 680, 1050, 310, BALANCED, true),
        coin_species("frosttalon", "Frosttalon", HabitatTheme::Arctic, 3.2, 600, 1150, 310, SPRINTER, true),
        coin_species("shellantler", "Shellantler", HabitatTheme::Arctic, 2.4, 1300, 1100, 450, TANK, true),
        coin_species("palevelvet", "Pale Velvet", HabitatTheme::Forest, 3.4, 760, 1400, 370, BALANCED, true),
        // Secondary drops for the early forest starters.
        coin_species("kitnip", "Kitnip", HabitatTheme::Forest, 1.8, 380, 400, 190, SPRINTER, true),
        coin_species("pipsplash", "Pipsplash", HabitatTheme::Forest, 2.0, 320, 360, 180, SPRINTER, true),
        coin_species("bogtrot", "Bogtrot", HabitatTheme::Forest, 2.2, 440, 460, 210, BALANCED, true),
        coin_species("cinderpaw", "Cinderpaw", HabitatTheme::Forest, 2.8, 540, 660, 250, BALANCED, true),
        coin_species("marshpride", "Marsh Pride", HabitatTheme::Forest, 2.6, 580, 600, 250, TANK, true),
        coin_species("squeakmane", "Squeakmane", HabitatTheme::Forest, 2.4, 480, 520, 220, SPRINTER, true),
        coin_species("tunnelnib", "Tunnelnib", HabitatTheme::Forest, 1.7, 540, 400, 210, TANK, true),
        coin_species("snowprowl", "Snowprowl", HabitatTheme::Forest, 3.0, 620, 760, 270, BALANCED, true),
        // Secondary drops for the concept (exotic-cross) recipes.
        coin_species("fizzhopper", "Fizzhopper", HabitatTheme::Wetland, 6.4, 12500, 40000, 33000, SPRINTER, true),
        coin_species("bitsqueak", "Bitsqueak", HabitatTheme::Forest, 14.5, 7200, 57000, 40000, SPRINTER, true),
        coin_species("frostmoo", "Frostmoo", HabitatTheme::Ocean, 7.4, 24500, 73000, 47500, TANK, true),
        coin_species("gnawhop", "Gnawhop", HabitatTheme::Forest, 6.4, 15500, 44000, 36500, BALANCED, true),
        coin_species("ashpaw", "Ashpaw", HabitatTheme::Arctic, 16.5, 9200, 98000, 51000, BALANCED, true),
        coin_species("gleamantler", "Gleamantler", HabitatTheme::Arctic, 8.4, 28500, 91000, 54500, TANK, true),
        coin_species("creasewing", "Creasewing", HabitatTheme::Wetland, 6.8, 9200, 34000, 29200, BALANCED, true),
        coin_species("gearhoot", "Gearhoot", HabitatTheme::Arctic, 10.5, 12200, 62000, 42000, SPRINTER, true),
        coin_species("comettail", "Comettail", HabitatTheme::Ocean, 9.4, 63000, 170000, 87000, TANK, true),
        coin_species("shroomleap", "Shroomleap", HabitatTheme::Wetland, 5.2, 16500, 27000, 25600, TANK, true),
        coin_species("umbraprowl", "Umbraprowl", HabitatTheme::Jungle, 22.5, 8200, 128000, 60500, SPRINTER, true),
        coin_species("fluffmaw", "Fluffmaw", HabitatTheme::Arctic, 5.7, 34500, 66000, 51000, TANK, true),
        coin_species("glowtail", "Glowtail", HabitatTheme::Jungle, 12.5, 6700, 52000, 34800, SPRINTER, true),
        coin_species("confettihoof", "Confettihoof", HabitatTheme::Farmland, 6.3, 13500, 47500, 38200, BALANCED, true),
        coin_species("cogwhale", "Cogwhale", HabitatTheme::Ocean, 15.5, 41000, 185000, 90500, TANK, true),
        coin_species("cryptfang", "Cryptfang", HabitatTheme::Jungle, 18.5, 12500, 138000, 65200, SPRINTER, true),
        // Tertiary (jackpot) drops — the rarest tier on a handful of recipes
        // that roll three possible hybrids.
        coin_species("nightfang", "Nightfang", HabitatTheme::Forest, 4.0, 720, 1500, 340, SPRINTER, true),
        coin_species("glacialith", "Glacialith", HabitatTheme::Arctic, 4.8, 1900, 2300, 500, TANK, true),
        coin_species("colossatusk", "Colossatusk", HabitatTheme::Savanna, 4.6, 2600, 2700, 560, TANK, true),
        coin_species("blitzfang", "Blitzfang", HabitatTheme::Savanna, 5.0, 900, 2400, 430, SPRINTER, true),
        coin_species("auroracrown", "Auroracrown", HabitatTheme::Arctic, 9.0, 30000, 99000, 56000, TANK, true),
        coin_species("voidsong", "Voidsong", HabitatTheme::Ocean, 10.0, 66000, 185000, 88000, TANK, true),
        coin_species("eclipsemaw", "Eclipsemaw", HabitatTheme::Jungle, 24.0, 9000, 142000, 62000, SPRINTER, true),
        coin_species("singularis", "Singularis", HabitatTheme::Ocean, 16.5, 44000, 200000, 92000, TANK, true),
    ]);

    // ─── New-biome wild rosters (proc-gen overhaul) ──────────────────────────
    // Themed fauna populating the biomes added in the world-gen overhaul. These
    // are regular catchable/purchasable species (art falls back to placeholder
    // drawing until PNGs are authored). Costs are spread so commons tame in a
    // catch or two while the apex predators of each biome take several.
    entries.extend([
        // ── Desert ──────────────────────────────────────────────────────────
        coin_species("fennec_fox", "Fennec Fox", HabitatTheme::Desert, 0.7, 90, 40, 80, SPRINTER, false),
        coin_species("jerboa", "Jerboa", HabitatTheme::Desert, 0.6, 70, 30, 70, SPRINTER, false),
        coin_species("desert_hare", "Desert Hare", HabitatTheme::Desert, 0.7, 90, 45, 80, SPRINTER, false),
        coin_species("horned_lizard", "Horned Lizard", HabitatTheme::Desert, 0.8, 130, 70, 120, TANK, false),
        coin_species("desert_tortoise", "Desert Tortoise", HabitatTheme::Desert, 0.3, 500, 150, 600, TANK, false),
        coin_species("scorpion", "Desert Scorpion", HabitatTheme::Desert, 0.9, 110, 90, 130, BALANCED, false),
        coin_species("camel", "Dromedary Camel", HabitatTheme::Desert, 1.5, 600, 300, 360, TANK, false),
        coin_species("roadrunner", "Roadrunner", HabitatTheme::Desert, 1.3, 160, 180, 170, SPRINTER, false),
        coin_species("sand_viper", "Sand Viper", HabitatTheme::Desert, 1.1, 120, 150, 150, BALANCED, false),
        coin_species("vulture", "Desert Vulture", HabitatTheme::Desert, 1.6, 220, 260, 240, BALANCED, false),
        coin_species("sidewinder", "Sidewinder", HabitatTheme::Desert, 1.4, 180, 240, 220, SPRINTER, false),
        coin_species("dust_jackal", "Dust Jackal", HabitatTheme::Desert, 2.0, 360, 420, 260, BALANCED, false),
        // ── Tundra ──────────────────────────────────────────────────────────
        coin_species("arctic_hare", "Arctic Hare", HabitatTheme::Tundra, 0.7, 90, 45, 80, SPRINTER, false),
        coin_species("lemming", "Lemming", HabitatTheme::Tundra, 0.5, 60, 25, 60, SPRINTER, false),
        coin_species("snow_vole", "Snow Vole", HabitatTheme::Tundra, 0.6, 70, 30, 70, BALANCED, false),
        coin_species("snow_bunting", "Snow Bunting", HabitatTheme::Tundra, 0.7, 80, 40, 80, SPRINTER, false),
        coin_species("ptarmigan", "Ptarmigan", HabitatTheme::Tundra, 0.9, 110, 80, 120, BALANCED, false),
        coin_species("stoat", "Stoat", HabitatTheme::Tundra, 1.0, 140, 110, 150, SPRINTER, false),
        coin_species("ermine", "Ermine", HabitatTheme::Tundra, 1.1, 150, 130, 160, SPRINTER, false),
        coin_species("caribou", "Caribou", HabitatTheme::Tundra, 1.7, 400, 340, 300, BALANCED, false),
        coin_species("snow_fox", "Snow Fox", HabitatTheme::Tundra, 1.4, 240, 220, 200, SPRINTER, false),
        coin_species("musk_ox", "Musk Ox", HabitatTheme::Tundra, 2.0, 800, 500, 420, TANK, false),
        coin_species("wolverine", "Wolverine", HabitatTheme::Tundra, 2.4, 420, 520, 300, SPRINTER, false),
        coin_species("tundra_wolf", "Tundra Wolf", HabitatTheme::Tundra, 2.6, 520, 640, 340, BALANCED, false),
        // ── Taiga ───────────────────────────────────────────────────────────
        coin_species("chipmunk", "Chipmunk", HabitatTheme::Taiga, 0.6, 80, 35, 70, SPRINTER, false),
        coin_species("red_squirrel", "Red Squirrel", HabitatTheme::Taiga, 0.7, 90, 40, 80, SPRINTER, false),
        coin_species("crossbill", "Crossbill", HabitatTheme::Taiga, 0.8, 90, 50, 90, SPRINTER, false),
        coin_species("capercaillie", "Capercaillie", HabitatTheme::Taiga, 1.0, 140, 110, 150, BALANCED, false),
        coin_species("pine_marten", "Pine Marten", HabitatTheme::Taiga, 1.2, 200, 150, 170, SPRINTER, false),
        coin_species("sable", "Sable", HabitatTheme::Taiga, 1.3, 180, 170, 180, SPRINTER, false),
        coin_species("boreal_owl", "Boreal Owl", HabitatTheme::Taiga, 1.5, 160, 180, 200, BALANCED, false),
        coin_species("siberian_lynx", "Siberian Lynx", HabitatTheme::Taiga, 1.9, 300, 340, 240, SPRINTER, false),
        coin_species("elk", "Elk", HabitatTheme::Taiga, 2.4, 700, 520, 360, TANK, false),
        coin_species("timber_wolf", "Timber Wolf", HabitatTheme::Taiga, 2.6, 520, 640, 340, BALANCED, false),
        coin_species("moose", "Moose", HabitatTheme::Taiga, 3.0, 1200, 1000, 460, TANK, false),
        coin_species("brown_bear", "Brown Bear", HabitatTheme::Taiga, 3.2, 1000, 900, 440, TANK, false),
        // ── Volcanic ────────────────────────────────────────────────────────
        coin_species("ash_beetle", "Ash Beetle", HabitatTheme::Volcanic, 0.7, 100, 40, 80, TANK, false),
        coin_species("lava_newt", "Lava Newt", HabitatTheme::Volcanic, 1.0, 130, 90, 130, BALANCED, false),
        coin_species("obsidian_toad", "Obsidian Toad", HabitatTheme::Volcanic, 0.9, 180, 110, 160, TANK, false),
        coin_species("cinder_lizard", "Cinder Lizard", HabitatTheme::Volcanic, 1.1, 150, 130, 160, BALANCED, false),
        coin_species("ember_moth", "Ember Moth", HabitatTheme::Volcanic, 1.2, 140, 150, 170, SPRINTER, false),
        coin_species("magma_crab", "Magma Crab", HabitatTheme::Volcanic, 1.3, 260, 180, 220, TANK, false),
        coin_species("fire_salamander", "Fire Salamander", HabitatTheme::Volcanic, 1.4, 180, 220, 220, BALANCED, false),
        coin_species("sulfur_serpent", "Sulfur Serpent", HabitatTheme::Volcanic, 1.6, 220, 300, 260, SPRINTER, false),
        coin_species("ashen_vulture", "Ashen Vulture", HabitatTheme::Volcanic, 1.7, 240, 300, 260, BALANCED, false),
        coin_species("rock_python", "Rock Python", HabitatTheme::Volcanic, 1.8, 320, 360, 280, BALANCED, false),
        coin_species("magma_hound", "Magma Hound", HabitatTheme::Volcanic, 2.6, 520, 640, 360, SPRINTER, false),
        coin_species("cinder_drake", "Cinder Drake", HabitatTheme::Volcanic, 3.0, 600, 900, 440, SPRINTER, false),
        // ── Badlands ────────────────────────────────────────────────────────
        coin_species("prairie_dog", "Prairie Dog", HabitatTheme::Badlands, 0.6, 80, 35, 70, SPRINTER, false),
        coin_species("jackrabbit", "Jackrabbit", HabitatTheme::Badlands, 0.7, 90, 45, 80, SPRINTER, false),
        coin_species("gila_woodpecker", "Gila Woodpecker", HabitatTheme::Badlands, 0.9, 100, 70, 100, SPRINTER, false),
        coin_species("horned_toad", "Horned Toad", HabitatTheme::Badlands, 0.9, 130, 80, 130, TANK, false),
        coin_species("armadillo", "Armadillo", HabitatTheme::Badlands, 0.8, 200, 90, 150, TANK, false),
        coin_species("rattlesnake", "Rattlesnake", HabitatTheme::Badlands, 1.2, 150, 160, 170, BALANCED, false),
        coin_species("kit_fox", "Kit Fox", HabitatTheme::Badlands, 1.3, 200, 170, 180, SPRINTER, false),
        coin_species("turkey_vulture", "Turkey Vulture", HabitatTheme::Badlands, 1.6, 220, 260, 240, BALANCED, false),
        coin_species("coyote", "Coyote", HabitatTheme::Badlands, 1.8, 320, 300, 240, SPRINTER, false),
        coin_species("bighorn_sheep", "Bighorn Sheep", HabitatTheme::Badlands, 2.2, 600, 440, 340, TANK, false),
        coin_species("cougar", "Cougar", HabitatTheme::Badlands, 3.0, 520, 820, 360, SPRINTER, false),
        coin_species("bison", "Bison", HabitatTheme::Badlands, 3.2, 1400, 1100, 480, TANK, false),
        // ── Beach ───────────────────────────────────────────────────────────
        coin_species("sandpiper", "Sandpiper", HabitatTheme::Beach, 0.7, 80, 40, 80, SPRINTER, false),
        coin_species("hermit_crab", "Hermit Crab", HabitatTheme::Beach, 0.6, 120, 40, 90, TANK, false),
        coin_species("fiddler_crab", "Fiddler Crab", HabitatTheme::Beach, 0.7, 100, 50, 90, SPRINTER, false),
        coin_species("sanderling", "Sanderling", HabitatTheme::Beach, 0.7, 80, 45, 80, SPRINTER, false),
        coin_species("ghost_crab", "Ghost Crab", HabitatTheme::Beach, 0.9, 110, 90, 130, SPRINTER, false),
        coin_species("seagull", "Seagull", HabitatTheme::Beach, 1.0, 130, 100, 140, BALANCED, false),
        coin_species("horseshoe_crab", "Horseshoe Crab", HabitatTheme::Beach, 0.5, 200, 90, 160, TANK, false),
        coin_species("sea_turtle", "Sea Turtle", HabitatTheme::Beach, 0.4, 500, 180, 600, TANK, false),
        coin_species("pelican", "Pelican", HabitatTheme::Beach, 1.4, 240, 200, 220, BALANCED, false),
        coin_species("osprey", "Osprey", HabitatTheme::Beach, 1.7, 220, 280, 260, SPRINTER, false),
        coin_species("sea_lion", "Sea Lion", HabitatTheme::Beach, 2.0, 520, 360, 300, TANK, false),
        coin_species("coastal_jackal", "Coastal Jackal", HabitatTheme::Beach, 2.0, 360, 420, 280, BALANCED, false),
        // ── Highlands ───────────────────────────────────────────────────────
        coin_species("pika", "Pika", HabitatTheme::Highlands, 0.6, 80, 35, 70, SPRINTER, false),
        coin_species("marmot", "Marmot", HabitatTheme::Highlands, 0.7, 120, 50, 90, TANK, false),
        coin_species("alpine_hare", "Alpine Hare", HabitatTheme::Highlands, 0.7, 90, 45, 80, SPRINTER, false),
        coin_species("rock_ptarmigan", "Rock Ptarmigan", HabitatTheme::Highlands, 0.8, 100, 60, 100, BALANCED, false),
        coin_species("chamois", "Chamois", HabitatTheme::Highlands, 1.5, 320, 260, 260, SPRINTER, false),
        coin_species("mountain_goat", "Mountain Goat", HabitatTheme::Highlands, 1.6, 360, 280, 260, TANK, false),
        coin_species("ibex", "Ibex", HabitatTheme::Highlands, 1.8, 400, 340, 300, BALANCED, false),
        coin_species("condor", "Condor", HabitatTheme::Highlands, 2.0, 280, 400, 300, BALANCED, false),
        coin_species("golden_eagle", "Golden Eagle", HabitatTheme::Highlands, 2.4, 300, 520, 320, SPRINTER, false),
        coin_species("yak", "Yak", HabitatTheme::Highlands, 2.4, 900, 560, 420, TANK, false),
        coin_species("highland_wolf", "Highland Wolf", HabitatTheme::Highlands, 2.6, 520, 640, 340, BALANCED, false),
        coin_species("snow_leopard", "Snow Leopard", HabitatTheme::Highlands, 3.2, 520, 860, 380, SPRINTER, false),
        // ── Mythical ────────────────────────────────────────────────────────
        coin_species("gnome", "Garden Gnome", HabitatTheme::Mythical, 1.2, 220, 180, 200, TANK, false),
        coin_species("jackalope", "Jackalope", HabitatTheme::Mythical, 1.4, 200, 220, 220, SPRINTER, false),
        coin_species("pixie", "Pixie", HabitatTheme::Mythical, 1.5, 150, 200, 200, SPRINTER, false),
        coin_species("will_o_wisp", "Will-o'-Wisp", HabitatTheme::Mythical, 1.6, 160, 260, 240, SPRINTER, false),
        coin_species("faun", "Faun", HabitatTheme::Mythical, 1.8, 300, 320, 260, BALANCED, false),
        coin_species("griffon_chick", "Griffon Chick", HabitatTheme::Mythical, 2.0, 260, 400, 300, SPRINTER, false),
        coin_species("kelpie", "Kelpie", HabitatTheme::Mythical, 2.2, 420, 480, 320, BALANCED, false),
        coin_species("unicorn_foal", "Unicorn Foal", HabitatTheme::Mythical, 2.4, 400, 560, 360, BALANCED, false),
        coin_species("sprite_stag", "Sprite Stag", HabitatTheme::Mythical, 2.6, 460, 640, 380, BALANCED, false),
        coin_species("phoenix_chick", "Phoenix Chick", HabitatTheme::Mythical, 3.0, 500, 900, 440, SPRINTER, false),
        coin_species("basilisk", "Basilisk", HabitatTheme::Mythical, 3.2, 560, 1000, 460, BALANCED, false),
        coin_species("wyvern", "Wyvern", HabitatTheme::Mythical, 3.4, 600, 1100, 460, SPRINTER, false),
        // ── Void ────────────────────────────────────────────────────────────
        coin_species("gloom_bat", "Gloom Bat", HabitatTheme::Void, 1.0, 120, 90, 130, SPRINTER, false),
        coin_species("void_moth", "Void Moth", HabitatTheme::Void, 1.2, 140, 160, 180, SPRINTER, false),
        coin_species("cosmic_jelly", "Cosmic Jelly", HabitatTheme::Void, 1.3, 260, 200, 220, TANK, false),
        coin_species("null_crawler", "Null Crawler", HabitatTheme::Void, 1.4, 220, 220, 220, BALANCED, false),
        coin_species("dusk_raven", "Dusk Raven", HabitatTheme::Void, 1.5, 180, 220, 220, BALANCED, false),
        coin_species("shade_wisp", "Shade Wisp", HabitatTheme::Void, 1.6, 160, 240, 240, SPRINTER, false),
        coin_species("phantom_stag", "Phantom Stag", HabitatTheme::Void, 2.4, 460, 560, 360, BALANCED, false),
        coin_species("eclipse_hound", "Eclipse Hound", HabitatTheme::Void, 2.6, 520, 640, 360, SPRINTER, false),
        coin_species("nightmare_foal", "Nightmare Foal", HabitatTheme::Void, 2.6, 480, 680, 380, BALANCED, false),
        coin_species("abyss_serpent", "Abyss Serpent", HabitatTheme::Void, 2.8, 500, 820, 420, SPRINTER, false),
        coin_species("star_eater", "Star Eater", HabitatTheme::Void, 3.0, 600, 900, 460, BALANCED, false),
        coin_species("singularity_wyrm", "Singularity Wyrm", HabitatTheme::Void, 3.4, 700, 1200, 480, SPRINTER, false),
        // ── Festive ─────────────────────────────────────────────────────────
        coin_species("peppermint_hare", "Peppermint Hare", HabitatTheme::Festive, 0.8, 90, 50, 90, SPRINTER, false),
        coin_species("candy_cardinal", "Candy Cardinal", HabitatTheme::Festive, 0.9, 100, 70, 100, SPRINTER, false),
        coin_species("cocoa_pup", "Cocoa Pup", HabitatTheme::Festive, 1.0, 140, 90, 120, BALANCED, false),
        coin_species("gift_goose", "Gift Goose", HabitatTheme::Festive, 1.2, 180, 140, 170, BALANCED, false),
        coin_species("jingle_fox", "Jingle Fox", HabitatTheme::Festive, 1.4, 220, 200, 200, SPRINTER, false),
        coin_species("starlight_dove", "Starlight Dove", HabitatTheme::Festive, 1.4, 160, 200, 220, SPRINTER, false),
        coin_species("tinsel_cat", "Tinsel Cat", HabitatTheme::Festive, 1.5, 200, 220, 220, SPRINTER, false),
        coin_species("garland_owl", "Garland Owl", HabitatTheme::Festive, 1.6, 180, 240, 240, BALANCED, false),
        coin_species("reindeer_calf", "Reindeer Calf", HabitatTheme::Festive, 1.7, 360, 300, 260, BALANCED, false),
        coin_species("sugarplum_doe", "Sugarplum Doe", HabitatTheme::Festive, 1.8, 320, 320, 260, BALANCED, false),
        coin_species("frostbell_stag", "Frostbell Stag", HabitatTheme::Festive, 2.4, 460, 560, 360, TANK, false),
        coin_species("sleigh_hound", "Sleigh Hound", HabitatTheme::Festive, 2.6, 520, 640, 360, SPRINTER, false),
        // ── Food ────────────────────────────────────────────────────────────
        coin_species("muffin_mouse", "Muffin Mouse", HabitatTheme::Food, 0.6, 80, 30, 70, SPRINTER, false),
        coin_species("cheddar_rat", "Cheddar Rat", HabitatTheme::Food, 0.6, 90, 35, 70, SPRINTER, false),
        coin_species("berry_finch", "Berry Finch", HabitatTheme::Food, 0.7, 80, 40, 80, SPRINTER, false),
        coin_species("jelly_slug", "Jelly Slug", HabitatTheme::Food, 0.7, 160, 50, 110, TANK, false),
        coin_species("cookie_crab", "Cookie Crab", HabitatTheme::Food, 0.8, 120, 70, 110, TANK, false),
        coin_species("popcorn_quail", "Popcorn Quail", HabitatTheme::Food, 0.9, 110, 80, 120, SPRINTER, false),
        coin_species("marshmallow_lamb", "Marshmallow Lamb", HabitatTheme::Food, 1.0, 200, 120, 160, TANK, false),
        coin_species("noodle_serpent", "Noodle Serpent", HabitatTheme::Food, 1.3, 200, 180, 200, SPRINTER, false),
        coin_species("donut_seal", "Donut Seal", HabitatTheme::Food, 1.6, 360, 280, 260, TANK, false),
        coin_species("caramel_stag", "Caramel Stag", HabitatTheme::Food, 2.0, 400, 420, 320, BALANCED, false),
        coin_species("honey_badger", "Honey Badger", HabitatTheme::Food, 2.2, 420, 460, 300, SPRINTER, false),
        coin_species("pancake_turtle", "Pancake Turtle", HabitatTheme::Food, 0.4, 400, 140, 500, TANK, false),
    ]);

    // ─── New-biome crossbreed hybrids (see RECIPES) ──────────────────────────
    // One or two hybrids per new biome, bred from that biome's wild fauna.
    // Stronger than their parents, themed to the biome.
    entries.extend([
        // Desert
        coin_species("sandwyrm", "Sand Wyrm", HabitatTheme::Desert, 2.6, 520, 900, 300, SPRINTER, true),
        coin_species("dunestrider", "Dunestrider", HabitatTheme::Desert, 2.2, 640, 820, 320, TANK, true),
        coin_species("dashfennec", "Dash Fennec", HabitatTheme::Desert, 2.4, 420, 760, 260, SPRINTER, true),
        // Tundra
        coin_species("glaciox", "Glaciox", HabitatTheme::Tundra, 3.2, 1100, 1300, 420, TANK, true),
        coin_species("tundratitan", "Tundra Titan", HabitatTheme::Tundra, 3.6, 1400, 1700, 460, TANK, true),
        coin_species("snowdart", "Snowdart", HabitatTheme::Tundra, 1.6, 300, 420, 220, SPRINTER, true),
        // Taiga
        coin_species("black_moose", "Black Moose", HabitatTheme::Taiga, 3.8, 1500, 1900, 470, TANK, true),
        coin_species("pinegrizzle", "Pine Grizzle", HabitatTheme::Taiga, 3.4, 1100, 1500, 440, TANK, true),
        coin_species("shadepine", "Shadepine", HabitatTheme::Taiga, 2.6, 520, 900, 300, SPRINTER, true),
        // Volcanic
        coin_species("emberclaw", "Emberclaw", HabitatTheme::Volcanic, 3.0, 640, 1100, 360, BALANCED, true),
        coin_species("magmadrake", "Magma Drake", HabitatTheme::Volcanic, 3.6, 720, 1500, 460, SPRINTER, true),
        coin_species("infernewt", "Infernewt", HabitatTheme::Volcanic, 2.2, 480, 760, 280, BALANCED, true),
        // Badlands
        coin_species("dustcharger", "Dust Charger", HabitatTheme::Badlands, 3.4, 1100, 1400, 420, TANK, true),
        coin_species("mesatitan", "Mesa Titan", HabitatTheme::Badlands, 3.8, 1500, 1800, 470, TANK, true),
        coin_species("fangstalker", "Fangstalker", HabitatTheme::Badlands, 3.2, 560, 1100, 340, SPRINTER, true),
        // Beach
        coin_species("tidehunter", "Tidehunter", HabitatTheme::Beach, 2.8, 620, 1000, 320, SPRINTER, true),
        coin_species("surfmane", "Surfmane", HabitatTheme::Beach, 2.4, 560, 820, 300, BALANCED, true),
        coin_species("shellshade", "Shellshade", HabitatTheme::Beach, 1.6, 360, 520, 240, TANK, true),
        // Highlands
        coin_species("peakprowler", "Peak Prowler", HabitatTheme::Highlands, 3.4, 640, 1300, 380, SPRINTER, true),
        coin_species("summitmaw", "Summit Maw", HabitatTheme::Highlands, 3.6, 1100, 1600, 450, TANK, true),
        coin_species("cragsoar", "Cragsoar", HabitatTheme::Highlands, 2.8, 460, 900, 320, SPRINTER, true),
        // Mythical
        coin_species("skytalon", "Skytalon", HabitatTheme::Mythical, 3.6, 700, 1500, 440, SPRINTER, true),
        coin_species("dracogriff", "Dracogriff", HabitatTheme::Mythical, 4.0, 820, 1900, 470, SPRINTER, true),
        coin_species("fae_sprite", "Fae Sprite", HabitatTheme::Mythical, 2.2, 360, 760, 260, SPRINTER, true),
        // Void
        coin_species("voidmaw", "Voidmaw", HabitatTheme::Void, 3.6, 700, 1500, 460, BALANCED, true),
        coin_species("riftserpent", "Rift Serpent", HabitatTheme::Void, 3.8, 760, 1700, 470, SPRINTER, true),
        coin_species("wraithstag", "Wraith Stag", HabitatTheme::Void, 2.8, 520, 1000, 340, BALANCED, true),
        // Festive
        coin_species("yulebeast", "Yule Beast", HabitatTheme::Festive, 3.2, 900, 1300, 400, TANK, true),
        coin_species("tinselmane", "Tinselmane", HabitatTheme::Festive, 3.0, 640, 1100, 360, BALANCED, true),
        coin_species("candyglow", "Candyglow", HabitatTheme::Festive, 2.0, 360, 700, 260, SPRINTER, true),
        // Food
        coin_species("pastrycoil", "Pastry Coil", HabitatTheme::Food, 2.4, 520, 820, 300, BALANCED, true),
        coin_species("creamfin", "Creamfin", HabitatTheme::Food, 2.6, 640, 900, 320, TANK, true),
        coin_species("toffeestag", "Toffee Stag", HabitatTheme::Food, 2.8, 560, 1000, 340, BALANCED, true),

        // ── Collections content ───────────────────────────────────────────────
        // Obtainable starters (shop, open-vendor themes) — collection requirements.
        coin_species("wedding_dove", "Wedding Dove", HabitatTheme::Farmland, 0.7, 80, 60, 70, BALANCED, false),
        coin_species("flamingo", "Flamingo", HabitatTheme::Wetland, 0.9, 110, 140, 90, BALANCED, false),
        coin_species("ruby_rabbit", "Ruby Rabbit", HabitatTheme::Farmland, 0.8, 90, 120, 80, SPRINTER, false),
        coin_species("ducklet", "Ducklet", HabitatTheme::Farmland, 0.5, 60, 30, 55, BALANCED, false),
        coin_species("lamblet", "Lamblet", HabitatTheme::Farmland, 0.5, 70, 35, 60, TANK, false),
        coin_species("piglet", "Piglet", HabitatTheme::Farmland, 0.6, 70, 40, 60, BALANCED, false),
        coin_species("petal_deer", "Petal Deer", HabitatTheme::Forest, 0.9, 120, 150, 95, BALANCED, false),
        coin_species("glow_moth", "Glow Moth", HabitatTheme::Forest, 0.7, 80, 90, 75, SPRINTER, false),
        coin_species("dew_sprite", "Dew Sprite", HabitatTheme::Wetland, 0.8, 90, 110, 80, SPRINTER, false),
        coin_species("dapper_seal", "Dapper Seal", HabitatTheme::Arctic, 1.0, 140, 180, 100, BALANCED, false),
        coin_species("acro_monkey", "Acro Monkey", HabitatTheme::Jungle, 1.0, 130, 170, 100, SPRINTER, false),
        coin_species("tophat_bear", "Top-Hat Bear", HabitatTheme::Forest, 1.1, 160, 200, 110, TANK, false),
        coin_species("comet_cat", "Comet Cat", HabitatTheme::Forest, 1.0, 120, 160, 100, SPRINTER, false),
        coin_species("lunar_hare", "Lunar Hare", HabitatTheme::Arctic, 0.9, 110, 150, 95, SPRINTER, false),
        coin_species("solar_finch", "Solar Finch", HabitatTheme::Savanna, 0.8, 90, 130, 85, BALANCED, false),
        // Extra early-game hybrids (breeding) — general content + collection fodder.
        coin_species("bunnybird", "Bunnybird", HabitatTheme::Forest, 1.4, 220, 300, 150, SPRINTER, true),
        coin_species("frogoat", "Frogoat", HabitatTheme::Farmland, 1.3, 260, 280, 150, TANK, true),
        // Exclusive collection rewards — collection-only (see COLLECTION_ONLY_IDS),
        // never sold/spawned/bred; granted at Neon. High cost → tier-5 power.
        coin_species("golden_goose", "Golden Goose", HabitatTheme::Farmland, 14.0, 5000, 90000, 3600, SPRINTER, false),
        coin_species("aurora_bear", "Aurora Bear", HabitatTheme::Arctic, 16.0, 6000, 110000, 4200, TANK, false),
        coin_species("sunmane_lion", "Sunmane Lion", HabitatTheme::Savanna, 18.0, 5500, 130000, 4200, SPRINTER, false),
        coin_species("cupid_swan", "Cupid Swan", HabitatTheme::Farmland, 15.0, 5200, 100000, 3900, BALANCED, false),
        coin_species("golden_chick", "Golden Chick", HabitatTheme::Farmland, 13.0, 4800, 85000, 3600, SPRINTER, false),
        coin_species("bloomcat", "Bloomcat", HabitatTheme::Forest, 15.0, 5200, 105000, 3900, BALANCED, false),
        coin_species("ringmaster_lion", "Ringmaster Lion", HabitatTheme::Savanna, 19.0, 6000, 140000, 4500, SPRINTER, false),
        coin_species("astral_owl", "Astral Owl", HabitatTheme::Mythical, 20.0, 6500, 160000, 4800, SPRINTER, false),
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
            "red_fox",
            "treeFrog",
            &[
                PoolEntry { species: "red_fox", weight: 45 },
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

    // Bulk recipes in compact form: (parentA, parentB, wA, wB, &[(hybrid, w)]).
    // The number of hybrid outcomes is deliberately mixed — some crosses drop a
    // single hybrid, some two, a few roll three (the last being a rare jackpot).
    // Weights are varied per recipe so odds don't feel mechanical; hybrids are
    // always rarer than the parents, and successive hybrids get rarer still.
    #[rustfmt::skip]
    let simple: &[(SpeciesId, SpeciesId, u32, u32, &[(SpeciesId, u32)])] = &[
        ("rabbit", "chicken",       42, 42, &[("chickbit", 16)]),
        ("sheep", "goat",           41, 41, &[("shoat", 13), ("billowool", 5)]),
        ("cow", "sheep",            40, 40, &[("woolcow", 15), ("hornwool_bovram", 5)]),
        ("duck", "beaver",          43, 43, &[("duckver", 14)]),
        ("heron", "salamander",     43, 40, &[("heronder", 12), ("marshplume", 5)]),
        ("blue_frog", "duck",       44, 44, &[("frock", 12)]),
        ("hedgehog", "badger",      39, 39, &[("hedger", 15), ("bramblesett", 7)]),
        ("raccoon", "squirrel",     42, 42, &[("rascurrel", 15)]),
        ("grey_wolf", "boar",            41, 41, &[("wolboar", 12), ("tuskhowl", 6)]),
        ("red_fox", "grey_wolf",             40, 40, &[("direfox", 12), ("duskrunner", 6), ("nightfang", 2)]),
        ("field_mouse", "hedgehog", 40, 40, &[("prickmouse", 14), ("quillsqueak", 6)]),
        ("arctic_fox", "seal",      42, 40, &[("sealfox", 12), ("frostpup", 6)]),
        ("walrus", "reindeer",      41, 41, &[("walrideer", 13), ("tundratusk", 5)]),
        ("penguin", "seal",         43, 43, &[("penseal", 13)]),
        ("polar_bear", "walrus",    41, 41, &[("tuskbear", 11), ("blizzardmaw", 5), ("glacialith", 2)]),
        ("cheetah", "giraffe",      40, 42, &[("cheeraffe", 12), ("spotspire", 6)]),
        ("elephant", "rhino",       40, 40, &[("elephino", 12), ("pachyhorn", 6), ("colossatusk", 2)]),
        ("meerkat", "ostrich",      40, 40, &[("meerich", 14), ("sentryplume", 6)]),
        ("lion", "cheetah",         40, 40, &[("liotah", 12), ("prideflash", 6), ("blitzfang", 2)]),
        ("zebra", "giraffe",        41, 41, &[("zebraffe", 13), ("stripespire", 5)]),
        ("parrot", "jaguar",        42, 42, &[("parrojag", 11), ("plumeprowl", 5)]),
        ("sloth", "chameleon",      40, 40, &[("slowmeleon", 14), ("mosslimber", 6)]),
        ("monkey", "parrot",        41, 43, &[("monrot", 11), ("chatterperch", 5)]),
        ("toucan", "parrot",        44, 44, &[("torrot", 12)]),
        ("goldenToucan", "sloth",   42, 40, &[("goldsloth", 12), ("gildedyawn", 6)]),
        ("dolphin", "octopus",      41, 41, &[("doctopus", 13), ("inkfin", 5)]),
        ("pufferfish", "crab",      40, 42, &[("puffcrab", 13), ("spineshell", 5)]),
        ("reefSeahorse", "dolphin", 42, 42, &[("seadolph", 11), ("tidecurl", 5)]),
        ("goldenCarp", "pufferfish",43, 43, &[("goldpuff", 10), ("gildedspine", 4)]),
        ("otter", "beaver",         40, 40, &[("ottaver", 14), ("rivergnaw", 6)]),
        ("snowyOwl", "arctic_fox",  42, 40, &[("owlfox", 12), ("frosttalon", 6)]),
        ("giantTortoise", "reindeer",44, 44, &[("tortdeer", 8), ("shellantler", 4)]),
        ("albinoDeer", "reindeer",  41, 41, &[("ghostdeer", 13), ("palevelvet", 5)]),
        // Early-game forest hybrids — more generous hybrid odds so the opening
        // hours pay off quickly.
        ("red_fox", "field_mouse",      38, 38, &[("scamp", 17), ("kitnip", 7)]),
        ("field_mouse", "blue_frog",38, 38, &[("lilyleap", 17), ("pipsplash", 7)]),
        ("blue_frog", "red_fox",        39, 39, &[("marshmask", 15), ("bogtrot", 7)]),
        ("red_fox", "lion",             40, 40, &[("embermane", 14), ("cinderpaw", 6)]),
        ("blue_frog", "lion",       40, 40, &[("bogmane", 14), ("marshpride", 6)]),
        ("field_mouse", "lion",     39, 39, &[("pridelet", 15), ("squeakmane", 7)]),
        ("mole", "field_mouse",     38, 40, &[("burrowkin", 16), ("tunnelnib", 6)]),
        ("rabbit", "robin",         39, 39, &[("bunnybird", 16)]),
        ("blue_frog", "goat",       39, 39, &[("frogoat", 15)]),
        ("lynx", "albinoDeer",      41, 41, &[("stagstalker", 12), ("snowprowl", 6)]),
        // Concept hybrids: a themed exotic crossed with something from the roster.
        ("candy_dove", "blue_frog", 40, 40, &[("gumdrop", 14), ("fizzhopper", 6)]),
        ("robot_cat", "field_mouse",41, 41, &[("glitchpaw", 12), ("bitsqueak", 6)]),
        ("moophin", "penguin",      42, 40, &[("tuxtide", 12), ("frostmoo", 6)]),
        ("zombie_dog", "rabbit",    40, 40, &[("hopocalypse", 14), ("gnawhop", 6)]),
        ("lava_lynx", "arctic_fox", 41, 41, &[("glass_fox", 12), ("ashpaw", 6)]),
        ("crystal_stag", "albinoDeer",40, 40, &[("prismhart", 11), ("gleamantler", 5), ("auroracrown", 2)]),
        ("origami_crane", "heron",  41, 41, &[("foldfeather", 12), ("creasewing", 6)]),
        ("clockwork_owl", "snowyOwl",42, 42, &[("ticktalon", 11), ("gearhoot", 5)]),
        ("galaxy_whale", "dolphin", 40, 40, &[("stardive", 10), ("comettail", 5), ("voidsong", 2)]),
        ("mushroom_toad", "treeFrog",40, 40, &[("sporehop", 14), ("shroomleap", 6)]),
        ("shadow_panther", "jaguar",40, 40, &[("nightmaw", 11), ("umbraprowl", 5), ("eclipsemaw", 2)]),
        ("plush_bear", "polar_bear",41, 41, &[("snugfang", 12), ("fluffmaw", 6)]),
        ("neon_gecko", "chameleon", 40, 42, &[("voltscale", 12), ("glowtail", 6)]),
        ("candy_dove", "birthday_horse",42, 42, &[("jack_of_all_manes", 11), ("confettihoof", 5)]),
        ("robot_cat", "galaxy_whale",40, 40, &[("mechabyss", 10), ("cogwhale", 5), ("singularis", 2)]),
        ("zombie_dog", "shadow_panther",42, 42, &[("gravestalker", 11), ("cryptfang", 5)]),
        // ── New-biome crossbreeds (proc-gen overhaul fauna) ──────────────────
        // Desert
        ("camel", "sand_viper",       40, 40, &[("sandwyrm", 13), ("dunestrider", 5)]),
        ("fennec_fox", "roadrunner",  42, 42, &[("dashfennec", 12)]),
        // Tundra
        ("musk_ox", "tundra_wolf",    40, 40, &[("glaciox", 12), ("tundratitan", 5)]),
        ("arctic_hare", "ermine",     43, 43, &[("snowdart", 12)]),
        // Taiga
        ("moose", "brown_bear",       40, 40, &[("black_moose", 12), ("pinegrizzle", 5)]),
        ("pine_marten", "sable",      42, 42, &[("shadepine", 12)]),
        // Volcanic
        ("magma_crab", "cinder_drake",40, 40, &[("emberclaw", 13), ("magmadrake", 5)]),
        ("lava_newt", "fire_salamander",42, 42, &[("infernewt", 12)]),
        // Badlands
        ("coyote", "bison",           40, 40, &[("dustcharger", 12), ("mesatitan", 5)]),
        ("rattlesnake", "cougar",     42, 42, &[("fangstalker", 12)]),
        // Beach
        ("sea_lion", "osprey",        40, 40, &[("tidehunter", 13), ("surfmane", 5)]),
        ("hermit_crab", "ghost_crab", 43, 43, &[("shellshade", 12)]),
        // Highlands
        ("yak", "snow_leopard",       40, 40, &[("peakprowler", 12), ("summitmaw", 5)]),
        ("ibex", "golden_eagle",      42, 42, &[("cragsoar", 12)]),
        // Mythical
        ("griffon_chick", "wyvern",   40, 40, &[("skytalon", 13), ("dracogriff", 5)]),
        ("pixie", "faun",             42, 42, &[("fae_sprite", 12)]),
        // Void
        ("eclipse_hound", "abyss_serpent",40, 40, &[("voidmaw", 12), ("riftserpent", 5)]),
        ("shade_wisp", "phantom_stag",42, 42, &[("wraithstag", 12)]),
        // Festive
        ("frostbell_stag", "sleigh_hound",40, 40, &[("yulebeast", 13), ("tinselmane", 5)]),
        ("jingle_fox", "sugarplum_doe",42, 42, &[("candyglow", 12)]),
        // Food
        ("donut_seal", "noodle_serpent",40, 40, &[("pastrycoil", 12), ("creamfin", 5)]),
        ("honey_badger", "caramel_stag",42, 42, &[("toffeestag", 12)]),
    ];
    for (a, b, wa, wb, hybrids) in simple {
        let mut pool = vec![
            PoolEntry { species: *a, weight: *wa },
            PoolEntry { species: *b, weight: *wb },
        ];
        for (h, w) in *hybrids {
            pool.push(PoolEntry { species: *h, weight: *w });
        }
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

/// Species obtainable **only** by completing a collection (see
/// `game::collection`). They live in the catalog (so icons / power / lookup work)
/// but must never appear in any shop, wild spawn, or crossbreed pool.
pub const COLLECTION_ONLY_IDS: &[SpeciesId] = &[
    "golden_goose",
    "aurora_bear",
    "sunmane_lion",
    "cupid_swan",
    "golden_chick",
    "bloomcat",
    "ringmaster_lion",
    "astral_owl",
];

/// True if `id` is a collection-only reward species.
pub fn is_collection_only(id: SpeciesId) -> bool {
    COLLECTION_ONLY_IDS.contains(&id)
}

/// Regular shop species — neither crossbreed offspring, exotics, nor
/// collection-only rewards.
pub fn all_purchasable() -> impl Iterator<Item = &'static SpeciesDef> {
    CATALOG.values().filter(|d| !d.hybrid && !d.exotic && !is_collection_only(d.id))
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
        // A pool can have two non-parent hybrids (primary + rarer secondary);
        // the codex summary lists the primary (highest-weight) one. Outcome
        // panels read the full pool via `crossbreed_pool` instead.
        if let Some(hybrid) = pool
            .iter()
            .filter(|e| e.species != *a && e.species != *b)
            .max_by_key(|e| e.weight)
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

    /// At least 90% of the *breeding roster* must participate in some recipe —
    /// either as a parent or as the hybrid outcome. The new-biome wild fauna
    /// (added in the proc-gen overhaul) are intentionally catch-only and not
    /// part of the breeding economy yet, so they're excluded here. They're
    /// identified by living in one of the new biome themes, which no
    /// pre-existing breeding species uses.
    #[test]
    fn most_species_belong_to_a_recipe() {
        use HabitatTheme::*;
        let is_breeding_theme = |t: HabitatTheme| {
            matches!(t, Farmland | Forest | Arctic | Savanna | Wetland | Jungle | Ocean)
        };
        let mut in_recipe: HashSet<SpeciesId> = HashSet::new();
        for ((a, b), pool) in RECIPES.iter() {
            in_recipe.insert(*a);
            in_recipe.insert(*b);
            for e in pool {
                in_recipe.insert(e.species);
            }
        }
        // Collection content (the required starters + the exclusive rewards) is a
        // separate acquisition path (shop + collection claim), intentionally
        // outside the breeding economy — exclude it from this guardrail.
        let mut collection_ids: HashSet<SpeciesId> = HashSet::new();
        for c in crate::game::collection::COLLECTIONS {
            for s in c.required {
                collection_ids.insert(*s);
            }
            if let crate::game::collection::Reward::Animal(sp) = c.reward {
                collection_ids.insert(sp);
            }
        }
        let roster: Vec<&SpeciesDef> = CATALOG
            .values()
            .filter(|d| is_breeding_theme(d.theme) && !collection_ids.contains(d.id))
            .collect();
        let total = roster.len();
        let covered = roster.iter().filter(|d| in_recipe.contains(d.id)).count();
        let pct = covered as f32 / total as f32;
        assert!(
            pct >= 0.90,
            "only {covered}/{total} ({:.1}%) breeding-roster species belong to a recipe",
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

    /// Outcome counts should be mixed: some crosses drop one hybrid, some two,
    /// and a few roll three. Every recipe has 1–3 hybrids.
    #[test]
    fn recipe_outcome_counts_are_mixed() {
        let mut counts: HashSet<usize> = HashSet::new();
        for (a, b, _) in all_recipes() {
            let pool = crossbreed_pool(a, b).expect("recipe pool exists");
            let n = pool.iter().filter(|e| e.species != a && e.species != b).count();
            assert!((1..=3).contains(&n), "recipe {a}+{b} has {n} hybrid outcomes");
            counts.insert(n);
        }
        assert!(counts.contains(&1), "expected some single-hybrid recipes");
        assert!(counts.contains(&2), "expected some two-hybrid recipes");
        assert!(counts.contains(&3), "expected some three-hybrid recipes");
    }

    /// Odds must be sane and varied: every hybrid is rarer than each parent,
    /// successive hybrids in a pool get strictly rarer, and the per-recipe
    /// weight sets aren't all identical.
    #[test]
    fn recipe_odds_are_varied_and_hybrids_rarer() {
        // Two original hand-tuned exotic pools deliberately weight an exotic
        // parent below a hybrid, so they're exempt from the ordering invariant.
        let legacy = |a: SpeciesId, b: SpeciesId| {
            let mut pair = [a, b];
            pair.sort_unstable();
            pair == ["biggy_cheese", "polar_bear"] || pair == ["reefSeahorse", "stained_butterfly"]
        };
        let mut weight_sets: HashSet<Vec<u32>> = HashSet::new();
        for (a, b, _) in all_recipes() {
            let pool = crossbreed_pool(a, b).unwrap();
            let mut ws: Vec<u32> = pool.iter().map(|e| e.weight).collect();
            ws.sort_unstable();
            weight_sets.insert(ws);
            if legacy(a, b) {
                continue;
            }
            let parent_min = pool
                .iter()
                .filter(|e| e.species == a || e.species == b)
                .map(|e| e.weight)
                .min()
                .unwrap();
            // Hybrids appear after the parents, in descending rarity.
            let hyb: Vec<u32> = pool
                .iter()
                .filter(|e| e.species != a && e.species != b)
                .map(|e| e.weight)
                .collect();
            for w in &hyb {
                assert!(*w < parent_min, "recipe {a}+{b}: hybrid not rarer than parents");
            }
            for pair in hyb.windows(2) {
                assert!(pair[0] > pair[1], "recipe {a}+{b}: hybrids must get rarer in order");
            }
        }
        assert!(
            weight_sets.len() >= 6,
            "expected varied recipe odds, found only {} distinct weight sets",
            weight_sets.len()
        );
    }
}

