//! The world scene: a flat soft-green ground plane with freely roaming critter
//! sprites (billboards + drop shadows). Pure drawing from `GameApp` state.

use chrono::{DateTime, Utc};
use macroquad::prelude::*;

use crate::app::GameApp;
use crate::catching;
use crate::game::avatar::{Facing, PlayerAvatar};
use crate::game::biome;
use crate::game::species::{self, IncomeKind};
use super::view::{self, Camera, CRITTER_H, PLANE_H, PLANE_W};

const BG: Color = color_u8!(14, 15, 18, 255);
// GROUND_GREEN replaced by per-tile biome colours in draw_scene.
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

    // --- Ground: per-tile biome colours with soft Voronoi blending --------
    // We draw only tiles that are (a) visible on screen and (b) within world
    // bounds. Tiles outside the world show the dark BG already cleared above.
    const BTILE: f32 = 128.0; // must match TILE_W for consistent alignment
    let (cam_tl, cam_br) = view::camera_world_rect(&cam, screen_width(), screen_height());
    let tx0 = (cam_tl.x / BTILE).floor() as i32;
    let tx1 = (cam_br.x / BTILE).ceil()  as i32;
    let ty0 = (cam_tl.y / BTILE).floor() as i32;
    let ty1 = (cam_br.y / BTILE).ceil()  as i32;
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let tile_cx = (tx as f32 + 0.5) * BTILE;
            let tile_cy = (ty as f32 + 0.5) * BTILE;
            if tile_cx < 0.0 || tile_cx > PLANE_W || tile_cy < 0.0 || tile_cy > PLANE_H {
                continue;
            }
            let color = biome::biome_color_at(vec2(tile_cx, tile_cy));
            let (pos, size) = view::tile_rect(tx, ty, BTILE, &cam);
            // +1 px overlap prevents seams between tiles.
            draw_rectangle(pos.x, pos.y, size.x + 1.0, size.y + 1.0, color);
        }
    }

    // --- Zoo plot: tinted floor + fence outline marking the home enclosure -
    draw_zoo_plot(&cam);

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

    // Wild animals render behind tame critters / avatars.
    draw_wild_animals(app);

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

// ── Zoo plot (home enclosure) ─────────────────────────────────────────────────

/// Draw the enclosed 9×9 home zoo: a subtle floor tint plus a fence outline so
/// the player can always see where home ends and the wilds begin.
fn draw_zoo_plot(cam: &Camera) {
    use crate::game::world_chunks::{zoo_center, zoo_half_extent};
    let c = zoo_center();
    let half = zoo_half_extent();
    let tl = view::world_to_screen(vec2(c.x - half, c.y - half), cam);
    let br = view::world_to_screen(vec2(c.x + half, c.y + half), cam);
    let (w, h) = (br.x - tl.x, br.y - tl.y);

    // Warm floor tint to read as a tended, owned plot.
    draw_rectangle(tl.x, tl.y, w, h, color_u8!(196, 178, 132, 40));

    // Fence: a bright double outline.
    let fence = color_u8!(120, 86, 54, 255);
    let thick = (3.0 * cam.zoom).max(2.0);
    draw_rectangle_lines(tl.x, tl.y, w, h, thick, fence);
    draw_rectangle_lines(
        tl.x + thick, tl.y + thick,
        w - thick * 2.0, h - thick * 2.0,
        (thick * 0.5).max(1.0),
        color_u8!(160, 120, 78, 200),
    );
}

// ── Wild animals + catch circle ───────────────────────────────────────────────

/// Draw wild animals visible in the camera frustum and, in catch mode, the
/// capture circle.  Only chunks overlapping the screen are queried — this is
/// the primary render-side optimisation from the chunk system.
fn draw_wild_animals(app: &mut GameApp) {
    let cam = app.camera;
    let (cam_tl, cam_br) = view::camera_world_rect(&cam, screen_width(), screen_height());
    let catch_active = app.catch_state.active;
    let catch_target = app.catch_state.target;
    let catch_fill   = app.catch_state.fill;

    // Collect (species, pos, vel, id) from visible chunks — owned copies so
    // the immutable borrow on app.world ends before we touch app.textures.
    let visible: Vec<(&'static str, macroquad::math::Vec2, macroquad::math::Vec2, uuid::Uuid, u32)> = {
        app.world
            .visible_animals(cam_tl, cam_br)
            .into_iter()
            .map(|a| (a.species, a.pos, a.vel, a.id, a.catches))
            .collect()
    };

    for (species, pos, vel, id, catches) in visible {
        let feet     = view::world_to_screen(pos, &cam);
        let sprite_h = CRITTER_H * cam.zoom;
        let sink     = sprite_h * FOOT_SINK;
        let bottom   = feet.y + sink;
        let top      = bottom - sprite_h;
        let flip_x   = vel.x > 0.0;

        // Shadow.
        draw_ellipse(feet.x, feet.y, sprite_h * 0.30, sprite_h * 0.10, 0.0, SHADOW);

        // Sprite drawn at full colour (no wild tint).
        let tex = app.textures.animal(species);
        match tex {
            Some(ref t) => {
                let aspect = if t.height() > 0.0 { t.width() / t.height() } else { 1.0 };
                let w = sprite_h * aspect;
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
            }
            None => {
                draw_circle(feet.x, bottom - sprite_h * 0.4, sprite_h * 0.3,
                            color_u8!(200, 200, 200, 220));
            }
        }

        // Catch-progress badge — shown only once this exact animal has been
        // caught at least once; never above unattempted animals (catches 0).
        if catches >= 1 {
            let required = species::captures_required(species);
            let label = format!("{catches}/{required}");
            let fs = (sprite_h * 0.20).clamp(12.0, 22.0);
            let dim = measure_text(&label, None, fs as u16, 1.0);
            draw_text(&label, feet.x - dim.width * 0.5, top - 4.0, fs, TEXT);
        }

        // Catch circle — shown when catch mode is active.
        if catch_active {
            let center  = catching::animal_screen_center(pos, &cam);
            let outer_r = crate::game::wild_animal::CATCH_SCREEN_RADIUS_BASE * cam.zoom;
            let ring_w  = (outer_r * 0.14).max(3.0);
            let inner_r = outer_r - ring_w;

            // Dim background ring.
            draw_ring_arc(center.x, center.y, inner_r, outer_r, 1.0,
                          color_u8!(255, 255, 255, 30));
            draw_circle_lines(center.x, center.y, outer_r, 1.2,
                              color_u8!(255, 255, 255, 60));

            // Progress arc for the targeted animal.
            if catch_target == Some(id) && catch_fill > 0.0 {
                draw_ring_arc(center.x, center.y, inner_r, outer_r, catch_fill,
                              color_u8!(180, 255, 80, 230));
                draw_circle_lines(center.x, center.y, outer_r, 1.5,
                                  color_u8!(200, 255, 100, 190));
            }
        }
    }
}

/// Draw a filled arc as a ring strip (inner_r → outer_r, clockwise from top).
/// Each segment is a quad built from two triangles.
fn draw_ring_arc(cx: f32, cy: f32, inner_r: f32, outer_r: f32, fill: f32, color: Color) {
    if fill <= 0.0 { return; }
    const N: usize = 48;
    let filled = ((fill * N as f32).ceil() as usize).min(N);
    let start  = -std::f32::consts::FRAC_PI_2;
    let center = vec2(cx, cy);
    for i in 0..filled {
        let t0 = i as f32 / N as f32;
        let t1 = ((i + 1) as f32 / N as f32).min(fill);
        let a0 = start + t0 * std::f32::consts::TAU;
        let a1 = start + t1 * std::f32::consts::TAU;
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();
        let oi = center + vec2(c0, s0) * inner_r;
        let oo = center + vec2(c0, s0) * outer_r;
        let ni = center + vec2(c1, s1) * inner_r;
        let no = center + vec2(c1, s1) * outer_r;
        draw_triangle(oo, no, ni, color);
        draw_triangle(oo, ni, oi, color);
    }
}

// ── HUD ───────────────────────────────────────────────────────────────────────

/// Height of the top letterbox panel the HUD text rests on.
const TOPBAR_H: f32 = 56.0;

/// HUD text overlay (coins, hints, status, error log). Draw on the screen,
/// never into a render target.
pub fn draw_hud(app: &mut GameApp) {
    // Letterbox panel behind the top HUD text so it stays legible over any
    // background. A faint lower edge gives it a defined border.
    draw_rectangle(0.0, 0.0, screen_width(), TOPBAR_H, color_u8!(0, 0, 0, 200));
    draw_rectangle(0.0, TOPBAR_H, screen_width(), 1.0, color_u8!(255, 255, 255, 30));

    draw_text(
        &format!(
            "coins {}   food {}   DNA {}",
            app.zoo.coins, app.zoo.food, app.zoo.dna_helix
        ),
        14.0, 24.0, 20.0, TEXT,
    );
    let hint = if app.catch_state.active {
        "C exit catch · hover a wild animal to catch it"
    } else {
        "1 Shop · 2 Breeding · 3 Settings · WASD move · C catch · scroll zoom"
    };
    draw_text(hint, 14.0, 46.0, 18.0, TEXT_DIM);

    if app.catch_state.active {
        draw_text(
            "CATCH MODE",
            screen_width() * 0.5 - 48.0, 28.0, 22.0,
            color_u8!(180, 255, 80, 230),
        );
    }

    // ── Bottom-left stack: errors (red) then status (amber) ──────────────
    let mut bottom_y = screen_height() - 14.0;

    if let Some((msg, _)) = &app.status {
        draw_text(msg, 14.0, bottom_y, 19.0, color_u8!(211, 165, 92, 255));
        bottom_y -= 22.0;
    }

    // Error log — most recent at the bottom, older lines above.
    for (msg, _) in app.errors.iter().rev() {
        draw_text(msg, 14.0, bottom_y, 18.0, color_u8!(255, 80, 80, 230));
        bottom_y -= 21.0;
    }

    draw_notifications(app);
}

// ── Right-edge notifications ────────────────────────────────────────────────────

const NOTIF_W: f32 = 250.0;
const NOTIF_H: f32 = 58.0;
const NOTIF_GAP: f32 = 10.0;

/// Draw the obtain/collect toast stack on the right edge: newest at top, each
/// fading in from the right, holding, then fading out.
fn draw_notifications(app: &mut GameApp) {
    use crate::app::{NotifIcon, NOTIF_FADE_IN, NOTIF_FADE_OUT, NOTIF_LIFETIME};

    let now = get_time();
    // Snapshot the fields we need so we can borrow `app.textures` mutably below.
    let toasts: Vec<(String, String, NotifIcon, f64)> = app
        .notifications
        .iter()
        .map(|n| (n.title.clone(), n.amount.clone(), n.icon, n.created_at))
        .collect();

    let mut y = TOPBAR_H + 16.0;
    // Newest on top.
    for (title, amount, icon, created_at) in toasts.into_iter().rev() {
        let age = now - created_at;
        // Fade-in then fade-out alpha envelope.
        let a_in = (age / NOTIF_FADE_IN).clamp(0.0, 1.0);
        let a_out = ((NOTIF_LIFETIME - age) / NOTIF_FADE_OUT).clamp(0.0, 1.0);
        let alpha = (a_in.min(a_out)) as f32;
        if alpha <= 0.0 {
            continue;
        }
        // Slide in from the right as it fades in.
        let slide = (1.0 - a_in as f32) * 28.0;
        let x = screen_width() - NOTIF_W - 16.0 + slide;

        let bg = Color::new(0.0, 0.0, 0.0, 0.82 * alpha);
        let border = Color::new(1.0, 1.0, 1.0, 0.16 * alpha);
        draw_rectangle(x, y, NOTIF_W, NOTIF_H, bg);
        draw_rectangle_lines(x, y, NOTIF_W, NOTIF_H, 2.0, border);

        // Icon on the left.
        let icon_box = NOTIF_H - 14.0;
        let icon_x = x + 8.0;
        let icon_y = y + 7.0;
        let tex = match icon {
            NotifIcon::Currency(id) => app.textures.icon(id),
            NotifIcon::Animal(id) => app.textures.animal(id),
        };
        match tex {
            Some(t) => {
                let aspect = if t.height() > 0.0 { t.width() / t.height() } else { 1.0 };
                let (mut w, mut h) = (icon_box * aspect, icon_box);
                if w > icon_box {
                    w = icon_box;
                    h = icon_box / aspect;
                }
                draw_texture_ex(
                    &t,
                    icon_x + (icon_box - w) * 0.5,
                    icon_y + (icon_box - h) * 0.5,
                    Color::new(1.0, 1.0, 1.0, alpha),
                    DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() },
                );
            }
            None => {
                draw_circle(
                    icon_x + icon_box * 0.5,
                    icon_y + icon_box * 0.5,
                    icon_box * 0.4,
                    Color::new(1.0, 0.82, 0.35, alpha),
                );
            }
        }

        // Title (top) + amount (below), to the right of the icon.
        let text_x = icon_x + icon_box + 12.0;
        let title_c = Color::new(0.90, 0.91, 0.92, alpha);
        let amount_c = Color::new(1.0, 0.82, 0.35, alpha);
        draw_text(&title, text_x, y + 26.0, 21.0, title_c);
        draw_text(&amount, text_x, y + 47.0, 19.0, amount_c);

        y += NOTIF_H + NOTIF_GAP;
    }
}
