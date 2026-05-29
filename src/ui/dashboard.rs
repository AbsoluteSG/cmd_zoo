//! Dashboard tab — two summary cards (habitats + structures) and a
//! breeding/pending strip. Click-through to drill into a specific tab.

use chrono::{DateTime, Utc};
use egui::{RichText, Ui};

use super::{theme, widgets};
use crate::app::{EguiApp, Tab};

pub fn draw(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.columns(2, |cols| {
            draw_habitats_summary(&mut cols[0], app, now);
            draw_structures_summary(&mut cols[1], app, now);
        });
        ui.add_space(theme::PAD_L);
        draw_breeding_strip(ui, app, now);
    });
}

fn draw_habitats_summary(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    widgets::card(ui, |ui| {
        widgets::section_heading(
            ui,
            "Habitats",
            Some(&format!("{} owned", app.zoo.habitats.len())),
        );
        ui.add_space(theme::PAD_S);

        if app.zoo.habitats.is_empty() {
            ui.label(RichText::new("no habitats yet").color(theme::TEXT_DIM));
            return;
        }

        for habitat in app.zoo.habitats.clone().iter() {
            let stored: u64 = habitat
                .animal_ids
                .iter()
                .filter_map(|id| app.zoo.animals.get(id))
                .map(|a| a.stored_at(now))
                .sum();
            let cap: u64 = habitat
                .animal_ids
                .iter()
                .filter_map(|id| app.zoo.animals.get(id))
                .map(|a| a.storage_cap())
                .sum();
            let fraction = if cap == 0 {
                0.0
            } else {
                stored as f32 / cap as f32
            };

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(habitat.theme.name())
                        .color(theme::TEXT_PRIMARY)
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("L{}", habitat.level))
                        .color(theme::TEXT_DIM)
                        .size(11.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{}/{}",
                            habitat.animal_ids.len(),
                            habitat.capacity()
                        ))
                        .color(theme::TEXT_DIM)
                        .family(egui::FontFamily::Monospace),
                    );
                });
            });
            widgets::progress_bar(ui, fraction, ui.available_width(), theme::ACCENT);
            ui.label(
                RichText::new(format!("{} / {} coins", super::fmt_int(stored), super::fmt_int(cap)))
                    .color(theme::TEXT_DIM)
                    .size(11.0)
                    .family(egui::FontFamily::Monospace),
            );
            ui.add_space(theme::PAD_S);
        }

        ui.add_space(theme::PAD_S);
        if widgets::primary_button(ui, "Open habitats →").clicked() {
            app.tab = Tab::Habitats;
        }
    });
}

fn draw_structures_summary(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    widgets::card(ui, |ui| {
        widgets::section_heading(
            ui,
            "Structures",
            Some(&format!("{} owned", app.zoo.structures.len())),
        );
        ui.add_space(theme::PAD_S);

        if app.zoo.structures.is_empty() {
            ui.label(RichText::new("no structures yet").color(theme::TEXT_DIM));
            return;
        }

        for s in app.zoo.structures.clone().iter() {
            let def = crate::game::structure_kind::get(s.kind);
            let stored = s.stored_at(now);
            let cap = s.food_cap();
            let fraction = if cap == 0 {
                0.0
            } else {
                stored as f32 / cap as f32
            };

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
                    ui.label(
                        RichText::new(format!("{:.1}/s", s.food_rate()))
                            .color(theme::TEXT_DIM)
                            .family(egui::FontFamily::Monospace),
                    );
                });
            });
            widgets::progress_bar(ui, fraction, ui.available_width(), theme::ACCENT);
            ui.label(
                RichText::new(format!("{} / {} food", super::fmt_int(stored), super::fmt_int(cap)))
                    .color(theme::TEXT_DIM)
                    .size(11.0)
                    .family(egui::FontFamily::Monospace),
            );
            ui.add_space(theme::PAD_S);
        }

        ui.add_space(theme::PAD_S);
        if widgets::primary_button(ui, "Open structures →").clicked() {
            app.tab = Tab::Structures;
        }
    });
}

fn draw_breeding_strip(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    widgets::card(ui, |ui| {
        widgets::section_heading(ui, "Breeding", None);
        ui.add_space(theme::PAD_S);

        let gestations = app.active_gestations();
        // Count finished gestations (timer elapsed, waiting for the player to
        // click the ready nest card to redeem).
        let ready = gestations
            .iter()
            .filter(|(_, _, ends_at)| *ends_at <= now)
            .count();

        if gestations.is_empty() {
            ui.label(
                RichText::new("no active gestations · pair two animals in the Breeding tab")
                    .color(theme::TEXT_DIM),
            );
        } else {
            for (a, b, ends_at) in gestations.iter().take(4) {
                let remaining = (ends_at.signed_duration_since(now)).num_seconds().max(0);
                let def_a = crate::game::species::get(a.species);
                let def_b = crate::game::species::get(b.species);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} + {} (L{}+L{})",
                            def_a.display_name, def_b.display_name, a.level, b.level
                        ))
                        .color(theme::TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, color) = if remaining <= 0 {
                            ("READY".to_string(), theme::CONFIRM)
                        } else {
                            (super::fmt_duration_short(remaining), theme::ACCENT)
                        };
                        ui.label(
                            RichText::new(label)
                                .color(color)
                                .family(egui::FontFamily::Monospace)
                                .strong(),
                        );
                    });
                });
            }
            if ready > 0 {
                ui.label(
                    RichText::new(format!("{ready} ready to redeem — click a nest"))
                        .color(theme::CONFIRM),
                );
            }
        }

        ui.add_space(theme::PAD_S);
        if widgets::primary_button(ui, "Open breeding →").clicked() {
            app.tab = Tab::Breeding;
        }
    });
}
