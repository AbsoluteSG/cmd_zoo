//! Share tab — friend codes for gifting and snapshot viewing. Renders QR
//! images via `share::render_qr_rgba` and caches the resulting texture in
//! `app.share_show` so we don't re-encode every frame.

use chrono::{DateTime, Utc};
use egui::{RichText, Ui};

use super::{theme, widgets};
use crate::app::{EguiApp, ShareMode, ShowCodeState};
use crate::game::species;
use crate::share::{self, GiftContents, Payload};

pub fn draw(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    let mut want_save = false;
    ui.horizontal(|ui| {
        sub_tab_button(ui, app, ShareMode::Menu, "Menu");
        sub_tab_button(ui, app, ShareMode::PickGift, "Send gift");
        sub_tab_button(ui, app, ShareMode::Claim, "Claim code");
    });
    ui.add_space(theme::PAD_M);

    egui::ScrollArea::vertical().show(ui, |ui| match app.share_mode {
        ShareMode::Menu => draw_menu(ui, app, now),
        ShareMode::PickGift => want_save |= draw_pick_gift(ui, app, now),
        ShareMode::ShowCode => draw_show_code(ui, app),
        ShareMode::Claim => want_save |= draw_claim(ui, app, now),
        ShareMode::ViewSnapshot => draw_view_snapshot(ui, app),
    });

    if want_save {
        app.save_under_lock(now);
    }
}

fn sub_tab_button(ui: &mut Ui, app: &mut EguiApp, mode: ShareMode, label: &str) {
    let selected = app.share_mode == mode;
    let text = RichText::new(label).color(if selected {
        theme::ACCENT
    } else {
        theme::TEXT_DIM
    });
    let btn = egui::Button::new(text).fill(if selected {
        theme::SURFACE_ACTIVE
    } else {
        theme::SURFACE
    });
    if ui.add(btn).clicked() {
        app.share_mode = mode;
    }
}

fn draw_menu(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    widgets::card(ui, |ui| {
        widgets::section_heading(ui, "Share your zoo", None);
        ui.add_space(theme::PAD_S);
        ui.label(
            RichText::new(
                "Generate a read-only snapshot code your friend can paste in to see your zoo.",
            )
            .color(theme::TEXT_DIM),
        );
        ui.add_space(theme::PAD_S);
        if widgets::primary_button(ui, "Generate snapshot code").clicked() {
            let payload = app.zoo.build_shared_snapshot(now);
            match share::encode(&Payload::Snapshot(payload)) {
                Ok(code) => {
                    let qr = make_qr_texture(ui.ctx(), &code, "snapshot");
                    app.share_show = Some(ShowCodeState {
                        code,
                        label: "Zoo snapshot — share with a friend".into(),
                        qr_texture: qr,
                    });
                    app.share_mode = ShareMode::ShowCode;
                }
                Err(e) => app.set_status(format!("encode failed: {e}"), true, now),
            }
        }
    });
}

fn draw_pick_gift(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) -> bool {
    let mut dirty = false;
    widgets::card(ui, |ui| {
        widgets::section_heading(
            ui,
            "Send a gift",
            Some("idle animals only · the animal leaves your zoo on send"),
        );
        ui.add_space(theme::PAD_S);
        let animals: Vec<_> = app.giftable_animals().iter().map(|a| (a.id, a.species, a.level)).collect();
        if animals.is_empty() {
            ui.label(RichText::new("no idle animals to gift").color(theme::TEXT_DIM));
            return;
        }
        for (aid, sp, lvl) in animals {
            let def = species::get(sp);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} L{}", def.display_name, lvl))
                        .color(theme::TEXT_PRIMARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary_button(ui, "Gift").clicked() {
                        match app.zoo.send_animal_gift(aid, now) {
                            Ok(gift) => {
                                let species_name = match &gift.contents {
                                    GiftContents::Animal { species_id, .. } => species::try_get(species_id)
                                        .map(|d| d.display_name)
                                        .unwrap_or("?"),
                                };
                                match share::encode(&Payload::Gift(gift)) {
                                    Ok(code) => {
                                        let qr = make_qr_texture(ui.ctx(), &code, "gift");
                                        app.share_show = Some(ShowCodeState {
                                            code,
                                            label: format!(
                                                "Gift — {species_name} (animal removed locally)"
                                            ),
                                            qr_texture: qr,
                                        });
                                        app.share_mode = ShareMode::ShowCode;
                                        dirty = true;
                                    }
                                    Err(e) => app.set_status(format!("encode failed: {e}"), true, now),
                                }
                            }
                            Err(e) => app.set_status(format!("{e}"), true, now),
                        }
                    }
                });
            });
            ui.add_space(4.0);
        }
    });
    dirty
}

fn draw_show_code(ui: &mut Ui, app: &mut EguiApp) {
    let Some(show) = app.share_show.as_ref() else {
        ui.label(RichText::new("no code generated").color(theme::TEXT_DIM));
        return;
    };
    widgets::card(ui, |ui| {
        ui.label(
            RichText::new(&show.label)
                .color(theme::TEXT_PRIMARY)
                .strong(),
        );
        ui.add_space(theme::PAD_S);
        ui.columns(2, |cols| {
            // QR on the left.
            if let Some(tex) = &show.qr_texture {
                let size = tex.size_vec2();
                let max = cols[0].available_width().min(320.0);
                let scale = (max / size.x).min(1.0);
                cols[0].image((tex.id(), size * scale));
            } else {
                cols[0].label(
                    RichText::new("(code too long for a QR — copy text)")
                        .color(theme::WARNING)
                        .italics(),
                );
            }

            // Code text + copy button on the right.
            cols[1].vertical(|ui| {
                let mut code = show.code.clone();
                ui.add(
                    egui::TextEdit::multiline(&mut code)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(8)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(theme::PAD_S);
                if widgets::primary_button(ui, "Copy to clipboard").clicked() {
                    ui.output_mut(|o| o.copied_text = show.code.clone());
                }
            });
        });
        ui.add_space(theme::PAD_S);
        if widgets::secondary_button(ui, "Back to menu").clicked() {
            app.share_mode = ShareMode::Menu;
        }
    });
}

fn draw_claim(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) -> bool {
    let mut dirty = false;
    widgets::card(ui, |ui| {
        widgets::section_heading(
            ui,
            "Claim a code",
            Some("paste a czoo1: code from a friend"),
        );
        ui.add_space(theme::PAD_S);
        ui.add(
            egui::TextEdit::multiline(&mut app.share_claim_buffer)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text("paste here"),
        );
        ui.add_space(theme::PAD_S);
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "Decode & claim").clicked() {
                let code = app.share_claim_buffer.trim().to_string();
                if code.is_empty() {
                    app.set_status("paste a czoo1: code first", true, now);
                } else {
                    match share::decode(&code) {
                        Ok(Payload::Gift(gift)) => match app.zoo.claim_gift(&gift, now) {
                            Ok(_) => {
                                let species_name = match &gift.contents {
                                    GiftContents::Animal { species_id, .. } => species::try_get(species_id)
                                        .map(|d| d.display_name)
                                        .unwrap_or("?"),
                                };
                                app.set_status(format!("claimed gift: {species_name}"), false, now);
                                app.share_claim_buffer.clear();
                                app.share_mode = ShareMode::Menu;
                                dirty = true;
                            }
                            Err(e) => app.set_status(format!("{e}"), true, now),
                        },
                        Ok(Payload::Snapshot(snap)) => {
                            app.share_snapshot_view = Some(snap);
                            app.share_mode = ShareMode::ViewSnapshot;
                            app.share_claim_buffer.clear();
                        }
                        Err(e) => app.set_status(format!("decode failed: {e}"), true, now),
                    }
                }
            }
            if widgets::secondary_button(ui, "Clear").clicked() {
                app.share_claim_buffer.clear();
            }
        });
    });
    dirty
}

fn draw_view_snapshot(ui: &mut Ui, app: &mut EguiApp) {
    // Clone the payload out so the closure below doesn't need an active borrow.
    let snap = match app.share_snapshot_view.clone() {
        Some(s) => s,
        None => {
            ui.label(RichText::new("no snapshot loaded").color(theme::TEXT_DIM));
            return;
        }
    };
    let mut back_clicked = false;
    widgets::card(ui, |ui| {
        widgets::section_heading(
            ui,
            &format!("{}'s zoo", snap.sender_name),
            Some(&snap.taken_at.format("%Y-%m-%d %H:%M UTC").to_string()),
        );
        ui.add_space(theme::PAD_S);
        widgets::label_value_row(ui, "coins", &super::fmt_int(snap.view.coins));
        widgets::label_value_row(ui, "food", &super::fmt_int(snap.view.food));
        widgets::label_value_row(ui, "habitats", &snap.view.habitat_count.to_string());
        widgets::label_value_row(ui, "structures", &snap.view.structure_count.to_string());
        widgets::label_value_row(ui, "animals", &snap.view.animal_count.to_string());

        ui.add_space(theme::PAD_M);
        widgets::section_heading(ui, "Species", None);
        for entry in &snap.view.species_tally {
            let name = species::try_get(&entry.species_id)
                .map(|d| d.display_name.to_string())
                .unwrap_or_else(|| entry.species_id.clone());
            let avg = if entry.count > 0 {
                entry.total_level as f32 / entry.count as f32
            } else {
                0.0
            };
            widgets::label_value_row(
                ui,
                &name,
                &format!("×{} · avg L{:.1}", entry.count, avg),
            );
        }
        ui.add_space(theme::PAD_M);
        if widgets::secondary_button(ui, "Back").clicked() {
            back_clicked = true;
        }
    });
    if back_clicked {
        app.share_mode = ShareMode::Menu;
        app.share_snapshot_view = None;
    }
}

fn make_qr_texture(
    ctx: &egui::Context,
    code: &str,
    name: &str,
) -> Option<egui::TextureHandle> {
    let (side, rgba) = share::render_qr_rgba(code, 4, 4).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied([side, side], &rgba);
    Some(ctx.load_texture(format!("qr_{name}"), image, egui::TextureOptions::NEAREST))
}
