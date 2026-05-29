//! Settings tab — player rename and the danger-zone reset.

use chrono::{DateTime, Utc};
use egui::{RichText, Ui};

use super::{theme, widgets};
use crate::app::EguiApp;
use crate::game::player::MAX_PLAYER_NAME_LEN;

pub fn draw(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    let mut want_save = false;

    widgets::card(ui, |ui| {
        widgets::section_heading(ui, "Player", None);
        ui.add_space(theme::PAD_S);
        widgets::label_value_row(ui, "id", &app.zoo.player.id.to_string());
        widgets::label_value_row(ui, "current name", &app.zoo.player.name);
        ui.add_space(theme::PAD_M);
        ui.label(
            RichText::new(format!(
                "Rename (max {MAX_PLAYER_NAME_LEN} chars; letters, digits, space, _, -)"
            ))
            .color(theme::TEXT_DIM)
            .size(11.0),
        );
        ui.add(
            egui::TextEdit::singleline(&mut app.rename_buffer)
                .desired_width(280.0)
                .hint_text("new name"),
        );
        // Constrain to the allowed character set each frame.
        app.rename_buffer = EguiApp::rename_filter(&app.rename_buffer);

        ui.add_space(theme::PAD_S);
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "Save name").clicked() {
                let new = app.rename_buffer.clone();
                app.zoo.player.rename(new);
                let name = app.zoo.player.name.clone();
                app.rename_buffer = name.clone();
                app.set_status(format!("renamed to '{name}'"), false, now);
                want_save = true;
            }
            if widgets::secondary_button(ui, "Reset to current").clicked() {
                app.rename_buffer = app.zoo.player.name.clone();
            }
        });
    });

    ui.add_space(theme::PAD_L);
    draw_danger_zone(ui, app, now);

    if want_save {
        app.save_under_lock(now);
    }
}

fn draw_danger_zone(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    widgets::card(ui, |ui| {
        ui.label(
            RichText::new("Danger zone")
                .color(theme::ERROR)
                .size(16.0)
                .strong(),
        );
        ui.add_space(theme::PAD_S);
        ui.label(
            RichText::new("Resetting wipes everything: animals, habitats, structures, coins, food, nests, and discovered recipes. Player id is regenerated. No undo.")
                .color(theme::TEXT_DIM)
                .size(11.0),
        );
        ui.add_space(theme::PAD_S);

        if !app.confirming_reset {
            if widgets::danger_button(ui, "Reset game").clicked() {
                app.confirming_reset = true;
                app.set_status("click 'Confirm reset' to wipe", false, now);
            }
        } else {
            ui.horizontal(|ui| {
                if widgets::danger_button(ui, "Confirm reset").clicked() {
                    app.reset_game(now);
                }
                if widgets::secondary_button(ui, "Cancel").clicked() {
                    app.confirming_reset = false;
                    app.set_status("reset cancelled", false, now);
                }
            });
        }
    });
}
