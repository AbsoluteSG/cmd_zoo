//! Player actions as data — the single vocabulary for mutating the host's
//! authoritative `Zoo`.
//!
//! In co-op the host owns the one true zoo. The host applies its own actions
//! directly; a visitor sends the same `Action` as a `NetMessage::Command` and
//! the host applies it on their behalf, then broadcasts the result. Routing
//! every mutation through this one enum keeps a single source of truth and
//! removes the old "visitor mutates a local copy that gets overwritten" bug.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::species::{self, SpeciesId};
use super::zoo::{CollectResult, Zoo, ZooError};

/// A player-initiated mutation of the zoo. Wire-serializable: species are sent
/// as owned ids (the domain uses `&'static str`, resolved on apply).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    CollectAnimal(Uuid),
    CollectFoodStructure(Uuid),
    DepositInNest { nest: Uuid, animal: Uuid },
    RemoveFromNest { nest: Uuid, slot: usize },
    NestBreed(Uuid),
    NestCollect(Uuid),
    BuyNest,
    BuyFoodStructure,
    UpgradeStructure(Uuid),
    /// Regular shop purchase (charges the species' own cost/currency).
    Purchase(String),
    /// Spawn a freeform critter at no cost (gift/catch paths).
    SpawnFreeform { species: String, level: u8 },
    Sell(Uuid),
    LevelUp(Uuid),
    StartZooUpgrade,
    ClaimZooUpgrade,
    SkipExoticWait,
    /// Buy one pedestal into the hotbar inventory (charges escalating DNA).
    BuyPedestalItem,
    /// Place one unplaced pedestal from the hotbar onto `tile` (already paid for).
    PlacePedestal { tile: (i32, i32) },
    /// Relocate an existing pedestal to `tile`.
    MovePedestal { pedestal: Uuid, tile: (i32, i32) },
    /// Remove a pedestal back into the hotbar inventory (frees its animal).
    RemovePedestal(Uuid),
    /// Park `animal` on `pedestal` so its income auto-collects.
    DedicateAnimal { pedestal: Uuid, animal: Uuid },
    /// Release the dedicated animal from `pedestal`.
    UndedicateAnimal(Uuid),
    /// Host-only: grant/revoke a permission bit for a visitor. Rejected if a
    /// visitor sends it (see `dispatch_remote`).
    GrantPermission { target: Uuid, bit: u32, grant: bool },
    /// A visitor completed a catch on a host-streamed wild animal. Handled
    /// specially in `dispatch_remote` (it mutates the wild world + zoo, which
    /// `apply_action` can't reach), not here.
    RegisterCatch(Uuid),
    /// Claim a completed collection's reward (see `game::collection`).
    ClaimCollection { id: String },
}

/// What an applied action produced, for local feedback (particles, sounds,
/// notifications) on the device that initiated it. Host-side only — never sent
/// over the wire, so it may hold `&'static str` species ids.
pub enum ActionOutcome {
    Collected(CollectResult),
    Food(u64),
    Sold(u64),
    Leveled(u8),
    Offspring { species: SpeciesId, is_hybrid: bool },
    Done,
}

/// Apply `action` to the authoritative `zoo`. The host calls this for its own
/// actions and for every visitor command. Returns the outcome for feedback, or
/// a `ZooError` to surface/reject.
pub fn apply_action(
    zoo: &mut Zoo,
    action: Action,
    now: DateTime<Utc>,
) -> Result<ActionOutcome, ZooError> {
    use Action::*;
    Ok(match action {
        CollectAnimal(id) => ActionOutcome::Collected(zoo.collect_animal(id, now)),
        CollectFoodStructure(id) => ActionOutcome::Food(zoo.collect_food_structure(id, now)),
        DepositInNest { nest, animal } => {
            zoo.deposit_in_nest(nest, animal)?;
            ActionOutcome::Done
        }
        RemoveFromNest { nest, slot } => {
            zoo.remove_from_nest(nest, slot)?;
            ActionOutcome::Done
        }
        NestBreed(id) => {
            zoo.nest_breed(id, now)?;
            ActionOutcome::Done
        }
        NestCollect(id) => {
            let (species, is_hybrid) = zoo.nest_collect(id, now)?;
            ActionOutcome::Offspring { species, is_hybrid }
        }
        BuyNest => {
            zoo.buy_nest()?;
            ActionOutcome::Done
        }
        BuyFoodStructure => {
            zoo.buy_food_structure(now)?;
            ActionOutcome::Done
        }
        UpgradeStructure(id) => {
            zoo.upgrade_structure(id, now)?;
            ActionOutcome::Done
        }
        Purchase(sp) => {
            zoo.purchase_animal(resolve(&sp)?, now)?;
            ActionOutcome::Done
        }
        SpawnFreeform { species: sp, level } => {
            zoo.spawn_animal_freeform(resolve(&sp)?, level, now)?;
            ActionOutcome::Done
        }
        Sell(id) => ActionOutcome::Sold(zoo.sell_animal(id, now)?),
        LevelUp(id) => ActionOutcome::Leveled(zoo.level_up_animal(id, now)?),
        StartZooUpgrade => {
            zoo.start_zoo_upgrade(now)?;
            ActionOutcome::Done
        }
        ClaimZooUpgrade => {
            zoo.claim_zoo_upgrade(now)?;
            ActionOutcome::Done
        }
        SkipExoticWait => {
            zoo.skip_exotic_wait(now)?;
            ActionOutcome::Done
        }
        BuyPedestalItem => {
            zoo.buy_pedestal_item()?;
            ActionOutcome::Done
        }
        PlacePedestal { tile } => {
            zoo.place_pedestal(tile)?;
            ActionOutcome::Done
        }
        MovePedestal { pedestal, tile } => {
            zoo.move_pedestal(pedestal, tile)?;
            ActionOutcome::Done
        }
        RemovePedestal(id) => {
            zoo.remove_pedestal(id, now)?;
            ActionOutcome::Done
        }
        DedicateAnimal { pedestal, animal } => {
            zoo.dedicate_animal(pedestal, animal, now)?;
            ActionOutcome::Done
        }
        UndedicateAnimal(id) => {
            zoo.undedicate_animal(id, now)?;
            ActionOutcome::Done
        }
        GrantPermission { target, bit, grant } => {
            if let Some(rec) = zoo.visitors.get_mut(&target) {
                rec.permissions.set(bit, grant);
            }
            ActionOutcome::Done
        }
        // Catch resolution touches the wild world (not just the zoo), so it's
        // intercepted in `GameApp::dispatch_remote` and never reaches here.
        RegisterCatch(_) => ActionOutcome::Done,
        ClaimCollection { id } => {
            zoo.claim_collection(&id, now)?;
            ActionOutcome::Done
        }
    })
}

/// Whether a visitor `player_id` is allowed to run `action` on the host's zoo.
/// Visitors can do everything except: sell (needs the `SELL` grant) and grant
/// permissions (host-only). The host's own actions bypass this entirely.
pub fn remote_action_allowed(
    visitors: &std::collections::HashMap<Uuid, crate::game::visitor::VisitorRecord>,
    player_id: Uuid,
    action: &Action,
) -> bool {
    use crate::game::visitor::PermissionSet;
    match action {
        Action::Sell(_) => visitors
            .get(&player_id)
            .is_some_and(|r| r.permissions.has(PermissionSet::SELL)),
        Action::GrantPermission { .. } => false,
        // Claiming a collection mutates the owner's own zoo — a visitor must not
        // claim on the host's behalf (online it's already self-scoped).
        Action::ClaimCollection { .. } => false,
        _ => true,
    }
}

/// Resolve a wire species id (owned) to a catalog `SpeciesId` (`&'static str`).
fn resolve(species_id: &str) -> Result<SpeciesId, ZooError> {
    species::try_get(species_id).map(|d| d.id).ok_or(ZooError::UnknownSpecies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 5, 12, 0, 0).unwrap()
    }

    #[test]
    fn purchase_then_sell_round_trips() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        assert_eq!(zoo.animals.len(), 0);

        let out = apply_action(&mut zoo, Action::Purchase("field_mouse".into()), now).unwrap();
        assert!(matches!(out, ActionOutcome::Done));
        assert_eq!(zoo.animals.len(), 1);

        let id = *zoo.animals.keys().next().unwrap();
        let out = apply_action(&mut zoo, Action::Sell(id), now).unwrap();
        assert!(matches!(out, ActionOutcome::Sold(_)));
        assert_eq!(zoo.animals.len(), 0);
    }

    #[test]
    fn unknown_species_is_rejected() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        let err = apply_action(&mut zoo, Action::Purchase("not_a_real_species".into()), now);
        assert!(matches!(err, Err(ZooError::UnknownSpecies)));
    }

    #[test]
    fn sell_is_gated_by_permission() {
        use crate::game::visitor::{PermissionSet, VisitorRecord};
        let mut visitors = std::collections::HashMap::new();
        let pid = Uuid::new_v4();
        visitors.insert(pid, VisitorRecord::new(pid, "V", ts()));

        let sell = Action::Sell(Uuid::new_v4());
        assert!(!remote_action_allowed(&visitors, pid, &sell), "no grant → rejected");
        visitors.get_mut(&pid).unwrap().permissions.set(PermissionSet::SELL, true);
        assert!(remote_action_allowed(&visitors, pid, &sell), "granted → allowed");

        // Granting permissions is host-only, never allowed from a visitor.
        let grant = Action::GrantPermission { target: pid, bit: PermissionSet::SELL, grant: true };
        assert!(!remote_action_allowed(&visitors, pid, &grant));

        // Everything else is open to any visitor.
        assert!(remote_action_allowed(&visitors, pid, &Action::BuyNest));
    }

    /// Buy a pedestal into the hotbar, place it, dedicate an animal, and (after
    /// the 48h lock) release it.
    #[test]
    fn pedestal_buy_place_dedicate_and_undedicate() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        zoo.dna_helix = 100_000;

        // Buy into inventory, then place from the hotbar onto a free tile.
        apply_action(&mut zoo, Action::BuyPedestalItem, now).unwrap();
        assert_eq!(zoo.unplaced_pedestals, 1);
        apply_action(&mut zoo, Action::PlacePedestal { tile: (1, 1) }, now).unwrap();
        assert_eq!(zoo.unplaced_pedestals, 0);
        assert_eq!(zoo.pedestals.len(), 1);
        let ped = zoo.pedestals[0].id;

        // Dedicate an owned animal to it.
        apply_action(&mut zoo, Action::Purchase("field_mouse".into()), now).unwrap();
        let animal = *zoo.animals.keys().next().unwrap();
        apply_action(&mut zoo, Action::DedicateAnimal { pedestal: ped, animal }, now).unwrap();
        assert!(zoo.animal_on_any_pedestal(animal));

        // A dedicated animal can't be deposited in a nest or fed.
        zoo.buy_nest().unwrap();
        let nest = zoo.nests[0].id;
        assert!(matches!(
            apply_action(&mut zoo, Action::DepositInNest { nest, animal }, now),
            Err(ZooError::AlreadyNested)
        ));
        assert!(matches!(
            apply_action(&mut zoo, Action::LevelUp(animal), now),
            Err(ZooError::AnimalLocked)
        ));

        // Release is refused during the 48h lock, allowed afterwards.
        assert!(matches!(
            apply_action(&mut zoo, Action::UndedicateAnimal(ped), now),
            Err(ZooError::AnimalLocked)
        ));
        let later = now + Duration::hours(49);
        apply_action(&mut zoo, Action::UndedicateAnimal(ped), later).unwrap();
        assert!(!zoo.animal_on_any_pedestal(animal));
        // ...which puts the pedestal on cooldown: re-dedicating is refused.
        assert!(matches!(
            apply_action(&mut zoo, Action::DedicateAnimal { pedestal: ped, animal }, later),
            Err(ZooError::PedestalOnCooldown)
        ));
    }

    /// Online a pedestal banks 1× per sweep; offline it banks up to 10×.
    #[test]
    fn pedestal_offline_banks_10x() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.dna_helix = 100;
        let ped = zoo.buy_pedestal_item().map(|_| zoo.place_pedestal((2, 0)).unwrap()).unwrap();
        apply_action(&mut zoo, Action::Purchase("field_mouse".into()), now).unwrap();
        let animal = *zoo.animals.keys().next().unwrap();
        zoo.dedicate_animal(ped, animal, now).unwrap();
        let cap = zoo.animals[&animal].storage_cap();

        // Below cap: nothing swept.
        assert_eq!(zoo.collect_pedestals(now).total(), 0);

        // A short online-style gap (just past 1× fill) banks ~1×.
        zoo.coins = 0;
        zoo.animals.get_mut(&animal).unwrap().last_collected_at = now - Duration::days(1);
        // First reset to a precise 1× fill window: mouse is 0.5/s, cap 60 → 120s.
        zoo.animals.get_mut(&animal).unwrap().last_collected_at = now - Duration::seconds(120);
        let online = zoo.collect_pedestals(now);
        assert_eq!(online.coins, cap, "online sweep banks one cap");

        // A long offline gap banks the full 10×.
        zoo.coins = 0;
        zoo.animals.get_mut(&animal).unwrap().last_collected_at = now - Duration::days(7);
        let offline = zoo.collect_pedestals(now);
        assert_eq!(offline.coins, cap * 10, "offline banks up to 10× cap");
    }

    /// Buying escalates DNA cost and caps at 6 owned (placed + unplaced).
    #[test]
    fn buy_pedestal_item_escalates_and_caps() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.dna_helix = 10 + 49 + 99 + 299 + 499 + 999; // exactly six
        for _ in 0..6 {
            apply_action(&mut zoo, Action::BuyPedestalItem, now).unwrap();
        }
        assert_eq!(zoo.unplaced_pedestals, 6);
        assert_eq!(zoo.dna_helix, 0);
        // Seventh is capped.
        assert!(matches!(
            apply_action(&mut zoo, Action::BuyPedestalItem, now),
            Err(ZooError::PedestalCapReached)
        ));
    }

    /// Placing requires stock and doesn't consume it on a rejected tile.
    #[test]
    fn place_pedestal_requires_stock_and_validates() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        // No stock yet.
        assert!(matches!(
            apply_action(&mut zoo, Action::PlacePedestal { tile: (0, 0) }, now),
            Err(ZooError::PedestalEmpty)
        ));
        zoo.unplaced_pedestals = 2;
        apply_action(&mut zoo, Action::PlacePedestal { tile: (0, 0) }, now).unwrap();
        assert_eq!(zoo.unplaced_pedestals, 1);
        // Same tile again is rejected and does NOT consume stock.
        assert!(matches!(
            apply_action(&mut zoo, Action::PlacePedestal { tile: (0, 0) }, now),
            Err(ZooError::TileOccupied)
        ));
        assert_eq!(zoo.unplaced_pedestals, 1);
    }

    /// Removing a pedestal refunds it to the hotbar; refused while locked.
    #[test]
    fn remove_pedestal_refunds_and_respects_lock() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.unplaced_pedestals = 1;
        let ped = zoo.place_pedestal((3, 3)).unwrap();
        apply_action(&mut zoo, Action::Purchase("field_mouse".into()), now).unwrap();
        let animal = *zoo.animals.keys().next().unwrap();
        zoo.dedicate_animal(ped, animal, now).unwrap();
        // Locked → can't remove.
        assert!(matches!(
            apply_action(&mut zoo, Action::RemovePedestal(ped), now),
            Err(ZooError::AnimalLocked)
        ));
        // After the lock, removing refunds to inventory and frees the animal.
        let later = now + Duration::hours(49);
        apply_action(&mut zoo, Action::RemovePedestal(ped), later).unwrap();
        assert_eq!(zoo.unplaced_pedestals, 1);
        assert!(zoo.pedestals.is_empty());
        assert!(!zoo.animal_on_any_pedestal(animal));
    }

    #[test]
    fn collect_returns_collected_outcome() {
        let now = ts();
        let mut zoo = Zoo::new(now);
        zoo.coins = 100_000;
        apply_action(&mut zoo, Action::Purchase("field_mouse".into()), now).unwrap();
        let id = *zoo.animals.keys().next().unwrap();
        // Backdate so income has accrued to the cap.
        if let Some(a) = zoo.animals.get_mut(&id) {
            a.last_collected_at = now - Duration::days(1);
        }
        let out = apply_action(&mut zoo, Action::CollectAnimal(id), now).unwrap();
        assert!(matches!(out, ActionOutcome::Collected(_)));
    }
}
