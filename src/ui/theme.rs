//! Dark monochrome theme with a single mint accent.
//!
//! All UI code should pull colors/spacing from this module rather than baking
//! hex values inline so the look-and-feel stays consistent and easy to retune.

use egui::{Color32, FontFamily, FontId, Rounding, Stroke, Style, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x0e, 0x0f, 0x12);
pub const SURFACE: Color32 = Color32::from_rgb(0x16, 0x18, 0x1d);
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x1e, 0x21, 0x28);
pub const SURFACE_ACTIVE: Color32 = Color32::from_rgb(0x24, 0x28, 0x30);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x26, 0x2a, 0x32);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xe6, 0xe8, 0xeb);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x99);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x55, 0x5a, 0x66);
pub const ACCENT: Color32 = Color32::from_rgb(0x7b, 0xcf, 0xa7);
pub const ACCENT_MUTED: Color32 = Color32::from_rgb(0x4a, 0x82, 0x68);
pub const WARNING: Color32 = Color32::from_rgb(0xd3, 0xa5, 0x5c);
pub const ERROR: Color32 = Color32::from_rgb(0xd9, 0x6a, 0x6a);

/// Bright orange — reserved for live timer text (gestation countdowns, etc.)
/// so timers stand out from the normal "info-amber" of WARNING.
pub const TIMER: Color32 = Color32::from_rgb(0xe8, 0x9b, 0x4a);

/// Pink — used to mark "the specific thing in play", e.g. the names of
/// animals currently mid-breeding and the first-pick highlight in the
/// pairing flow.
pub const SPECIAL: Color32 = Color32::from_rgb(0xe8, 0x9b, 0xd0);

/// Constructive non-default action (the green KEEP button in the breeding
/// picker). Distinct from `ACCENT` so the primary BREED button stays the most
/// visually prominent action when both buttons are visible.
pub const CONFIRM: Color32 = Color32::from_rgb(0x8a, 0xd9, 0x77);

/// Blue/purple — primary commit action (BREED button). Stands apart from
/// ACCENT so a row of REMOVE/KEEP/BREED reads as three distinct affordances.
pub const PRIMARY: Color32 = Color32::from_rgb(0x8a, 0x95, 0xe6);

pub const RADIUS: f32 = 6.0;
pub const PAD_S: f32 = 8.0;
pub const PAD_M: f32 = 12.0;
pub const PAD_L: f32 = 16.0;
pub const PAD_XL: f32 = 24.0;

/// Apply the canonical theme to an egui `Context`. Call once during app construction.
pub fn install(ctx: &egui::Context) {
    let mut style = Style::default();
    let mut visuals = Visuals::dark();

    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = SURFACE;
    visuals.code_bg_color = SURFACE;

    visuals.window_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.window_rounding = Rounding::same(RADIUS);
    visuals.menu_rounding = Rounding::same(RADIUS);

    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.weak_bg_fill = SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.noninteractive.rounding = Rounding::same(RADIUS);

    visuals.widgets.inactive.bg_fill = SURFACE;
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.rounding = Rounding::same(RADIUS);

    visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
    visuals.widgets.hovered.weak_bg_fill = SURFACE_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.hovered.rounding = Rounding::same(RADIUS);

    visuals.widgets.active.bg_fill = SURFACE_ACTIVE;
    visuals.widgets.active.weak_bg_fill = SURFACE_ACTIVE;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_MUTED);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.active.rounding = Rounding::same(RADIUS);

    visuals.widgets.open.bg_fill = SURFACE_ACTIVE;
    visuals.widgets.open.weak_bg_fill = SURFACE_ACTIVE;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    visuals.selection.bg_fill = ACCENT_MUTED;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;

    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(PAD_S, PAD_S);
    style.spacing.button_padding = egui::vec2(PAD_M, PAD_S);
    style.spacing.window_margin = egui::Margin::same(PAD_M);
    style.spacing.indent = PAD_L;

    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::new(20.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        FontId::new(12.5, FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );

    ctx.set_style(style);
}

/// Convenience: a labelled value pair displayed inline (e.g. "coins  1,234").
pub fn dim_label(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(TEXT_DIM).size(11.0)
}

pub fn heading_label(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .color(TEXT_PRIMARY)
        .size(15.0)
        .strong()
}

pub fn accent_label(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(ACCENT).strong()
}

pub fn numeric_label(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .color(TEXT_PRIMARY)
        .family(FontFamily::Monospace)
}
