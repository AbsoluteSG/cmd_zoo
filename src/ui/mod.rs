//! Top-level egui entry point. Lays out the three persistent zones
//! (header strip, tab strip, central content) and dispatches the active
//! tab to its panel module.

pub mod images;
pub mod theme;
pub mod widgets;

mod breeding;
mod dashboard;
mod habitats;
mod settings;
mod share_panel;
mod shop;
mod structures;

use chrono::{DateTime, Utc};
use egui::{Color32, Frame, Margin, RichText, Rounding, Stroke};

use crate::app::{EguiApp, Tab};

pub fn draw(ctx: &egui::Context, app: &mut EguiApp, now: DateTime<Utc>) {
    handle_global_shortcuts(ctx, app);

    egui::TopBottomPanel::top("header_strip")
        .frame(header_frame())
        .show(ctx, |ui| draw_header(ui, app));

    egui::TopBottomPanel::top("tab_strip")
        .frame(tabs_frame())
        .show(ctx, |ui| draw_tabs(ui, app));

    egui::TopBottomPanel::bottom("status_strip")
        .frame(status_frame())
        .show(ctx, |ui| draw_status(ui, app, now));

    egui::CentralPanel::default()
        .frame(central_frame())
        .show(ctx, |ui| match app.tab {
            Tab::Dashboard => dashboard::draw(ui, app, now),
            Tab::Habitats => habitats::draw(ui, app, now),
            Tab::Structures => structures::draw(ui, app, now),
            Tab::Shop => shop::draw(ui, app, now),
            Tab::Breeding => breeding::draw(ui, app, now),
            Tab::Share => share_panel::draw(ui, app, now),
            Tab::Settings => settings::draw(ui, app, now),
        });
}

fn header_frame() -> Frame {
    Frame::none()
        .fill(theme::BG)
        .inner_margin(Margin::symmetric(theme::PAD_L, theme::PAD_M))
        .stroke(Stroke::NONE)
}

fn tabs_frame() -> Frame {
    Frame::none()
        .fill(theme::BG)
        .inner_margin(Margin {
            left: theme::PAD_L,
            right: theme::PAD_L,
            top: 0.0,
            bottom: 0.0,
        })
        .stroke(Stroke {
            width: 1.0,
            color: theme::BORDER_SUBTLE,
        })
}

fn central_frame() -> Frame {
    Frame::none()
        .fill(theme::BG)
        .inner_margin(Margin::same(theme::PAD_L))
}

fn status_frame() -> Frame {
    Frame::none()
        .fill(theme::BG)
        .inner_margin(Margin::symmetric(theme::PAD_L, theme::PAD_S))
        .stroke(Stroke {
            width: 1.0,
            color: theme::BORDER_SUBTLE,
        })
}

fn draw_header(ui: &mut egui::Ui, app: &EguiApp) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("cmd_zoo")
                .color(theme::ACCENT)
                .strong()
                .size(18.0),
        );
        ui.add_space(theme::PAD_L);
        ui.label(
            RichText::new(&app.zoo.player.name)
                .color(theme::TEXT_PRIMARY)
                .strong(),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Right-to-left order: rightmost first. dna sits to the right
            // of coins/food since it's the rarest, most-prized currency.
            dna_chip(ui, &fmt_int(app.zoo.dna_helix));
            ui.add_space(theme::PAD_M);
            chip(ui, "food", &fmt_int(app.zoo.food));
            ui.add_space(theme::PAD_M);
            chip(ui, "coins", &fmt_int(app.zoo.coins));
        });
    });
}

fn draw_tabs(ui: &mut egui::Ui, app: &mut EguiApp) {
    ui.horizontal(|ui| {
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let selected = app.tab == *tab;
            let label = format!("{}  {}", i + 1, tab.label());
            let resp = tab_button(ui, &label, selected);
            if resp.clicked() {
                app.tab = *tab;
            }
        }
    });
}

fn tab_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let color = if selected {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_DIM
    };
    let underline = if selected {
        theme::ACCENT
    } else {
        Color32::TRANSPARENT
    };
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(
            ui.fonts(|f| f.layout_no_wrap(label.into(), egui::FontId::proportional(13.0), color))
                .rect
                .width()
                + theme::PAD_L * 2.0,
            32.0,
        ),
        egui::Sense::click(),
    );

    // Hover background lift.
    let bg = if resp.hovered() && !selected {
        theme::SURFACE_HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, Rounding::ZERO, bg);

    // Label centered.
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(label.into(), egui::FontId::proportional(13.0), color)
    });
    let text_pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(text_pos, galley, color);

    // Accent underline for selection.
    let bar = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + theme::PAD_M, rect.max.y - 2.0),
        egui::pos2(rect.max.x - theme::PAD_M, rect.max.y),
    );
    ui.painter().rect_filled(bar, Rounding::ZERO, underline);

    resp
}

fn chip(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .color(theme::TEXT_DIM)
                .size(11.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(value)
                .color(theme::TEXT_PRIMARY)
                .family(egui::FontFamily::Monospace)
                .strong(),
        );
    });
}

/// DNA Helix chip — colored pink (`theme::SPECIAL`) so it reads as the
/// "rare, in-play" currency at a glance.
fn dna_chip(ui: &mut egui::Ui, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("DNA")
                .color(theme::SPECIAL)
                .size(11.0)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(value)
                .color(theme::SPECIAL)
                .family(egui::FontFamily::Monospace)
                .strong(),
        );
    });
}

fn draw_status(ui: &mut egui::Ui, app: &EguiApp, _now: DateTime<Utc>) {
    ui.horizontal(|ui| {
        match &app.status {
            Some(s) => {
                let color = if s.is_error { theme::ERROR } else { theme::ACCENT };
                ui.label(RichText::new("●").color(color).size(10.0));
                ui.label(RichText::new(&s.text).color(theme::TEXT_PRIMARY));
            }
            None => {
                ui.label(RichText::new("●").color(theme::TEXT_MUTED).size(10.0));
                ui.label(
                    RichText::new("idle")
                        .color(theme::TEXT_MUTED)
                        .size(11.0),
                );
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let gestations = app.active_gestations();
            let breeding = gestations.len();
            let ready = gestations
                .iter()
                .filter(|(_, _, ends_at)| *ends_at <= _now)
                .count();
            if breeding > 0 {
                let text = if ready > 0 {
                    format!("{breeding} breeding · {ready} READY")
                } else {
                    format!("{breeding} breeding")
                };
                let color = if ready > 0 { theme::CONFIRM } else { theme::TEXT_DIM };
                ui.label(
                    RichText::new(text)
                        .color(color)
                        .size(11.0)
                        .strong(),
                );
            }
        });
    });
}

fn handle_global_shortcuts(ctx: &egui::Context, app: &mut EguiApp) {
    ctx.input(|i| {
        // Don't steal numeric keys while a TextEdit has focus.
        let typing = i.focused;
        if typing {
            return;
        }
        for (i_idx, tab) in Tab::ALL.iter().enumerate() {
            let key = match i_idx {
                0 => egui::Key::Num1,
                1 => egui::Key::Num2,
                2 => egui::Key::Num3,
                3 => egui::Key::Num4,
                4 => egui::Key::Num5,
                5 => egui::Key::Num6,
                6 => egui::Key::Num7,
                _ => continue,
            };
            if i.key_pressed(key) {
                app.tab = *tab;
            }
        }
    });
}

// -- formatting helpers -----------------------------------------------------

pub fn fmt_int(n: u64) -> String {
    // Thousands separator with apostrophes (compact + script-neutral).
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

pub fn fmt_duration_short(seconds: i64) -> String {
    let s = seconds.max(0);
    if s >= 3600 {
        let h = s / 3600;
        let m = (s % 3600) / 60;
        format!("{h}h {m:02}m")
    } else if s >= 60 {
        let m = s / 60;
        let r = s % 60;
        format!("{m}m {r:02}s")
    } else {
        format!("{s}s")
    }
}
