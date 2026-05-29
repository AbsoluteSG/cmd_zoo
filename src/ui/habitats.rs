//! Habitats tab — left rail of habitats, right pane with detail. The detail
//! pane shows a paginated grid of 256px animal icons: click an icon to collect
//! that animal's coins, and each icon carries its own level-up button. No
//! scrolling — the grid paginates to fit the available space.

use chrono::{DateTime, Utc};
use egui::{RichText, Ui};
use uuid::Uuid;

use super::{theme, widgets};
use crate::app::EguiApp;
use crate::game::habitat::{MAX_HABITAT_LEVEL, habitat_upgrade_cost};

pub fn draw(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    if app.selected_habitat.is_none() {
        app.selected_habitat = app.zoo.habitats.first().map(|h| h.id);
        app.habitat_page = 0;
    }

    let mut want_save = false;

    egui::SidePanel::left("habitat_rail")
        .resizable(false)
        .default_width(260.0)
        .frame(rail_frame())
        .show_inside(ui, |ui| {
            draw_rail(ui, app);
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        if let Some(hid) = app.selected_habitat {
            want_save |= draw_detail(ui, app, hid, now);
        } else {
            ui.label(RichText::new("no habitat selected").color(theme::TEXT_DIM));
        }
    });

    if want_save {
        app.save_under_lock(now);
    }
}

fn rail_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(theme::BG)
        .inner_margin(egui::Margin::same(theme::PAD_S))
        .stroke(egui::Stroke {
            width: 1.0,
            color: theme::BORDER_SUBTLE,
        })
}

fn draw_rail(ui: &mut Ui, app: &mut EguiApp) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let habitats: Vec<_> = app.zoo.habitats.clone();
        for habitat in habitats {
            let selected = app.selected_habitat == Some(habitat.id);
            let bg = if selected {
                theme::SURFACE_ACTIVE
            } else {
                theme::SURFACE
            };
            let frame = egui::Frame::none()
                .fill(bg)
                .inner_margin(egui::Margin::same(theme::PAD_S))
                .rounding(egui::Rounding::same(theme::RADIUS));
            let resp = frame
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(habitat.theme.name())
                                .color(if selected {
                                    theme::ACCENT
                                } else {
                                    theme::TEXT_PRIMARY
                                })
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("L{}", habitat.level))
                                .color(theme::TEXT_DIM)
                                .size(11.0),
                        );
                    });
                    ui.label(
                        RichText::new(format!(
                            "{}/{} animals",
                            habitat.animal_ids.len(),
                            habitat.capacity()
                        ))
                        .color(theme::TEXT_DIM)
                        .size(11.0)
                        .family(egui::FontFamily::Monospace),
                    );
                })
                .response
                .interact(egui::Sense::click());
            if resp.clicked() && app.selected_habitat != Some(habitat.id) {
                app.selected_habitat = Some(habitat.id);
                app.habitat_page = 0;
            }
            ui.add_space(4.0);
        }
    });
}

fn draw_detail(ui: &mut Ui, app: &mut EguiApp, hid: Uuid, now: DateTime<Utc>) -> bool {
    let mut dirty = false;
    let habitat = match app.zoo.habitats.iter().find(|h| h.id == hid).cloned() {
        Some(h) => h,
        None => {
            ui.label(RichText::new("habitat gone").color(theme::ERROR));
            return false;
        }
    };

    // -- Header: name, level, upgrade, collect-all (no progress bars) --------
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(habitat.theme.name())
                    .color(theme::TEXT_PRIMARY)
                    .size(18.0)
                    .strong(),
            );
            ui.label(RichText::new(format!("Level {}", habitat.level)).color(theme::TEXT_DIM));
            ui.label(
                RichText::new(format!(
                    "· {}/{} animals",
                    habitat.animal_ids.len(),
                    habitat.capacity()
                ))
                .color(theme::TEXT_DIM)
                .size(11.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if habitat.level >= MAX_HABITAT_LEVEL {
                    widgets::disabled_button(ui, "MAX level");
                } else if let Some(ends_at) = habitat.upgrade_finishes_at {
                    // Upgrade in flight.
                    let remaining = (ends_at - now).num_seconds();
                    if remaining <= 0 {
                        // Ready — redeem.
                        if widgets::confirm_button(ui, "REDEEM upgrade").clicked() {
                            match app.zoo.claim_habitat_upgrade(hid, now) {
                                Ok(lvl) => {
                                    app.set_status(
                                        format!("habitat upgraded to L{lvl}"),
                                        false,
                                        now,
                                    );
                                    dirty = true;
                                }
                                Err(e) => app.set_status(format!("{e}"), true, now),
                            }
                        }
                    } else {
                        widgets::disabled_button(
                            ui,
                            &format!("Upgrading… {}", super::fmt_duration_short(remaining)),
                        );
                    }
                } else {
                    let cost = habitat_upgrade_cost(habitat.level);
                    let affordable = app.zoo.coins >= cost;
                    let dur =
                        crate::game::habitat::habitat_upgrade_duration(habitat.level)
                            .num_seconds();
                    let label = format!(
                        "Upgrade → L{}  ·  {} coins  ·  ~{}",
                        habitat.level + 1,
                        super::fmt_int(cost),
                        super::fmt_duration_short(dur)
                    );
                    if widgets::cost_button(ui, &label, affordable).clicked() {
                        match app.zoo.start_habitat_upgrade(hid, now) {
                            Ok(ends_at) => {
                                let remaining = (ends_at - now).num_seconds().max(0);
                                app.set_status(
                                    format!("upgrade started — ready in {remaining}s"),
                                    false,
                                    now,
                                );
                                dirty = true;
                            }
                            Err(e) => app.set_status(format!("{e}"), true, now),
                        }
                    }
                }
            });
        });
        ui.add_space(theme::PAD_S);
        if widgets::primary_button(ui, "Collect all from this habitat").clicked() {
            let gained = app.zoo.collect_habitat(hid, now);
            let msg = match (gained.coins, gained.dna) {
                (0, 0) => "nothing at cap yet — animals must be full".to_string(),
                (c, 0) => format!("collected {} coins", super::fmt_int(c)),
                (0, d) => format!("collected {} DNA Helix", super::fmt_int(d)),
                (c, d) => format!(
                    "collected {} coins · {} DNA",
                    super::fmt_int(c),
                    super::fmt_int(d)
                ),
            };
            let is_error = gained.total() == 0;
            app.set_status(msg, is_error, now);
            if !is_error {
                dirty = true;
            }
        }
    });

    ui.add_space(theme::PAD_M);

    let animal_ids = habitat.animal_ids.clone();
    if animal_ids.is_empty() {
        ui.label(
            RichText::new("no animals yet · buy some in the Shop tab").color(theme::TEXT_DIM),
        );
        return dirty;
    }

    // -- Paginated grid of 256px animal icons --------------------------------
    const ICON: f32 = 256.0;
    const CELL_EXTRA: f32 = 64.0; // name line + level-up button below the icon
    let spacing = theme::PAD_M;

    let avail_w = ui.available_width();
    let cols = (((avail_w + spacing) / (ICON + spacing)).floor() as usize).max(1);

    let cell_h = ICON + CELL_EXTRA;
    // Reserve a strip at the bottom for the pagination controls.
    let avail_h = (ui.available_height() - 36.0).max(cell_h);
    let rows = (((avail_h + spacing) / (cell_h + spacing)).floor() as usize).max(1);

    let per_page = (cols * rows).max(1);
    let page_count = animal_ids.len().div_ceil(per_page);
    if app.habitat_page >= page_count {
        app.habitat_page = page_count.saturating_sub(1);
    }
    let start = app.habitat_page * per_page;
    let end = (start + per_page).min(animal_ids.len());
    let page_ids = animal_ids[start..end].to_vec();

    for chunk in page_ids.chunks(cols) {
        ui.horizontal(|ui| {
            for aid in chunk {
                dirty |= draw_animal_cell(ui, app, *aid, now, ICON);
                ui.add_space(spacing);
            }
        });
        ui.add_space(spacing);
    }

    // -- Pagination controls -------------------------------------------------
    if page_count > 1 {
        ui.horizontal(|ui| {
            if app.habitat_page == 0 {
                widgets::disabled_button(ui, "◀ Prev");
            } else if widgets::secondary_button(ui, "◀ Prev").clicked() {
                app.habitat_page -= 1;
            }

            ui.label(
                RichText::new(format!("Page {} / {}", app.habitat_page + 1, page_count))
                    .color(theme::TEXT_DIM)
                    .family(egui::FontFamily::Monospace),
            );

            if app.habitat_page + 1 >= page_count {
                widgets::disabled_button(ui, "Next ▶");
            } else if widgets::secondary_button(ui, "Next ▶").clicked() {
                app.habitat_page += 1;
            }
        });
    }

    dirty
}

/// One grid cell: a 256px clickable icon (collect-on-click), the animal's name
/// and accrual/timer line, and a level-up button (or MAX / breeding state).
fn draw_animal_cell(
    ui: &mut Ui,
    app: &mut EguiApp,
    aid: Uuid,
    now: DateTime<Utc>,
    icon_size: f32,
) -> bool {
    use crate::game::animal::{AnimalState, MAX_ANIMAL_LEVEL, animal_level_up_cost};

    let mut dirty = false;
    let Some(animal) = app.zoo.animals.get(&aid).cloned() else {
        return false;
    };
    let def = crate::game::species::get(animal.species);
    let breeding = matches!(animal.state, AnimalState::Breeding { .. });
    let stored = animal.stored_at(now);
    let cap = animal.storage_cap();

    ui.allocate_ui_with_layout(
        egui::Vec2::new(icon_size, icon_size + 64.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            // Icon — click to collect (unless busy breeding).
            let tex = app.icons.texture(ui.ctx(), animal.species);
            let resp = widgets::icon_image(ui, tex, def.display_name, icon_size);
            let hover = if breeding {
                format!("{} L{} · breeding", def.display_name, animal.level)
            } else {
                format!(
                    "{} L{} · {}/{} · click to collect",
                    def.display_name,
                    animal.level,
                    super::fmt_int(stored),
                    super::fmt_int(cap)
                )
            };
            let resp = resp.on_hover_text(hover);
            if resp.clicked() && !breeding {
                let g = app.zoo.collect_animal(aid, now);
                let msg = match (g.coins, g.dna) {
                    (0, 0) => "not at cap yet".to_string(),
                    (c, 0) => format!("collected {} coins", super::fmt_int(c)),
                    (0, d) => format!("collected {} DNA Helix", super::fmt_int(d)),
                    (c, d) => format!(
                        "collected {} coins · {} DNA",
                        super::fmt_int(c),
                        super::fmt_int(d)
                    ),
                };
                let is_error = g.total() == 0;
                app.set_status(msg, is_error, now);
                if !is_error {
                    dirty = true;
                }
            }

            // Name + level.
            ui.label(
                RichText::new(format!("{} L{}", def.display_name, animal.level))
                    .color(theme::TEXT_PRIMARY)
                    .strong(),
            );

            // Status / accrual line.
            if breeding {
                if let AnimalState::Breeding { ends_at, .. } = animal.state {
                    let remaining = ends_at.signed_duration_since(now).num_seconds();
                    ui.label(
                        RichText::new(super::fmt_duration_short(remaining))
                            .color(theme::TIMER)
                            .family(egui::FontFamily::Monospace),
                    );
                }
            } else {
                ui.label(
                    RichText::new(format!("{}/{}", super::fmt_int(stored), super::fmt_int(cap)))
                        .color(theme::TEXT_DIM)
                        .size(11.0)
                        .family(egui::FontFamily::Monospace),
                );
            }

            // Level-up button.
            if breeding {
                widgets::disabled_button(ui, "breeding");
            } else if animal.level >= MAX_ANIMAL_LEVEL {
                widgets::disabled_button(ui, "MAX");
            } else {
                let cost = animal_level_up_cost(def.purchase_cost, animal.level);
                let affordable = app.zoo.food >= cost;
                let label = format!("Lvl → {} · {} food", animal.level + 1, super::fmt_int(cost));
                if widgets::cost_button(ui, &label, affordable).clicked() {
                    match app.zoo.level_up_animal(aid, now) {
                        Ok(l) => {
                            app.set_status(format!("leveled to L{l}"), false, now);
                            dirty = true;
                        }
                        Err(e) => app.set_status(format!("{e}"), true, now),
                    }
                }
            }
        },
    );

    dirty
}
