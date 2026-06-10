use std::collections::{HashMap, HashSet};
use std::fmt;

use chrono::{DateTime, Duration, Utc};
use glam::Vec2;
use uuid::Uuid;

use super::animal::{Animal, AnimalState, MAX_ANIMAL_LEVEL, animal_level_up_cost, animal_sell_value};
use super::habitat::{
    Habitat, MAX_HABITAT_LEVEL, footprints_overlap, habitat_purchase_cost, habitat_upgrade_cost,
    habitat_upgrade_duration,
};
use super::pedestal::{PEDESTAL_OFFLINE_CAP_MULT, Pedestal, pedestal_cost};
use super::player::Player;
use super::rank;
use super::species::{self, HabitatTheme, SpeciesId};
use super::visitor::VisitorRecord;
use super::structure::{
    FOOD_KIND, MAX_FOOD_STRUCTURES, MAX_STRUCTURE_LEVEL, Structure, food_structure_unlock_cost,
    structure_upgrade_cost,
};
use crate::share::{
    GiftContents, GiftPayload, SharedSnapshotPayload, SnapshotView, SpeciesTallyEntry,
};

/// Derive a stable per-world procedural seed from the owning player's id, so a
/// given save always regenerates the same world. New games get a fresh (random)
/// seed because the player id is itself random; the v12→v13 migration uses the
/// same derivation to give existing saves a stable world.
pub fn world_seed_from_player(id: Uuid) -> u64 {
    let (lo, hi) = id.as_u64_pair();
    lo ^ hi.rotate_left(32)
}

/// A player-placed fast-travel marker in the wild world. The home zoo is an
/// implicit default destination and is *not* stored here.
#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub id: Uuid,
    pub name: String,
    pub pos: Vec2,
}

/// A physical breeding nest sitting in the home zoo. Holds up to two deposited
/// animals; breeding/ready state is derived from the occupants' `AnimalState`.
/// Its world position is derived from its index via [`Zoo::nest_pos`], which
/// resolves the nest's assigned plot tile onto the zoo's `plot_origin`.
#[derive(Clone, Debug, PartialEq)]
pub struct Nest {
    pub id: Uuid,
    /// Up to two deposited animal ids (the breeding pair). `None` = empty slot.
    pub slots: [Option<Uuid>; 2],
    /// A freshly-bred offspring waiting to be collected. Set automatically when
    /// breeding completes (the parents are released back to the zoo at that
    /// moment); cleared when the player collects it. In-session only — not
    /// persisted, so a reload treats an uncollected offspring as already free.
    pub offspring: Option<Uuid>,
}

impl Nest {
    pub fn new() -> Self {
        Self { id: crate::game::ids::new_id(), slots: [None, None], offspring: None }
    }
    /// Occupant ids currently in the nest (0–2). Does not include a pending
    /// offspring — only the deposited breeding pair.
    pub fn occupants(&self) -> Vec<Uuid> {
        self.slots.iter().flatten().copied().collect()
    }
    /// Index of the first free slot, if any.
    pub fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_none())
    }
    pub fn contains(&self, id: Uuid) -> bool {
        self.slots.iter().any(|s| *s == Some(id))
    }
}

impl Default for Nest {
    fn default() -> Self {
        Self::new()
    }
}

/// Derived state of a nest for UI + parking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NestStatus {
    /// No occupants.
    Empty,
    /// One occupant; needs a second to breed.
    Partial,
    /// Two occupants, idle, and a valid cross pool exists.
    ReadyToBreed,
    /// Two occupants, idle, but no valid cross (same species / no pool).
    Incompatible,
    /// Mid-gestation; payload is when it finishes.
    Breeding(DateTime<Utc>),
    /// Gestation finished — offspring can be collected.
    ReadyToCollect,
}

pub struct Zoo {
    /// The local owner of this zoo (host in M2 co-op). Keeping the field name
    /// `player` here for source compatibility; semantically this is the host.
    pub player: Player,
    /// Persisted state for non-owner players who have visited. Keyed by their
    /// stable `player_id` so a returning visitor matches their existing
    /// record. Empty in pure single-player saves. Added in schema v12.
    pub visitors: HashMap<Uuid, VisitorRecord>,
    pub coins: u64,
    pub food: u64,
    /// Secondary "DNA Helix" currency. Earned from rare hybrid drops on
    /// crossbreed redemption and from the income of DNA-tier exotic animals
    /// (e.g. Snow Lion). Spent at the exotic shop on otherwise-unobtainable
    /// species.
    pub dna_helix: u64,
    pub habitats: Vec<Habitat>,
    pub animals: HashMap<Uuid, Animal>,
    /// Lifetime count of *duplicate* acquisitions per species, driving each
    /// animal's Rank. Persists even when the species isn't currently owned (so
    /// Rank survives sell + recapture). One-of-each: `animals` holds at most one
    /// animal per species at a time.
    pub species_dupes: HashMap<SpeciesId, u32>,
    pub structures: Vec<Structure>,
    /// Placeable/moveable income stands. Each holds at most one dedicated animal
    /// whose income is auto-swept into the wallet at cap. Added in schema v18.
    pub pedestals: Vec<Pedestal>,
    /// Unplaced pedestals held in the hotbar (bought from the merchant, not yet
    /// dropped in the world). Added in schema v19.
    pub unplaced_pedestals: u32,
    pub claimed_gifts: HashSet<Uuid>,
    /// Crossbreed recipes the player has unlocked by rolling a hybrid drop.
    /// Recorded only on `claim_completed_breeding` when offspring is not a
    /// parent — parent drops don't count as discoveries.
    pub discovered_recipes: HashSet<SpeciesId>,
    /// How many nests the player has unlocked (also the concurrent-breeding
    /// cap). Starts at 0 — all `MAX_NESTS` begin locked. Kept in sync with
    /// `nests.len()`.
    pub nest_count: u8,
    /// Physical breeding nests in the home zoo. `nests.len() == nest_count`.
    /// Each holds up to two deposited animals; breeding is driven through them.
    /// Added in schema v15.
    pub nests: Vec<Nest>,
    /// When set, the index of an exotic-shop window the player paid DNA Helix
    /// to open early during its closed gap. Honored only while that window is
    /// the *next* one (see `exotic_shop::effective_window`); self-expires once
    /// it opens naturally. `None` normally.
    pub exotic_skip_window: Option<i64>,
    /// Procedural-generation seed for this player's world. Drives deterministic
    /// cosmetic scatter (grass, terrain props) and seeds expedition arrangement.
    /// Added in schema v13.
    pub world_seed: u64,
    /// Player-placed fast-travel waypoints (the home zoo is implicit). Added v14.
    pub waypoints: Vec<Waypoint>,
    /// World-space centre of this zoo's plot. All plot-relative geometry (nests,
    /// food structures, …) is laid out around this point. For a single-player /
    /// solo zoo it's the world centre; on a shared hub each player's zoo gets a
    /// distinct origin. Runtime-only (not yet persisted): set on construction and
    /// load, and reassigned when a zoo is placed on a hub.
    pub plot_origin: Vec2,
    /// Zoo expansion level (0 = starting plot). Drives the physical plot size
    /// (via `plot_half_extent` / `plot_tile_radius`) and the animal capacity.
    /// Added v16.
    pub zoo_level: u8,
    /// When `Some`, an expansion from `zoo_level` to `zoo_level + 1` is in
    /// flight, completing at this instant; the player claims it with
    /// `claim_zoo_upgrade`. Mirrors the habitat upgrade two-phase pattern.
    pub zoo_upgrade_finishes_at: Option<DateTime<Utc>>,
    pub last_saved_at: DateTime<Utc>,
}

/// Outcome of a successful `claim_completed_breeding`. The UI uses
/// `is_hybrid_drop` to pick the status message ("Frox! +1 DNA Helix" vs
/// "Fox cub") and to decide whether to play any drop-specific feedback.
#[derive(Debug, Clone, Copy)]
pub struct ClaimedBreeding {
    pub habitat_id: Uuid,
    pub animal_id: Uuid,
    pub offspring_species: SpeciesId,
    pub is_hybrid_drop: bool,
}

/// Aggregate of a collect action across one or more animals. UI displays
/// "+N coins · +M DNA" when both are non-zero, or just the non-zero side.
#[derive(Debug, Clone, Copy, Default)]
pub struct CollectResult {
    pub coins: u64,
    pub dna: u64,
}

impl CollectResult {
    pub fn total(&self) -> u64 {
        self.coins.saturating_add(self.dna)
    }
}

/// Hard cap on nests. All five start locked; the first two are unlocked with
/// coins, the remaining three with DNA Helix.
pub const MAX_NESTS: u8 = 5;

// ── Zoo expansion (plot size + animal capacity) ───────────────────────────────
//
// The home zoo starts small and is expanded with coins. Each expansion grows
// the physical plot (via the per-zoo `plot_*` helpers) *and* the animal
// capacity. Cost climbs on a moderate exponential curve; the build time climbs
// on a gentle linear one — early expansions are quick and cheap, late ones are
// a real investment without ballooning out of reach.

/// Hard cap on zoo expansion level (0 = starting plot). The world is vast, so
/// the progression is long: enough levels that capacity climbs all the way to
/// ~1 000 — eventually enough to hold one of every animal as the roster grows.
pub const MAX_ZOO_LEVEL: u8 = 50;
/// Total animals a level-0 zoo can hold.
pub const ZOO_CAPACITY_BASE: usize = 12;
/// Extra animal capacity granted per expansion level. Linear growth: at
/// [`MAX_ZOO_LEVEL`] the zoo holds `12 + 50·20 = 1012` animals.
pub const ZOO_CAPACITY_PER_LEVEL: usize = 20;

/// Max number of animals a zoo at `level` can hold.
pub fn zoo_animal_capacity(level: u8) -> usize {
    ZOO_CAPACITY_BASE + level as usize * ZOO_CAPACITY_PER_LEVEL
}

/// Coins to expand the zoo from `level` to `level + 1`. Moderate exponential
/// tuned for the long 50-level track: `2000 · 1.20^level`, rounded — 2 000 →
/// 2 400 → 2 880 → … reaching ~18M at the final level. Returns `None` once
/// [`MAX_ZOO_LEVEL`] is reached.
pub fn zoo_upgrade_cost(level: u8) -> Option<u64> {
    if level >= MAX_ZOO_LEVEL {
        return None;
    }
    let cost = 2_000.0 * 1.20_f64.powi(level as i32);
    Some(cost.round() as u64)
}

/// Hard ceiling on a single expansion's build time: 4 real days.
pub const ZOO_UPGRADE_MAX_SECS: i64 = 4 * 24 * 60 * 60;

/// Real time to build the expansion from `level` to `level + 1`. Ramps gently
/// across the 50-level track so it starts in minutes, climbs through hours, and
/// only reaches the [4-day] (`ZOO_UPGRADE_MAX_SECS`) ceiling near the very top:
/// `180s · 1.18^level`, clamped — ~3 min early, ~1.5 h around level 20, ~1.5
/// days around level 40, then 4 days. Returns `None` past max level.
pub fn zoo_upgrade_duration(level: u8) -> Option<Duration> {
    if level >= MAX_ZOO_LEVEL {
        return None;
    }
    let secs = (180.0 * 1.18_f64.powi(level as i32)).round() as i64;
    Some(Duration::seconds(secs.min(ZOO_UPGRADE_MAX_SECS)))
}

/// What it costs to unlock a nest, paid in one currency or the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NestCost {
    Coins(u64),
    Dna(u64),
}

/// Cost to unlock the next nest given how many the player already owns. The
/// first two slots are coin-gated, the last three DNA-gated. Returns `None`
/// once all [`MAX_NESTS`] are owned.
pub fn nest_unlock_cost(owned: u8) -> Option<NestCost> {
    match owned {
        0 => Some(NestCost::Coins(1_500)),
        1 => Some(NestCost::Coins(25_000)),
        2 => Some(NestCost::Dna(5)),
        3 => Some(NestCost::Dna(15)),
        4 => Some(NestCost::Dna(40)),
        _ => None,
    }
}

impl Zoo {
    pub fn new(now: DateTime<Utc>) -> Self {
        let starter_habitat = Habitat::new(HabitatTheme::Forest);
        let player = Player::new_default();
        let world_seed = world_seed_from_player(player.id);
        Self {
            player,
            visitors: HashMap::new(),
            coins: 100,
            food: 0,
            dna_helix: 0,
            habitats: vec![starter_habitat],
            animals: HashMap::new(),
            species_dupes: HashMap::new(),
            // All five food structures start locked, like nests.
            structures: Vec::new(),
            pedestals: Vec::new(),
            unplaced_pedestals: 0,
            claimed_gifts: HashSet::new(),
            discovered_recipes: HashSet::new(),
            nest_count: 0,
            nests: Vec::new(),
            exotic_skip_window: None,
            world_seed,
            waypoints: Vec::new(),
            plot_origin: super::plot::world_center(),
            zoo_level: 0,
            zoo_upgrade_finishes_at: None,
            last_saved_at: now,
        }
    }

    /// Maximum number of player-placed waypoints.
    pub const MAX_WAYPOINTS: usize = 12;

    /// Add a fast-travel waypoint at `pos`. Returns its id, or `None` if the
    /// waypoint cap has been reached.
    pub fn add_waypoint(&mut self, name: impl Into<String>, pos: Vec2) -> Option<Uuid> {
        if self.waypoints.len() >= Self::MAX_WAYPOINTS {
            return None;
        }
        let id = crate::game::ids::new_id();
        self.waypoints.push(Waypoint { id, name: name.into(), pos });
        Some(id)
    }

    /// Remove a waypoint by id. Returns true if one was removed.
    pub fn remove_waypoint(&mut self, id: Uuid) -> bool {
        let before = self.waypoints.len();
        self.waypoints.retain(|w| w.id != id);
        self.waypoints.len() != before
    }

    /// A default auto-generated waypoint name (`Waypoint N`).
    pub fn next_waypoint_name(&self) -> String {
        format!("Waypoint {}", self.waypoints.len() + 1)
    }

    /// Pay `SKIP_WAIT_DNA_COST` DNA Helix to open the next exotic-shop window
    /// early during its closed gap. Fails if the shop is already open or the
    /// player can't afford it. Idempotent within a gap: re-paying while the
    /// override is already set still charges (caller should gate on the UI),
    /// so callers check `is_open_with_skip` first.
    pub fn skip_exotic_wait(&mut self, now: DateTime<Utc>) -> Result<(), ZooError> {
        use crate::game::exotic_shop;
        if exotic_shop::is_open_with_skip(now, self.exotic_skip_window) {
            return Err(ZooError::ExoticShopOpen);
        }
        if self.dna_helix < exotic_shop::SKIP_WAIT_DNA_COST {
            return Err(ZooError::NotEnoughDna);
        }
        self.dna_helix -= exotic_shop::SKIP_WAIT_DNA_COST;
        self.exotic_skip_window = Some(exotic_shop::window_index(now) + 1);
        Ok(())
    }

    /// Number of distinct breeding pairs currently mid-gestation. Used to
    /// gate new breedings against `nest_count`.
    pub fn active_breeding_pair_count(&self) -> usize {
        self.animals
            .values()
            .filter(|a| matches!(a.state, AnimalState::Breeding { .. }))
            .count()
            / 2
    }

    /// Unlock the next nest, paying coins or DNA per [`nest_unlock_cost`].
    /// Capped at `MAX_NESTS`. Adds a physical `Nest` (its world position is
    /// derived from its index).
    pub fn buy_nest(&mut self) -> Result<u8, ZooError> {
        let cost = nest_unlock_cost(self.nest_count).ok_or(ZooError::NestCapReached)?;
        match cost {
            NestCost::Coins(c) => {
                if self.coins < c {
                    return Err(ZooError::NotEnoughCoins);
                }
                self.coins -= c;
            }
            NestCost::Dna(d) => {
                if self.dna_helix < d {
                    return Err(ZooError::NotEnoughDna);
                }
                self.dna_helix -= d;
            }
        }
        self.nest_count += 1;
        self.nests.push(Nest::new());
        Ok(self.nest_count)
    }

    // ── Plot tile grid ────────────────────────────────────────────────────────
    //
    // A zoo plot is a square grid of `ZOO_TILE_W`-wide tiles centred on
    // `plot_origin`. Tile (0,0) is the plot centre and tile coordinates are
    // centre-relative integers, so the grid re-bases cleanly onto any plot
    // origin on a shared hub. Every durable plot object — breeding nests, food
    // structures, pedestals — lives on a tile, so its world position derives
    // from this single mapping rather than ad-hoc offset arithmetic.

    /// Half this zoo's plot edge length, in world units (plot spans
    /// `plot_origin ± plot_half_extent` on each axis). Derived from the zoo's own
    /// expansion level, independent of the process-global plot geometry.
    pub fn plot_half_extent(&self) -> f32 {
        crate::game::plot::zoo_tiles_for_level(self.zoo_level) as f32
            * crate::game::plot::ZOO_TILE_W
            * 0.5
    }

    /// Half-width of the plot in tiles: the valid tile range is `-r..=r` on each
    /// axis (a `2r+1` square centred on tile 0). Derived from this zoo's own
    /// expansion level.
    pub fn plot_tile_radius(&self) -> i32 {
        (crate::game::plot::zoo_tiles_for_level(self.zoo_level) - 1) / 2
    }

    /// World-space centre of `tile` within this plot.
    pub fn tile_to_world(&self, tile: (i32, i32)) -> Vec2 {
        self.plot_origin
            + Vec2::new(tile.0 as f32, tile.1 as f32) * crate::game::plot::ZOO_TILE_W
    }

    /// Snap a world position to the nearest plot tile (centre-relative).
    pub fn world_to_tile(&self, world: Vec2) -> (i32, i32) {
        let rel = (world - self.plot_origin) / crate::game::plot::ZOO_TILE_W;
        (rel.x.round() as i32, rel.y.round() as i32)
    }

    /// True when `tile` lies on this plot's grid.
    pub fn tile_in_bounds(&self, tile: (i32, i32)) -> bool {
        let r = self.plot_tile_radius();
        tile.0.abs() <= r && tile.1.abs() <= r
    }

    /// Lay `N` items in an **equidistant** row at `row_y`, centred on the plot.
    /// Uses a single integer tile `step` (the largest that still fits within one
    /// tile of each fence), so every gap is identical — no float rounding, no
    /// clustering at the ends. Shared by the nest and food-structure rows.
    fn row_tiles<const N: usize>(&self, row_y: i32) -> [(i32, i32); N] {
        let edge = (self.plot_tile_radius() - 1).max(0);
        let mut out = [(0, 0); N];
        if N <= 1 {
            if let Some(p) = out.first_mut() {
                *p = (0, row_y);
            }
            return out;
        }
        let n = N as i32;
        // Largest uniform step keeping the half-span (step·(n-1)/2) within `edge`.
        let step = ((2 * edge) / (n - 1)).max(1);
        let start = -(step * (n - 1)) / 2;
        for (i, p) in out.iter_mut().enumerate() {
            *p = (start + i as i32 * step, row_y);
        }
        out
    }

    // ── Physical nests ──────────────────────────────────────────────────────

    /// Centre-relative tiles of the (up to `MAX_NESTS`) breeding nests, laid in
    /// a row inset one tile from the **top** fence.
    pub fn nest_tiles(&self) -> [(i32, i32); MAX_NESTS as usize] {
        let row_y = -(self.plot_tile_radius() - 1).max(0);
        self.row_tiles(row_y)
    }

    /// Centre-relative tiles of the (up to [`MAX_FOOD_STRUCTURES`]) food
    /// structures, mirrored in a row inset one tile from the **bottom** fence.
    pub fn food_structure_tiles(&self) -> [(i32, i32); MAX_FOOD_STRUCTURES] {
        let row_y = (self.plot_tile_radius() - 1).max(0);
        self.row_tiles(row_y)
    }

    /// World position of the nest at `index`, on its assigned plot tile.
    pub fn nest_pos(&self, index: usize) -> Vec2 {
        let tiles = self.nest_tiles();
        self.tile_to_world(tiles[index.min(tiles.len() - 1)])
    }

    /// World position of the food structure at `index`, on its assigned plot tile.
    pub fn food_structure_pos(&self, index: usize) -> Vec2 {
        let tiles = self.food_structure_tiles();
        self.tile_to_world(tiles[index.min(tiles.len() - 1)])
    }

    /// True if `animal_id` currently sits in any nest slot.
    pub fn animal_in_any_nest(&self, animal_id: Uuid) -> bool {
        self.nests.iter().any(|n| n.contains(animal_id))
    }

    /// Deposit `animal_id` into the first free slot of nest `nest_id`.
    pub fn deposit_in_nest(&mut self, nest_id: Uuid, animal_id: Uuid) -> Result<(), ZooError> {
        if !self.animals.contains_key(&animal_id) {
            return Err(ZooError::UnknownAnimal);
        }
        if self.animal_in_any_nest(animal_id) || self.animal_on_any_pedestal(animal_id) {
            return Err(ZooError::AlreadyNested);
        }
        let nest = self.nests.iter_mut().find(|n| n.id == nest_id).ok_or(ZooError::UnknownNest)?;
        let slot = nest.free_slot().ok_or(ZooError::NestFull)?;
        nest.slots[slot] = Some(animal_id);
        Ok(())
    }

    /// Remove the occupant in `slot` of nest `nest_id`. Refused while that
    /// animal is mid-breed. Returns the removed animal id.
    pub fn remove_from_nest(&mut self, nest_id: Uuid, slot: usize) -> Result<Uuid, ZooError> {
        let occupant = {
            let nest = self.nests.iter().find(|n| n.id == nest_id).ok_or(ZooError::UnknownNest)?;
            *nest.slots.get(slot).ok_or(ZooError::UnknownNest)?
        };
        let id = occupant.ok_or(ZooError::UnknownAnimal)?;
        if matches!(self.animals.get(&id).map(|a| &a.state), Some(AnimalState::Breeding { .. })) {
            return Err(ZooError::OccupantBreeding);
        }
        if let Some(nest) = self.nests.iter_mut().find(|n| n.id == nest_id) {
            nest.slots[slot] = None;
        }
        Ok(id)
    }

    /// Begin breeding the two occupants of `nest_id` (reuses `start_breeding`,
    /// which validates the cross pool, gestation, and nest capacity).
    pub fn nest_breed(&mut self, nest_id: Uuid, now: DateTime<Utc>) -> Result<DateTime<Utc>, ZooError> {
        let occ = self
            .nests
            .iter()
            .find(|n| n.id == nest_id)
            .ok_or(ZooError::UnknownNest)?
            .occupants();
        if occ.len() < 2 {
            return Err(ZooError::SpeciesMismatch);
        }
        self.start_breeding(occ[0], occ[1], now)
    }

    /// Auto-advance every nest whose gestation has finished: roll the offspring,
    /// create it as an owned L1 animal, **release both parents** back to the zoo
    /// (Idle, out of the nest), and leave the offspring sitting in the nest until
    /// the player collects it. Returns `(nest_index, offspring_species)` for each
    /// nest that just completed, so the caller can play feedback. Idempotent —
    /// a nest already holding a pending offspring is skipped.
    pub fn advance_nests(&mut self, now: DateTime<Utc>) -> Vec<(usize, SpeciesId)> {
        let mut hatched = Vec::new();
        for i in 0..self.nests.len() {
            if self.nests[i].offspring.is_some() {
                continue;
            }
            let occ = self.nests[i].occupants();
            if occ.len() < 2 {
                continue;
            }
            let (a_id, b_id) = (occ[0], occ[1]);
            // Both parents share the same breeding `ends_at`; read it off parent A.
            let ends_at = match self.animals.get(&a_id).map(|a| &a.state) {
                Some(AnimalState::Breeding { ends_at, .. }) => *ends_at,
                _ => continue,
            };
            if ends_at > now {
                continue;
            }
            let species_a = self.animals[&a_id].species;
            let species_b = self.animals[&b_id].species;
            // Deterministic roll from the pair + ends_at (reload-stable).
            let offspring_species = match species::crossbreed_pool(species_a, species_b) {
                Some(pool) if !pool.is_empty() => {
                    let seed = species::pool_seed(a_id, b_id, ends_at.timestamp());
                    species::roll_pool(seed, pool)
                }
                _ => species_a,
            };
            let off_id = match self.spawn_animal_freeform(offspring_species, 1, now) {
                Ok(id) => id,
                Err(_) => continue,
            };
            // Release both parents from the nest, back to Idle.
            for x in [a_id, b_id] {
                if let Some(a) = self.animals.get_mut(&x) {
                    a.state = AnimalState::Idle;
                    a.last_collected_at = now;
                }
            }
            self.nests[i].slots = [None, None];
            self.nests[i].offspring = Some(off_id);
            hatched.push((i, offspring_species));
        }
        hatched
    }

    /// Collect a nest's finished offspring. The offspring is already an owned
    /// animal (created when breeding completed); collecting simply releases it
    /// from the nest into the zoo. Awards the hybrid bonus (DNA + codex) here,
    /// when the player actually claims it. Returns `(species, is_hybrid_drop)`.
    pub fn nest_collect(
        &mut self,
        nest_id: Uuid,
        _now: DateTime<Utc>,
    ) -> Result<(SpeciesId, bool), ZooError> {
        let off_id = {
            let nest = self
                .nests
                .iter_mut()
                .find(|n| n.id == nest_id)
                .ok_or(ZooError::UnknownNest)?;
            nest.offspring.take().ok_or(ZooError::NotReady)?
        };
        let species = self
            .animals
            .get(&off_id)
            .map(|a| a.species)
            .ok_or(ZooError::UnknownAnimal)?;
        // A hybrid (cross-only) offspring is the player's discovery payoff.
        let is_hybrid = species::get(species).hybrid;
        if is_hybrid {
            self.dna_helix = self.dna_helix.saturating_add(1);
            self.discovered_recipes.insert(species);
        }
        Ok((species, is_hybrid))
    }

    /// Derived status of a nest for the UI + critter parking. (`_now` is kept
    /// for call-site symmetry; completion is now driven by `advance_nests`.)
    pub fn nest_status(&self, nest_id: Uuid, _now: DateTime<Utc>) -> NestStatus {
        let Some(nest) = self.nests.iter().find(|n| n.id == nest_id) else {
            return NestStatus::Empty;
        };
        // A pending offspring is the highest-priority state.
        if nest.offspring.is_some() {
            return NestStatus::ReadyToCollect;
        }
        let occ = nest.occupants();
        // Mid-gestation (auto-completes via `advance_nests` once `ends_at` passes).
        if let Some(ends_at) = occ.iter().find_map(|id| match self.animals.get(id).map(|a| &a.state) {
            Some(AnimalState::Breeding { ends_at, .. }) => Some(*ends_at),
            _ => None,
        }) {
            return NestStatus::Breeding(ends_at);
        }
        match occ.len() {
            0 => NestStatus::Empty,
            1 => NestStatus::Partial,
            _ => {
                let sa = self.animals.get(&occ[0]).map(|a| a.species);
                let sb = self.animals.get(&occ[1]).map(|a| a.species);
                match (sa, sb) {
                    (Some(a), Some(b)) if species::crossbreed_pool(a, b).is_some() => {
                        NestStatus::ReadyToBreed
                    }
                    _ => NestStatus::Incompatible,
                }
            }
        }
    }

    /// Map of `animal_id → nest world position` for every nested occupant, so
    /// the render layer can park those critters at their nest.
    pub fn nested_animal_positions(&self) -> HashMap<Uuid, Vec2> {
        let mut out = HashMap::new();
        for (i, nest) in self.nests.iter().enumerate() {
            let base = self.nest_pos(i);
            for (slot, occ) in nest.slots.iter().enumerate() {
                if let Some(id) = occ {
                    // Spread the two occupants either side of the nest centre.
                    let off = if slot == 0 { -28.0 } else { 28.0 };
                    out.insert(*id, Vec2::new(base.x + off, base.y + 20.0));
                }
            }
            // A pending offspring sits in the middle of the nest.
            if let Some(off_id) = nest.offspring {
                out.insert(off_id, Vec2::new(base.x, base.y + 20.0));
            }
        }
        out
    }

    // ── Pedestals (placeable income stands) ──────────────────────────────────

    /// Total pedestals owned — placed plus unplaced (in the hotbar). Drives the
    /// escalating cost index and the [`MAX_PEDESTALS`] cap.
    pub fn pedestals_owned(&self) -> usize {
        self.pedestals.len() + self.unplaced_pedestals as usize
    }

    /// Buy one pedestal from a structure merchant into the hotbar inventory,
    /// paying its escalating DNA-Helix cost. Fails at the cap or when the player
    /// can't afford it.
    pub fn buy_pedestal_item(&mut self) -> Result<(), ZooError> {
        let cost = pedestal_cost(self.pedestals_owned()).ok_or(ZooError::PedestalCapReached)?;
        if self.dna_helix < cost {
            return Err(ZooError::NotEnoughDna);
        }
        self.dna_helix -= cost;
        self.unplaced_pedestals += 1;
        Ok(())
    }

    /// Place one unplaced pedestal from the hotbar onto `tile`. Already paid for
    /// at purchase, so no DNA is charged. Fails with `PedestalEmpty` when the
    /// inventory is empty, or off-plot / overlapping. Returns the new id.
    pub fn place_pedestal(&mut self, tile: (i32, i32)) -> Result<Uuid, ZooError> {
        if self.unplaced_pedestals == 0 {
            return Err(ZooError::PedestalEmpty);
        }
        if !self.tile_in_bounds(tile) {
            return Err(ZooError::OutOfBounds);
        }
        if !self.pedestal_tile_free(tile, None) {
            return Err(ZooError::TileOccupied);
        }
        self.unplaced_pedestals -= 1;
        let ped = Pedestal::new(tile);
        let id = ped.id;
        self.pedestals.push(ped);
        Ok(id)
    }

    /// Relocate pedestal `id` to `tile`. Free to do; validates bounds + overlap.
    pub fn move_pedestal(&mut self, id: Uuid, tile: (i32, i32)) -> Result<(), ZooError> {
        if !self.tile_in_bounds(tile) {
            return Err(ZooError::OutOfBounds);
        }
        if !self.pedestal_tile_free(tile, Some(id)) {
            return Err(ZooError::TileOccupied);
        }
        let ped = self
            .pedestals
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(ZooError::UnknownPedestal)?;
        ped.tile = tile;
        Ok(())
    }

    /// Remove pedestal `id` back into the hotbar inventory (DNA isn't lost — it
    /// re-stacks). Its dedicated animal, if any, is freed to roam. Refused while
    /// it still holds a locked animal.
    pub fn remove_pedestal(&mut self, id: Uuid, now: DateTime<Utc>) -> Result<(), ZooError> {
        let ped = self.pedestals.iter().find(|p| p.id == id).ok_or(ZooError::UnknownPedestal)?;
        if ped.is_locked(now) {
            return Err(ZooError::AnimalLocked);
        }
        self.pedestals.retain(|p| p.id != id);
        self.unplaced_pedestals += 1;
        Ok(())
    }

    /// Dedicate `animal_id` to pedestal `pedestal_id`. The animal must exist, be
    /// idle, and not already be nested or on another pedestal; the pedestal must
    /// be empty and not on cooldown. Starts the animal's 48h lock.
    pub fn dedicate_animal(
        &mut self,
        pedestal_id: Uuid,
        animal_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), ZooError> {
        match self.animals.get(&animal_id) {
            None => return Err(ZooError::UnknownAnimal),
            Some(a) if !matches!(a.state, AnimalState::Idle) => return Err(ZooError::NotIdle),
            _ => {}
        }
        if self.animal_in_any_nest(animal_id) || self.animal_on_any_pedestal(animal_id) {
            return Err(ZooError::AlreadyNested);
        }
        let ped = self
            .pedestals
            .iter_mut()
            .find(|p| p.id == pedestal_id)
            .ok_or(ZooError::UnknownPedestal)?;
        if ped.animal.is_some() {
            return Err(ZooError::PedestalOccupied);
        }
        if ped.on_cooldown(now) {
            return Err(ZooError::PedestalOnCooldown);
        }
        ped.animal = Some(animal_id);
        ped.dedicated_at = Some(now);
        Ok(())
    }

    /// Release the dedicated animal from pedestal `pedestal_id` and start the
    /// pedestal's 24h cooldown. Refused while the animal is still locked.
    /// Returns the freed animal id.
    pub fn undedicate_animal(
        &mut self,
        pedestal_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Uuid, ZooError> {
        let ped = self
            .pedestals
            .iter_mut()
            .find(|p| p.id == pedestal_id)
            .ok_or(ZooError::UnknownPedestal)?;
        if ped.is_locked(now) {
            return Err(ZooError::AnimalLocked);
        }
        let id = ped.animal.take().ok_or(ZooError::PedestalEmpty)?;
        ped.dedicated_at = None;
        ped.cooldown_until = Some(now + super::pedestal::pedestal_cooldown());
        Ok(id)
    }

    /// True if `animal_id` is currently dedicated to any pedestal.
    pub fn animal_on_any_pedestal(&self, animal_id: Uuid) -> bool {
        self.pedestals.iter().any(|p| p.animal == Some(animal_id))
    }

    /// True if no pedestal (other than `ignore`) occupies `tile`.
    pub fn pedestal_tile_free(&self, tile: (i32, i32), ignore: Option<Uuid>) -> bool {
        !self
            .pedestals
            .iter()
            .any(|p| p.tile == tile && Some(p.id) != ignore)
    }

    /// Map of `animal_id → pedestal world position` for every dedicated animal,
    /// so the render layer parks those critters on their stand.
    pub fn pedestal_animal_positions(&self) -> HashMap<Uuid, Vec2> {
        let mut out = HashMap::new();
        for p in &self.pedestals {
            if let Some(id) = p.animal {
                let w = self.tile_to_world(p.tile);
                // Sit the critter just above the slab's centre.
                out.insert(id, Vec2::new(w.x, w.y - 10.0));
            }
        }
        out
    }

    /// Sweep every dedicated animal that has filled to its (1×) storage cap into
    /// the wallet, in its own currency. Called on the host's tick so pedestal
    /// income auto-collects.
    ///
    /// Banking uses the **10× offline cap**: online, the tick runs every frame,
    /// so an animal is swept the instant it crosses 1× and never accrues past it;
    /// offline, no tick runs, so it banks up to 10× on the first tick back. We
    /// inline the credit here (rather than calling `collect_animal`, which would
    /// re-clamp to 1× and underpay), gating on the 1× fill point so short gaps
    /// still collect.
    pub fn collect_pedestals(&mut self, now: DateTime<Utc>) -> CollectResult {
        let ids: Vec<Uuid> = self.pedestals.iter().filter_map(|p| p.animal).collect();
        let mut result = CollectResult::default();
        for id in ids {
            let Some(a) = self.animals.get_mut(&id) else { continue };
            if !a.is_at_cap(now) {
                continue;
            }
            let gained = a.stored_at_with_cap(now, PEDESTAL_OFFLINE_CAP_MULT);
            a.last_collected_at = now;
            match species::get(a.species).income_kind {
                species::IncomeKind::Coin => result.coins = result.coins.saturating_add(gained),
                species::IncomeKind::DnaHelix => result.dna = result.dna.saturating_add(gained),
            }
        }
        self.coins = self.coins.saturating_add(result.coins);
        self.dna_helix = self.dna_helix.saturating_add(result.dna);
        result
    }

    /// Possible offspring of nest `nest_id`, as `(species, percent, discovered)`
    /// sorted by descending chance. `discovered` is true when the outcome is a
    /// parent species or an already-unlocked recipe — the UI renders `????` for
    /// the rest. Empty when the nest doesn't hold two crossable animals.
    pub fn nest_outcomes(&self, nest_id: Uuid) -> Vec<(SpeciesId, u32, bool)> {
        let Some(nest) = self.nests.iter().find(|n| n.id == nest_id) else {
            return Vec::new();
        };
        let occ = nest.occupants();
        if occ.len() < 2 {
            return Vec::new();
        }
        let (Some(sa), Some(sb)) = (
            self.animals.get(&occ[0]).map(|a| a.species),
            self.animals.get(&occ[1]).map(|a| a.species),
        ) else {
            return Vec::new();
        };
        let Some(pool) = species::crossbreed_pool(sa, sb) else {
            return Vec::new();
        };
        let total: u32 = pool.iter().map(|e| e.weight).sum();
        if total == 0 {
            return Vec::new();
        }
        let mut out: Vec<(SpeciesId, u32, bool)> = pool
            .iter()
            .map(|e| {
                let pct = (e.weight as u64 * 100 / total as u64) as u32;
                let is_parent = e.species == sa || e.species == sb;
                let discovered = is_parent || self.discovered_recipes.contains(&e.species);
                (e.species, pct, discovered)
            })
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out
    }

    /// After loading, ensure every in-progress breeding pair lives in a nest
    /// (older saves bred via the menu, so pairs may not be assigned yet). Also
    /// drops nest occupant ids whose animal no longer exists.
    pub fn relink_breeding_nests(&mut self) {
        // Prune stale occupant ids.
        let known: HashSet<Uuid> = self.animals.keys().copied().collect();
        for nest in &mut self.nests {
            for slot in nest.slots.iter_mut() {
                if let Some(id) = slot {
                    if !known.contains(id) {
                        *slot = None;
                    }
                }
            }
        }
        // Collect breeding pairs (canonical, deduped) not already nested.
        let mut seen: HashSet<Uuid> = HashSet::new();
        let mut pairs: Vec<(Uuid, Uuid)> = Vec::new();
        for a in self.animals.values() {
            if let AnimalState::Breeding { partner_id, .. } = a.state {
                if seen.contains(&a.id) || seen.contains(&partner_id) {
                    continue;
                }
                seen.insert(a.id);
                seen.insert(partner_id);
                let already = self.nests.iter().any(|n| n.contains(a.id) || n.contains(partner_id));
                if !already {
                    pairs.push((a.id, partner_id));
                }
            }
        }
        // Drop each unassigned pair into an empty nest.
        for (a, b) in pairs {
            if let Some(nest) = self.nests.iter_mut().find(|n| n.occupants().is_empty()) {
                nest.slots = [Some(a), Some(b)];
            }
        }
    }

    /// Cancel an active breeding for the pair containing `animal_id`. Both
    /// partners flip back to `Idle`; no offspring spawns and the codex is
    /// **not** updated — the player explicitly forfeited the outcome.
    pub fn cancel_breeding(
        &mut self,
        animal_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), ZooError> {
        let partner_id = match self.animals.get(&animal_id) {
            Some(a) => match a.state {
                AnimalState::Breeding { partner_id, .. } => partner_id,
                _ => return Err(ZooError::NotBreeding),
            },
            None => return Err(ZooError::UnknownAnimal),
        };
        for x in [animal_id, partner_id] {
            if let Some(a) = self.animals.get_mut(&x) {
                a.state = AnimalState::Idle;
                a.last_collected_at = now;
            }
        }
        Ok(())
    }

    pub fn start_breeding(
        &mut self,
        a_id: Uuid,
        b_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, ZooError> {
        if a_id == b_id {
            return Err(ZooError::SameAnimal);
        }
        let (species_a, idle_a) = {
            let a = self.animals.get(&a_id).ok_or(ZooError::UnknownAnimal)?;
            (a.species, matches!(a.state, AnimalState::Idle))
        };
        let (species_b, idle_b) = {
            let b = self.animals.get(&b_id).ok_or(ZooError::UnknownAnimal)?;
            (b.species, matches!(b.state, AnimalState::Idle))
        };
        if !idle_a || !idle_b {
            return Err(ZooError::NotIdle);
        }
        // Same-species pairs are no longer breedable — breeding is now
        // exclusively the hybrid-gamble mechanic. Each cross pair has a
        // weighted pool that includes both parents (high weight) plus the
        // hybrid (low weight); the roll happens at `claim_completed_breeding`.
        if species_a == species_b {
            return Err(ZooError::SameSpecies);
        }
        if species::crossbreed_pool(species_a, species_b).is_none() {
            return Err(ZooError::SpeciesMismatch);
        }
        // Gestation is ~1.5× the slower parent — long enough to feel risky.
        let slower = species::get(species_a)
            .gestation_seconds
            .max(species::get(species_b).gestation_seconds);
        let gestation = ((slower as f64) * 1.5) as u64;
        // Final preflight: every other rejection above is per-pair, but nest
        // capacity is a global resource — check it last so it never masks a
        // more specific error.
        if self.active_breeding_pair_count() >= self.nest_count as usize {
            return Err(ZooError::AllNestsBusy);
        }
        let ends_at = now + Duration::seconds(gestation as i64);

        // Sweep pending coins so the pause doesn't lose them.
        for id in [a_id, b_id] {
            let pending = if let Some(a) = self.animals.get_mut(&id) {
                let p = a.stored_at(now);
                a.last_collected_at = now;
                p
            } else {
                0
            };
            self.coins = self.coins.saturating_add(pending);
        }
        if let Some(a) = self.animals.get_mut(&a_id) {
            a.state = AnimalState::Breeding {
                partner_id: b_id,
                ends_at,
            };
        }
        if let Some(b) = self.animals.get_mut(&b_id) {
            b.state = AnimalState::Breeding {
                partner_id: a_id,
                ends_at,
            };
        }
        Ok(ends_at)
    }

    /// Redeem a completed gestation: roll the pool, transition both parents
    /// back to `Idle`, and place a fresh L1 offspring into a compatible
    /// habitat. Returns `(habitat_id, offspring_animal_id, ClaimOutcome)`.
    ///
    /// The roll is deterministic from the pair's UUIDs + gestation `ends_at`,
    /// so a reload-on-tick can't reroll the result. Outcomes:
    /// - **Parent drop** (most common): one of the parents is produced. No
    ///   DNA reward, no codex entry — this isn't a discovery.
    /// - **Hybrid drop** (rare): the cross-only offspring species. Awards
    ///   +1 DNA Helix and adds the hybrid to the codex.
    ///
    /// Errors and the resulting state:
    /// - `UnknownAnimal` if `animal_id` is not in the zoo.
    /// - `NotBreeding` if it's not in `Breeding` state.
    /// - `NotReady` if `ends_at > now` — the timer is still running.
    /// - `NoHabitatWithSpace` if no compatible habitat slot exists. The pair
    ///   stays in `Breeding` (still "Ready" in the UI) so the player can free
    ///   space and click the nest again.
    pub fn claim_completed_breeding(
        &mut self,
        animal_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<ClaimedBreeding, ZooError> {
        let (partner_id, species_a, ends_at) = match self.animals.get(&animal_id) {
            Some(a) => match a.state {
                AnimalState::Breeding { partner_id, ends_at } => (partner_id, a.species, ends_at),
                _ => return Err(ZooError::NotBreeding),
            },
            None => return Err(ZooError::UnknownAnimal),
        };
        if ends_at > now {
            return Err(ZooError::NotReady);
        }
        let species_b = self
            .animals
            .get(&partner_id)
            .ok_or(ZooError::UnknownAnimal)?
            .species;

        // Roll the pool. `start_breeding` rejected any pair without a pool,
        // but for belt-and-suspenders fall back to species_a if absent.
        let pool = species::crossbreed_pool(species_a, species_b);
        let offspring_species = match pool {
            Some(pool) if !pool.is_empty() => {
                let seed = species::pool_seed(animal_id, partner_id, ends_at.timestamp());
                species::roll_pool(seed, pool)
            }
            _ => species_a,
        };
        let is_hybrid_drop =
            offspring_species != species_a && offspring_species != species_b;

        // Place the offspring. Prefer a compatible habitat (legacy model); in
        // the freeform world there are no habitats, so fall back to spawning
        // the critter onto the open plane (`habitat_id` is then nil). Other
        // errors (e.g. unknown species) still propagate.
        let placed = match self.auto_place_animal(offspring_species, 1, now) {
            Ok(p) => p,
            Err(ZooError::NoHabitatWithSpace) => {
                let id = self.spawn_animal_freeform(offspring_species, 1, now)?;
                (Uuid::nil(), id)
            }
            Err(e) => return Err(e),
        };

        // Reward + codex are recorded only after a successful place — the
        // hybrid drop is the player's actual "win".
        if is_hybrid_drop {
            self.dna_helix = self.dna_helix.saturating_add(1);
            self.discovered_recipes.insert(offspring_species);
        }
        for x in [animal_id, partner_id] {
            if let Some(a) = self.animals.get_mut(&x) {
                a.state = AnimalState::Idle;
                a.last_collected_at = now;
            }
        }
        Ok(ClaimedBreeding {
            habitat_id: placed.0,
            animal_id: placed.1,
            offspring_species,
            is_hybrid_drop,
        })
    }

    pub fn count_habitats_with_theme(&self, theme: HabitatTheme) -> usize {
        self.habitats.iter().filter(|h| h.theme == theme).count()
    }

    /// Whether `theme`'s footprint anchored at `tile` is within grid bounds and
    /// free of any existing habitat (optionally ignoring one habitat id, used
    /// when relocating an existing habitat onto tiles it already covers).
    pub fn placement_ok(
        &self,
        theme: HabitatTheme,
        tile: (i32, i32),
        ignore_id: Option<Uuid>,
    ) -> Result<(), ZooError> {
        if !Habitat::footprint_in_bounds(theme, tile) {
            return Err(ZooError::OutOfBounds);
        }
        let collides = self
            .habitats
            .iter()
            .filter(|h| Some(h.id) != ignore_id)
            .any(|h| footprints_overlap(theme, tile, h.theme, h.tile));
        if collides {
            return Err(ZooError::TileOccupied);
        }
        Ok(())
    }

    /// Buy the (single) habitat of a theme and place it at `tile`. Fails with
    /// `HabitatAlreadyExists` if the player already owns one of that theme —
    /// single-habitat-per-theme is enforced; capacity grows through
    /// `start_habitat_upgrade` instead of buying duplicates. Placement is
    /// validated (bounds + collision) before any coins are spent.
    pub fn buy_habitat(&mut self, theme: HabitatTheme, tile: (i32, i32)) -> Result<Uuid, ZooError> {
        if self.count_habitats_with_theme(theme) > 0 {
            return Err(ZooError::HabitatAlreadyExists);
        }
        self.placement_ok(theme, tile, None)?;
        let cost = habitat_purchase_cost();
        if self.coins < cost {
            return Err(ZooError::NotEnoughCoins);
        }
        self.coins -= cost;
        let h = Habitat::new_at(theme, tile);
        let id = h.id;
        self.habitats.push(h);
        Ok(id)
    }

    /// Relocate an existing habitat to a new anchor tile, collision-checked
    /// against the other habitats (and grid bounds). Used by click-to-place.
    pub fn move_habitat(&mut self, habitat_id: Uuid, new_tile: (i32, i32)) -> Result<(), ZooError> {
        let theme = self
            .habitats
            .iter()
            .find(|h| h.id == habitat_id)
            .map(|h| h.theme)
            .ok_or(ZooError::UnknownHabitat)?;
        self.placement_ok(theme, new_tile, Some(habitat_id))?;
        if let Some(h) = self.habitats.iter_mut().find(|h| h.id == habitat_id) {
            h.tile = new_tile;
        }
        Ok(())
    }

    /// Unlock the next physical food structure, paying coins per
    /// [`food_structure_unlock_cost`]. Capped at [`MAX_FOOD_STRUCTURES`]. Mirrors
    /// [`Zoo::buy_nest`]. Returns the new owned count.
    pub fn buy_food_structure(&mut self, now: DateTime<Utc>) -> Result<usize, ZooError> {
        let cost = food_structure_unlock_cost(self.structures.len())
            .ok_or(ZooError::StructureCapReached)?;
        if self.coins < cost {
            return Err(ZooError::NotEnoughCoins);
        }
        self.coins -= cost;
        self.structures.push(Structure::new(FOOD_KIND, now));
        Ok(self.structures.len())
    }

    /// Sweep one structure's accrued food into the bank. Returns food gained.
    pub fn collect_food_structure(&mut self, structure_id: Uuid, now: DateTime<Utc>) -> u64 {
        let Some(s) = self.structures.iter_mut().find(|s| s.id == structure_id) else {
            return 0;
        };
        let g = s.stored_at(now);
        s.last_collected_at = now;
        self.food = self.food.saturating_add(g);
        g
    }

    // ── One-of-each ownership + Rank ────────────────────────────────────────

    /// The id of the single owned animal of `species`, if any.
    pub fn animal_id_for_species(&self, species: SpeciesId) -> Option<Uuid> {
        self.animals
            .iter()
            .find(|(_, a)| a.species == species)
            .map(|(id, _)| *id)
    }

    /// Whether `species` is currently owned.
    pub fn owns_species(&self, species: SpeciesId) -> bool {
        self.animal_id_for_species(species).is_some()
    }

    /// Current Rank stage of `species` (derived from lifetime duplicates),
    /// regardless of whether it is currently owned.
    pub fn rank_of(&self, species: SpeciesId) -> u8 {
        rank::rank_for_dupes(self.species_dupes.get(species).copied().unwrap_or(0))
    }

    /// Habitat containing `animal_id`, if it lives in one.
    fn habitat_id_of(&self, animal_id: Uuid) -> Option<Uuid> {
        self.habitats
            .iter()
            .find(|h| h.animal_ids.contains(&animal_id))
            .map(|h| h.id)
    }

    /// Record acquiring a duplicate of an already-owned species: bump the
    /// lifetime count and refresh the live animal's Rank. Returns whether the
    /// Rank advanced this time.
    fn register_duplicate(&mut self, species: SpeciesId) -> bool {
        let count = self.species_dupes.entry(species).or_insert(0);
        *count += 1;
        let new_stage = rank::rank_for_dupes(*count);
        if let Some(id) = self.animal_id_for_species(species) {
            if let Some(a) = self.animals.get_mut(&id) {
                let advanced = a.stage != new_stage;
                a.stage = new_stage;
                return advanced;
            }
        }
        false
    }

    /// Max number of animals this zoo can currently hold (grows with expansion).
    pub fn max_animal_capacity(&self) -> usize {
        zoo_animal_capacity(self.zoo_level)
    }

    /// True when the zoo is full and can't take a *new* species. Acquiring a
    /// duplicate of an already-owned species never needs capacity (it just
    /// advances Rank), so callers only consult this for brand-new animals.
    pub fn at_animal_capacity(&self) -> bool {
        self.animals.len() >= self.max_animal_capacity()
    }

    /// Place a new animal (no cost). Used by gift claims and internally by
    /// `buy_animal`. If the species is already owned, this advances its Rank
    /// instead of adding a second copy (one-of-each). Returns
    /// (habitat_id, animal_id) — for a duplicate, the existing animal's ids.
    pub fn auto_place_animal(
        &mut self,
        species_id: SpeciesId,
        level: u8,
        now: DateTime<Utc>,
    ) -> Result<(Uuid, Uuid), ZooError> {
        let def = species::try_get(species_id).ok_or(ZooError::UnknownSpecies)?;
        if let Some(existing) = self.animal_id_for_species(def.id) {
            self.register_duplicate(def.id);
            let hid = self.habitat_id_of(existing).unwrap_or_else(Uuid::nil);
            return Ok((hid, existing));
        }
        // A brand-new species counts against the zoo-wide animal capacity.
        if self.at_animal_capacity() {
            return Err(ZooError::ZooAtCapacity);
        }
        let target_idx = self
            .habitats
            .iter()
            .position(|h| h.theme == def.theme && h.animal_ids.len() < h.capacity())
            .ok_or(ZooError::NoHabitatWithSpace)?;
        let habitat_id = self.habitats[target_idx].id;
        let mut animal = Animal::new(def.id, now);
        animal.level = level.clamp(1, MAX_ANIMAL_LEVEL);
        animal.stage = self.rank_of(def.id);
        let animal_id = animal.id;
        self.habitats[target_idx].animal_ids.push(animal_id);
        self.animals.insert(animal_id, animal);
        Ok((habitat_id, animal_id))
    }

    /// Charge coins and auto-place. Validates affordability and space before
    /// mutating. Buying a species you already own advances its Rank (and needs
    /// no habitat space).
    pub fn buy_animal(
        &mut self,
        species_id: SpeciesId,
        now: DateTime<Utc>,
    ) -> Result<(Uuid, Uuid), ZooError> {
        let def = species::try_get(species_id).ok_or(ZooError::UnknownSpecies)?;
        let owned = self.owns_species(def.id);
        // Validate affordability in the species' purchase currency before
        // touching anything.
        match def.purchase_currency {
            species::IncomeKind::Coin => {
                if self.coins < def.purchase_cost {
                    return Err(ZooError::NotEnoughCoins);
                }
            }
            species::IncomeKind::DnaHelix => {
                if self.dna_helix < def.purchase_cost {
                    return Err(ZooError::NotEnoughDna);
                }
            }
        }
        // Only a brand-new animal needs habitat space.
        if !owned {
            let has_room = self
                .habitats
                .iter()
                .any(|h| h.theme == def.theme && h.animal_ids.len() < h.capacity());
            if !has_room {
                return Err(ZooError::NoHabitatWithSpace);
            }
        }
        match def.purchase_currency {
            species::IncomeKind::Coin => self.coins -= def.purchase_cost,
            species::IncomeKind::DnaHelix => self.dna_helix -= def.purchase_cost,
        }
        self.auto_place_animal(def.id, 1, now)
    }

    /// Spawn an animal directly into the world with no habitat — the freeform
    /// model where critters roam the open plane. Returns the animal id. If the
    /// species is already owned, advances its Rank instead of duplicating.
    pub fn spawn_animal_freeform(
        &mut self,
        species_id: SpeciesId,
        level: u8,
        now: DateTime<Utc>,
    ) -> Result<Uuid, ZooError> {
        let def = species::try_get(species_id).ok_or(ZooError::UnknownSpecies)?;
        if let Some(existing) = self.animal_id_for_species(def.id) {
            self.register_duplicate(def.id);
            return Ok(existing);
        }
        // A brand-new species counts against the zoo-wide animal capacity.
        if self.at_animal_capacity() {
            return Err(ZooError::ZooAtCapacity);
        }
        let mut animal = Animal::new(def.id, now);
        animal.level = level.clamp(1, MAX_ANIMAL_LEVEL);
        animal.stage = self.rank_of(def.id);
        let animal_id = animal.id;
        self.animals.insert(animal_id, animal);
        Ok(animal_id)
    }

    /// Freeform shop buy: validate affordability in the species' purchase
    /// currency, charge, and spawn the critter onto the plane (no habitat
    /// required). Returns the animal id (advancing Rank if already owned).
    pub fn purchase_animal(
        &mut self,
        species_id: SpeciesId,
        now: DateTime<Utc>,
    ) -> Result<Uuid, ZooError> {
        let def = species::try_get(species_id).ok_or(ZooError::UnknownSpecies)?;
        match def.purchase_currency {
            species::IncomeKind::Coin => {
                if self.coins < def.purchase_cost {
                    return Err(ZooError::NotEnoughCoins);
                }
            }
            species::IncomeKind::DnaHelix => {
                if self.dna_helix < def.purchase_cost {
                    return Err(ZooError::NotEnoughDna);
                }
            }
        }
        match def.purchase_currency {
            species::IncomeKind::Coin => self.coins -= def.purchase_cost,
            species::IncomeKind::DnaHelix => self.dna_helix -= def.purchase_cost,
        }
        self.spawn_animal_freeform(def.id, 1, now)
    }

    /// Sell an owned animal for coins (inspect-panel Sell action). Removes it
    /// from the zoo and its habitat. Rank (`species_dupes`) is **retained** so it
    /// survives a later recapture. Refused while breeding or nested. Returns
    /// coins gained. Feeding/levelling reuses the existing [`Zoo::level_up_animal`].
    pub fn sell_animal(&mut self, animal_id: Uuid, now: DateTime<Utc>) -> Result<u64, ZooError> {
        let (level, base, breeding) = {
            let a = self.animals.get(&animal_id).ok_or(ZooError::UnknownAnimal)?;
            (
                a.level,
                species::get(a.species).purchase_cost,
                matches!(a.state, AnimalState::Breeding { .. }),
            )
        };
        if breeding {
            return Err(ZooError::NotIdle);
        }
        if self.animal_in_any_nest(animal_id) {
            return Err(ZooError::AlreadyNested);
        }
        // A pedestal animal can't be sold while it's still locked.
        if self
            .pedestals
            .iter()
            .any(|p| p.animal == Some(animal_id) && p.is_locked(now))
        {
            return Err(ZooError::AnimalLocked);
        }
        // Sweep any pending at-cap income first so it isn't silently lost.
        let pending = self.animals.get(&animal_id).map_or(0, |a| a.stored_at(now));
        let value = animal_sell_value(base, level).saturating_add(pending);
        for h in self.habitats.iter_mut() {
            h.animal_ids.retain(|aid| *aid != animal_id);
        }
        // Selling a dedicated animal vacates its pedestal (past the lock by the
        // check above) and starts that pedestal's cooldown.
        for p in self.pedestals.iter_mut() {
            if p.animal == Some(animal_id) {
                p.animal = None;
                p.dedicated_at = None;
                p.cooldown_until = Some(now + super::pedestal::pedestal_cooldown());
            }
        }
        self.animals.remove(&animal_id);
        self.coins = self.coins.saturating_add(value);
        Ok(value)
    }

    /// Sweep at-cap income from every animal in a habitat into the
    /// appropriate currency bank. Animals that haven't reached their
    /// `storage_cap` are **skipped** — the player must wait until the
    /// storage bar is full before collecting. This is the at-cap-only
    /// collection rule.
    pub fn collect_habitat(&mut self, habitat_id: Uuid, now: DateTime<Utc>) -> CollectResult {
        let ids: Vec<Uuid> = match self.habitats.iter().find(|h| h.id == habitat_id) {
            Some(h) => h.animal_ids.clone(),
            None => return CollectResult::default(),
        };
        let mut result = CollectResult::default();
        for id in ids {
            let Some(a) = self.animals.get_mut(&id) else { continue };
            if !a.is_at_cap(now) {
                continue;
            }
            let gained = a.stored_at(now);
            a.last_collected_at = now;
            let kind = species::get(a.species).income_kind;
            match kind {
                species::IncomeKind::Coin => {
                    result.coins = result.coins.saturating_add(gained);
                }
                species::IncomeKind::DnaHelix => {
                    result.dna = result.dna.saturating_add(gained);
                }
            }
        }
        self.coins = self.coins.saturating_add(result.coins);
        self.dna_helix = self.dna_helix.saturating_add(result.dna);
        result
    }

    /// Sweep a single animal's accrued income into the right currency bank,
    /// **only** if the animal is at its storage cap. Used by the per-icon
    /// collect-on-click in the habitat grid. Returns the amounts collected;
    /// both zero when the animal is below cap, gone, or busy breeding.
    pub fn collect_animal(&mut self, animal_id: Uuid, now: DateTime<Utc>) -> CollectResult {
        let mut result = CollectResult::default();
        let Some(a) = self.animals.get_mut(&animal_id) else { return result };
        if !a.is_at_cap(now) {
            return result;
        }
        let gained = a.stored_at(now);
        a.last_collected_at = now;
        let kind = species::get(a.species).income_kind;
        match kind {
            species::IncomeKind::Coin => {
                result.coins = gained;
                self.coins = self.coins.saturating_add(gained);
            }
            species::IncomeKind::DnaHelix => {
                result.dna = gained;
                self.dna_helix = self.dna_helix.saturating_add(gained);
            }
        }
        result
    }

    pub fn collect_all_structures(&mut self, now: DateTime<Utc>) -> u64 {
        let mut total = 0u64;
        for s in self.structures.iter_mut() {
            let g = s.stored_at(now);
            s.last_collected_at = now;
            total = total.saturating_add(g);
        }
        self.food = self.food.saturating_add(total);
        total
    }

    /// Start a habitat size upgrade: charges coins immediately and sets
    /// `upgrade_finishes_at`. Returns the instant the upgrade will be
    /// ready to redeem. The level only actually increments on
    /// `claim_habitat_upgrade`.
    pub fn start_habitat_upgrade(
        &mut self,
        habitat_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, ZooError> {
        let idx = self
            .habitats
            .iter()
            .position(|h| h.id == habitat_id)
            .ok_or(ZooError::UnknownHabitat)?;
        if self.habitats[idx].upgrade_finishes_at.is_some() {
            return Err(ZooError::UpgradeInProgress);
        }
        let current = self.habitats[idx].level;
        if current >= MAX_HABITAT_LEVEL {
            return Err(ZooError::MaxLevel);
        }
        let cost = habitat_upgrade_cost(current);
        if self.coins < cost {
            return Err(ZooError::NotEnoughCoins);
        }
        self.coins -= cost;
        let ends_at = now + habitat_upgrade_duration(current);
        self.habitats[idx].upgrade_finishes_at = Some(ends_at);
        Ok(ends_at)
    }

    /// Apply a completed habitat upgrade — increments level and clears the
    /// timer. Errors `UpgradeNotReady` if the timer is still running and
    /// `NotBreeding`-equivalent `UnknownHabitat` if the id is bogus.
    pub fn claim_habitat_upgrade(
        &mut self,
        habitat_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<u8, ZooError> {
        let idx = self
            .habitats
            .iter()
            .position(|h| h.id == habitat_id)
            .ok_or(ZooError::UnknownHabitat)?;
        let Some(ends_at) = self.habitats[idx].upgrade_finishes_at else {
            return Err(ZooError::UpgradeNotReady);
        };
        if ends_at > now {
            return Err(ZooError::UpgradeNotReady);
        }
        self.habitats[idx].level += 1;
        self.habitats[idx].upgrade_finishes_at = None;
        Ok(self.habitats[idx].level)
    }

    /// Coins to expand the zoo from its current level, or `None` at max size.
    pub fn zoo_upgrade_cost(&self) -> Option<u64> {
        zoo_upgrade_cost(self.zoo_level)
    }

    /// True while a zoo expansion is being built.
    pub fn zoo_upgrade_in_progress(&self) -> bool {
        self.zoo_upgrade_finishes_at.is_some()
    }

    /// Start a zoo expansion: charges coins immediately and sets the build
    /// timer. Returns the instant it will be ready to claim. The plot and
    /// capacity only actually grow on [`claim_zoo_upgrade`]. Mirrors the
    /// habitat two-phase upgrade flow.
    pub fn start_zoo_upgrade(&mut self, now: DateTime<Utc>) -> Result<DateTime<Utc>, ZooError> {
        if self.zoo_upgrade_finishes_at.is_some() {
            return Err(ZooError::ZooUpgradeInProgress);
        }
        let cost = zoo_upgrade_cost(self.zoo_level).ok_or(ZooError::ZooMaxSize)?;
        let dur = zoo_upgrade_duration(self.zoo_level).ok_or(ZooError::ZooMaxSize)?;
        if self.coins < cost {
            return Err(ZooError::NotEnoughCoins);
        }
        self.coins -= cost;
        let ends_at = now + dur;
        self.zoo_upgrade_finishes_at = Some(ends_at);
        Ok(ends_at)
    }

    /// Apply a finished zoo expansion: bumps the level (growing both the plot
    /// size and the animal capacity), syncs the global plot geometry, and clears
    /// the timer. Errors `ZooUpgradeNotReady` if no build is queued or the timer
    /// is still running.
    pub fn claim_zoo_upgrade(&mut self, now: DateTime<Utc>) -> Result<u8, ZooError> {
        let Some(ends_at) = self.zoo_upgrade_finishes_at else {
            return Err(ZooError::ZooUpgradeNotReady);
        };
        if ends_at > now {
            return Err(ZooError::ZooUpgradeNotReady);
        }
        self.zoo_level = (self.zoo_level + 1).min(MAX_ZOO_LEVEL);
        self.zoo_upgrade_finishes_at = None;
        Ok(self.zoo_level)
    }

    pub fn upgrade_structure(
        &mut self,
        structure_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<u8, ZooError> {
        let idx = self
            .structures
            .iter()
            .position(|s| s.id == structure_id)
            .ok_or(ZooError::UnknownStructure)?;
        let current = self.structures[idx].level;
        if current >= MAX_STRUCTURE_LEVEL {
            return Err(ZooError::MaxLevel);
        }
        let cost = structure_upgrade_cost(current);
        if self.coins < cost {
            return Err(ZooError::NotEnoughCoins);
        }
        // Collect-then-grow so the new cap doesn't retro-apply.
        let pending = self.structures[idx].stored_at(now);
        self.food = self.food.saturating_add(pending);
        self.coins -= cost;
        let s = &mut self.structures[idx];
        s.last_collected_at = now;
        s.level = current + 1;
        Ok(s.level)
    }

    /// Remove an idle animal from this zoo and produce a sealed `GiftPayload`
    /// the recipient can paste in via `claim_gift`. The animal is dropped
    /// locally on success — gifting transfers ownership, no duplication.
    /// Refuses to gift an animal that's mid-breeding.
    pub fn send_animal_gift(
        &mut self,
        animal_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<GiftPayload, ZooError> {
        let a = self.animals.get(&animal_id).ok_or(ZooError::UnknownAnimal)?;
        if !matches!(a.state, AnimalState::Idle) {
            return Err(ZooError::NotIdle);
        }
        // Snapshot before removal so the payload is self-contained.
        let species_id = a.species;
        let level = a.level;
        // Sweep pending coins so the player isn't surprised by the loss.
        let pending = a.stored_at(now);
        self.coins = self.coins.saturating_add(pending);
        // Remove from owning habitat + animal map.
        for h in self.habitats.iter_mut() {
            h.animal_ids.retain(|id| *id != animal_id);
        }
        self.animals.remove(&animal_id);

        Ok(GiftPayload {
            version: 1,
            gift_id: crate::game::ids::new_id(),
            sender_id: self.player.id,
            sender_name: self.player.name.clone(),
            created_at: now,
            contents: GiftContents::Animal {
                species_id: species_id.to_string(),
                level,
            },
        })
    }

    /// Build a compact, read-only view of this zoo for the snapshot share code.
    pub fn build_shared_snapshot(&self, now: DateTime<Utc>) -> SharedSnapshotPayload {
        let mut tally: HashMap<&'static str, (usize, u32)> = HashMap::new();
        for a in self.animals.values() {
            let entry = tally.entry(a.species).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += a.level as u32;
        }
        let mut species_tally: Vec<SpeciesTallyEntry> = tally
            .into_iter()
            .map(|(sp, (count, total_level))| SpeciesTallyEntry {
                species_id: sp.to_string(),
                count,
                total_level,
            })
            .collect();
        species_tally.sort_by(|a, b| a.species_id.cmp(&b.species_id));

        SharedSnapshotPayload {
            version: 1,
            sender_id: self.player.id,
            sender_name: self.player.name.clone(),
            taken_at: now,
            view: SnapshotView {
                coins: self.coins,
                food: self.food,
                habitat_count: self.habitats.len(),
                structure_count: self.structures.len(),
                animal_count: self.animals.len(),
                species_tally,
            },
        }
    }

    /// Apply an incoming gift. Idempotent: a second claim with the same `gift_id` is rejected.
    pub fn claim_gift(
        &mut self,
        gift: &GiftPayload,
        now: DateTime<Utc>,
    ) -> Result<(Uuid, Uuid), ZooError> {
        if self.claimed_gifts.contains(&gift.gift_id) {
            return Err(ZooError::AlreadyClaimed);
        }
        let (habitat_id, animal_id) = match &gift.contents {
            GiftContents::Animal { species_id, level } => {
                let def = species::try_get(species_id).ok_or(ZooError::UnknownSpecies)?;
                self.auto_place_animal(def.id, *level, now)?
            }
        };
        self.claimed_gifts.insert(gift.gift_id);
        Ok((habitat_id, animal_id))
    }

    pub fn level_up_animal(
        &mut self,
        animal_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<u8, ZooError> {
        // A dedicated pedestal animal produces income only — it can't be fed.
        if self.animal_on_any_pedestal(animal_id) {
            return Err(ZooError::AnimalLocked);
        }
        let a = self.animals.get(&animal_id).ok_or(ZooError::UnknownAnimal)?;
        if a.level >= MAX_ANIMAL_LEVEL {
            return Err(ZooError::MaxLevel);
        }
        let def = species::try_get(a.species).ok_or(ZooError::UnknownSpecies)?;
        let cost = animal_level_up_cost(def.purchase_cost, a.level);
        if self.food < cost {
            return Err(ZooError::NotEnoughFood);
        }
        let pending_coins = a.stored_at(now);
        self.coins = self.coins.saturating_add(pending_coins);
        self.food = self.food.saturating_sub(cost);
        let a = self.animals.get_mut(&animal_id).unwrap();
        a.last_collected_at = now;
        a.level += 1;
        Ok(a.level)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ZooError {
    UnknownSpecies,
    UnknownHabitat,
    UnknownAnimal,
    UnknownStructure,
    #[allow(dead_code)]
    UnknownStructureKind,
    HabitatCapReached,
    StructureCapReached,
    NoHabitatWithSpace,
    NotEnoughCoins,
    NotEnoughFood,
    NotEnoughDna,
    MaxLevel,
    SameAnimal,
    /// New in the pool design: pairing two animals of the same species is
    /// no longer breedable. Breeding is exclusively a cross-species hybrid
    /// gamble now — get a coin trickle or food the conventional way.
    SameSpecies,
    NotIdle,
    NotBreeding,
    /// `claim_completed_breeding` called while `ends_at` is still in the future.
    NotReady,
    SpeciesMismatch,
    /// Tried to buy a second habitat of a theme that already exists.
    HabitatAlreadyExists,
    /// Placement footprint extends outside the grid bounds.
    OutOfBounds,
    /// Placement footprint overlaps an existing habitat.
    TileOccupied,
    /// Tried to start an upgrade on a habitat whose upgrade is already in flight.
    UpgradeInProgress,
    /// `claim_habitat_upgrade` called while the upgrade timer is still running.
    UpgradeNotReady,
    AllNestsBusy,
    NestCapReached,
    /// Tried to deposit into a nest that has no free slot.
    NestFull,
    /// Referenced a nest id that doesn't exist.
    UnknownNest,
    /// Tried to remove an occupant that is mid-breed.
    OccupantBreeding,
    /// Tried to deposit an animal that is already in a nest.
    AlreadyNested,
    /// Tried to pay to skip the exotic-shop wait while it's already open.
    ExoticShopOpen,
    /// Tried to acquire a new species while the zoo is at its animal capacity.
    ZooAtCapacity,
    /// Tried to expand the zoo while an expansion is already being built.
    ZooUpgradeInProgress,
    /// `claim_zoo_upgrade` called with no build queued or while it's still running.
    ZooUpgradeNotReady,
    /// Tried to expand a zoo that's already at the maximum plot size.
    ZooMaxSize,
    /// Referenced a pedestal id that doesn't exist.
    UnknownPedestal,
    /// All pedestals are already placed.
    PedestalCapReached,
    /// Tried to dedicate an animal to a pedestal that already holds one.
    PedestalOccupied,
    /// Tried to remove a dedicated animal from an empty pedestal, or place a
    /// pedestal with none left in the hotbar inventory.
    PedestalEmpty,
    /// Tried to release/sell an animal still inside its 48h pedestal lock.
    AnimalLocked,
    /// Tried to dedicate to a pedestal still inside its post-release cooldown.
    PedestalOnCooldown,
    #[allow(dead_code)]
    AlreadyClaimed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share::{GiftContents, GiftPayload};
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 28, 9, 0, 0).unwrap()
    }

    /// Unlock one nest for free (nests start locked) so breeding tests that
    /// aren't about the nest economy can drive `start_breeding` directly.
    fn grant_nest(zoo: &mut Zoo) {
        zoo.nests.push(Nest::new());
        zoo.nest_count += 1;
    }

    // ── Plot tile grid ────────────────────────────────────────────────────────

    /// Tile (0,0) is the plot centre and snapping a tile's world centre back to
    /// a tile round-trips exactly.
    #[test]
    fn world_to_tile_round_trips_tile_centres() {
        let zoo = Zoo::new(ts());
        assert_eq!(zoo.tile_to_world((0, 0)), zoo.plot_origin);
        for tile in [(0, 0), (3, -2), (-4, 4), (1, -3)] {
            assert_eq!(zoo.world_to_tile(zoo.tile_to_world(tile)), tile);
        }
    }

    /// All durable plot geometry is anchored on `plot_origin`, so moving the
    /// plot shifts every derived position by exactly the same delta — the
    /// invariant a shared hub relies on to place each player's plot.
    #[test]
    fn plot_geometry_rebases_with_origin() {
        let mut zoo = Zoo::new(ts());
        let before_nest = zoo.nest_pos(0);
        let before_food = zoo.food_structure_pos(0);
        let before_tile = zoo.tile_to_world((2, -1));
        let delta = Vec2::new(10_000.0, -7_500.0);
        zoo.plot_origin += delta;
        assert_eq!(zoo.nest_pos(0), before_nest + delta);
        assert_eq!(zoo.food_structure_pos(0), before_food + delta);
        assert_eq!(zoo.tile_to_world((2, -1)), before_tile + delta);
        assert_eq!(zoo.tile_to_world((0, 0)), zoo.plot_origin);
    }

    /// Nests line the top fence row, food structures the bottom; both rows sit
    /// on the plot grid and spread left→right.
    #[test]
    fn nest_and_food_rows_are_on_grid_and_separated() {
        let zoo = Zoo::new(ts()); // level 0
        let inset = zoo.plot_tile_radius() - 1;
        let nests = zoo.nest_tiles();
        let food = zoo.food_structure_tiles();
        assert!(nests.iter().all(|t| t.1 == -inset));
        assert!(food.iter().all(|t| t.1 == inset));
        assert!(nests.iter().all(|t| zoo.tile_in_bounds(*t)));
        assert!(food.iter().all(|t| zoo.tile_in_bounds(*t)));
        assert!(nests.first().unwrap().0 < nests.last().unwrap().0);
        assert!(food.first().unwrap().0 < food.last().unwrap().0);
        // Equidistant: every adjacent gap is identical (no 2-1-2 clustering).
        let gaps: Vec<i32> = nests.windows(2).map(|w| w[1].0 - w[0].0).collect();
        assert!(gaps.iter().all(|g| *g == gaps[0]), "nest spacing not uniform: {gaps:?}");
        let fgaps: Vec<i32> = food.windows(2).map(|w| w[1].0 - w[0].0).collect();
        assert!(fgaps.iter().all(|g| *g == fgaps[0]), "food spacing not uniform: {fgaps:?}");
        // The row is centred on the plot.
        assert_eq!(nests[nests.len() / 2].0, 0);
    }

    #[test]
    fn level_up_animal_requires_food() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 1000;
        let (_, aid) = zoo.buy_animal("field_mouse", now).unwrap();
        // No food → fails.
        let err = zoo.level_up_animal(aid, now).unwrap_err();
        assert!(matches!(err, ZooError::NotEnoughFood));
        // Grant food and retry.
        zoo.food = 200;
        let level = zoo.level_up_animal(aid, now).unwrap();
        assert_eq!(level, 2);
    }

    #[test]
    fn buy_animal_rejects_when_no_compatible_habitat() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        // Default zoo has only a Forest habitat — buying a Penguin (Arctic) fails.
        let err = zoo.buy_animal("penguin", now).unwrap_err();
        assert!(matches!(err, ZooError::NoHabitatWithSpace));
        // Should not have charged.
        assert_eq!(zoo.coins, 100_000);
    }

    #[test]
    fn habitat_buy_rejects_second_of_same_theme() {
        // New rule: at most one habitat per theme. The starter Forest
        // habitat means trying to buy a second Forest immediately fails.
        let mut zoo = Zoo::new(ts());
        zoo.coins = 100_000;
        let err = zoo.buy_habitat(HabitatTheme::Forest, (4, 0)).unwrap_err();
        assert!(matches!(err, ZooError::HabitatAlreadyExists));
        // A different theme is fine.
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        // But buying a second of *that* theme also errors.
        let err = zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap_err();
        assert!(matches!(err, ZooError::HabitatAlreadyExists));
    }

    #[test]
    fn habitat_placement_rejects_collision_and_oob() {
        let mut zoo = Zoo::new(ts());
        zoo.coins = 100_000;
        // Starter Forest occupies (0,0)..(1,1). A Wetland overlapping it fails.
        let err = zoo.buy_habitat(HabitatTheme::Wetland, (1, 1)).unwrap_err();
        assert!(matches!(err, ZooError::TileOccupied));
        // Off-grid placement fails.
        let err = zoo
            .buy_habitat(HabitatTheme::Wetland, (super::super::habitat::GRID_W - 1, 0))
            .unwrap_err();
        assert!(matches!(err, ZooError::OutOfBounds));
        // A clear, in-bounds spot succeeds and is charged.
        let before = zoo.coins;
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 4)).unwrap();
        assert!(zoo.coins < before);
    }

    #[test]
    fn move_habitat_relocates_and_collision_checks() {
        let mut zoo = Zoo::new(ts());
        zoo.coins = 100_000;
        let wid = zoo.buy_habitat(HabitatTheme::Wetland, (4, 4)).unwrap();
        // Move onto a free spot works.
        zoo.move_habitat(wid, (8, 8)).unwrap();
        assert_eq!(
            zoo.habitats.iter().find(|h| h.id == wid).unwrap().tile,
            (8, 8)
        );
        // Moving onto the starter Forest (0,0) collides.
        let err = zoo.move_habitat(wid, (0, 0)).unwrap_err();
        assert!(matches!(err, ZooError::TileOccupied));
        // Moving an unknown habitat errors.
        let err = zoo.move_habitat(Uuid::new_v4(), (2, 2)).unwrap_err();
        assert!(matches!(err, ZooError::UnknownHabitat));
    }

    #[test]
    fn purchase_animal_spawns_freeform_and_charges() {
        let mut zoo = Zoo::new(ts());
        zoo.habitats.clear(); // no habitats in the freeform world
        zoo.coins = 1000;
        let before = zoo.animals.len();
        // blue_frog costs 50 coins and needs no habitat.
        let id = zoo.purchase_animal("blue_frog", ts()).unwrap();
        assert_eq!(zoo.animals.len(), before + 1);
        assert!(zoo.animals.contains_key(&id));
        assert_eq!(zoo.coins, 950);
        // Can't afford a second when broke.
        zoo.coins = 0;
        assert!(matches!(
            zoo.purchase_animal("blue_frog", ts()),
            Err(ZooError::NotEnoughCoins)
        ));
    }

    #[test]
    fn claim_breeding_falls_back_to_freeform_without_habitat() {
        let mut zoo = Zoo::new(ts());
        zoo.habitats.clear(); // freeform: no habitats to place into
        let a = zoo.spawn_animal_freeform("red_fox", 1, ts()).unwrap();
        let b = zoo.spawn_animal_freeform("treeFrog", 1, ts()).unwrap();
        grant_nest(&mut zoo);
        let ends = zoo.start_breeding(a, b, ts()).unwrap();
        let later = ends + chrono::Duration::seconds(1);
        let claimed = zoo.claim_completed_breeding(a, later).unwrap();
        // No habitat → nil habitat id, but the offspring still spawned.
        assert_eq!(claimed.habitat_id, Uuid::nil());
        assert!(zoo.animals.contains_key(&claimed.animal_id));
    }

    #[test]
    fn habitat_upgrade_two_phase() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        let hid = zoo.habitats[0].id;
        // Start an upgrade. Before the timer elapses, claim is rejected.
        let ends_at = zoo.start_habitat_upgrade(hid, now).unwrap();
        assert!(matches!(
            zoo.claim_habitat_upgrade(hid, now),
            Err(ZooError::UpgradeNotReady)
        ));
        // A second start is rejected — only one upgrade in flight at a time.
        assert!(matches!(
            zoo.start_habitat_upgrade(hid, now),
            Err(ZooError::UpgradeInProgress)
        ));
        // Past ends_at, the claim applies and clears the slot.
        let later = ends_at + chrono::Duration::seconds(1);
        let new_level = zoo.claim_habitat_upgrade(hid, later).unwrap();
        assert_eq!(new_level, 2);
        assert_eq!(zoo.habitats[0].level, 2);
        assert!(zoo.habitats[0].upgrade_finishes_at.is_none());
    }

    #[test]
    fn nest_unlock_mixes_coins_then_dna_up_to_cap() {
        let mut zoo = Zoo::new(ts());
        zoo.coins = 1_000_000;
        zoo.dna_helix = 1_000;
        assert_eq!(zoo.nest_count, 0, "all nests start locked");
        // First two are coin-gated.
        assert_eq!(zoo.buy_nest().unwrap(), 1);
        assert_eq!(zoo.buy_nest().unwrap(), 2);
        assert_eq!(zoo.coins, 1_000_000 - (1_500 + 25_000));
        // Last three are DNA-gated.
        assert_eq!(zoo.buy_nest().unwrap(), 3);
        assert_eq!(zoo.buy_nest().unwrap(), 4);
        assert_eq!(zoo.buy_nest().unwrap(), 5);
        assert_eq!(zoo.dna_helix, 1_000 - (5 + 15 + 40));
        // Sixth is refused.
        assert!(matches!(zoo.buy_nest().unwrap_err(), ZooError::NestCapReached));
        assert_eq!(zoo.nests.len(), 5);
    }

    #[test]
    fn nest_unlock_rejects_when_too_poor() {
        let mut zoo = Zoo::new(ts());
        // Too few coins for the first (coin-gated) nest.
        zoo.coins = 100;
        assert!(matches!(zoo.buy_nest().unwrap_err(), ZooError::NotEnoughCoins));
        assert_eq!(zoo.nest_count, 0);
        assert_eq!(zoo.coins, 100);
        // Afford the two coin nests, then fail the DNA-gated third.
        zoo.coins = 1_000_000;
        zoo.buy_nest().unwrap();
        zoo.buy_nest().unwrap();
        zoo.dna_helix = 1;
        assert!(matches!(zoo.buy_nest().unwrap_err(), ZooError::NotEnoughDna));
        assert_eq!(zoo.nest_count, 2);
    }

    #[test]
    fn start_breeding_rejected_when_all_nests_busy() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_nest().unwrap(); // one nest → only one pair can breed at a time
        // Set up two legal cross-species pairs (each with a pool).
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        zoo.buy_habitat(HabitatTheme::Savanna, (8, 0)).unwrap();
        let (_, fox) = zoo.buy_animal("red_fox", now).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        zoo.start_breeding(fox, frog, now).unwrap();
        // Second cross-species pair attempts to claim a second nest.
        let (_, lion) = zoo.buy_animal("lion", now).unwrap();
        let (_, zebra) = zoo.buy_animal("zebra", now).unwrap();
        let err = zoo.start_breeding(lion, zebra, now).unwrap_err();
        assert!(matches!(err, ZooError::AllNestsBusy));
    }

    #[test]
    fn cancel_breeding_leaves_no_offspring_and_no_codex_entry() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_nest().unwrap();
        // Set up a crossbreed pair so a successful redeem *would* add to
        // discovered_recipes; cancelling must skip that.
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, fox) = zoo.buy_animal("red_fox", now).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        zoo.start_breeding(fox, frog, now).unwrap();
        assert_eq!(zoo.active_breeding_pair_count(), 1);

        zoo.cancel_breeding(fox, now).unwrap();
        assert_eq!(zoo.active_breeding_pair_count(), 0);
        // Both parents are back to Idle.
        assert!(matches!(zoo.animals.get(&fox).unwrap().state, AnimalState::Idle));
        assert!(matches!(zoo.animals.get(&frog).unwrap().state, AnimalState::Idle));
        // No codex entry, only the two parent species exist.
        assert!(zoo.animals.values().all(|a| a.species == "red_fox" || a.species == "treeFrog"));
        assert!(zoo.discovered_recipes.is_empty());
        // Nest is freed — a new pair can start immediately.
        zoo.start_breeding(fox, frog, now).unwrap();
    }

    #[test]
    fn cancel_breeding_rejects_idle_animal() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100;
        let (_, mouse) = zoo.buy_animal("field_mouse", now).unwrap();
        let err = zoo.cancel_breeding(mouse, now).unwrap_err();
        assert!(matches!(err, ZooError::NotBreeding));
    }

    #[test]
    fn deposit_and_remove_round_trip_through_a_nest() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_nest().unwrap();
        let (_, mouse) = zoo.buy_animal("field_mouse", now).unwrap();
        let nest_id = zoo.nests[0].id;

        zoo.deposit_in_nest(nest_id, mouse).unwrap();
        assert!(zoo.animal_in_any_nest(mouse));
        // Re-depositing the same animal is refused.
        assert!(matches!(
            zoo.deposit_in_nest(nest_id, mouse).unwrap_err(),
            ZooError::AlreadyNested
        ));
        // Removing returns it and frees the slot.
        let removed = zoo.remove_from_nest(nest_id, 0).unwrap();
        assert_eq!(removed, mouse);
        assert!(!zoo.animal_in_any_nest(mouse));
    }

    #[test]
    fn nest_full_rejects_third_occupant() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_nest().unwrap();
        // One-of-each: three *distinct* species so they're three distinct ids.
        let a = zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        let b = zoo.spawn_animal_freeform("red_fox", 1, now).unwrap();
        let c = zoo.spawn_animal_freeform("lion", 1, now).unwrap();
        let nest_id = zoo.nests[0].id;
        zoo.deposit_in_nest(nest_id, a).unwrap();
        zoo.deposit_in_nest(nest_id, b).unwrap();
        assert!(matches!(
            zoo.deposit_in_nest(nest_id, c).unwrap_err(),
            ZooError::NestFull
        ));
    }

    #[test]
    fn nest_breed_starts_from_deposited_pair_and_blocks_removal() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_nest().unwrap();
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, fox) = zoo.buy_animal("red_fox", now).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        let nest_id = zoo.nests[0].id;
        zoo.deposit_in_nest(nest_id, fox).unwrap();
        zoo.deposit_in_nest(nest_id, frog).unwrap();
        assert!(matches!(zoo.nest_status(nest_id, now), NestStatus::ReadyToBreed));

        zoo.nest_breed(nest_id, now).unwrap();
        assert!(matches!(zoo.nest_status(nest_id, now), NestStatus::Breeding(_)));
        // Occupants can't be yanked mid-breed.
        assert!(matches!(
            zoo.remove_from_nest(nest_id, 0).unwrap_err(),
            ZooError::OccupantBreeding
        ));
    }

    #[test]
    fn nest_completion_releases_parents_and_collect_frees_offspring() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, fox) = zoo.buy_animal("red_fox", now).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        grant_nest(&mut zoo);
        let nest_id = zoo.nests[0].id;
        zoo.deposit_in_nest(nest_id, fox).unwrap();
        zoo.deposit_in_nest(nest_id, frog).unwrap();
        let ends = zoo.nest_breed(nest_id, now).unwrap();

        // Nothing happens before the timer is up.
        assert!(zoo.advance_nests(now).is_empty());

        // On completion: parents auto-released, offspring left in the nest.
        let hatched = zoo.advance_nests(ends);
        assert_eq!(hatched.len(), 1);
        assert_eq!(zoo.nests[0].occupants().len(), 0, "parents left the nest");
        assert!(zoo.nests[0].offspring.is_some(), "offspring left in the nest");
        assert!(matches!(zoo.animals[&fox].state, AnimalState::Idle));
        assert!(matches!(zoo.animals[&frog].state, AnimalState::Idle));
        assert!(matches!(zoo.nest_status(nest_id, ends), NestStatus::ReadyToCollect));

        // Re-running advance is idempotent (offspring already pending).
        assert!(zoo.advance_nests(ends).is_empty());

        // Collecting frees the offspring into the zoo and empties the nest.
        let off_id = zoo.nests[0].offspring.unwrap();
        let (species, _is_hybrid) = zoo.nest_collect(nest_id, ends).unwrap();
        assert!(zoo.animals.contains_key(&off_id), "offspring is an owned animal");
        assert_eq!(zoo.animals[&off_id].species, species);
        assert!(zoo.nests[0].offspring.is_none(), "nest emptied after collect");
        // Collecting again errors — nothing left to collect.
        assert!(zoo.nest_collect(nest_id, ends).is_err());
    }

    #[test]
    fn nest_outcomes_lists_pool_with_discovery_flags() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_nest().unwrap();
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, fox) = zoo.buy_animal("red_fox", now).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        let nest_id = zoo.nests[0].id;
        zoo.deposit_in_nest(nest_id, fox).unwrap();
        zoo.deposit_in_nest(nest_id, frog).unwrap();

        let outcomes = zoo.nest_outcomes(nest_id);
        assert!(!outcomes.is_empty(), "a valid cross should list outcomes");
        // Parent species are always treated as discovered.
        let fox_sp = zoo.animals.get(&fox).unwrap().species;
        assert!(outcomes.iter().any(|(sp, _, disc)| *sp == fox_sp && *disc));
        // Percentages sum to roughly 100 (integer floor may shave a point).
        let sum: u32 = outcomes.iter().map(|(_, p, _)| p).sum();
        assert!((97..=100).contains(&sum), "got {sum}");
    }

    #[test]
    fn one_of_each_and_rank_advances_on_duplicates() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        // First acquisition creates the animal at Regular.
        let id = zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        assert_eq!(zoo.animals.len(), 1);
        assert_eq!(zoo.animals[&id].stage, 0);
        // Each further acquisition is a duplicate, never a second map entry.
        for _ in 0..10 {
            let dup = zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
            assert_eq!(dup, id, "one-of-each: same animal id");
            assert_eq!(zoo.animals.len(), 1);
        }
        // 10 duplicates → Silver.
        assert_eq!(*zoo.species_dupes.get("field_mouse").unwrap(), 10);
        assert_eq!(zoo.animals[&id].stage, 1);
        // Rank boosts income (Silver = ×1.5 over Regular).
        let regular = Animal::new("field_mouse", now).rate_per_sec();
        assert!((zoo.animals[&id].rate_per_sec() - regular * 1.5).abs() < 1e-9);
        // Push to 30 duplicates → Gold.
        for _ in 0..20 {
            zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        }
        assert_eq!(zoo.animals[&id].stage, 2);
    }

    #[test]
    fn sell_keeps_rank_and_reacquire_restores_it() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        let id = zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        for _ in 0..10 {
            zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        }
        assert_eq!(zoo.animals[&id].stage, 1); // Silver
        // Selling removes the animal but keeps the species' Rank progress.
        zoo.coins = 0;
        let coins = zoo.sell_animal(id, now).unwrap();
        assert!(coins > 0);
        assert_eq!(zoo.coins, coins);
        assert!(zoo.animals.is_empty());
        assert_eq!(*zoo.species_dupes.get("field_mouse").unwrap(), 10);
        // Re-acquiring restores the earned Rank (no increment for the rebuy).
        let again = zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        assert_eq!(zoo.animals[&again].stage, 1);
        assert_eq!(*zoo.species_dupes.get("field_mouse").unwrap(), 10);
    }

    #[test]
    fn sell_refused_while_nested() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_nest().unwrap();
        let id = zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        let nest_id = zoo.nests[0].id;
        zoo.deposit_in_nest(nest_id, id).unwrap();
        assert!(matches!(zoo.sell_animal(id, now).unwrap_err(), ZooError::AlreadyNested));
    }

    #[test]
    fn feed_levels_up_to_thirty_cap() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        let id = zoo.spawn_animal_freeform("field_mouse", 1, now).unwrap();
        zoo.food = 10_000_000;
        for _ in 1..MAX_ANIMAL_LEVEL {
            zoo.level_up_animal(id, now).unwrap();
        }
        assert_eq!(zoo.animals[&id].level, MAX_ANIMAL_LEVEL);
        assert_eq!(MAX_ANIMAL_LEVEL, 30);
        assert!(matches!(zoo.level_up_animal(id, now).unwrap_err(), ZooError::MaxLevel));
    }

    #[test]
    fn food_structures_unlock_in_sequence_up_to_cap() {
        let mut zoo = Zoo::new(ts());
        zoo.coins = 1_000_000;
        assert_eq!(zoo.structures.len(), 0, "all food structures start locked");
        for n in 1..=MAX_FOOD_STRUCTURES {
            assert_eq!(zoo.buy_food_structure(ts()).unwrap(), n);
        }
        let err = zoo.buy_food_structure(ts()).unwrap_err();
        assert!(matches!(err, ZooError::StructureCapReached));
        assert_eq!(zoo.structures.len(), MAX_FOOD_STRUCTURES);
    }

    #[test]
    fn food_structure_upgrade_and_collect() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 1_000_000;
        zoo.buy_food_structure(now).unwrap();
        let id = zoo.structures[0].id;
        // Upgrade to the max level.
        for _ in 1..MAX_STRUCTURE_LEVEL {
            zoo.upgrade_structure(id, now).unwrap();
        }
        assert_eq!(zoo.structures[0].level, MAX_STRUCTURE_LEVEL);
        assert!(matches!(zoo.upgrade_structure(id, now).unwrap_err(), ZooError::MaxLevel));
        // Collect sweeps accrued food into the bank.
        let later = now + chrono::Duration::seconds(120);
        let gained = zoo.collect_food_structure(id, later);
        assert!(gained > 0);
        assert_eq!(zoo.food, gained);
    }

    #[test]
    fn crossbreed_gestation_is_longer_than_parents() {
        // Verifies the cross-species gestation curve (1.5× slower parent).
        // The pool-based roll itself is non-deterministic from a single
        // attempt — see `hybrid_drop_awards_dna_helix` for that path.
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 10_000;
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, fox) = zoo.buy_animal("red_fox", now).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        grant_nest(&mut zoo);

        let ends_at = zoo.start_breeding(fox, frog, now).unwrap();
        let max_solo = species::get("red_fox")
            .gestation_seconds
            .max(species::get("treeFrog").gestation_seconds);
        let cross = (ends_at - now).num_seconds() as u64;
        assert!(
            cross > max_solo,
            "cross gestation {cross}s should exceed slower parent {max_solo}s"
        );

        let later = ends_at + chrono::Duration::seconds(1);
        let outcome = zoo.claim_completed_breeding(fox, later).unwrap();
        // The roll yields fox, treeFrog, or frox — all are legal.
        assert!(matches!(
            outcome.offspring_species,
            "red_fox" | "treeFrog" | "frox"
        ));
        // Codex is only updated on a hybrid drop. DNA mirrors the same rule.
        if outcome.is_hybrid_drop {
            assert_eq!(outcome.offspring_species, "frox");
            assert!(zoo.discovered_recipes.contains("frox"));
            assert!(zoo.dna_helix >= 1);
        } else {
            assert!(zoo.discovered_recipes.is_empty());
            assert_eq!(zoo.dna_helix, 0);
        }
    }

    #[test]
    fn start_breeding_rejects_pair_without_recipe() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 10_000;
        // mouse (Forest) + frog (Wetland) — no recipe defined for that pair.
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, mouse) = zoo.buy_animal("field_mouse", now).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        assert!(matches!(
            zoo.start_breeding(mouse, frog, now),
            Err(ZooError::SpeciesMismatch)
        ));
    }

    #[test]
    fn start_breeding_rejects_same_animal_same_species_and_no_pool() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 10_000;
        let (_, mouse_a) = zoo.buy_animal("field_mouse", now).unwrap();
        // One-of-each forbids owning two mice, but the SameSpecies guard must
        // still hold — craft a second same-species animal directly to test it.
        let mouse_b = {
            let extra = Animal::new("field_mouse", now);
            let id = extra.id;
            zoo.animals.insert(id, extra);
            id
        };
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        zoo.buy_habitat(HabitatTheme::Savanna, (8, 0)).unwrap();
        let (_, lion) = zoo.buy_animal("lion", now).unwrap();

        // Same-animal: still a SameAnimal error.
        assert!(matches!(
            zoo.start_breeding(mouse_a, mouse_a, now),
            Err(ZooError::SameAnimal)
        ));
        // Same-species: NEW SameSpecies error.
        assert!(matches!(
            zoo.start_breeding(mouse_a, mouse_b, now),
            Err(ZooError::SameSpecies)
        ));
        // Different species but no pool defined: SpeciesMismatch.
        assert!(matches!(
            zoo.start_breeding(mouse_a, frog, now),
            Err(ZooError::SpeciesMismatch)
        ));
        // Different species with a pool: succeeds. Use lion+frog... wait, no
        // pool for that. Use fox+frog (has a pool) instead.
        let (_, fox) = zoo.buy_animal("red_fox", now).unwrap();
        grant_nest(&mut zoo);
        zoo.start_breeding(fox, frog, now).unwrap();
        // Now `fox` is breeding; pairing it again returns NotIdle.
        assert!(matches!(
            zoo.start_breeding(fox, lion, now),
            Err(ZooError::NotIdle)
        ));
        let _ = (mouse_b,); // suppress unused warning
    }

    #[test]
    fn claim_completed_breeding_spawns_offspring_and_clears_state() {
        // Cross-species pair (fox + treeFrog has a pool). The roll picks one
        // of [fox, treeFrog, frox] — all three are valid outcomes; the test
        // just asserts the lifecycle, not the specific species.
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 10_000;
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, a) = zoo.buy_animal("red_fox", now).unwrap();
        let (_, b) = zoo.buy_animal("treeFrog", now).unwrap();
        grant_nest(&mut zoo);
        let before = zoo.animals.len();
        let ends_at = zoo.start_breeding(a, b, now).unwrap();
        // While breeding, the two animals stop accruing.
        assert!(matches!(
            zoo.animals.get(&a).unwrap().state,
            AnimalState::Breeding { .. }
        ));
        // Before ends_at, claim is rejected — the timer's still running.
        let early = ends_at - chrono::Duration::seconds(1);
        assert!(matches!(
            zoo.claim_completed_breeding(a, early),
            Err(ZooError::NotReady)
        ));
        // After ends_at, the click-on-ready-nest action redeems.
        let later = ends_at + chrono::Duration::seconds(1);
        let outcome = zoo.claim_completed_breeding(a, later).unwrap();
        // Both parents are Idle again.
        assert!(matches!(zoo.animals.get(&a).unwrap().state, AnimalState::Idle));
        assert!(matches!(zoo.animals.get(&b).unwrap().state, AnimalState::Idle));
        // The outcome is one of the three pool entries.
        assert!(matches!(
            outcome.offspring_species,
            "red_fox" | "treeFrog" | "frox"
        ));
        // One-of-each: a hybrid (frox) is a brand-new animal; a parent outcome
        // is already owned, so it advances that species' Rank instead.
        if outcome.offspring_species == "frox" {
            assert_eq!(zoo.animals.len(), before + 1);
        } else {
            assert_eq!(zoo.animals.len(), before);
            assert_eq!(
                zoo.species_dupes.get(outcome.offspring_species).copied(),
                Some(1)
            );
        }
    }

    #[test]
    fn claim_completed_breeding_places_offspring_via_freeform_fallback() {
        // With no compatible habitat space for the offspring, the claim still
        // succeeds by spawning the critter freeform — the pair returns to Idle.
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, a) = zoo.buy_animal("red_fox", now).unwrap();
        let (_, b) = zoo.buy_animal("treeFrog", now).unwrap();
        // Pack the Forest habitat so a Forest-bound hybrid has to go freeform.
        if let Some(forest) = zoo.habitats.iter_mut().find(|h| h.theme == HabitatTheme::Forest) {
            while forest.animal_ids.len() < forest.capacity() {
                forest.animal_ids.push(Uuid::new_v4());
            }
        }
        grant_nest(&mut zoo);
        let ends_at = zoo.start_breeding(a, b, now).unwrap();
        let later = ends_at + chrono::Duration::seconds(1);
        zoo.claim_completed_breeding(a, later).unwrap();
        assert!(matches!(zoo.animals.get(&a).unwrap().state, AnimalState::Idle));
        assert!(matches!(zoo.animals.get(&b).unwrap().state, AnimalState::Idle));
    }

    #[test]
    fn send_animal_gift_removes_locally_and_encodes_round_trip() {
        let now = ts();
        let mut sender = Zoo::new(now);
        sender.coins = 1_000;
        let (_, mouse_id) = sender.buy_animal("field_mouse", now).unwrap();
        assert_eq!(sender.animals.len(), 1);

        let gift = sender.send_animal_gift(mouse_id, now).unwrap();
        // Sender no longer has the animal anywhere.
        assert!(sender.animals.is_empty());
        assert!(sender.habitats.iter().all(|h| h.animal_ids.is_empty()));

        // Round-trip through the share-code pipe.
        let code = crate::share::encode(&crate::share::Payload::Gift(gift.clone())).unwrap();
        let decoded = crate::share::decode(&code).unwrap();
        let crate::share::Payload::Gift(decoded_gift) = decoded else {
            panic!("expected a Gift payload");
        };

        // Recipient claims; gift is idempotent against re-paste.
        let mut recipient = Zoo::new(now);
        recipient.claim_gift(&decoded_gift, now).unwrap();
        assert_eq!(recipient.animals.len(), 1);
        let err = recipient.claim_gift(&decoded_gift, now).unwrap_err();
        assert!(matches!(err, ZooError::AlreadyClaimed));
    }

    #[test]
    fn send_animal_gift_rejects_breeding_animal() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 10_000;
        zoo.buy_habitat(HabitatTheme::Wetland, (4, 0)).unwrap();
        let (_, a) = zoo.buy_animal("red_fox", now).unwrap();
        let (_, b) = zoo.buy_animal("treeFrog", now).unwrap();
        grant_nest(&mut zoo);
        zoo.start_breeding(a, b, now).unwrap();
        let err = zoo.send_animal_gift(a, now).unwrap_err();
        assert!(matches!(err, ZooError::NotIdle));
        // Animal is still present.
        assert!(zoo.animals.contains_key(&a));
    }

    #[test]
    fn build_shared_snapshot_tallies_species() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 10_000;
        // One-of-each: distinct species, one of each.
        zoo.buy_animal("field_mouse", now).unwrap();
        zoo.buy_animal("red_fox", now).unwrap();
        let snap = zoo.build_shared_snapshot(now);
        assert_eq!(snap.view.animal_count, 2);
        let mouse_entry = snap
            .view
            .species_tally
            .iter()
            .find(|e| e.species_id == "field_mouse")
            .unwrap();
        assert_eq!(mouse_entry.count, 1);
        assert_eq!(mouse_entry.total_level, 1);
    }

    #[test]
    fn claim_gift_is_idempotent() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        let gift = GiftPayload {
            version: 1,
            gift_id: Uuid::new_v4(),
            sender_id: Uuid::new_v4(),
            sender_name: "Friend".into(),
            created_at: now,
            contents: GiftContents::Animal {
                species_id: "field_mouse".into(),
                level: 2,
            },
        };
        zoo.claim_gift(&gift, now).unwrap();
        let err = zoo.claim_gift(&gift, now).unwrap_err();
        assert!(matches!(err, ZooError::AlreadyClaimed));
        // The placed mouse exists at the gifted level.
        let placed = zoo.animals.values().next().unwrap();
        assert_eq!(placed.species, "field_mouse");
        assert_eq!(placed.level, 2);
    }

    #[test]
    fn zoo_capacity_and_cost_curves() {
        // Capacity grows linearly with expansion level.
        assert_eq!(zoo_animal_capacity(0), ZOO_CAPACITY_BASE);
        assert_eq!(zoo_animal_capacity(1), ZOO_CAPACITY_BASE + ZOO_CAPACITY_PER_LEVEL);
        assert_eq!(zoo_animal_capacity(2), ZOO_CAPACITY_BASE + 2 * ZOO_CAPACITY_PER_LEVEL);

        // Capacity climbs linearly to ~1 000 over the full 50-level track.
        assert!(zoo_animal_capacity(MAX_ZOO_LEVEL) >= 1_000);

        // Cost is a moderate exponential across the long track.
        assert_eq!(zoo_upgrade_cost(0), Some(2_000));
        assert_eq!(zoo_upgrade_cost(1), Some(2_400));
        assert_eq!(zoo_upgrade_cost(2), Some(2_880));
        // Duration ramps minutes → hours → days, clamped to the 4-day ceiling
        // only near the very top of the track.
        assert_eq!(zoo_upgrade_duration(0).unwrap().num_seconds(), 180);
        assert_eq!(zoo_upgrade_duration(1).unwrap().num_seconds(), 212);
        assert_eq!(zoo_upgrade_duration(2).unwrap().num_seconds(), 251);
        assert!(zoo_upgrade_duration(20).unwrap().num_seconds() < ZOO_UPGRADE_MAX_SECS);
        assert_eq!(zoo_upgrade_duration(MAX_ZOO_LEVEL - 1).unwrap().num_seconds(), ZOO_UPGRADE_MAX_SECS);

        // Both bottom out at the max level.
        assert_eq!(zoo_upgrade_cost(MAX_ZOO_LEVEL), None);
        assert_eq!(zoo_upgrade_duration(MAX_ZOO_LEVEL), None);
    }

    #[test]
    fn freeform_spawn_rejected_at_capacity() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        // Spawn distinct purchasable species (freeform needs no habitat) until
        // the base cap is reached; the next *new* species is rejected.
        let ids: Vec<SpeciesId> = species::all_purchasable().map(|d| d.id).collect();
        assert!(ids.len() > ZOO_CAPACITY_BASE, "need enough species to fill the zoo");
        for &id in ids.iter().take(ZOO_CAPACITY_BASE) {
            zoo.spawn_animal_freeform(id, 1, now).unwrap();
        }
        assert!(zoo.at_animal_capacity());
        let err = zoo
            .spawn_animal_freeform(ids[ZOO_CAPACITY_BASE], 1, now)
            .unwrap_err();
        assert!(matches!(err, ZooError::ZooAtCapacity));

        // A duplicate of an already-owned species still works (advances Rank,
        // needs no capacity).
        let dup = zoo.spawn_animal_freeform(ids[0], 1, now).unwrap();
        assert_eq!(dup, zoo.animal_id_for_species(ids[0]).unwrap());
        assert_eq!(zoo.animals.len(), ZOO_CAPACITY_BASE);
    }

    #[test]
    fn zoo_expansion_two_phase() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 10_000;
        assert_eq!(zoo.zoo_level, 0);
        let base_cap = zoo.max_animal_capacity();

        // Starting an expansion charges coins and queues a build timer.
        let cost = zoo.zoo_upgrade_cost().unwrap();
        let ends_at = zoo.start_zoo_upgrade(now).unwrap();
        assert_eq!(zoo.coins, 10_000 - cost);
        assert!(zoo.zoo_upgrade_in_progress());

        // Can't start a second build, and can't claim before it's done.
        assert!(matches!(zoo.start_zoo_upgrade(now), Err(ZooError::ZooUpgradeInProgress)));
        assert!(matches!(zoo.claim_zoo_upgrade(now), Err(ZooError::ZooUpgradeNotReady)));

        // After the timer, claiming bumps the level and grows capacity.
        let later = ends_at + Duration::seconds(1);
        let new_level = zoo.claim_zoo_upgrade(later).unwrap();
        assert_eq!(new_level, 1);
        assert!(!zoo.zoo_upgrade_in_progress());
        assert_eq!(zoo.max_animal_capacity(), base_cap + ZOO_CAPACITY_PER_LEVEL);
    }

    #[test]
    fn zoo_expansion_rejects_when_broke() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 0;
        assert!(matches!(zoo.start_zoo_upgrade(now), Err(ZooError::NotEnoughCoins)));
    }
}

impl fmt::Display for ZooError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ZooError::UnknownSpecies => "unknown species",
            ZooError::UnknownHabitat => "unknown habitat",
            ZooError::UnknownAnimal => "unknown animal",
            ZooError::UnknownStructure => "unknown structure",
            ZooError::UnknownStructureKind => "unknown structure kind",
            ZooError::HabitatCapReached => "max habitats of this theme owned (4)",
            ZooError::StructureCapReached => "all 5 food structures already built",
            ZooError::NoHabitatWithSpace => "no room — buy or upgrade a habitat",
            ZooError::NotEnoughCoins => "not enough coins",
            ZooError::NotEnoughFood => "not enough food",
            ZooError::NotEnoughDna => "not enough DNA Helix",
            ZooError::MaxLevel => "already at max level",
            ZooError::SameAnimal => "can't breed an animal with itself",
            ZooError::SameSpecies => "same-species pairs can't be bred — try a crossbreed",
            ZooError::NotIdle => "both animals must be idle",
            ZooError::NotBreeding => "animal is not currently breeding",
            ZooError::NotReady => "gestation has not finished yet",
            ZooError::SpeciesMismatch => "no breeding pool for this pair",
            ZooError::HabitatAlreadyExists => "you already own a habitat of this theme — upgrade it instead",
            ZooError::OutOfBounds => "that spot is off the grid",
            ZooError::TileOccupied => "those tiles are already occupied",
            ZooError::UpgradeInProgress => "this habitat is already upgrading",
            ZooError::UpgradeNotReady => "habitat upgrade has not finished yet",
            ZooError::AllNestsBusy => "all nests are in use — wait or buy another",
            ZooError::NestCapReached => "already own the max number of nests (4)",
            ZooError::NestFull => "this nest is full",
            ZooError::UnknownNest => "unknown nest",
            ZooError::OccupantBreeding => "can't remove an animal mid-breed",
            ZooError::AlreadyNested => "that animal is already in a nest",
            ZooError::ExoticShopOpen => "exotic shop is already open",
            ZooError::AlreadyClaimed => "gift already claimed",
            ZooError::ZooAtCapacity => "zoo is at capacity — expand it to hold more animals",
            ZooError::ZooUpgradeInProgress => "the zoo is already being expanded",
            ZooError::ZooUpgradeNotReady => "zoo expansion has not finished yet",
            ZooError::ZooMaxSize => "the zoo is already at its maximum size",
            ZooError::UnknownPedestal => "unknown pedestal",
            ZooError::PedestalCapReached => "all pedestals are already placed",
            ZooError::PedestalOccupied => "this pedestal already has a dedicated animal",
            ZooError::PedestalEmpty => "no pedestal here / none left in your hotbar",
            ZooError::AnimalLocked => "this animal is locked to its pedestal for 48h",
            ZooError::PedestalOnCooldown => "this pedestal is cooling down — try again later",
        };
        f.write_str(s)
    }
}



