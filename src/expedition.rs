//! Client-side expedition controller (Phase 3).
//!
//! Owns the player's current biome instance and the live catch engagement,
//! translating discrete player commands (target, use ability, hit skill check)
//! into calls on the headless core ([`crate::game::biome_instance`] +
//! [`crate::game::catch`]) and surfacing capture events back to `GameApp`.
//!
//! Deliberately **macroquad-free and pure** so it can be unit-tested in the
//! binary crate; `GameApp` owns the input wiring and rendering. This is the
//! integration seam for the new hub ↔ expedition loop — the in-world UI and the
//! eventual removal of the legacy open world build on top of it.

use glam::Vec2;
use uuid::Uuid;

use crate::game::biome_instance::BiomeInstance;
use crate::game::catch::{AbilityKind, CatchEngagement, CatchStats, EngagementOutcome};
use crate::game::species::{HabitatTheme, SpeciesId};

/// One in-progress expedition: a single bounded biome instance plus the catch
/// engagement against the currently-targeted spawn (if any).
pub struct Expedition {
    pub instance: BiomeInstance,
    /// The spawn the player is currently engaging, if any.
    pub target: Option<Uuid>,
    /// Live engagement state for `target`. `Some` exactly when `target` is.
    pub engagement: Option<CatchEngagement>,
}

impl Expedition {
    /// Launch a fresh expedition into `theme`, arranged from `seed`.
    pub fn launch(theme: HabitatTheme, seed: u64) -> Self {
        Self { instance: BiomeInstance::new(theme, seed), target: None, engagement: None }
    }

    /// Begin engaging spawn `id`. Returns `false` for an unknown or already
    /// captured spawn (the current target is left untouched in that case).
    pub fn engage(&mut self, id: Uuid) -> bool {
        match self.instance.engage(id) {
            Some(e) => {
                self.target = Some(id);
                self.engagement = Some(e);
                true
            }
            None => false,
        }
    }

    /// Begin engaging the live spawn nearest `pos`. Returns its id if one was
    /// targeted. (Used once the instance renders in world space; the keyboard
    /// harness uses [`Self::engage_first_live`].)
    pub fn engage_nearest(&mut self, pos: Vec2) -> Option<Uuid> {
        let id = self.instance.nearest_live(pos)?.id;
        self.engage(id).then_some(id)
    }

    /// Begin engaging the first remaining live spawn (keyboard-harness target
    /// selection until in-world click targeting lands). Returns its id.
    pub fn engage_first_live(&mut self) -> Option<Uuid> {
        let id = self.instance.live().next()?.id;
        self.engage(id).then_some(id)
    }

    /// Drop the current target/engagement (the animal is left in the instance).
    pub fn cancel(&mut self) {
        self.target = None;
        self.engagement = None;
    }

    /// Trigger an equipped ability against the current target, if engaging.
    pub fn use_ability(&mut self, ability: AbilityKind, stats: &CatchStats) {
        if let Some(e) = &mut self.engagement {
            e.use_ability(ability, stats);
        }
    }

    /// The player hit the live skill check. Returns true if one was up.
    pub fn hit_skill_check(&mut self, stats: &CatchStats) -> bool {
        self.engagement.as_mut().is_some_and(|e| e.hit_skill_check(stats))
    }

    /// Advance the active engagement by `dt`. On capture, marks the spawn caught
    /// in the instance, clears the engagement, and returns the captured species
    /// so the caller can grant it to the hub zoo. Returns `None` otherwise.
    pub fn tick(&mut self, dt: f32, stats: &CatchStats) -> Option<SpeciesId> {
        let id = self.target?;
        let outcome = self.engagement.as_mut()?.tick(dt, stats);
        if outcome == EngagementOutcome::Captured {
            let species = self.instance.capture(id);
            self.target = None;
            self.engagement = None;
            return species;
        }
        None
    }

    /// Whether an engagement is currently in progress.
    pub fn is_engaging(&self) -> bool {
        self.engagement.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::catch::AbilityKind;

    fn strong_stats() -> CatchStats {
        CatchStats { catch_power: 5_000.0, ..Default::default() }
    }

    #[test]
    fn engage_nearest_targets_a_spawn() {
        let mut exp = Expedition::launch(HabitatTheme::Forest, 1);
        let pos = exp.instance.spawns[0].pos;
        let id = exp.engage_nearest(pos).expect("a spawn is targeted");
        assert_eq!(exp.target, Some(id));
        assert!(exp.is_engaging());
    }

    #[test]
    fn tick_to_capture_returns_species_and_clears() {
        let mut exp = Expedition::launch(HabitatTheme::Forest, 2);
        let first = exp.instance.spawns[0].id;
        let expected = exp.instance.spawns[0].species;
        assert!(exp.engage(first));
        let stats = strong_stats();
        // Overwhelming power → captured on the next tick.
        let captured = exp.tick(1.0, &stats);
        assert_eq!(captured, Some(expected));
        assert!(exp.target.is_none() && !exp.is_engaging());
        // The instance reflects the capture.
        assert!(exp.instance.spawn(first).unwrap().captured);
    }

    #[test]
    fn abilities_and_skill_check_are_noops_without_a_target() {
        let mut exp = Expedition::launch(HabitatTheme::Forest, 3);
        let stats = CatchStats::default();
        exp.use_ability(AbilityKind::Net, &stats); // no panic, no target
        assert!(!exp.hit_skill_check(&stats));
        assert!(exp.tick(0.5, &stats).is_none());
    }

    #[test]
    fn cancel_drops_engagement_but_keeps_spawn() {
        let mut exp = Expedition::launch(HabitatTheme::Forest, 4);
        let id = exp.instance.spawns[0].id;
        exp.engage(id);
        exp.cancel();
        assert!(!exp.is_engaging());
        assert!(!exp.instance.spawn(id).unwrap().captured, "cancel never captures");
    }
}
