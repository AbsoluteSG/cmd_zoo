//! Shared UI primitives + palette. The rounded-rect, fade, and easing helpers
//! used to live privately in `menus.rs`; they're centralized here so the HUD
//! (`world.rs`) and the menus draw with one consistent vocabulary and colour
//! set. Pure presentation — text + primitives only, drawn on the screen.

use macroquad::prelude::*;

use super::textures::Textures;

// ── Palette ───────────────────────────────────────────────────────────────────

/// Menu / panel surface.
pub const PANEL: Color = color_u8!(24, 27, 33, 250);
/// Panel outline.
pub const PANEL_EDGE: Color = color_u8!(64, 72, 86, 255);
/// Primary text.
pub const TEXT: Color = color_u8!(231, 233, 236, 255);
/// Dimmed / secondary text.
pub const TEXT_DIM: Color = color_u8!(150, 156, 166, 255);
/// Mint accent used for headings / highlights.
pub const ACCENT: Color = color_u8!(123, 207, 167, 255);

// Currency + status colours, shared by HUD chips, toasts, and particles.
pub const COIN_GOLD: Color = color_u8!(255, 210, 90, 255);
pub const DNA_PINK: Color = color_u8!(196, 120, 220, 255);
pub const FOOD_GREEN: Color = color_u8!(150, 210, 120, 255);
pub const STATUS_AMBER: Color = color_u8!(211, 165, 92, 255);
pub const ERROR_RED: Color = color_u8!(255, 90, 90, 255);

/// Translucent backing used behind floating HUD chips / pills.
const CHIP_BG: Color = color_u8!(0, 0, 0, 175);
const CHIP_EDGE: Color = color_u8!(255, 255, 255, 28);

// ── Primitives ──────────────────────────────────────────────────────────────────

/// Filled rounded rectangle (straight edges + corner discs).
pub fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    draw_rectangle(x + r, y, w - 2.0 * r, h, color);
    draw_rectangle(x, y + r, w, h - 2.0 * r, color);
    draw_circle(x + r, y + r, r, color);
    draw_circle(x + w - r, y + r, r, color);
    draw_circle(x + r, y + h - r, r, color);
    draw_circle(x + w - r, y + h - r, r, color);
}

/// Outline matching [`rrect`] (straight segments along each edge).
pub fn rrect_outline(x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    draw_line(x + r, y, x + w - r, y, 1.5, color);
    draw_line(x + r, y + h, x + w - r, y + h, 1.5, color);
    draw_line(x, y + r, x, y + h - r, 1.5, color);
    draw_line(x + w, y + r, x + w, y + h - r, 1.5, color);
}

/// Multiply a colour's alpha (for fade-in/out transforms).
pub fn fade(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a)
}

/// easeOutBack — overshoots slightly before settling, for a subtle pop.
pub fn ease_out_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    let u = t - 1.0;
    1.0 + c3 * u * u * u + c1 * u * u
}

/// A rounded translucent pill (filled + faint outline) at `alpha`. Used for the
/// status/error toasts; the fill/edge are the standard chip colours.
pub fn pill(x: f32, y: f32, w: f32, h: f32, alpha: f32) {
    rrect(x, y, w, h, h * 0.5, fade(CHIP_BG, alpha));
    rrect_outline(x, y, w, h, h * 0.5, fade(CHIP_EDGE, alpha));
}

/// Draw a currency chip — rounded backing with an icon (or coloured-dot
/// fallback) on the left and `value` text on the right — and return its total
/// width so callers can flow chips left to right.
///
/// `icon_id` is looked up in the icon table; a missing icon falls back to a
/// filled dot in `color`, mirroring the notification toast behaviour.
pub fn currency_chip(
    textures: &mut Textures,
    x: f32,
    y: f32,
    icon_id: &str,
    value: &str,
    color: Color,
) -> f32 {
    const H: f32 = 30.0;
    const PAD: f32 = 9.0;
    const ICON: f32 = 18.0;
    const GAP: f32 = 7.0;
    const FS: f32 = 18.0;

    let dim = measure_text(value, None, FS as u16, 1.0);
    let w = PAD + ICON + GAP + dim.width + PAD;

    rrect(x, y, w, H, H * 0.5, CHIP_BG);
    rrect_outline(x, y, w, H, H * 0.5, CHIP_EDGE);

    // Icon (or fallback dot), vertically centred.
    let icon_cx = x + PAD + ICON * 0.5;
    let icon_cy = y + H * 0.5;
    match textures.icon(icon_id) {
        Some(t) => {
            let aspect = if t.height() > 0.0 { t.width() / t.height() } else { 1.0 };
            let (mut iw, mut ih) = (ICON * aspect, ICON);
            if iw > ICON {
                iw = ICON;
                ih = ICON / aspect;
            }
            draw_texture_ex(
                &t,
                icon_cx - iw * 0.5,
                icon_cy - ih * 0.5,
                WHITE,
                DrawTextureParams { dest_size: Some(vec2(iw, ih)), ..Default::default() },
            );
        }
        None => draw_circle(icon_cx, icon_cy, ICON * 0.45, color),
    }

    // Value text, baseline roughly centred.
    let text_x = x + PAD + ICON + GAP;
    draw_text(value, text_x, y + H * 0.5 + dim.offset_y * 0.35, FS, TEXT);

    w
}
