//! `EguiApp` — the immediate-mode application state.
//!
//! Owns the `Zoo`, the file-backed repository handle, and per-tab UI state.
//! `impl eframe::App` lives at the bottom and does the load/advance/save tick.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::game::animal::AnimalState;
use crate::game::player::MAX_PLAYER_NAME_LEN;
use crate::game::species::{self, SpeciesId};
use crate::game::{Animal, Zoo, economy};
use crate::persistence::json_file::JsonFileRepository;
use crate::share::SharedSnapshotPayload;

// -- enums -------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Dashboard,
    Habitats,
    Structures,
    Shop,
    Breeding,
    Share,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Dashboard,
        Tab::Habitats,
        Tab::Structures,
        Tab::Shop,
        Tab::Breeding,
        Tab::Share,
        Tab::Settings,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Habitats => "Habitats",
            Tab::Structures => "Structures",
            Tab::Shop => "Shop",
            Tab::Breeding => "Breeding",
            Tab::Share => "Share",
            Tab::Settings => "Settings",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShopTab {
    Habitats,
    Structures,
    Animals,
    /// New in the exotic-shop milestone. Rotates every 4h45m, only buyable
    /// during the open window. Stocked from the hybrid catalog and priced
    /// in either very-high coins or DNA Helix.
    Exotic,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ShareMode {
    Menu,
    PickGift,
    ShowCode,
    Claim,
    ViewSnapshot,
}

// -- transient status --------------------------------------------------------

pub struct Status {
    pub text: String,
    pub at: DateTime<Utc>,
    pub is_error: bool,
}

// -- generated code state ----------------------------------------------------

pub struct ShowCodeState {
    pub code: String,
    pub label: String,
    /// Cached QR texture. `None` if encoding to a QR failed (code too long).
    pub qr_texture: Option<egui::TextureHandle>,
}

// -- the app -----------------------------------------------------------------

pub struct EguiApp {
    pub zoo: Zoo,
    pub repo: Arc<JsonFileRepository>,
    pub last_modtime: SystemTime,

    pub tab: Tab,
    pub status: Option<Status>,
    /// Cached species sprites (lazy, keyed by species id). Lives on `app` so
    /// every panel can request the same texture handle without re-decoding.
    pub icons: crate::ui::images::AnimalIcons,

    // Habitats tab
    pub selected_habitat: Option<Uuid>,
    /// Current page index in the paginated animal grid for the selected
    /// habitat. Reset to 0 whenever the selected habitat changes.
    pub habitat_page: usize,
    // Shop tab
    pub shop_tab: ShopTab,
    // Breeding tab
    pub breeding_first_pick: Option<Uuid>,
    /// Second-pick slot for the redesigned breeding picker. `Some` once the
    /// player has staged two parents; the REMOVE/KEEP/BREED buttons act on it.
    pub breeding_second_pick: Option<Uuid>,
    // Share tab
    pub share_mode: ShareMode,
    pub share_claim_buffer: String,
    pub share_show: Option<ShowCodeState>,
    pub share_snapshot_view: Option<SharedSnapshotPayload>,
    // Settings tab
    pub rename_buffer: String,
    /// Two-stage reset confirmation: first click sets this true; the inline
    /// "Confirm" button only appears while it is.
    pub confirming_reset: bool,
}

impl EguiApp {
    pub fn new(zoo: Zoo, repo: Arc<JsonFileRepository>, last_modtime: SystemTime) -> Self {
        let rename_buffer = zoo.player.name.clone();
        Self {
            zoo,
            repo,
            last_modtime,
            tab: Tab::Dashboard,
            status: None,
            icons: crate::ui::images::AnimalIcons::new(),
            selected_habitat: None,
            habitat_page: 0,
            shop_tab: ShopTab::Animals,
            breeding_first_pick: None,
            breeding_second_pick: None,
            share_mode: ShareMode::Menu,
            share_claim_buffer: String::new(),
            share_show: None,
            share_snapshot_view: None,
            rename_buffer,
            confirming_reset: false,
        }
    }

    /// Wipe the in-memory zoo, save the fresh state under lock, and reset all
    /// per-tab UI state that referenced ids from the old zoo. Called by the
    /// red "Reset game" button on the Settings tab.
    pub fn reset_game(&mut self, now: DateTime<Utc>) {
        self.zoo = crate::game::Zoo::new(now);
        // Reset UI state.
        self.tab = Tab::Dashboard;
        self.selected_habitat = None;
        self.habitat_page = 0;
        self.shop_tab = ShopTab::Animals;
        self.breeding_first_pick = None;
        self.breeding_second_pick = None;
        self.share_mode = ShareMode::Menu;
        self.share_claim_buffer.clear();
        self.share_show = None;
        self.share_snapshot_view = None;
        self.rename_buffer = self.zoo.player.name.clone();
        self.confirming_reset = false;
        self.save_under_lock(now);
        self.set_status("game reset", false, now);
    }

    // -- status -------------------------------------------------------------

    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool, now: DateTime<Utc>) {
        self.status = Some(Status {
            text: text.into(),
            at: now,
            is_error,
        });
    }

    pub fn clear_stale_status(&mut self, now: DateTime<Utc>) {
        if let Some(s) = &self.status {
            if (now - s.at).num_seconds() >= 4 {
                self.status = None;
            }
        }
    }

    // -- persistence --------------------------------------------------------

    /// Acquire the file lock, pick up any external write, advance the clock,
    /// and persist if anything actually changed (breeding completed). Mirrors
    /// the per-tick critical section from the old ratatui main loop.
    pub fn tick(&mut self, now: DateTime<Utc>) {
        // Clone the Arc so the lock guard borrows it instead of self.
        let repo = self.repo.clone();
        let access = match repo.lock().context("tick lock") {
            Ok(a) => a,
            Err(_) => return,
        };
        if let Ok(Some((zoo, mtime, warnings))) = access.load_if_newer(self.last_modtime) {
            self.zoo = zoo;
            self.last_modtime = mtime;
            self.reconcile_after_reload();
            // Warnings from a mid-session reload are rarer but possible if
            // another instance saved with a stale code path; surface the
            // first one and log the rest so a user investigating "where did
            // my animals go?" has a breadcrumb.
            if !warnings.is_empty() {
                for w in &warnings {
                    eprintln!("reload warning: {w}");
                }
                self.set_status(warnings[0].clone(), true, now);
            }
        }
        let breeding_before = self.breeding_pair_count();
        economy::advance(&mut self.zoo, now);
        let breeding_after = self.breeding_pair_count();
        if breeding_after < breeding_before {
            let completed = breeding_before - breeding_after;
            self.set_status(
                format!("{completed} gestation(s) completed"),
                false,
                now,
            );
            self.zoo.last_saved_at = now;
            if let Ok(mt) = access.save(&self.zoo) {
                self.last_modtime = mt;
            }
        }
    }

    /// Lock, save, update modtime. Call after any user-driven mutation.
    pub fn save_under_lock(&mut self, now: DateTime<Utc>) {
        self.zoo.last_saved_at = now;
        let repo = self.repo.clone();
        if let Ok(access) = repo.lock() {
            if let Ok(mt) = access.save(&self.zoo) {
                self.last_modtime = mt;
            }
        }
    }

    fn breeding_pair_count(&self) -> usize {
        self.zoo
            .animals
            .values()
            .filter(|a| matches!(a.state, AnimalState::Breeding { .. }))
            .count()
            / 2
    }

    /// Re-anchor any per-instance UI state that referenced ids no longer in
    /// the zoo (because another instance gifted them away, etc.).
    pub fn reconcile_after_reload(&mut self) {
        if let Some(id) = self.selected_habitat {
            if !self.zoo.habitats.iter().any(|h| h.id == id) {
                self.selected_habitat = None;
                self.habitat_page = 0;
            }
        }
        if let Some(id) = self.breeding_first_pick {
            let still_eligible = self
                .zoo
                .animals
                .get(&id)
                .map(|a| matches!(a.state, AnimalState::Idle))
                .unwrap_or(false);
            if !still_eligible {
                self.breeding_first_pick = None;
            }
        }
    }

    // -- data accessors used by panels --------------------------------------

    pub fn giftable_animals(&self) -> Vec<&Animal> {
        let mut v: Vec<&Animal> = self
            .zoo
            .animals
            .values()
            .filter(|a| matches!(a.state, AnimalState::Idle))
            .collect();
        v.sort_by(|a, b| {
            a.species
                .cmp(b.species)
                .then(b.level.cmp(&a.level))
                .then(a.id.cmp(&b.id))
        });
        v
    }

    pub fn breeding_candidates(&self, first_pick: Option<Uuid>) -> Vec<&Animal> {
        let first_species: Option<SpeciesId> = first_pick
            .and_then(|id| self.zoo.animals.get(&id))
            .map(|a| a.species);

        let mut v: Vec<&Animal> = self
            .zoo
            .animals
            .values()
            .filter(|a| matches!(a.state, AnimalState::Idle))
            // After picking a first animal, only show eligible partners:
            // same-species pairs are no longer breedable, so this filter is
            // strictly "different species AND has a pool".
            .filter(|a| match (first_pick, first_species) {
                (Some(fid), Some(sp)) => {
                    a.id != fid
                        && a.species != sp
                        && species::crossbreed_pool(sp, a.species).is_some()
                }
                _ => true,
            })
            .collect();
        v.sort_by(|a, b| {
            a.species
                .cmp(b.species)
                .then(b.level.cmp(&a.level))
                .then(a.id.cmp(&b.id))
        });
        v
    }

    pub fn active_gestations(&self) -> Vec<(&Animal, &Animal, DateTime<Utc>)> {
        let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        let mut out: Vec<(&Animal, &Animal, DateTime<Utc>)> = Vec::new();
        for a in self.zoo.animals.values() {
            if seen.contains(&a.id) {
                continue;
            }
            if let AnimalState::Breeding {
                partner_id,
                ends_at,
                ..
            } = a.state
            {
                if let Some(b) = self.zoo.animals.get(&partner_id) {
                    seen.insert(a.id);
                    seen.insert(partner_id);
                    out.push((a, b, ends_at));
                }
            }
        }
        out.sort_by(|x, y| x.2.cmp(&y.2));
        out
    }

    // -- rename --------------------------------------------------------------

    pub fn rename_filter(s: &str) -> String {
        let mut buf = String::new();
        for c in s.chars() {
            if buf.chars().count() >= MAX_PLAYER_NAME_LEN {
                break;
            }
            if c.is_alphanumeric() || c == ' ' || c == '_' || c == '-' {
                buf.push(c);
            }
        }
        buf
    }
}

// -- eframe glue -------------------------------------------------------------

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Utc::now();
        self.tick(now);
        self.clear_stale_status(now);
        crate::ui::draw(ctx, self, now);
        // Idle-game progress bars + countdowns: keep repainting.
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}
