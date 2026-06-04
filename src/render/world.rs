//! The world scene: a flat soft-green ground plane with freely roaming critter
//! sprites (billboards + drop shadows). Pure drawing from `GameApp` state.

use chrono::{DateTime, Utc};
use macroquad::prelude::*;

use crate::app::GameApp;
use crate::catching;
use crate::game::avatar::PlayerAvatar;
use uuid::Uuid;
use crate::game::biome;
use crate::game::rank;
use crate::game::species::{self, IncomeKind};
use super::ui;
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
    draw_hud(app, now);
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
            let color = biome::biome_tile_color(vec2(tile_cx, tile_cy), app.zoo.world_seed);
            let (pos, size) = view::tile_rect(tx, ty, BTILE, &cam);
            // +1 px overlap prevents seams between tiles.
            draw_rectangle(pos.x, pos.y, size.x + 1.0, size.y + 1.0, color);
        }
    }

    // --- Zoo plot: tinted floor + fence outline marking the home enclosure -
    draw_zoo_plot(&cam);

    // --- Breeding nests on the ground inside the plot ----------------------
    draw_nests(app, now);
    draw_food_structures(app, now);

    // --- Waypoint beacons: glowing beams on the ground ---------------------
    draw_waypoint_beams(app);

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
                draw_critter(pos, dir, tex.as_ref(), icon.as_ref(), fallback, at_cap, scale, WHITE, &cam);
            }
            Item::Avatar(id) => {
                // Procedural toon-ball avatar (no sprite asset needed).
                if let Some(a) = app.session.avatars.get(&id) {
                    draw_avatar(a, &cam);
                }
            }
        }
    }

    // Pixel particle bursts (income, captures, hits, births) — pure primitives,
    // so they're safe in this render-target pass and project in world space.
    app.particles.draw(&cam);

    // Throw-impact danger zones: a faint outer boundary ring marks the hit
    // radius; a filled red disc shrinks with the fuse to telegraph time-to-land.
    for z in &app.danger_zones {
        let c = view::world_to_screen(z.center, &cam);
        let r_out = z.radius * cam.zoom;
        let frac = (z.remaining / z.fuse).clamp(0.0, 1.0);
        draw_circle(c.x, c.y, r_out * frac, color_u8!(220, 40, 40, 70));
        draw_circle_lines(c.x, c.y, r_out, 2.5, color_u8!(255, 60, 60, 200));
    }
}

/// Biome-preview debug view: paints only the biome colour field across the
/// (far zoomed-out) visible area as coarse tiles — no critters, plot,
/// structures, beams, or HUD chrome. Recorded for clean biome-layout
/// showcases. The world-space tile size scales with zoom so the tile count
/// stays bounded no matter how far the camera pulls out.
pub fn draw_biome_debug(app: &GameApp) {
    clear_background(BG);
    let cam = app.camera;
    let seed = app.zoo.world_seed;

    // Aim for ~6 screen px per tile; clamps keep both the on-screen resolution
    // and the total tile count reasonable across the full zoom range.
    let btile = (6.0 / cam.zoom).clamp(128.0, 20_000.0);

    let (tl, br) = view::camera_world_rect(&cam, screen_width(), screen_height());
    let tx0 = (tl.x / btile).floor() as i32;
    let tx1 = (br.x / btile).ceil()  as i32;
    let ty0 = (tl.y / btile).floor() as i32;
    let ty1 = (br.y / btile).ceil()  as i32;
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let cx = (tx as f32 + 0.5) * btile;
            let cy = (ty as f32 + 0.5) * btile;
            if cx < 0.0 || cx > PLANE_W || cy < 0.0 || cy > PLANE_H {
                continue;
            }
            let color = biome::biome_tile_color(vec2(cx, cy), seed);
            let (pos, size) = view::tile_rect(tx, ty, btile, &cam);
            // +1 px overlap prevents seams between tiles.
            draw_rectangle(pos.x, pos.y, size.x + 1.0, size.y + 1.0, color);
        }
    }

    // Per-chunk flag markers: a small dot at each flagged chunk centre, tinted
    // by its headline flag. Only drawn when zoomed in enough that the chunk
    // count stays bounded (flags are rare, but iterating every chunk at full
    // zoom-out would be millions of lookups).
    {
        use crate::game::world_chunks::CHUNK_SIZE;
        let cx0 = (tl.x / CHUNK_SIZE).floor() as i32;
        let cx1 = (br.x / CHUNK_SIZE).ceil()  as i32;
        let cy0 = (tl.y / CHUNK_SIZE).floor() as i32;
        let cy1 = (br.y / CHUNK_SIZE).ceil()  as i32;
        let chunk_count = (cx1 - cx0 + 1) as i64 * (cy1 - cy0 + 1) as i64;
        if chunk_count <= 40_000 {
            for cy in cy0..=cy1 {
                for cx in cx0..=cx1 {
                    let f = app.world.chunk_flags((cx, cy));
                    if f.is_empty() {
                        continue;
                    }
                    // Headline colour: pick the rarest set flag for contrast.
                    let col = if f.meteor_site {
                        color_u8!(255, 120, 60, 255)
                    } else if f.exotic_merchant {
                        color_u8!(255, 215, 90, 255)
                    } else if f.ancient_ruins {
                        color_u8!(200, 180, 255, 255)
                    } else if f.albino_surge {
                        color_u8!(240, 240, 255, 255)
                    } else if f.dna_rich {
                        color_u8!(196, 120, 220, 255)
                    } else if f.dense_pack {
                        color_u8!(255, 90, 90, 255)
                    } else {
                        color_u8!(120, 235, 200, 255) // biome_agnostic_spawns
                    };
                    let world_c = vec2((cx as f32 + 0.5) * CHUNK_SIZE, (cy as f32 + 0.5) * CHUNK_SIZE);
                    let p = view::world_to_screen(world_c, &cam);
                    draw_circle(p.x, p.y, 3.0, col);
                }
            }
        }
    }

    // Minimal HUD — drawn straight on the screen (no render target here, so
    // text is safe).
    let info = format!("BIOME DEBUG    seed {seed:#018x}    zoom {:.4}", cam.zoom);
    text_shadow(&info, 16.0, 28.0, 20.0, TEXT);
    text_shadow(
        "R reseed · wheel zoom · WASD pan · F3 exit",
        16.0, 52.0, 18.0, TEXT_DIM,
    );
}

/// Screen-space red vignette overlay for a venom hit. `intensity` (0–1) scales
/// the alpha. Cheap approximation: a few inset translucent-red border bands,
/// strongest at the screen edge and fading inward. Pure primitives, drawn on
/// the screen (never into a render target).
pub fn draw_red_vignette(intensity: f32) {
    let (w, h) = (screen_width(), screen_height());
    const BANDS: usize = 6;
    let reach = w.min(h) * 0.28;
    for i in 0..BANDS {
        let t = i as f32 / BANDS as f32; // 0 at the edge → inward
        let inset = t * reach;
        let alpha = (1.0 - t) * 0.5 * intensity.clamp(0.0, 1.0);
        let col = Color::new(0.75, 0.05, 0.08, alpha);
        draw_rectangle_lines(inset, inset, w - inset * 2.0, h - inset * 2.0, 60.0, col);
    }
}

// ── Procedural toon-ball avatar ────────────────────────────────────────────────
//
// The player is a stylised cel-shaded sphere drawn purely from primitives (no
// sprite). The 3-D illusion comes from stacking concentric, slightly-offset
// ellipses for the quantised toon bands (deep-shadow → shadow → midtone →
// highlight) plus a small soft specular dot, a dark outline, and momentum-driven
// bob / squash-stretch / lean read from the avatar's smoothed `viz` state.

/// Outline + spec endpoints. The body tones are derived per-player from a base
/// hue so each player reads as a distinct soft-rendered marble.
const BALL_OUTLINE: Color = color_u8!(14, 16, 30, 255);
const BALL_SPEC: Color = color_u8!(250, 253, 255, 255);

/// Rotate a screen-space vector by `ang` radians (screen y points down).
fn rot(v: Vec2, ang: f32) -> Vec2 {
    let (s, c) = ang.sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Linear blend between two colours.
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// Scale a colour's RGB toward black (multiplicative shade).
fn scale_rgb(c: Color, m: f32) -> Color {
    Color::new((c.r * m).min(1.0), (c.g * m).min(1.0), (c.b * m).min(1.0), c.a)
}

/// Deterministic, pleasant base colour for a player, hashed from their UUID so
/// every client paints the same player the same hue with no extra sync. A nil
/// id (single-player avatar) maps to a calm blue marble.
fn player_color(id: Uuid) -> Color {
    if id.is_nil() {
        return macroquad::color::hsl_to_rgb(0.58, 0.62, 0.58);
    }
    // FNV-1a over the id bytes → stable hue.
    let mut h: u32 = 2166136261;
    for &x in id.as_bytes() {
        h = (h ^ x as u32).wrapping_mul(16777619);
    }
    macroquad::color::hsl_to_rgb((h % 360) as f32 / 360.0, 0.60, 0.58)
}

/// Draw one soft-rendered ball at screen `center`, radius `r`, with squash
/// factors `(sx, sy)`, `lean` (radians), an `outline` ring width, a screen-space
/// `light` direction the shading shifts toward, and a per-player `base` colour.
/// `alpha` fades the whole thing (used for dash after-images).
///
/// The body is built from many finely-stepped translucent ellipses that ramp
/// from a shaded edge to a bright core nudged toward the light, so the colour
/// blends smoothly into a soft round-render — no hard cel bands or "cone".
#[allow(clippy::too_many_arguments)]
fn draw_toon_ball(center: Vec2, r: f32, sx: f32, sy: f32, lean: f32, outline: f32, light: Vec2, base: Color, alpha: f32) {
    let l = light.normalize_or(vec2(0.0, -1.0));
    let deg = lean.to_degrees();

    // Tone ramp derived from the base hue: deep shade → base → bright tint.
    let deep = scale_rgb(base, 0.42);
    let high = mix(base, WHITE, 0.55);

    let ell = |cx: f32, cy: f32, rx: f32, ry: f32, col: Color, a: f32| {
        draw_ellipse(cx, cy, rx, ry, deg, ui::fade(col, alpha * a));
    };

    // Outline: a dark ellipse slightly larger than the body, behind the shading.
    ell(center.x, center.y, (r + outline) * sx, (r + outline) * sy, BALL_OUTLINE, 1.0);

    // Soft directional gradient: edge (full size, shaded) → small bright core
    // eased toward the light. Many steps make the falloff smooth.
    let steps = 22;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32; // 0 edge → 1 core
        let scale = 1.0 - 0.60 * t;
        let off = 0.42 * t * t; // ease the core toward the light
        let o = rot(l * (off * r), lean);
        ell(center.x + o.x * sx, center.y + o.y * sy, r * scale * sx, r * scale * sy, mix(deep, high, t), 1.0);
    }

    // Reflected rim light: a faint lighter pool hugging the shaded side opposite
    // the light — the ambient bounce that sells a round, soft-rendered look.
    let ro = rot(l * (-0.66 * r), lean);
    ell(center.x + ro.x * sx, center.y + ro.y * sy, r * 0.52 * sx, r * 0.44 * sy, mix(base, high, 0.55), 0.16);

    // Highlight "eye": a soft glow + glint that slides in straight lines along
    // the input vector (no orbiting around the rim, no body-lean rotation) so it
    // reads like a pupil looking where the player moves. The offset is linear in
    // the light direction and stays near the centre so it never rides the edge.
    let look = l * (0.30 * r);
    let hc = vec2(center.x + look.x * sx, center.y + look.y * sy);
    let blob = |dx: f32, dy: f32, rx: f32, ry: f32, col: Color, a: f32| {
        ell(hc.x + dx * r * sx, hc.y + dy * r * sy, r * rx * sx, r * ry * sy, col, a);
    };
    blob(0.0, 0.0, 0.30, 0.30, high, 0.52);
    // Tight glossy glint at the pupil centre.
    blob(0.0, 0.0, 0.15, 0.15, BALL_SPEC, 0.9);
}

/// Draw the player avatar as a procedural toon-shaded ball with a planted drop
/// shadow, momentum bob, squash/stretch and lean. Works for the local player
/// and every session avatar (each carries its own smoothed `viz` state).
fn draw_avatar(avatar: &PlayerAvatar, cam: &Camera) {
    let base_h = CRITTER_H * cam.zoom;
    let r = base_h * 0.30;
    let outline = (2.5 * cam.zoom).max(1.5);
    let viz = &avatar.viz;
    let base = player_color(avatar.player_id);

    // Dash after-images: faded cool ghosts of the ball at past positions.
    for img in &avatar.afterimages {
        let frac = (img.life / img.max_life).clamp(0.0, 1.0);
        let g = view::world_to_screen(img.pos, cam);
        let gc = vec2(g.x, g.y - r * 0.95);
        draw_toon_ball(gc, r, 1.0, 1.0, viz.lean, outline, viz.light_dir, base, 0.45 * frac);
    }

    let feet = view::world_to_screen(avatar.pos, cam);

    // Bob: a brisk walk bounce blended with slow idle breathing.
    let bob = viz.bob_phase.sin();
    let breathe = viz.breathe_phase.sin();
    let walk_amp = 3.5 * cam.zoom * viz.bob_amp;
    let idle_amp = 0.9 * cam.zoom * (1.0 - viz.bob_amp);
    let bob_off = -(bob * walk_amp) - (breathe * idle_amp);

    // Squash/stretch (volume-preserving): stretch tall at the top of the bounce,
    // squash flat at the bottom. Very subtle.
    let s = bob * (0.06 * viz.bob_amp) + breathe * (0.02 * (1.0 - viz.bob_amp));
    let sy = 1.0 + s;
    let sx = 1.0 / sy;

    // Drop shadow stays planted at the feet, shrinking as the ball rises.
    let lift = (bob * 0.5 + 0.5) * viz.bob_amp;
    let sh = 1.0 - 0.18 * lift;
    draw_ellipse(feet.x, feet.y, r * 0.85 * sh, r * 0.30 * sh, 0.0, SHADOW);

    // Body centre sits just above the shadow, plus the bob offset.
    let center = vec2(feet.x, feet.y - r * 0.95 + bob_off);
    draw_toon_ball(center, r, sx, sy, viz.lean, outline, viz.light_dir, base, 1.0);
}

/// The income-currency icon id + a fallback color for `species`.
fn income_icon(species: &str) -> (&'static str, Color) {
    match species::try_get(species).map(|d| d.income_kind) {
        Some(IncomeKind::DnaHelix) => ("dna_helix", DNA_PINK),
        _ => ("coin", COIN_GOLD),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_critter(
    pos: Vec2,
    dir: Vec2,
    tex: Option<&Texture2D>,
    icon: Option<&Texture2D>,
    fallback: Color,
    at_cap: bool,
    scale: f32,
    tint: Color,
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
                tint,
                DrawTextureParams {
                    dest_size: Some(vec2(w, sprite_h)),
                    flip_x,
                    ..Default::default()
                },
            );
            top
        }
        None => {
            draw_circle(
                feet.x,
                bottom - sprite_h * 0.4,
                sprite_h * 0.3,
                ui::fade(color_u8!(90, 150, 210, 255), tint.a),
            );
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

/// Spotlight deposit view: dim the whole screen with a black gradient and
/// redraw the player's following animals bright on top. The hovered one pops
/// and gets a glowing pad so it reads as selectable. Clicking is resolved in
/// `GameApp::try_deposit_select`; Escape exits.
pub fn draw_deposit_overlay(app: &mut GameApp) {
    let cam = app.camera;
    let (w, h) = (screen_width(), screen_height());

    // Solid black scrim at high opacity — the lit followers sit on top of it.
    draw_rectangle(0.0, 0.0, w, h, color_u8!(0, 0, 0, 240));

    let hovered = app.deposit_hovered();
    // Species already in the nest (when one slot is filled): valid second picks
    // must crossbreed with it; the rest get dimmed.
    let partner = app.deposit_partner_species();

    // Instruction banner.
    let msg = "Click an animal to deposit  ·  Esc to cancel";
    let dim = measure_text(msg, None, 24, 1.0);
    text_shadow(msg, (w - dim.width) * 0.5, 64.0, 24.0, TEXT);

    // Snapshot the followers' render data first so the texture lookups below
    // (which take `&mut self`) don't clash with borrowing the critter list.
    let shots: Vec<(uuid::Uuid, &'static str, Vec2, Vec2)> = app
        .following
        .iter()
        .filter_map(|fid| {
            app.critters
                .iter()
                .find(|c| c.animal_id == *fid)
                .map(|c| (*fid, c.species, c.pos, c.dir))
        })
        .collect();

    // Redraw each follower on top of the scrim; pop + glow the hovered one and
    // dim any that can't crossbreed with the already-deposited partner.
    for (fid, sp, pos, dir) in shots {
        let valid = partner.map_or(true, |p| species::crossbreed_pool(p, sp).is_some());
        let is_hover = valid && Some(fid) == hovered;
        if is_hover {
            let feet = view::world_to_screen(pos, &cam);
            let base = CRITTER_H * cam.zoom;
            let pulse = (get_time() as f32 * 5.0).sin() * 0.5 + 0.5;
            draw_ellipse(feet.x, feet.y, base * (0.44 + 0.06 * pulse), base * 0.17, 0.0,
                color_u8!(255, 210, 90, 110));
        }
        let tex = app.textures.animal(sp);
        let (icon_id, fallback) = income_icon(sp);
        let icon = app.textures.icon(icon_id);
        let scale = if is_hover { 1.22 } else { 1.0 };
        let tint = if valid { WHITE } else { color_u8!(255, 255, 255, 70) };
        draw_critter(pos, dir, tex.as_ref(), icon.as_ref(), fallback, false, scale, tint, &cam);
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

/// Draw all five breeding-nest pads as woven bowls on the ground. Owned nests
/// are tinted by status; still-locked pads render dim with a padlock, and the
/// next purchasable one shows its coin/DNA price. A reach prompt appears when
/// the avatar is close.
fn draw_nests(app: &GameApp, now: DateTime<Utc>) {
    use crate::game::zoo::{MAX_NESTS, NestCost, NestStatus, Zoo, nest_unlock_cost};
    let cam = app.camera;
    let apos = app.session.my_avatar().pos;
    let owned = app.zoo.nest_count as usize;
    for i in 0..MAX_NESTS as usize {
        let world = Zoo::nest_pos(i);
        let p = view::world_to_screen(world, &cam);
        let rx = 34.0 * cam.zoom;
        let ry = 20.0 * cam.zoom;
        let unlocked = i < owned;
        let near = (world - apos).length() <= crate::app::INTERACT_RANGE;

        // Shadow + woven bowl (dim while locked).
        let (rim, bowl) = if unlocked {
            (color_u8!(120, 86, 54, 255), color_u8!(86, 60, 38, 255))
        } else {
            (color_u8!(70, 64, 58, 200), color_u8!(48, 44, 40, 200))
        };
        draw_ellipse(p.x, p.y + ry * 0.35, rx * 1.05, ry, 0.0, color_u8!(0, 0, 0, 60));
        draw_ellipse(p.x, p.y, rx, ry, 0.0, rim);
        draw_ellipse(p.x, p.y - ry * 0.18, rx * 0.78, ry * 0.7, 0.0, bowl);

        if unlocked {
            let status = app.zoo.nest_status(app.zoo.nests[i].id, now);
            let pip = match status {
                NestStatus::ReadyToCollect => Some(color_u8!(123, 207, 167, 255)),
                NestStatus::ReadyToBreed => Some(color_u8!(255, 210, 90, 255)),
                NestStatus::Breeding(_) => Some(color_u8!(196, 120, 220, 255)),
                _ => None,
            };
            if let Some(col) = pip {
                let pulse = (get_time() as f32 * 3.0).sin() * 0.5 + 0.5;
                draw_circle(p.x, p.y - ry * 0.2, (5.0 + pulse * 2.0) * cam.zoom, col);
            }
            if near {
                text_shadow("[E] Nest", p.x - 30.0, p.y - ry - 8.0, 18.0, TEXT);
            }
        } else {
            // Padlock dot.
            draw_circle(p.x, p.y - ry * 0.2, 4.0 * cam.zoom, color_u8!(180, 180, 188, 220));
            // Only the next-in-sequence pad is purchasable; show its price.
            if i == owned {
                let price = match nest_unlock_cost(owned as u8) {
                    Some(NestCost::Coins(c)) => format!("{c} coins"),
                    Some(NestCost::Dna(d)) => format!("{d} DNA"),
                    None => String::new(),
                };
                let lbl = if near { format!("[E] Unlock · {price}") } else { format!("Locked · {price}") };
                let col = if near { COIN_GOLD } else { TEXT_DIM };
                let w = measure_text(&lbl, None, 18, 1.0).width;
                text_shadow(&lbl, p.x - w * 0.5, p.y - ry - 8.0, 18.0, col);
            } else {
                text_shadow("Locked", p.x - 24.0, p.y - ry - 8.0, 16.0, TEXT_DIM);
            }
        }
    }
}

/// Draw the five food-structure pads along the bottom edge as squat silos.
/// Owned ones show a level pip + an "almost full" glow; locked ones render dim
/// with a padlock, and the next purchasable shows its coin price. Mirrors
/// [`draw_nests`].
fn draw_food_structures(app: &GameApp, now: DateTime<Utc>) {
    use crate::game::structure::{MAX_FOOD_STRUCTURES, food_structure_unlock_cost};
    use crate::game::zoo::Zoo;
    let cam = app.camera;
    let apos = app.session.my_avatar().pos;
    let owned = app.zoo.structures.len();
    for i in 0..MAX_FOOD_STRUCTURES {
        let world = Zoo::food_structure_pos(i);
        let p = view::world_to_screen(world, &cam);
        let half_w = 26.0 * cam.zoom;
        let h = 40.0 * cam.zoom;
        let unlocked = i < owned;
        let near = (world - apos).length() <= crate::app::INTERACT_RANGE;

        // Shadow + silo body (a rounded bin), dim while locked.
        let body = if unlocked {
            color_u8!(150, 130, 78, 255)
        } else {
            color_u8!(64, 60, 52, 200)
        };
        draw_ellipse(p.x, p.y, half_w * 1.1, 9.0 * cam.zoom, 0.0, color_u8!(0, 0, 0, 60));
        draw_rectangle(p.x - half_w, p.y - h, half_w * 2.0, h, body);
        draw_ellipse(p.x, p.y - h, half_w, 8.0 * cam.zoom, 0.0,
            if unlocked { color_u8!(180, 158, 96, 255) } else { color_u8!(80, 74, 64, 200) });

        if unlocked {
            let s = &app.zoo.structures[i];
            // Fill glow when near cap.
            let stored = s.stored_at(now);
            let cap = s.food_cap().max(1);
            if stored * 100 / cap >= 80 {
                let pulse = (get_time() as f32 * 3.0).sin() * 0.5 + 0.5;
                draw_circle(p.x, p.y - h - 6.0 * cam.zoom, (4.0 + pulse * 2.0) * cam.zoom,
                    color_u8!(150, 210, 120, 255));
            }
            let lbl = format!("Lv {}", s.level);
            let w = measure_text(&lbl, None, 16, 1.0).width;
            text_shadow(&lbl, p.x - w * 0.5, p.y - h - 14.0, 16.0, TEXT);
            if near {
                text_shadow("[E] Food", p.x - 32.0, p.y - h - 32.0, 18.0, TEXT);
            }
        } else {
            draw_circle(p.x, p.y - h * 0.5, 4.0 * cam.zoom, color_u8!(180, 180, 188, 220));
            if i == owned {
                let price = food_structure_unlock_cost(owned)
                    .map(|c| format!("{c} coins"))
                    .unwrap_or_default();
                let lbl = if near { format!("[E] Build · {price}") } else { format!("Locked · {price}") };
                let col = if near { COIN_GOLD } else { TEXT_DIM };
                let w = measure_text(&lbl, None, 18, 1.0).width;
                text_shadow(&lbl, p.x - w * 0.5, p.y - h - 14.0, 18.0, col);
            } else {
                text_shadow("Locked", p.x - 24.0, p.y - h - 14.0, 16.0, TEXT_DIM);
            }
        }
    }
}

// ── Waypoint beacons ───────────────────────────────────────────────────────────

const BEAM_COLOR: Color = color_u8!(120, 235, 200, 255);

/// Draw a glowing vertical beam at every player waypoint within view.
fn draw_waypoint_beams(app: &GameApp) {
    let cam = app.camera;
    let (tl, br) = view::camera_world_rect(&cam, screen_width(), screen_height());
    let pulse = (get_time() as f32 * 2.2).sin() * 0.5 + 0.5;
    let margin = 300.0;
    for w in &app.zoo.waypoints {
        if w.pos.x < tl.x - margin || w.pos.x > br.x + margin
            || w.pos.y < tl.y - margin || w.pos.y > br.y + margin
        {
            continue;
        }
        draw_beam(view::world_to_screen(w.pos, &cam), cam.zoom, pulse);
    }
}

/// A single ground beacon: base glow + a fading vertical column + bright core.
fn draw_beam(feet: Vec2, zoom: f32, pulse: f32) {
    let g = BEAM_COLOR;

    // Base glow discs on the ground.
    let base_rx = (38.0 + pulse * 8.0) * zoom;
    let base_ry = base_rx * 0.36;
    draw_ellipse(feet.x, feet.y, base_rx, base_ry, 0.0, Color::new(g.r, g.g, g.b, 0.26));
    draw_ellipse(feet.x, feet.y, base_rx * 0.55, base_ry * 0.55, 0.0, Color::new(g.r, g.g, g.b, 0.5));

    // Vertical column — stacked segments fading and narrowing upward.
    let height = (230.0 + pulse * 30.0) * zoom;
    let width = 18.0 * zoom;
    const SEGS: usize = 24;
    for i in 0..SEGS {
        let t = i as f32 / SEGS as f32; // 0 bottom → 1 top
        let yb = feet.y - t * height;
        let seg_h = height / SEGS as f32 + 1.0;
        let wseg = width * (1.0 - t * 0.55);
        let a = (1.0 - t) * 0.5 * (0.7 + 0.3 * pulse);
        draw_rectangle(feet.x - wseg * 0.5, yb - seg_h, wseg, seg_h, Color::new(g.r, g.g, g.b, a));
    }

    // Bright core line.
    draw_line(
        feet.x, feet.y,
        feet.x, feet.y - height * 0.85,
        2.0 * zoom,
        Color::new(1.0, 1.0, 1.0, 0.5 * (0.6 + 0.4 * pulse)),
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

/// Fade-out window (seconds) applied to status/error pills at the end of their
/// life, matching the lifetimes enforced in `GameApp::clear_stale_status`.
const TOAST_FADE: f64 = 0.6;
const STATUS_LIFE: f64 = 4.0;
const ERROR_LIFE: f64 = 8.0;

/// HUD overlay: floating currency chips, hint line, status/error toasts.
/// Draw on the screen, never into a render target.
pub fn draw_hud(app: &mut GameApp, now_utc: DateTime<Utc>) {
    // ── Top-left: floating currency chips (icon + value) ─────────────────
    let mut x = 14.0;
    let chip_y = 12.0;
    let coins = format!("{}", app.zoo.coins);
    let food = format!("{}", app.zoo.food);
    let dna = format!("{}", app.zoo.dna_helix);
    x += ui::currency_chip(&mut app.textures, x, chip_y, "coin", &coins, ui::COIN_GOLD) + 8.0;
    x += ui::currency_chip(&mut app.textures, x, chip_y, "food", &food, ui::FOOD_GREEN) + 8.0;
    ui::currency_chip(&mut app.textures, x, chip_y, "dna_helix", &dna, ui::DNA_PINK);

    // Hint line below the chips, with a soft drop shadow so it stays legible
    // over the world (the old letterbox bar is gone).
    let hint = if app.catch_state.active {
        "C exit catch · hover a wild animal to catch it"
    } else {
        "WASD move · Shift sprint · Space dash · E inspect / nest · C catch · 1 Shop 3 Settings 4 Waypoints"
    };
    text_shadow(hint, 16.0, 62.0, 18.0, TEXT_DIM);

    if app.catch_state.active {
        text_shadow(
            "CATCH MODE",
            screen_width() * 0.5 - 48.0, 30.0, 22.0,
            color_u8!(180, 255, 80, 230),
        );
    }

    // ── Bottom-left stack: errors (red) then status (amber), as pills ────
    let now = get_time();
    let mut bottom_y = screen_height() - 14.0;

    if let Some((msg, at)) = &app.status {
        let alpha = ((STATUS_LIFE - (now - *at)) / TOAST_FADE).clamp(0.0, 1.0) as f32;
        bottom_y = toast(msg, bottom_y, ui::STATUS_AMBER, alpha);
    }

    // Error log — most recent at the bottom, older lines above.
    for (msg, at) in app.errors.iter().rev() {
        let alpha = ((ERROR_LIFE - (now - *at)) / TOAST_FADE).clamp(0.0, 1.0) as f32;
        bottom_y = toast(msg, bottom_y, ui::ERROR_RED, alpha);
    }

    draw_notifications(app);
    draw_inspect_panel(app, now_utc);
}

/// Inspect-panel accent colour for a Rank stage. Regular falls back to the
/// neutral panel colour; the rest read as silver / gold / platinum / diamond /
/// ruby / neon.
fn rank_color(stage: u8) -> Color {
    match stage {
        0 => ui::PANEL_EDGE,
        1 => color_u8!(176, 180, 190, 255),
        2 => color_u8!(224, 178, 74, 255),
        3 => color_u8!(160, 206, 210, 255),
        4 => color_u8!(120, 200, 240, 255),
        5 => color_u8!(220, 74, 92, 255),
        _ => color_u8!(196, 84, 230, 255),
    }
}

/// Lerp from `a` toward `b` by `t` (0..1), preserving `a`'s alpha.
fn blend(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a,
    )
}

/// A self-contained inspect-panel button: draws + returns whether clicked.
fn inspect_button(x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
    let (mx, my) = mouse_position();
    let hover = enabled && mx >= x && mx <= x + w && my >= y && my <= y + h;
    let bg = if !enabled {
        color_u8!(30, 34, 41, 255)
    } else if hover {
        color_u8!(70, 80, 98, 255)
    } else {
        color_u8!(46, 52, 64, 255)
    };
    ui::rrect(x, y, w, h, 8.0, bg);
    let ld = measure_text(label, None, 18, 1.0);
    draw_text(
        label,
        x + (w - ld.width) * 0.5,
        y + h * 0.5 + ld.offset_y * 0.35,
        18.0,
        if enabled { ui::TEXT } else { ui::TEXT_DIM },
    );
    hover && is_mouse_button_pressed(MouseButton::Left)
}

/// Slide-in side panel showing the details of the inspected owned animal.
fn draw_inspect_panel(app: &mut GameApp, now: DateTime<Utc>) {
    let Some(ins) = &app.inspect else { return };
    let animal_id = ins.animal_id;
    let Some(animal) = app.zoo.animals.get(&animal_id) else { return };
    let def = species::get(animal.species);

    // Gather the display values up front (releases the borrow on app.zoo).
    let name = def.display_name.to_string();
    let species_id = animal.species;
    let level = animal.level;
    let stage = animal.stage;
    let theme = def.theme.name().to_string();
    let rate = animal.rate_per_sec();
    let cap = animal.storage_cap();
    let stored = animal.stored_at(now);
    let at_cap = animal.is_at_cap(now);
    let breeding = matches!(animal.state, crate::game::AnimalState::Breeding { .. });
    let from_right = ins.from_right;
    let t = ui::ease_out_back(ins.t.clamp(0.0, 1.0));

    // Rank progress + cost figures.
    let dupes = app.zoo.species_dupes.get(species_id).copied().unwrap_or(0);
    let rank_label = match rank::next_threshold(stage) {
        Some(next) => format!("{} ({}/{})", rank::rank_name(stage), dupes, next),
        None => format!("{} (max)", rank::rank_name(stage)),
    };
    let feed_cost = crate::game::animal::animal_level_up_cost(def.purchase_cost, level);
    let max_level = level >= crate::game::animal::MAX_ANIMAL_LEVEL;
    let in_nest = app.zoo.animal_in_any_nest(animal_id);
    let can_feed = !breeding && !max_level && app.zoo.food >= feed_cost;
    let can_sell = !breeding && !in_nest;

    let pw = 320.0;
    let ph = 470.0;
    let margin = 20.0;
    let y = (screen_height() - ph) * 0.5;
    let shown_x = if from_right { screen_width() - pw - margin } else { margin };
    let hidden_x = if from_right { screen_width() + 10.0 } else { -pw - 10.0 };
    let x = hidden_x + (shown_x - hidden_x) * t;

    // Panel recolored toward the Rank accent (Regular stays neutral).
    let accent = rank_color(stage);
    let bg = if stage == 0 { ui::PANEL } else { blend(ui::PANEL, accent, 0.18) };
    let edge = if stage == 0 { ui::PANEL_EDGE } else { accent };
    ui::rrect(x, y, pw, ph, 14.0, bg);
    ui::rrect_outline(x, y, pw, ph, 14.0, edge);

    let pad = 20.0;
    // Sprite thumbnail (or fallback disc) top-centre.
    let thumb = 96.0;
    let cx = x + pw * 0.5;
    let thumb_top = y + 18.0;
    if let Some(tx) = app.textures.animal(species_id) {
        let aspect = if tx.height() > 0.0 { tx.width() / tx.height() } else { 1.0 };
        let w = thumb * aspect;
        draw_texture_ex(
            &tx,
            cx - w * 0.5,
            thumb_top,
            WHITE,
            DrawTextureParams { dest_size: Some(vec2(w, thumb)), ..Default::default() },
        );
    } else {
        draw_circle(cx, thumb_top + thumb * 0.5, thumb * 0.4, color_u8!(120, 150, 200, 255));
    }

    // Title.
    let title_y = thumb_top + thumb + 26.0;
    let td = measure_text(&name, None, 24, 1.0);
    draw_text(&name, cx - td.width * 0.5, title_y, 24.0, ui::TEXT);

    // Detail rows: label (dim, left) + value (right-aligned).
    let mut ry = title_y + 30.0;
    let rows = [
        ("Rank".to_string(), rank_label),
        ("Level".to_string(), format!("{level} / {}", crate::game::animal::MAX_ANIMAL_LEVEL)),
        ("Habitat".to_string(), theme),
        ("Income".to_string(), format!("{rate:.2}/s")),
        ("Storage".to_string(), format!("{stored} / {cap}")),
        (
            "Status".to_string(),
            if breeding { "Breeding".to_string() }
            else if at_cap { "Ready to collect".to_string() }
            else { "Filling…".to_string() },
        ),
    ];
    for (label, value) in &rows {
        draw_text(label, x + pad, ry, 18.0, ui::TEXT_DIM);
        let vd = measure_text(value, None, 18, 1.0);
        let vcolor = if label == "Rank" && stage > 0 {
            accent
        } else if label == "Status" && *value == "Ready to collect" {
            ui::ACCENT
        } else {
            ui::TEXT
        };
        draw_text(value, x + pw - pad - vd.width, ry, 18.0, vcolor);
        ry += 26.0;
    }

    // Action buttons stacked above the footer: Follow / Feed / Sell.
    let already_following = app.following.contains(&animal_id);
    let bw = pw - pad * 2.0;
    let bh = 32.0;
    let bx = x + pad;
    let follow_by = y + ph - 16.0 - 8.0 - bh * 3.0 - 8.0 * 2.0;
    let feed_by = follow_by + bh + 8.0;
    let sell_by = feed_by + bh + 8.0;

    let follow_label = if in_nest {
        "In a nest"
    } else if already_following {
        "Stop following"
    } else {
        "Follow"
    };
    let feed_label = if max_level {
        "Feed (max level)".to_string()
    } else {
        format!("Feed · {feed_cost} food")
    };

    let do_follow = inspect_button(bx, follow_by, bw, bh, follow_label, !breeding && !in_nest);
    let do_feed = inspect_button(bx, feed_by, bw, bh, &feed_label, can_feed);
    let do_sell = inspect_button(bx, sell_by, bw, bh, "Sell", can_sell);

    // Footer hint.
    draw_text("E / Esc to close", x + pad, y + ph - 16.0, 15.0, ui::TEXT_DIM);

    if do_follow {
        if already_following {
            app.stop_following(animal_id);
            app.inspect = None;
            app.set_status("stopped following");
        } else {
            app.start_following(animal_id);
        }
    } else if do_feed {
        match app.zoo.level_up_animal(animal_id, now) {
            Ok(l) => {
                app.save_under_lock(now);
                app.set_status(format!("fed — now level {l}"));
            }
            Err(e) => app.set_status(format!("{e}")),
        }
    } else if do_sell {
        match app.zoo.sell_animal(animal_id, now) {
            Ok(coins) => {
                app.stop_following(animal_id);
                app.inspect = None;
                app.sync_critters();
                app.save_under_lock(now);
                app.set_status(format!("sold for {coins} coins"));
            }
            Err(e) => app.set_status(format!("{e}")),
        }
    }
}

/// Draw `text` twice — a dark offset copy then the coloured text — so small
/// labels stay readable over the varied world background.
fn text_shadow(text: &str, x: f32, y: f32, size: f32, color: Color) {
    draw_text(text, x + 1.0, y + 1.0, size, color_u8!(0, 0, 0, 170));
    draw_text(text, x, y, size, color);
}

/// Draw one bottom-left toast pill (rounded backing + text) at `bottom_y`,
/// returning the new stacking baseline above it. Fades with `alpha`.
fn toast(msg: &str, bottom_y: f32, color: Color, alpha: f32) -> f32 {
    if alpha <= 0.0 {
        return bottom_y;
    }
    const FS: f32 = 18.0;
    const PAD: f32 = 12.0;
    const H: f32 = 30.0;
    let dim = measure_text(msg, None, FS as u16, 1.0);
    let w = dim.width + PAD * 2.0;
    let y = bottom_y - H;
    ui::pill(14.0, y, w, H, alpha);
    draw_text(
        msg,
        14.0 + PAD,
        y + H * 0.5 + dim.offset_y * 0.35,
        FS,
        ui::fade(color, alpha),
    );
    y - 8.0
}

// ── Right-edge notifications ────────────────────────────────────────────────────

const NOTIF_W: f32 = 250.0;
const NOTIF_H: f32 = 58.0;
const NOTIF_GAP: f32 = 10.0;
/// Top of the notification stack — clears the floating currency-chip row.
const NOTIF_TOP: f32 = 52.0;

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

    let mut y = NOTIF_TOP + 16.0;
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
