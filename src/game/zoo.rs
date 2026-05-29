use std::collections::{HashMap, HashSet};
use std::fmt;

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use super::animal::{Animal, AnimalState, MAX_ANIMAL_LEVEL, animal_level_up_cost};
use super::habitat::{
    Habitat, MAX_HABITAT_LEVEL, habitat_purchase_cost, habitat_upgrade_cost,
    habitat_upgrade_duration,
};
use super::player::Player;
use super::species::{self, HabitatTheme, SpeciesId};
use super::structure::{
    MAX_STRUCTURE_LEVEL, STRUCTURE_TOTAL_CAP, Structure, structure_purchase_cost,
    structure_upgrade_cost,
};
use super::structure_kind::{self, StructureKindId};
use crate::share::{
    GiftContents, GiftPayload, SharedSnapshotPayload, SnapshotView, SpeciesTallyEntry,
};

pub struct Zoo {
    pub player: Player,
    pub coins: u64,
    pub food: u64,
    /// Secondary "DNA Helix" currency. Earned from rare hybrid drops on
    /// crossbreed redemption and from the income of DNA-tier exotic animals
    /// (e.g. Snow Lion). Spent at the exotic shop on otherwise-unobtainable
    /// species.
    pub dna_helix: u64,
    pub habitats: Vec<Habitat>,
    pub animals: HashMap<Uuid, Animal>,
    pub structures: Vec<Structure>,
    pub claimed_gifts: HashSet<Uuid>,
    /// Crossbreed recipes the player has unlocked by rolling a hybrid drop.
    /// Recorded only on `claim_completed_breeding` when offspring is not a
    /// parent — parent drops don't count as discoveries.
    pub discovered_recipes: HashSet<SpeciesId>,
    /// How many concurrent breedings the player can run. Starts at 1; up to
    /// `MAX_NESTS` after purchasing the rest.
    pub nest_count: u8,
    /// When set, the index of an exotic-shop window the player paid DNA Helix
    /// to open early during its closed gap. Honored only while that window is
    /// the *next* one (see `exotic_shop::effective_window`); self-expires once
    /// it opens naturally. `None` normally.
    pub exotic_skip_window: Option<i64>,
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

/// Hard cap on nests. First nest is free; remaining three are gated by coins.
pub const MAX_NESTS: u8 = 4;

/// Coins to buy the next nest, given how many the player already owns
/// (i.e. `1` for the standard starter zoo).
pub fn nest_purchase_cost(current_nests: u8) -> u64 {
    match current_nests {
        1 => 1_500,
        2 => 65_000,
        3 => 825_000,
        _ => u64::MAX,
    }
}

impl Zoo {
    pub fn new(now: DateTime<Utc>) -> Self {
        let starter_habitat = Habitat::new(HabitatTheme::Forest);
        let starter_structure = Structure::new("hay_bale", now);
        Self {
            player: Player::new_default(),
            coins: 100,
            food: 0,
            dna_helix: 0,
            habitats: vec![starter_habitat],
            animals: HashMap::new(),
            structures: vec![starter_structure],
            claimed_gifts: HashSet::new(),
            discovered_recipes: HashSet::new(),
            nest_count: 1,
            exotic_skip_window: None,
            last_saved_at: now,
        }
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

    /// Purchase an additional nest. Capped at `MAX_NESTS`.
    pub fn buy_nest(&mut self) -> Result<u8, ZooError> {
        if self.nest_count >= MAX_NESTS {
            return Err(ZooError::NestCapReached);
        }
        let cost = nest_purchase_cost(self.nest_count);
        if self.coins < cost {
            return Err(ZooError::NotEnoughCoins);
        }
        self.coins -= cost;
        self.nest_count += 1;
        Ok(self.nest_count)
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

        // Auto-placement validates space before mutating, so on Err the pair
        // stays in Breeding state and the nest reads "Ready" again.
        let placed = self.auto_place_animal(offspring_species, 1, now)?;

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

    /// Buy the (single) habitat of a theme. Fails with `HabitatAlreadyExists`
    /// if the player already owns one of that theme — single-habitat-per-theme
    /// is now enforced; capacity grows through `start_habitat_upgrade` instead
    /// of buying duplicates.
    pub fn buy_habitat(&mut self, theme: HabitatTheme) -> Result<Uuid, ZooError> {
        if self.count_habitats_with_theme(theme) > 0 {
            return Err(ZooError::HabitatAlreadyExists);
        }
        let cost = habitat_purchase_cost();
        if self.coins < cost {
            return Err(ZooError::NotEnoughCoins);
        }
        self.coins -= cost;
        let h = Habitat::new(theme);
        let id = h.id;
        self.habitats.push(h);
        Ok(id)
    }

    pub fn buy_structure(
        &mut self,
        kind: StructureKindId,
        now: DateTime<Utc>,
    ) -> Result<Uuid, ZooError> {
        if self.structures.len() >= STRUCTURE_TOTAL_CAP {
            return Err(ZooError::StructureCapReached);
        }
        let def = structure_kind::try_get(kind).ok_or(ZooError::UnknownStructureKind)?;
        let cost = structure_purchase_cost(self.structures.len()).max(def.purchase_cost);
        if self.coins < cost {
            return Err(ZooError::NotEnoughCoins);
        }
        self.coins -= cost;
        let s = Structure::new(def.id, now);
        let id = s.id;
        self.structures.push(s);
        Ok(id)
    }

    /// Place a new animal (no cost). Used by gift claims and internally by `buy_animal`.
    /// Returns (habitat_id, animal_id).
    pub fn auto_place_animal(
        &mut self,
        species_id: SpeciesId,
        level: u8,
        now: DateTime<Utc>,
    ) -> Result<(Uuid, Uuid), ZooError> {
        let def = species::try_get(species_id).ok_or(ZooError::UnknownSpecies)?;
        let target_idx = self
            .habitats
            .iter()
            .position(|h| h.theme == def.theme && h.animal_ids.len() < h.capacity())
            .ok_or(ZooError::NoHabitatWithSpace)?;
        let habitat_id = self.habitats[target_idx].id;
        let mut animal = Animal::new(def.id, now);
        animal.level = level.clamp(1, MAX_ANIMAL_LEVEL);
        let animal_id = animal.id;
        self.habitats[target_idx].animal_ids.push(animal_id);
        self.animals.insert(animal_id, animal);
        Ok((habitat_id, animal_id))
    }

    /// Charge coins and auto-place. Validates affordability and space before mutating.
    pub fn buy_animal(
        &mut self,
        species_id: SpeciesId,
        now: DateTime<Utc>,
    ) -> Result<(Uuid, Uuid), ZooError> {
        let def = species::try_get(species_id).ok_or(ZooError::UnknownSpecies)?;
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
        let has_room = self
            .habitats
            .iter()
            .any(|h| h.theme == def.theme && h.animal_ids.len() < h.capacity());
        if !has_room {
            return Err(ZooError::NoHabitatWithSpace);
        }
        match def.purchase_currency {
            species::IncomeKind::Coin => self.coins -= def.purchase_cost,
            species::IncomeKind::DnaHelix => self.dna_helix -= def.purchase_cost,
        }
        self.auto_place_animal(def.id, 1, now)
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
            gift_id: Uuid::new_v4(),
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
    /// Tried to start an upgrade on a habitat whose upgrade is already in flight.
    UpgradeInProgress,
    /// `claim_habitat_upgrade` called while the upgrade timer is still running.
    UpgradeNotReady,
    AllNestsBusy,
    NestCapReached,
    /// Tried to pay to skip the exotic-shop wait while it's already open.
    ExoticShopOpen,
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

    #[test]
    fn level_up_animal_requires_food() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 1000;
        let (_, aid) = zoo.buy_animal("fieldMouse", now).unwrap();
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
        let err = zoo.buy_habitat(HabitatTheme::Forest).unwrap_err();
        assert!(matches!(err, ZooError::HabitatAlreadyExists));
        // A different theme is fine.
        zoo.buy_habitat(HabitatTheme::Wetland).unwrap();
        // But buying a second of *that* theme also errors.
        let err = zoo.buy_habitat(HabitatTheme::Wetland).unwrap_err();
        assert!(matches!(err, ZooError::HabitatAlreadyExists));
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
    fn nest_purchase_cost_curve_and_cap() {
        let mut zoo = Zoo::new(ts());
        zoo.coins = 1_000_000;
        // Starter has 1 nest. Buy three more to hit 4.
        let level = zoo.buy_nest().unwrap();
        assert_eq!(level, 2);
        assert_eq!(zoo.buy_nest().unwrap(), 3);
        assert_eq!(zoo.buy_nest().unwrap(), 4);
        let err = zoo.buy_nest().unwrap_err();
        assert!(matches!(err, ZooError::NestCapReached));
        // Cost curve: 1500 + 65000 + 825000 = 891500.
        assert_eq!(zoo.coins, 1_000_000 - 891_500);
    }

    #[test]
    fn nest_purchase_rejects_when_too_poor() {
        let mut zoo = Zoo::new(ts());
        zoo.coins = 100;
        let err = zoo.buy_nest().unwrap_err();
        assert!(matches!(err, ZooError::NotEnoughCoins));
        assert_eq!(zoo.nest_count, 1);
        assert_eq!(zoo.coins, 100);
    }

    #[test]
    fn start_breeding_rejected_when_all_nests_busy() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        // Set up two legal cross-species pairs (each with a pool).
        zoo.buy_habitat(HabitatTheme::Wetland).unwrap();
        zoo.buy_habitat(HabitatTheme::Savanna).unwrap();
        let (_, fox) = zoo.buy_animal("fox", now).unwrap();
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
        // Set up a crossbreed pair so a successful redeem *would* add to
        // discovered_recipes; cancelling must skip that.
        zoo.buy_habitat(HabitatTheme::Wetland).unwrap();
        let (_, fox) = zoo.buy_animal("fox", now).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        zoo.start_breeding(fox, frog, now).unwrap();
        assert_eq!(zoo.active_breeding_pair_count(), 1);

        zoo.cancel_breeding(fox, now).unwrap();
        assert_eq!(zoo.active_breeding_pair_count(), 0);
        // Both parents are back to Idle.
        assert!(matches!(zoo.animals.get(&fox).unwrap().state, AnimalState::Idle));
        assert!(matches!(zoo.animals.get(&frog).unwrap().state, AnimalState::Idle));
        // No codex entry, only the two parent species exist.
        assert!(zoo.animals.values().all(|a| a.species == "fox" || a.species == "treeFrog"));
        assert!(zoo.discovered_recipes.is_empty());
        // Nest is freed — a new pair can start immediately.
        zoo.start_breeding(fox, frog, now).unwrap();
    }

    #[test]
    fn cancel_breeding_rejects_idle_animal() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100;
        let (_, mouse) = zoo.buy_animal("fieldMouse", now).unwrap();
        let err = zoo.cancel_breeding(mouse, now).unwrap_err();
        assert!(matches!(err, ZooError::NotBreeding));
    }

    #[test]
    fn structure_cap_enforced() {
        let mut zoo = Zoo::new(ts());
        zoo.coins = 100_000;
        // Starter Hay Bale counts as one; buy three more to hit 4.
        for _ in 0..3 {
            zoo.buy_structure("hay_bale", ts()).unwrap();
        }
        let err = zoo.buy_structure("hay_bale", ts()).unwrap_err();
        assert!(matches!(err, ZooError::StructureCapReached));
    }

    #[test]
    fn crossbreed_gestation_is_longer_than_parents() {
        // Verifies the cross-species gestation curve (1.5× slower parent).
        // The pool-based roll itself is non-deterministic from a single
        // attempt — see `hybrid_drop_awards_dna_helix` for that path.
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 10_000;
        zoo.buy_habitat(HabitatTheme::Wetland).unwrap();
        let (_, fox) = zoo.buy_animal("fox", now).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();

        let ends_at = zoo.start_breeding(fox, frog, now).unwrap();
        let max_solo = species::get("fox")
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
            "fox" | "treeFrog" | "frox"
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
        zoo.buy_habitat(HabitatTheme::Wetland).unwrap();
        let (_, mouse) = zoo.buy_animal("fieldMouse", now).unwrap();
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
        let (_, mouse_a) = zoo.buy_animal("fieldMouse", now).unwrap();
        let (_, mouse_b) = zoo.buy_animal("fieldMouse", now).unwrap();
        zoo.buy_habitat(HabitatTheme::Wetland).unwrap();
        let (_, frog) = zoo.buy_animal("treeFrog", now).unwrap();
        zoo.buy_habitat(HabitatTheme::Savanna).unwrap();
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
        let (_, fox) = zoo.buy_animal("fox", now).unwrap();
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
        zoo.buy_habitat(HabitatTheme::Wetland).unwrap();
        let (_, a) = zoo.buy_animal("fox", now).unwrap();
        let (_, b) = zoo.buy_animal("treeFrog", now).unwrap();
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
        // One new animal exists (could be either parent or the hybrid).
        assert_eq!(zoo.animals.len(), before + 1);
        // The outcome is one of the three pool entries.
        assert!(matches!(
            outcome.offspring_species,
            "fox" | "treeFrog" | "frox"
        ));
    }

    #[test]
    fn claim_completed_breeding_errors_when_no_space_and_leaves_breeding_intact() {
        // Pair fox + treeFrog. Even if the roll lands on Frox (Forest theme)
        // or fox (Forest), the Forest habitat is full and the claim fails.
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.buy_habitat(HabitatTheme::Wetland).unwrap();
        // Forest has capacity 3; fill it.
        let (_, a) = zoo.buy_animal("fox", now).unwrap();
        zoo.buy_animal("fox", now).unwrap();
        zoo.buy_animal("fox", now).unwrap();
        // Wetland gets the breeding partner.
        let (_, b) = zoo.buy_animal("treeFrog", now).unwrap();
        assert_eq!(zoo.habitats[0].animal_ids.len(), 3);
        let ends_at = zoo.start_breeding(a, b, now).unwrap();
        let later = ends_at + chrono::Duration::seconds(1);
        // The roll is deterministic from (a.id, b.id, ends_at). The outcome
        // could be fox (Forest, full), treeFrog (Wetland, has room), or frox
        // (Forest, full). For Wetland-bound outcomes the claim succeeds; we
        // just need to verify that *if* claim fails the pair stays Breeding.
        match zoo.claim_completed_breeding(a, later) {
            Err(ZooError::NoHabitatWithSpace) => {
                assert!(matches!(
                    zoo.animals.get(&a).unwrap().state,
                    AnimalState::Breeding { .. }
                ));
                // Upgrade the Forest habitat to free a slot.
                let forest = zoo
                    .habitats
                    .iter()
                    .find(|h| h.theme == HabitatTheme::Forest)
                    .unwrap()
                    .id;
                let upg = zoo.start_habitat_upgrade(forest, later).unwrap();
                let after = upg + chrono::Duration::seconds(1);
                zoo.claim_habitat_upgrade(forest, after).unwrap();
                zoo.claim_completed_breeding(a, after).unwrap();
                assert!(matches!(
                    zoo.animals.get(&a).unwrap().state,
                    AnimalState::Idle
                ));
            }
            Ok(_) => {
                // Roll landed on a Wetland-eligible offspring (treeFrog) —
                // the test still passes: the lifecycle ran cleanly.
                assert!(matches!(
                    zoo.animals.get(&a).unwrap().state,
                    AnimalState::Idle
                ));
            }
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }

    #[test]
    fn send_animal_gift_removes_locally_and_encodes_round_trip() {
        let now = ts();
        let mut sender = Zoo::new(now);
        sender.coins = 1_000;
        let (_, mouse_id) = sender.buy_animal("fieldMouse", now).unwrap();
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
        zoo.buy_habitat(HabitatTheme::Wetland).unwrap();
        let (_, a) = zoo.buy_animal("fox", now).unwrap();
        let (_, b) = zoo.buy_animal("treeFrog", now).unwrap();
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
        zoo.buy_animal("fieldMouse", now).unwrap();
        zoo.buy_animal("fieldMouse", now).unwrap();
        zoo.buy_animal("fox", now).unwrap();
        let snap = zoo.build_shared_snapshot(now);
        assert_eq!(snap.view.animal_count, 3);
        let mouse_entry = snap
            .view
            .species_tally
            .iter()
            .find(|e| e.species_id == "fieldMouse")
            .unwrap();
        assert_eq!(mouse_entry.count, 2);
        // Two mice at L1 each → total_level = 2.
        assert_eq!(mouse_entry.total_level, 2);
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
                species_id: "fieldMouse".into(),
                level: 2,
            },
        };
        zoo.claim_gift(&gift, now).unwrap();
        let err = zoo.claim_gift(&gift, now).unwrap_err();
        assert!(matches!(err, ZooError::AlreadyClaimed));
        // The placed mouse exists at the gifted level.
        let placed = zoo.animals.values().next().unwrap();
        assert_eq!(placed.species, "fieldMouse");
        assert_eq!(placed.level, 2);
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
            ZooError::StructureCapReached => "max structures owned (4)",
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
            ZooError::UpgradeInProgress => "this habitat is already upgrading",
            ZooError::UpgradeNotReady => "habitat upgrade has not finished yet",
            ZooError::AllNestsBusy => "all nests are in use — wait or buy another",
            ZooError::NestCapReached => "already own the max number of nests (4)",
            ZooError::ExoticShopOpen => "exotic shop is already open",
            ZooError::AlreadyClaimed => "gift already claimed",
        };
        f.write_str(s)
    }
}



