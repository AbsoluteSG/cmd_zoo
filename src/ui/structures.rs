//! Structures tab — list of owned structures with per-row collect / upgrade,
//! plus a "Collect all" action at the top.

use chrono::{DateTime, Utc};
use egui::{RichText, Ui};

use super::{theme, widgets};
use crate::app::EguiApp;
use crate::game::structure::{MAX_STRUCTURE_LEVEL, STRUCTURE_TOTAL_CAP, structure_upgrade_cost};

pub fn draw(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    let mut want_save = false;

    egui::ScrollArea::vertical().show(ui, |ui| {
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::section_heading(
                    ui,
                    "Structures",
                    Some(&format!(
                        "{}/{} owned · {} food",
                        app.zoo.structures.len(),
                        STRUCTURE_TOTAL_CAP,
                        super::fmt_int(app.zoo.food)
                    )),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary_button(ui, "Collect all").clicked() {
                        let g = app.zoo.collect_all_structures(now);
                        if g > 0 {
                            app.set_status(format!("collected {} food", super::fmt_int(g)), false, now);
                            want_save = true;
                        } else {
                            app.set_status("nothing to collect", false, now);
                        }
                    }
                });
            });
        });

        ui.add_space(theme::PAD_M);

        if app.zoo.structures.is_empty() {
            ui.label(
                RichText::new("no structures · buy one in the Shop tab")
                    .color(theme::TEXT_DIM),
            );
            return;
        }

        let structures = app.zoo.structures.clone();
        for s in structures {
            let def = crate::game::structure_kind::get(s.kind);
            let stored = s.stored_at(now);
            let cap = s.food_cap();
            let frac = if cap == 0 {
                0.0
            } else {
                stored as f32 / cap as f32
            };

            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(def.display_name)
                            .color(theme::TEXT_PRIMARY)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("L{}", s.level))
                            .color(theme::TEXT_DIM)
                            .size(11.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if s.level >= MAX_STRUCTURE_LEVEL {
                            widgets::disabled_button(ui, "MAX");
                        } else {
                            let cost = structure_upgrade_cost(s.level);
                            let affordable = app.zoo.coins >= cost;
                            let label = format!(
                                "Upgrade → L{}  ·  {} coins",
                                s.level + 1,
                                super::fmt_int(cost)
                            );
                            if widgets::cost_button(ui, &label, affordable).clicked() {
                                match app.zoo.upgrade_structure(s.id, now) {
                                    Ok(l) => {
                                        app.set_status(
                                            format!("upgraded to L{l}"),
                                            false,
                                            now,
                                        );
                                        want_save = true;
                                    }
                                    Err(e) => app.set_status(format!("{e}"), true, now),
                                }
                            }
                        }
                    });
                });
                widgets::progress_bar(ui, frac, ui.available_width(), theme::ACCENT);
                ui.label(
                    RichText::new(format!(
                        "{} / {} food · {:.2}/s",
                        super::fmt_int(stored),
                        super::fmt_int(cap),
                        s.food_rate()
                    ))
                    .color(theme::TEXT_DIM)
                    .size(11.0)
                    .family(egui::FontFamily::Monospace),
                );
            });
            ui.add_space(theme::PAD_S);
        }
    });

    if want_save {
        app.save_under_lock(now);
    }
}
