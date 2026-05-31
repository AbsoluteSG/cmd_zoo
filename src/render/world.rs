//! The world scene: a flat soft-green ground plane with freely roaming critter
//! sprites (billboards + drop shadows). Pure drawing from `GameApp` state.

use chrono::{DateTime, Utc};
use macroquad::prelude::*;

use crate::app::GameApp;
use crate::game::avatar::{Facing, PlayerAvatar};
use crate::game::species::{self, IncomeKind};
use super::view::{self, Camera, CRITTER_H, PLANE_H, PLANE_W};

const BG: Color = color_u8!(14, 15, 18, 255);
const GROUND_GREEN: Color = color_u8!(128, 170, 124, 255);
const SHADOW: Color = color_u8!(0, 0, 0, 70);
const TEXT_DIM: Color = color_u8!(138, 143, 153, 255);
const TEXT: Color = color_u8!(230, 232, 235, 255);
const COIN_GOLD: Color = color_u8!(255, 210, 90, 255);
const DNA_PINK: Color = color_u8!(196, 120, 220, 255);

/// Fraction of sprite height the art is nudged down so its visible base sits
/// on the shadow (compensates for transparent padding at the bottom of the art).
const FOOT_SINK: f32 = 0.16;

/// Full world frame: scene then HUD. Used for the direct (no render-target)
/// path. When rendering into an offscreen target for post-processing, call
/// `draw_scene` into the target and `draw_hud` afterward on the screen — text
/// must NEVER be drawn into a render target (it corrupts the font atlas).
pub fn draw(app: &mut GameApp, now: DateTime<Utc>) {
    draw_scene(app, now);
    draw_hud(app);
}

/// The world scene — ground + critters. No text (render-target safe).
pub fn draw_scene(app: &mut GameApp, now: DateTime<Utc>) {
    clear_background(BG);
    let cam = app.camera;

    // --- Ground: one flat soft-green field over the whole plane -----------
    let origin = view::world_to_screen(vec2(0.0, 0.0), &cam);
    let far = view::world_to_screen(vec2(PLANE_W, PLANE_H), &cam);
    draw_rectangle(
        origin.x,
        origin.y,
        far.x - origin.x,
        far.y - origin.y,
        GROUND_GREEN,
    );

    // --- Critters + every session avatar, depth-sorted by screen-Y (feet) -
    // All avatars (host + visitors) join the same painter's-algorithm pass
    // so habitats and critters can occlude them correctly.
    enum Item {
        Critter(usize),
        Avatar(uuid::Uuid),
    }
    let mut order: Vec<(f32, Item)> = app
        .critters
        .iter()
        .enumerate()
        .map(|(i, c)| (c.pos.y, Item::Critter(i)))
        .collect();
    for (id, a) in app.session.avatars.iter() {
        order.push((a.pos.y, Item::Avatar(*id)));
    }
    order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    for (_, item) in order {
        match item {
            Item::Critter(i) => {
                let c = &app.critters[i];
                let (sp, pos, dir, aid, pop) = (c.species, c.pos, c.dir, c.animal_id, c.pop);
                let at_cap = app
                    .zoo
                    .animals
                    .get(&aid)
                    .map(|a| a.is_at_cap(now))
                    .unwrap_or(false);
                let tex = app.textures.animal(sp);
                let (icon_id, fallback) = income_icon(sp);
                let icon = app.textures.icon(icon_id);
                let scale = view::pop_scale(pop);
                draw_critter(pos, dir, tex.as_ref(), icon.as_ref(), fallback, at_cap, scale, &cam);
            }
            Item::Avatar(id) => {
                // Placeholder art: reuse the cursor icon until a real avatar
                // sprite lands; falls back to a stick figure if unavailable.
                let tex = app.textures.icon("cursor");
                if let Some(a) = app.session.avatars.get(&id) {
                    draw_avatar(a, tex.as_ref(), &cam);
                }
            }
        }
    }
}

/// Draw the local player avatar in the same projection as critters: feet on
/// the ground point, sprite billboard upright, soft drop shadow underneath.
fn draw_avatar(avatar: &PlayerAvatar, tex: Option<&Texture2D>, cam: &Camera) {
    let feet = view::world_to_screen(avatar.pos, cam);
    let base_h = CRITTER_H * cam.zoom;
    let sink = base_h * FOOT_SINK;
    let bottom = feet.y + sink;
    let top = bottom - base_h;
    draw_ellipse(feet.x, feet.y, base_h * 0.30, base_h * 0.10, 0.0, SHADOW);

    match tex {
        Some(t) => {
            let aspect = if t.height() > 0.0 {
                t.width() / t.height()
            } else {
                1.0
            };
            let w = base_h * aspect;
            draw_texture_ex(
                t,
                feet.x - w * 0.5,
                top,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(w, base_h)),
                    flip_x: matches!(avatar.facing, Facing::E),
                    ..Default::default()
                },
            );
        }
        None => {
            // Two-color stick figure so the avatar is always visible.
            let body = color_u8!(90, 130, 220, 255);
            let head = color_u8!(245, 220, 190, 255);
            draw_rectangle(feet.x - base_h * 0.16, top + base_h * 0.35, base_h * 0.32, base_h * 0.55, body);
            draw_circle(feet.x, top + base_h * 0.20, base_h * 0.18, head);
        }
    }
}

/// The income-currency icon id + a fallback color for `species`.
fn income_icon(species: &str) -> (&'static str, Color) {
    match species::try_get(species).map(|d| d.income_kind) {
        Some(IncomeKind::DnaHelix) => ("dna_helix", DNA_PINK),
        _ => ("coin", COIN_GOLD),
    }
}

fn draw_critter(
    pos: Vec2,
    dir: Vec2,
    tex: Option<&Texture2D>,
    icon: Option<&Texture2D>,
    fallback: Color,
    at_cap: bool,
    scale: f32,
    cam: &Camera,
) {
    // Feet land on the projected ground position; the sprite stands up from it.
    let feet = view::world_to_screen(pos, cam);
    let base_h = CRITTER_H * cam.zoom; // unscaled — keeps the shadow planted
    let sprite_h = base_h * scale; // pop tween grows the sprite about its base
    let sink = base_h * FOOT_SINK;
    let bottom = feet.y + sink;
    // Art faces left by default: mirror X when moving right.
    let flip_x = dir.x > 0.0;

    // Shadow sits on the ground point, just under the sprite's base.
    draw_ellipse(feet.x, feet.y, base_h * 0.30, base_h * 0.10, 0.0, SHADOW);

    let sprite_top = match tex {
        Some(t) => {
            let aspect = if t.height() > 0.0 { t.width() / t.height() } else { 1.0 };
            let w = sprite_h * aspect;
            let top = bottom - sprite_h;
            draw_texture_ex(
                t,
                feet.x - w * 0.5,
                top,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(w, sprite_h)),
                    flip_x,
                    ..Default::default()
                },
            );
            top
        }
        None => {
            draw_circle(feet.x, bottom - sprite_h * 0.4, sprite_h * 0.3, color_u8!(90, 150, 210, 255));
            bottom - sprite_h
        }
    };

    // Income currency icon, bobbing above the animal — shown only when full
    // (ready to collect).
    if at_cap {
        draw_income_icon(feet.x, sprite_top, icon, fallback, cam);
    }
}

/// Draw the income icon centered above `sprite_top`, bobbing on a sine tween.
fn draw_income_icon(cx: f32, sprite_top: f32, icon: Option<&Texture2D>, fallback: Color, cam: &Camera) {
    let icon_h = 46.0 * cam.zoom;
    let bob = (get_time() as f32 * 3.0).sin() * 6.0 * cam.zoom;
    let center_y = sprite_top - icon_h * 0.6 + bob;
    match icon {
        Some(t) => {
            let aspect = if t.height() > 0.0 { t.width() / t.height() } else { 1.0 };
            let iw = icon_h * aspect;
            draw_texture_ex(
                t,
                cx - iw * 0.5,
                center_y - icon_h * 0.5,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(iw, icon_h)),
                    ..Default::default()
                },
            );
        }
        None => {
            // Placeholder coin/DNA token until icon art is added.
            draw_circle(cx, center_y, icon_h * 0.45, fallback);
            draw_circle_lines(cx, center_y, icon_h * 0.45, 2.0, color_u8!(20, 20, 20, 180));
        }
    }
}

/// HUD text overlay (coins, hints, status). Draw on the screen, never into a
/// render target.
pub fn draw_hud(app: &GameApp) {
    draw_text(
        &format!(
            "coins {}   food {}   DNA {}",
            app.zoo.coins, app.zoo.food, app.zoo.dna_helix
        ),
        14.0,
        24.0,
        20.0,
        TEXT,
    );
    draw_text(
        "1 Shop · 2 Breeding · 3 Settings · WASD move · scroll zoom",
        14.0,
        46.0,
        18.0,
        TEXT_DIM,
    );
    if let Some((msg, _)) = &app.status {
        draw_text(
            msg,
            14.0,
            screen_height() - 18.0,
            20.0,
            color_u8!(211, 165, 92, 255),
        );
    }
}
