//! Breeding tab — nests strip (each nest shows its active gestation or
//! "empty") plus the two-column pair picker.
//!
//! Color conventions (per design spec):
//! * orange (`theme::TIMER`) — countdown text on active gestations
//! * pink (`theme::SPECIAL`) — names of animals currently in gestation, and
//!   the highlighted picks in the pair picker
//! * red (`theme::ERROR`) — destructive actions (cancel, forfeit, REMOVE)
//! * green (`theme::CONFIRM`) — KEEP (constructive non-default)
//! * blue (`theme::PRIMARY`) — BREED (primary commit)

use chrono::{DateTime, Utc};
use egui::{RichText, Ui, Vec2};
use uuid::Uuid;

use super::{theme, widgets};
use crate::app::EguiApp;
use crate::game::species;
use crate::game::zoo::{MAX_NESTS, nest_purchase_cost};

const PARENT_SLOT_SIZE: f32 = 96.0;
const LIST_ROW_ICON_SIZE: f32 = 40.0;
const NEST_PARENT_SIZE: f32 = 44.0;

pub fn draw(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    let mut want_save = false;

    egui::ScrollArea::vertical().show(ui, |ui| {
        want_save |= draw_nests_strip(ui, app, now);
        ui.add_space(theme::PAD_M);
        want_save |= draw_picker(ui, app, now);
    });

    if want_save {
        app.save_under_lock(now);
    }

    // Esc clears any staging in the picker.
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escape
        && (app.breeding_first_pick.is_some() || app.breeding_second_pick.is_some())
    {
        app.breeding_first_pick = None;
        app.breeding_second_pick = None;
        app.set_status("cleared staged picks", false, now);
    }
}

/// Snapshot data needed to render an active gestation as a nest card. Built
/// up-front from `app.active_gestations()` so the card-drawing code can
/// re-borrow `app` mutably for icon textures + cancel / redeem actions.
#[derive(Clone)]
struct NestRow {
    a_id: Uuid,
    ends_at: DateTime<Utc>,
    species_a: &'static str,
    name_a: &'static str,
    lvl_a: u8,
    species_b: &'static str,
    name_b: &'static str,
    lvl_b: u8,
    gestation_total: u64,
}

fn draw_nests_strip(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) -> bool {
    let mut dirty = false;
    let owned = app.zoo.nest_count;
    let busy = app.zoo.active_breeding_pair_count();

    let rows: Vec<NestRow> = app
        .active_gestations()
        .into_iter()
        .map(|(a, b, ends_at)| {
            let def_a = species::get(a.species);
            let def_b = species::get(b.species);
            NestRow {
                a_id: a.id,
                ends_at,
                species_a: a.species,
                name_a: def_a.display_name,
                lvl_a: a.level,
                species_b: b.species,
                name_b: def_b.display_name,
                lvl_b: b.level,
                gestation_total: def_a.gestation_seconds,
            }
        })
        .collect();

    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::section_heading(
                ui,
                "Nests",
                Some(&format!("{busy}/{owned} in use · cap {MAX_NESTS}")),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if owned >= MAX_NESTS {
                    widgets::disabled_button(ui, "MAX");
                } else {
                    let cost = nest_purchase_cost(owned);
                    let affordable = app.zoo.coins >= cost;
                    let label = format!("Buy nest  ·  {} coins", super::fmt_int(cost));
                    if widgets::cost_button(ui, &label, affordable).clicked() {
                        match app.zoo.buy_nest() {
                            Ok(n) => {
                                app.set_status(format!("nest unlocked ({n}/{MAX_NESTS})"), false, now);
                                dirty = true;
                            }
                            Err(e) => app.set_status(format!("{e}"), true, now),
                        }
                    }
                }
            });
        });
        ui.add_space(theme::PAD_M);

        // Horizontal row of nest "slots". Each owned nest gets a card —
        // active gestation if one is in flight, empty placeholder otherwise.
        // active_gestations() iterates a HashMap so order isn't stable;
        // gestations fill slots left-to-right by index.
        let nest_count = owned as usize;
        let row_count = rows.len();
        ui.horizontal(|ui| {
            let total_w = ui.available_width();
            let gap = theme::PAD_S;
            let card_w = ((total_w - gap * (nest_count.saturating_sub(1) as f32))
                / nest_count.max(1) as f32)
                .clamp(160.0, 260.0);
            let card_h = 130.0;

            for slot_idx in 0..nest_count {
                if slot_idx > 0 {
                    ui.add_space(gap);
                }
                if let Some(row) = rows.get(slot_idx) {
                    if draw_active_nest(ui, app, row, slot_idx + 1, now, card_w, card_h) {
                        dirty = true;
                    }
                } else {
                    draw_empty_nest(ui, slot_idx + 1, card_w, card_h);
                }
                // Suppress unused-warning for row_count in debug.
                let _ = row_count;
            }
        });
    });
    dirty
}

/// Render an active gestation as a fixed-size nest card. Returns `true` if
/// the user mutated `app.zoo` (cancel forfeit, or redeem-on-click of a
/// ready nest).
///
/// Card states:
/// - **In progress** (`remaining > 0`): countdown + progress bar + Cancel.
/// - **Ready** (`remaining <= 0`): green-bordered, pulsing "READY · click
///   to redeem" prompt. Clicking the card claims the offspring; if no
///   habitat has room, surface an error and leave the nest as Ready so
///   the player can free space and click again.
fn draw_active_nest(
    ui: &mut Ui,
    app: &mut EguiApp,
    row: &NestRow,
    nest_index: usize,
    now: DateTime<Utc>,
    width: f32,
    height: f32,
) -> bool {
    let mut dirty = false;
    let remaining = row.ends_at.signed_duration_since(now).num_seconds();
    let ready = remaining <= 0;
    let total = row.gestation_total as i64;
    let elapsed = total - remaining.max(0);
    let frac = if total > 0 {
        (elapsed.max(0) as f32) / total as f32
    } else {
        0.0
    };

    let border = if ready {
        egui::Stroke { width: 2.0, color: theme::CONFIRM }
    } else {
        egui::Stroke { width: 1.0, color: theme::BORDER_SUBTLE }
    };

    // Wrap the whole card in an interactable response when ready, so clicking
    // anywhere on the card redeems. In-progress nests aren't clickable.
    let card_sense = if ready { egui::Sense::click() } else { egui::Sense::hover() };
    let card_resp = egui::Frame::none()
        .fill(theme::SURFACE)
        .stroke(border)
        .rounding(egui::Rounding::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(theme::PAD_S))
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(width, height));
            ui.allocate_ui_with_layout(
                Vec2::new(width, height),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    // Title row: nest number + (when ready) a green "READY" pill.
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Nest {nest_index}"))
                                .color(theme::TEXT_DIM)
                                .size(11.0),
                        );
                        if ready {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("READY")
                                            .color(theme::CONFIRM)
                                            .size(10.0)
                                            .strong(),
                                    );
                                },
                            );
                        }
                    });
                    ui.add_space(2.0);

                    // Parent images in a horizontal row.
                    ui.horizontal(|ui| {
                        ui.add_space((width - NEST_PARENT_SIZE * 2.0 - 20.0) / 2.0);
                        let tex_a = app.icons.texture(ui.ctx(), row.species_a);
                        widgets::icon_image(ui, tex_a, row.name_a, NEST_PARENT_SIZE);
                        ui.label(RichText::new("+").color(theme::TEXT_DIM).strong());
                        let tex_b = app.icons.texture(ui.ctx(), row.species_b);
                        widgets::icon_image(ui, tex_b, row.name_b, NEST_PARENT_SIZE);
                    });
                    ui.add_space(2.0);

                    // Parent names (smaller line).
                    ui.label(
                        RichText::new(format!(
                            "{} L{}  +  {} L{}",
                            row.name_a, row.lvl_a, row.name_b, row.lvl_b
                        ))
                        .color(theme::SPECIAL)
                        .size(10.0)
                        .strong(),
                    );
                    ui.add_space(2.0);

                    if ready {
                        // Ready state: hint + Cancel only (clicking the card body redeems).
                        ui.label(
                            RichText::new("click to redeem")
                                .color(theme::CONFIRM)
                                .strong(),
                        );
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if widgets::danger_button(ui, "Cancel").clicked() {
                                        if try_cancel(app, row, now) {
                                            dirty = true;
                                        }
                                    }
                                },
                            );
                        });
                    } else {
                        // In progress: progress bar + countdown + Cancel.
                        widgets::progress_bar(
                            ui,
                            frac.clamp(0.0, 1.0),
                            width - theme::PAD_S * 2.0,
                            theme::TIMER,
                        );
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(super::fmt_duration_short(remaining))
                                    .color(theme::TIMER)
                                    .family(egui::FontFamily::Monospace)
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if widgets::danger_button(ui, "Cancel").clicked() {
                                        if try_cancel(app, row, now) {
                                            dirty = true;
                                        }
                                    }
                                },
                            );
                        });
                    }
                },
            );
        })
        .response
        .interact(card_sense);

    if ready && card_resp.clicked() {
        match app.zoo.claim_completed_breeding(row.a_id, now) {
            Ok(outcome) => {
                let def = species::get(outcome.offspring_species);
                let msg = if outcome.is_hybrid_drop {
                    format!("redeemed: {}! +1 DNA Helix", def.display_name)
                } else {
                    format!("redeemed: {} (parent drop)", def.display_name)
                };
                app.set_status(msg, false, now);
                dirty = true;
            }
            Err(e) => app.set_status(format!("{e}"), true, now),
        }
    }

    dirty
}

/// Helper: cancel a breeding and surface a status message. Returns whether
/// the cancel succeeded (caller marks dirty).
fn try_cancel(app: &mut EguiApp, row: &NestRow, now: DateTime<Utc>) -> bool {
    match app.zoo.cancel_breeding(row.a_id, now) {
        Ok(()) => {
            app.set_status(
                format!("forfeited gestation: {} + {}", row.name_a, row.name_b),
                false,
                now,
            );
            true
        }
        Err(e) => {
            app.set_status(format!("{e}"), true, now);
            false
        }
    }
}

fn draw_empty_nest(ui: &mut Ui, nest_index: usize, width: f32, height: f32) {
    egui::Frame::none()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke { width: 1.0, color: theme::BORDER_SUBTLE })
        .rounding(egui::Rounding::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(theme::PAD_S))
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(width, height));
            ui.allocate_ui_with_layout(
                Vec2::new(width, height),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.label(
                        RichText::new(format!("Nest {nest_index}"))
                            .color(theme::TEXT_DIM)
                            .size(11.0),
                    );
                    ui.add_space(height * 0.3);
                    ui.label(
                        RichText::new("empty")
                            .color(theme::TEXT_MUTED)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("pair two animals below to start")
                            .color(theme::TEXT_MUTED)
                            .size(10.0),
                    );
                },
            );
        });
}

fn draw_picker(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) -> bool {
    let mut dirty = false;
    let first_pick = app.breeding_first_pick;
    let second_pick = app.breeding_second_pick;

    widgets::card(ui, |ui| {
        widgets::section_heading(ui, "Pair picker", None);
        ui.add_space(theme::PAD_S);

        let total_width = ui.available_width();
        let left_width = (total_width * 0.62).max(280.0);
        let right_width = total_width - left_width - theme::PAD_M;

        ui.horizontal_top(|ui| {
            // ---- LEFT: Selection List ------------------------------------
            ui.allocate_ui_with_layout(
                Vec2::new(left_width, 320.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    let picked_in_left = draw_selection_list(ui, app, first_pick, second_pick, now);
                    if let Some((slot, animal_id)) = picked_in_left {
                        match slot {
                            PickSlot::First => app.breeding_first_pick = Some(animal_id),
                            PickSlot::Second => app.breeding_second_pick = Some(animal_id),
                        }
                    }
                },
            );

            ui.add_space(theme::PAD_M);

            // ---- RIGHT: Staged area --------------------------------------
            ui.allocate_ui_with_layout(
                Vec2::new(right_width.max(220.0), 320.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    let actions = draw_staged_area(ui, app, first_pick, second_pick);
                    if actions.clear_first {
                        app.breeding_first_pick = None;
                    }
                    if actions.clear_second {
                        app.breeding_second_pick = None;
                    }
                    if actions.remove_all {
                        app.breeding_first_pick = None;
                        app.breeding_second_pick = None;
                        app.set_status("cleared picks", false, now);
                    }
                    if actions.start_breeding {
                        if let (Some(a), Some(b)) = (first_pick, second_pick) {
                            match app.zoo.start_breeding(a, b, now) {
                                Ok(ends_at) => {
                                    let remaining = (ends_at - now).num_seconds().max(0);
                                    app.set_status(
                                        format!(
                                            "breeding started — hatches in {remaining}s"
                                        ),
                                        false,
                                        now,
                                    );
                                    app.breeding_first_pick = None;
                                    app.breeding_second_pick = None;
                                    dirty = true;
                                }
                                Err(e) => app.set_status(format!("{e}"), true, now),
                            }
                        }
                    }
                },
            );
        });
    });
    dirty
}

#[derive(Clone, Copy)]
enum PickSlot {
    First,
    Second,
}

fn draw_selection_list(
    ui: &mut Ui,
    app: &mut EguiApp,
    first_pick: Option<Uuid>,
    second_pick: Option<Uuid>,
    _now: DateTime<Utc>,
) -> Option<(PickSlot, Uuid)> {
    let candidate_view: Vec<(Uuid, &'static str, &'static str, u8)> = app
        .breeding_candidates(first_pick)
        .into_iter()
        .map(|a| {
            let def = species::get(a.species);
            (a.id, a.species, def.display_name, a.level)
        })
        .collect();

    let mut picked: Option<(PickSlot, Uuid)> = None;

    egui::Frame::none()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke {
            width: 1.0,
            color: theme::ACCENT_MUTED,
        })
        .rounding(egui::Rounding::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(theme::PAD_M))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Selection list")
                    .color(theme::TEXT_DIM)
                    .size(11.0),
            );
            ui.add_space(theme::PAD_S);

            if candidate_view.is_empty() {
                ui.label(
                    RichText::new("no eligible idle animals — pair must be same species or a valid crossbreed.")
                        .color(theme::TEXT_DIM),
                );
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (animal_id, species_id, name, level) in &candidate_view {
                        let is_first = Some(*animal_id) == first_pick;
                        let is_second = Some(*animal_id) == second_pick;
                        let staged = is_first || is_second;

                        let row_frame = egui::Frame::none()
                            .fill(if staged {
                                theme::SURFACE_HOVER
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(if staged {
                                egui::Stroke {
                                    width: 1.0,
                                    color: theme::SPECIAL,
                                }
                            } else {
                                egui::Stroke::NONE
                            })
                            .rounding(egui::Rounding::same(4.0))
                            .inner_margin(egui::Margin::symmetric(theme::PAD_S, 4.0));

                        row_frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let tex = app.icons.texture(ui.ctx(), species_id);
                                widgets::icon_image(ui, tex, name, LIST_ROW_ICON_SIZE);
                                ui.add_space(theme::PAD_S);

                                let name_color = if staged {
                                    theme::SPECIAL
                                } else {
                                    theme::TEXT_PRIMARY
                                };
                                ui.label(
                                    RichText::new(format!("{name} L{level}"))
                                        .color(name_color)
                                        .strong(),
                                );

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let (label, target_slot, enabled) = if is_first {
                                            ("Picked A", PickSlot::First, false)
                                        } else if is_second {
                                            ("Picked B", PickSlot::Second, false)
                                        } else if first_pick.is_none() {
                                            ("Pick", PickSlot::First, true)
                                        } else if second_pick.is_none() {
                                            ("Pair", PickSlot::Second, true)
                                        } else {
                                            ("Full", PickSlot::Second, false)
                                        };
                                        if enabled {
                                            if widgets::primary_button(ui, label).clicked() {
                                                picked = Some((target_slot, *animal_id));
                                            }
                                        } else {
                                            widgets::disabled_button(ui, label);
                                        }
                                    },
                                );
                            });
                        });
                        ui.add_space(2.0);
                    }
                });
        });

    picked
}

/// Buttons emitted by the staged area. Read by the caller because the
/// caller owns `&mut app` and can apply them outside this closure.
#[derive(Default)]
struct StagedActions {
    clear_first: bool,
    clear_second: bool,
    remove_all: bool,
    start_breeding: bool,
}

fn draw_staged_area(
    ui: &mut Ui,
    app: &mut EguiApp,
    first_pick: Option<Uuid>,
    second_pick: Option<Uuid>,
) -> StagedActions {
    let mut actions = StagedActions::default();

    let resolve = |id: Option<Uuid>| -> Option<(&'static str, &'static str)> {
        let a = app.zoo.animals.get(&id?)?;
        let def = species::get(a.species);
        Some((a.species, def.display_name))
    };
    let pick_a = resolve(first_pick);
    let pick_b = resolve(second_pick);

    // -- Two parent slots side-by-side -------------------------------------
    ui.horizontal(|ui| {
        let slot_a_clicked = draw_parent_slot(ui, app, pick_a, "A");
        ui.add_space(theme::PAD_S);
        let slot_b_clicked = draw_parent_slot(ui, app, pick_b, "B");
        if slot_a_clicked {
            actions.clear_first = true;
        }
        if slot_b_clicked {
            actions.clear_second = true;
        }
    });
    ui.add_space(theme::PAD_L);

    // -- Action buttons (REMOVE / BREED). KEEP was retired when redemption
    //    moved onto the ready-nest card (click the card to claim).
    let btn_width = ui.available_width();
    let btn_size = Vec2::new(btn_width, 36.0);
    let can_breed = pick_a.is_some() && pick_b.is_some();

    ui.allocate_ui_with_layout(btn_size, egui::Layout::top_down_justified(egui::Align::Center), |ui| {
        if widgets::danger_button(ui, "REMOVE").clicked() {
            actions.remove_all = true;
        }
    });
    ui.add_space(theme::PAD_S);
    ui.allocate_ui_with_layout(btn_size, egui::Layout::top_down_justified(egui::Align::Center), |ui| {
        if can_breed {
            if widgets::primary_action_button(ui, "BREED").clicked() {
                actions.start_breeding = true;
            }
        } else {
            widgets::disabled_button(ui, "BREED");
        }
    });

    actions
}

fn draw_parent_slot(
    ui: &mut Ui,
    app: &mut EguiApp,
    pick: Option<(&'static str, &'static str)>,
    label: &str,
) -> bool {
    let mut clicked = false;
    egui::Frame::none()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke {
            width: 1.0,
            color: theme::BORDER_SUBTLE,
        })
        .rounding(egui::Rounding::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(theme::PAD_S))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                Vec2::splat(PARENT_SLOT_SIZE),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    match pick {
                        Some((species_id, name)) => {
                            let tex = app.icons.texture(ui.ctx(), species_id);
                            widgets::icon_image(ui, tex, name, PARENT_SLOT_SIZE - theme::PAD_S * 2.0);
                            let resp = ui.allocate_response(
                                Vec2::new(PARENT_SLOT_SIZE, 18.0),
                                egui::Sense::click(),
                            );
                            ui.painter().text(
                                resp.rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "click to clear",
                                egui::FontId::proportional(10.0),
                                theme::TEXT_DIM,
                            );
                            if resp.clicked() {
                                clicked = true;
                            }
                        }
                        None => {
                            ui.add_space(PARENT_SLOT_SIZE * 0.3);
                            ui.label(
                                RichText::new(format!("Parent {label}"))
                                    .color(theme::TEXT_MUTED)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new("empty")
                                    .color(theme::TEXT_MUTED)
                                    .size(10.0),
                            );
                        }
                    }
                },
            );
        });
    clicked
}

