//! Small reusable widgets shared across the panels: cards, progress bars,
//! key/value rows. Anything that's larger than a one-liner of egui code and
//! used in more than one place lives here.

use egui::{Color32, Frame, Margin, Response, RichText, Rounding, Stroke, Ui, Vec2};

use super::theme;

/// A surface-colored card with rounded corners and padding. Wrap a block of
/// widgets in one of these to give the panel sections their characteristic
/// "inset rectangle" look.
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::none()
        .fill(theme::SURFACE)
        .inner_margin(Margin::same(theme::PAD_M))
        .rounding(Rounding::same(theme::RADIUS))
        .stroke(Stroke {
            width: 1.0,
            color: theme::BORDER_SUBTLE,
        })
        .show(ui, |ui| add_contents(ui))
        .inner
}

/// A thin progress bar in the project's accent color.
pub fn progress_bar(ui: &mut Ui, fraction: f32, width: f32, color: Color32) {
    let height = 8.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, Rounding::same(4.0), theme::BORDER_SUBTLE);
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.0 {
        let fill = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.min.x + rect.width() * f, rect.max.y),
        );
        painter.rect_filled(fill, Rounding::same(4.0), color);
    }
}

/// A small accent-colored primary action button. Renders without a border;
/// hover lifts the surface tone.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let text = RichText::new(label).color(theme::ACCENT).strong();
    let btn = egui::Button::new(text).fill(theme::SURFACE);
    ui.add(btn)
}

/// A muted-text secondary button. Used for "Upgrade", "Collect", etc.
pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    let text = RichText::new(label).color(theme::TEXT_PRIMARY);
    let btn = egui::Button::new(text).fill(theme::SURFACE);
    ui.add(btn)
}

/// Same as `secondary_button` but visually inert (used when the action is
/// disallowed, e.g. "MAX level"). Doesn't return a clickable response.
pub fn disabled_button(ui: &mut Ui, label: &str) {
    let text = RichText::new(label).color(theme::TEXT_MUTED);
    let btn = egui::Button::new(text)
        .fill(theme::SURFACE)
        .sense(egui::Sense::hover());
    ui.add(btn);
}

/// Buy/upgrade button whose label color flips to red when `affordable == false`.
/// Still clickable (so the domain returns a clean error and we surface it as a
/// status message), but the visual makes "you can't afford this" obvious.
pub fn cost_button(ui: &mut Ui, label: &str, affordable: bool) -> Response {
    let color = if affordable {
        theme::ACCENT
    } else {
        theme::ERROR
    };
    let text = RichText::new(label).color(color).strong();
    let btn = egui::Button::new(text).fill(theme::SURFACE);
    ui.add(btn)
}

/// Like `cost_button` but for destructive actions (cancel breeding, reset
/// game). Always rendered in red.
pub fn danger_button(ui: &mut Ui, label: &str) -> Response {
    let text = RichText::new(label).color(theme::ERROR).strong();
    let btn = egui::Button::new(text).fill(theme::SURFACE);
    ui.add(btn)
}

/// Green "constructive non-default" action — used today for the KEEP button
/// in the breeding picker, which starts breeding but routes the offspring to
/// the holding pen instead of auto-placing. Visually distinct from
/// `primary_action_button` so a stack of REMOVE / KEEP / BREED reads as three.
pub fn confirm_button(ui: &mut Ui, label: &str) -> Response {
    let text = RichText::new(label).color(theme::CONFIRM).strong();
    let btn = egui::Button::new(text).fill(theme::SURFACE);
    ui.add(btn)
}

/// Blue/purple primary-commit button — used for BREED. Visually heavier than
/// the muted `primary_button` so it reads as the main action in a row.
pub fn primary_action_button(ui: &mut Ui, label: &str) -> Response {
    let text = RichText::new(label).color(theme::PRIMARY).strong();
    let btn = egui::Button::new(text).fill(theme::SURFACE);
    ui.add(btn)
}

/// Render an animal sprite at a fixed display size, falling back to the first
/// letter of `fallback_label` (the species display name) when no PNG is
/// bundled. Used inside selection lists, the breeding picker's parent/outcome
/// slots, the gestation rows, and the habitat animal grid.
///
/// `size` is the desired square side in points. Returns a click-sensing
/// `Response` so callers can treat the icon as a button (e.g. collect-on-click
/// in the habitat grid). Pass `"?"` for the cross-species mystery preview.
pub fn icon_image(
    ui: &mut Ui,
    texture: Option<egui::TextureHandle>,
    fallback_label: &str,
    size: f32,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
    match texture {
        Some(handle) => {
            egui::Image::new(&handle)
                .fit_to_exact_size(Vec2::splat(size))
                .rounding(Rounding::same(4.0))
                .paint_at(ui, rect);
        }
        None => {
            // Centre an initial inside a sized box so layout matches the image case.
            ui.painter()
                .rect_filled(rect, Rounding::same(4.0), theme::SURFACE_HOVER);
            let initial = fallback_label
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "?".to_string());
            let galley = ui.fonts(|f| {
                f.layout_no_wrap(
                    initial,
                    egui::FontId::proportional((size * 0.55).max(14.0)),
                    theme::TEXT_PRIMARY,
                )
            });
            let pos = rect.center() - galley.size() / 2.0;
            ui.painter().galley(pos, galley, theme::TEXT_PRIMARY);
        }
    }
    resp
}

/// `label  value` row with a dim left side and a monospace right side.
pub fn key_value(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).color(theme::TEXT_DIM).size(11.0));
        ui.label(
            RichText::new(value)
                .color(theme::TEXT_PRIMARY)
                .family(egui::FontFamily::Monospace),
        );
    });
}

/// A row with `label` on the left, value pushed to the right edge.
pub fn label_value_row(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).color(theme::TEXT_DIM).size(11.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .color(theme::TEXT_PRIMARY)
                    .family(egui::FontFamily::Monospace),
            );
        });
    });
}

/// Section heading with optional muted suffix (e.g. "Habitats  · 3 owned").
pub fn section_heading(ui: &mut Ui, title: &str, suffix: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .color(theme::TEXT_PRIMARY)
                .size(16.0)
                .strong(),
        );
        if let Some(s) = suffix {
            ui.label(RichText::new(format!(" · {s}")).color(theme::TEXT_DIM));
        }
    });
}
