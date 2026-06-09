//! The world scene: a flat soft-green ground plane with freely roaming critter
//! sprites (billboards + drop shadows). Pure drawing from `GameApp` state.

use chrono::{DateTime, Utc};
use macroquad::prelude::*;

use crate::app::GameApp;
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
const FOOT_SINK: f32 = 0.24;

/// On-screen height (px, pre-zoom) of a breeding-nest sprite when art is present.
const NEST_SPRITE_H: f32 = 128.0;

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
    // An active expedition renders its own bounded biome scene instead of the
    // hub world (Phase 3).
    if app.expedition.is_some() {
        draw_expedition_scene(app, now);
        return;
    }
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
            let color = biome_color(biome::biome_tile_color(vec2(tile_cx, tile_cy), app.zoo.world_seed));
            let (pos, size) = view::tile_rect(tx, ty, BTILE, &cam);
            // +1 px overlap prevents seams between tiles.
            draw_rectangle(pos.x, pos.y, size.x + 1.0, size.y + 1.0, color);
        }
    }

    // --- Grass: waving cosmetic field over the ground, under everything else -
    // Fetch the tuft atlas (mut borrow of the texture cache ends here) before the
    // immutable `draw_grass` borrow.
    let grass_atlas = app.textures.grass_atlas();
    super::grass::draw_grass(app, grass_atlas.as_ref(), app.grass_material.as_ref());

    // --- Zoo plot: tinted floor + fence outline marking the home enclosure -
    draw_zoo_plot(&cam, app.zoo.plot_origin, app.zoo.plot_half_extent());

    // --- Neighbouring plots on the shared hub (read-only; Phase 2 de-risk) --
    draw_peer_plots(app);

    // --- Breeding nests on the ground inside the plot ----------------------
    draw_nests(app, now);
    draw_food_structures(app, now);
    draw_pedestals(app, now);
    draw_npcs(app);

    // --- Debug: tile-grid placement overlay (F5) ---------------------------
    if app.debug_grid {
        draw_tile_grid_overlay(app);
    }

    // --- Waypoint beacons: glowing beams on the ground ---------------------
    draw_waypoint_beams(app);

    // --- Terrain props + critters + every session avatar, depth-sorted by
    // screen-Y (feet). Props (rocks/plants/trees scattered by world-gen) join the
    // same painter's-algorithm pass so a tree correctly occludes — or is occluded
    // by — a passing critter or avatar.
    let props = super::terrain::gather(app.zoo.world_seed, tx0, tx1, ty0, ty1);
    enum Item {
        Critter(usize),
        Avatar(uuid::Uuid),
        Prop(usize),
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
    for (i, p) in props.iter().enumerate() {
        order.push((p.world.y, Item::Prop(i)));
    }
    order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    for (_, item) in order {
        match item {
            Item::Critter(i) => {
                let c = &app.critters[i];
                let (sp, pos, dir, aid, pop) = (c.species, c.pos, c.dir, c.animal_id, c.pop);
                let (bob_phase, bob_amp) = (c.bob_phase, c.bob_amp);
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
                draw_critter(pos, dir, tex.as_ref(), icon.as_ref(), fallback, at_cap, scale, WHITE, bob_phase, bob_amp, &cam);
            }
            Item::Avatar(id) => {
                // Procedural toon-ball avatar (no sprite asset needed).
                if let Some(a) = app.session.avatars.get(&id) {
                    draw_avatar(a, &cam);
                }
            }
            Item::Prop(i) => {
                let p = &props[i];
                let (world, scale, flip) = (p.world, p.scale, p.flip);
                let tex = app.textures.terrain(p.id);
                draw_prop(world, tex.as_ref(), scale, flip, &cam);
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
            let color = biome_color(biome::biome_tile_color(vec2(cx, cy), seed));
            let (pos, size) = view::tile_rect(tx, ty, btile, &cam);
            // +1 px overlap prevents seams between tiles.
            draw_rectangle(pos.x, pos.y, size.x + 1.0, size.y + 1.0, color);
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
    bob_phase: f32,
    bob_amp: f32,
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

    // Walk bounce: a volume-preserving squash/stretch plus a small hop, scaled
    // by the smoothed walk intensity. The sprite is anchored at its base so the
    // feet stay planted while the body stretches up (top of the bounce) and
    // squashes down (landing) — no UV cropping needed.
    let bob = bob_phase.sin();
    let sy = 1.0 + bob * 0.08 * bob_amp; // tall up-beat, flat on landing
    let sx = 1.0 / sy; // preserve apparent volume
    let lift = (bob * 0.5 + 0.5) * bob_amp; // 0 at rest → 1 at the top of a hop
    let hop = lift * base_h * 0.07; // pixels the body rises off the ground

    // Shadow sits on the ground point, shrinking a touch as the body hops up.
    let sh = 1.0 - 0.16 * lift;
    draw_ellipse(feet.x, feet.y, base_h * 0.30 * sh, base_h * 0.10 * sh, 0.0, SHADOW);

    let sprite_top = match tex {
        Some(t) => {
            let aspect = if t.height() > 0.0 { t.width() / t.height() } else { 1.0 };
            let w = sprite_h * aspect * sx;
            let h = sprite_h * sy;
            let top = bottom - h - hop;
            draw_texture_ex(
                t,
                feet.x - w * 0.5,
                top,
                tint,
                DrawTextureParams {
                    dest_size: Some(vec2(w, h)),
                    flip_x,
                    ..Default::default()
                },
            );
            top
        }
        None => {
            draw_circle(
                feet.x,
                bottom - sprite_h * 0.4 - hop,
                sprite_h * 0.3,
                ui::fade(color_u8!(90, 150, 210, 255), tint.a),
            );
            bottom - sprite_h - hop
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

/// Draw a scattered terrain prop as a grounded billboard: native pixel size
/// scaled by zoom (so the source image's resolution controls its world size),
/// feet planted on the projected ground point, with a soft contact shadow.
/// No-op when the art is missing (props are purely cosmetic — no placeholder).
fn draw_prop(world: Vec2, tex: Option<&Texture2D>, scale: f32, flip: bool, cam: &Camera) {
    let Some(t) = tex else { return };
    let z = cam.zoom;
    let w = t.width() * z * scale;
    let h = t.height() * z * scale;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let feet = view::world_to_screen(world, cam);
    // Soft contact shadow at the base.
    draw_ellipse(feet.x, feet.y, w * 0.32, h * 0.07, 0.0, SHADOW);
    draw_texture_ex(
        t,
        feet.x - w * 0.5,
        feet.y - h,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(w, h)),
            flip_x: flip,
            ..Default::default()
        },
    );
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
    let msg = if app.dedicating.is_some() {
        "Click an animal to dedicate to the pedestal  ·  Esc to cancel"
    } else {
        "Click an animal to deposit  ·  Esc to cancel"
    };
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
        // Laid-out (held still) during deposit selection — no walk bounce.
        draw_critter(pos, dir, tex.as_ref(), icon.as_ref(), fallback, false, scale, tint, 0.0, 0.0, &cam);
    }
}

// ── Zoo plot (home enclosure) ─────────────────────────────────────────────────

/// Draw an enclosed home zoo plot at `center` with edge half-length `half`: a
/// subtle floor tint plus a fence outline so the player can always see where
/// home ends and the wilds begin. Taking the plot geometry as parameters lets
/// the same routine draw any plot on a shared hub, not just the world centre.
fn draw_zoo_plot(cam: &Camera, center: Vec2, half: f32) {
    let c = center;
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

/// Debug overlay (F5): draw the per-plot tile grid for the home zoo and every
/// neighbour, outlining each tile and filling those occupied by a structure
/// (owned nest, owned food structure, or placed pedestal). A placement aid for
/// verifying the tile layout — note habitats live on a separate isometric grid
/// and are intentionally not shown here.
fn draw_tile_grid_overlay(app: &GameApp) {
    use crate::game::structure::MAX_FOOD_STRUCTURES;
    use crate::game::world_chunks::ZOO_TILE_W;
    use std::collections::HashSet;
    let cam = app.camera;
    for zoo in std::iter::once(&app.zoo).chain(app.peer_zoos.values()) {
        // Tiles reserved by a structure.
        let mut occ: HashSet<(i32, i32)> = HashSet::new();
        let nests = zoo.nest_tiles();
        for t in nests.iter().take(zoo.nest_count as usize) {
            occ.insert(*t);
        }
        let foods = zoo.food_structure_tiles();
        for t in foods.iter().take(zoo.structures.len().min(MAX_FOOD_STRUCTURES)) {
            occ.insert(*t);
        }
        for p in &zoo.pedestals {
            occ.insert(p.tile);
        }

        let r = zoo.plot_tile_radius();
        // Tile screen size is squashed on the depth axis by the oblique TILT,
        // matching `world_to_screen` — drawing a full square here is what made
        // adjacent rows overlap.
        let s = vec2(ZOO_TILE_W, ZOO_TILE_W * view::TILT) * cam.zoom;
        for ty in -r..=r {
            for tx in -r..=r {
                let center = view::world_to_screen(zoo.tile_to_world((tx, ty)), &cam);
                let tl = center - s * 0.5;
                if occ.contains(&(tx, ty)) {
                    draw_rectangle(tl.x, tl.y, s.x, s.y, color_u8!(225, 90, 80, 90));
                }
                draw_rectangle_lines(tl.x, tl.y, s.x, s.y, 1.0, color_u8!(255, 255, 255, 48));
            }
        }
        // Mark the plot origin (tile 0,0 centre) so the grid is easy to orient.
        let o = view::world_to_screen(zoo.tile_to_world((0, 0)), &cam);
        draw_circle(o.x, o.y, 3.0 * cam.zoom.max(1.0), color_u8!(120, 200, 255, 220));
    }
}

/// Render the in-world expedition scene (Phase 3): the bounded biome ground
/// plus every live wild spawn as a clickable critter, the engaged target ringed
/// and showing a depleting catch bar overhead. The hub world is not drawn while
/// an expedition is active.
fn draw_expedition_scene(app: &mut GameApp, now: DateTime<Utc>) {
    let _ = now;
    clear_background(BG);
    let cam = app.camera;
    let Some(exp) = app.expedition.as_ref() else { return };
    let inst = &exp.instance;

    // Ground: the bounded map, tinted by the biome theme, with a fence border.
    let ground = biome_color(crate::game::biome::biome_color(inst.theme));
    let tl = view::world_to_screen(vec2(0.0, 0.0), &cam);
    let br = view::world_to_screen(vec2(inst.size.x, inst.size.y), &cam);
    draw_rectangle(tl.x, tl.y, br.x - tl.x, br.y - tl.y, ground);
    draw_rectangle_lines(tl.x, tl.y, br.x - tl.x, br.y - tl.y, 3.0, color_u8!(20, 24, 18, 220));

    // Collect roaming-animal render data first (ends the immutable borrow on
    // `exp` before we touch `app.textures`/`app.session` below).
    struct Draw {
        species: &'static str,
        pos: macroquad::math::Vec2,
        dir: macroquad::math::Vec2,
        tier: u8,
    }
    let mut animals: Vec<Draw> = inst
        .live()
        .map(|a| Draw {
            species: a.species,
            pos: vec2(a.pos.x, a.pos.y),
            // Face the way it's moving (art faces left by default).
            dir: if a.vel.x > 0.0 { vec2(1.0, 1.0) } else { vec2(-1.0, 1.0) },
            tier: crate::game::catch::catch_tier(a.species),
        })
        .collect();
    animals.sort_by(|a, b| a.pos.y.partial_cmp(&b.pos.y).unwrap_or(std::cmp::Ordering::Equal));
    let bar_remaining = exp.engagement.as_ref().map(|e| 1.0 - e.progress());
    let target_pos = exp.target_pos().map(|p| vec2(p.x, p.y));
    let avatar_y = app.session.my_avatar().pos.y;

    // Depth-sorted pass: roaming animals + the avatar, painter's-algorithm by
    // feet-Y so the avatar occludes / is occluded correctly as it walks.
    let mut avatar_drawn = false;
    for d in &animals {
        if !avatar_drawn && d.pos.y > avatar_y {
            draw_avatar(app.session.my_avatar(), &cam);
            avatar_drawn = true;
        }
        let tex = app.textures.animal(d.species);
        draw_critter(d.pos, d.dir, tex.as_ref(), None, COIN_GOLD, false, 1.0, WHITE, 0.0, 0.0, &cam);
        // Tier pip label above each animal.
        let screen = view::world_to_screen(d.pos, &cam);
        text_shadow(&format!("T{}", d.tier), screen.x - 8.0, screen.y - 70.0 * cam.zoom, 15.0, TEXT_DIM);
    }
    if !avatar_drawn {
        draw_avatar(app.session.my_avatar(), &cam);
    }

    // Top pass: the engaged target's ring + overhead catch bar at its live pos.
    if let (Some(tp), Some(rem)) = (target_pos, bar_remaining) {
        let screen = view::world_to_screen(tp, &cam);
        let pulse = (get_time() as f32 * 4.0).sin() * 0.5 + 0.5;
        draw_circle_lines(screen.x, screen.y, (34.0 + pulse * 5.0) * cam.zoom, 3.0, COIN_GOLD);
        let bw = 56.0 * cam.zoom;
        let bx = screen.x - bw * 0.5;
        let by = screen.y - 92.0 * cam.zoom;
        draw_rectangle(bx, by, bw, 7.0 * cam.zoom, color_u8!(40, 44, 52, 230));
        draw_rectangle(bx, by, bw * rem.clamp(0.0, 1.0), 7.0 * cam.zoom, color_u8!(225, 110, 110, 255));
        draw_rectangle_lines(bx, by, bw, 7.0 * cam.zoom, 1.0, color_u8!(255, 255, 255, 110));
    }

    app.particles.draw(&cam);
}

/// Bottom-centre expedition bars — they take the place of the item hotbar while
/// on an expedition (which is otherwise just free-roam in a biome instance). The
/// **stamina** bar is always shown; the **catch** bar (with the engaged animal's
/// name + a skill-check prompt) appears above it only while engaging.
fn draw_expedition_bars(app: &GameApp) {
    let Some(exp) = app.expedition.as_ref() else { return };
    let w = 360.0;
    let bx = (screen_width() - w) * 0.5;
    let stamina_y = screen_height() - 38.0;
    let catch_y = stamina_y - 34.0;

    // Catch bar — only while engaging a target.
    if let Some(eng) = exp.engagement.as_ref() {
        let name = crate::game::species::get(eng.target.species).display_name;
        text_shadow(&format!("Catching {name}  (T{})", eng.target.tier), bx, catch_y - 6.0, 15.0, TEXT);
        draw_rectangle(bx, catch_y, w, 13.0, color_u8!(40, 44, 52, 235));
        // Remaining catch-resistance, emptying toward capture.
        let remaining = (1.0 - eng.progress()).clamp(0.0, 1.0);
        draw_rectangle(bx, catch_y, w * remaining, 13.0, color_u8!(225, 110, 110, 255));
        draw_rectangle_lines(bx, catch_y, w, 13.0, 1.5, color_u8!(255, 255, 255, 90));
        // Skill-check prompt, centred above the catch bar.
        if eng.skill_check.is_some() {
            let pulse = (get_time() as f32 * 8.0).sin() * 0.5 + 0.5;
            let col = Color::new(1.0, 0.9, 0.3, 0.6 + 0.4 * pulse);
            let msg = "SKILL CHECK!  [SPACE]";
            let mw = measure_text(msg, None, 16, 1.0).width;
            text_shadow(msg, bx + (w - mw) * 0.5, catch_y - 24.0, 16.0, col);
        }
    }

    // Stamina bar — always shown on an expedition.
    let max = app.catch_stats().max_stamina.max(1.0);
    let frac = (app.stamina / max).clamp(0.0, 1.0);
    text_shadow(&format!("Stamina  {} / {}", app.stamina as i32, max as i32), bx, stamina_y - 6.0, 14.0, TEXT_DIM);
    draw_rectangle(bx, stamina_y, w, 11.0, color_u8!(40, 44, 52, 235));
    let col = if frac < 0.25 { color_u8!(225, 110, 110, 255) } else { color_u8!(120, 210, 130, 255) };
    draw_rectangle(bx, stamina_y, w * frac, 11.0, col);
    draw_rectangle_lines(bx, stamina_y, w, 11.0, 1.5, color_u8!(255, 255, 255, 90));
}

/// Draw each neighbouring plot on the shared hub (Phase 2 additive de-risk):
/// the plot rectangle plus representative nest/food/pedestal markers and a
/// placeholder avatar, all resolved from the peer zoo's own `plot_origin` via
/// the shared tile grid. Read-only — no interaction prompts, since these aren't
/// the local player's plot. Empty (and free) in normal play; populated only by
/// the F4 debug neighbour until real peers arrive in Phase 4.
fn draw_peer_plots(app: &GameApp) {
    use crate::game::structure::MAX_FOOD_STRUCTURES;
    use crate::game::zoo::MAX_NESTS;
    let cam = app.camera;
    for zoo in app.peer_zoos.values() {
        draw_zoo_plot(&cam, zoo.plot_origin, zoo.plot_half_extent());

        // Owned breeding nests as woven bowls.
        for i in 0..MAX_NESTS as usize {
            if i >= zoo.nest_count as usize {
                continue;
            }
            let p = view::world_to_screen(zoo.nest_pos(i), &cam);
            let (rx, ry) = (34.0 * cam.zoom, 20.0 * cam.zoom);
            draw_ellipse(p.x, p.y + ry * 0.35, rx * 1.05, ry, 0.0, color_u8!(0, 0, 0, 60));
            draw_ellipse(p.x, p.y, rx, ry, 0.0, color_u8!(120, 86, 54, 255));
            draw_ellipse(p.x, p.y - ry * 0.18, rx * 0.78, ry * 0.7, 0.0, color_u8!(86, 60, 38, 255));
        }

        // Owned food structures as squat silos.
        for i in 0..MAX_FOOD_STRUCTURES.min(zoo.structures.len()) {
            let p = view::world_to_screen(zoo.food_structure_pos(i), &cam);
            let (half_w, sh) = (26.0 * cam.zoom, 40.0 * cam.zoom);
            draw_ellipse(p.x, p.y, half_w * 1.1, 9.0 * cam.zoom, 0.0, color_u8!(0, 0, 0, 60));
            draw_rectangle(p.x - half_w, p.y - sh, half_w * 2.0, sh, color_u8!(150, 130, 78, 255));
            draw_ellipse(p.x, p.y - sh, half_w, 8.0 * cam.zoom, 0.0, color_u8!(180, 158, 96, 255));
        }

        // Placed pedestals.
        for ped in &zoo.pedestals {
            let p = view::world_to_screen(zoo.tile_to_world(ped.tile), &cam);
            draw_pedestal_shape(p, cam.zoom, color_u8!(150, 150, 165, 255));
        }

        // Placeholder neighbour avatar: a capped pillar just inside the top fence,
        // so the plot reads as occupied without standing in for the real avatar
        // rig (which arrives with networked peers in Phase 4).
        let head = view::world_to_screen(
            zoo.plot_origin + vec2(0.0, -zoo.plot_half_extent() * 0.25),
            &cam,
        );
        draw_circle(head.x, head.y - 18.0 * cam.zoom, 10.0 * cam.zoom, color_u8!(232, 196, 160, 255));
        draw_rectangle(
            head.x - 9.0 * cam.zoom, head.y - 8.0 * cam.zoom,
            18.0 * cam.zoom, 26.0 * cam.zoom,
            color_u8!(90, 140, 200, 255),
        );
    }
}

/// Draw all five breeding-nest pads as woven bowls on the ground. Owned nests
/// are tinted by status; still-locked pads render dim with a padlock, and the
/// next purchasable one shows its coin/DNA price. A reach prompt appears when
/// the avatar is close.
fn draw_nests(app: &mut GameApp, now: DateTime<Utc>) {
    use crate::game::zoo::{MAX_NESTS, NestCost, NestStatus, nest_unlock_cost};
    let cam = app.camera;
    let apos = app.session.my_avatar().pos;
    let owned = app.zoo.nest_count as usize;
    // Nest sprite (assets/structures/nest.png); None → woven-bowl placeholder.
    let nest_tex = app.textures.structure("nest");
    for i in 0..MAX_NESTS as usize {
        let world = app.zoo.nest_pos(i);
        let p = view::world_to_screen(world, &cam);
        let rx = 34.0 * cam.zoom;
        let ry = 20.0 * cam.zoom;
        let unlocked = i < owned;
        let near = (world - apos).length() <= crate::app::INTERACT_RANGE;

        // Contact shadow (shared by both render paths).
        draw_ellipse(p.x, p.y + ry * 0.35, rx * 1.05, ry, 0.0, color_u8!(0, 0, 0, 60));

        // Body: the structure sprite if bundled, else the woven-bowl primitives.
        // Locked nests are dimmed. `label_y` is where the status text/prompt sits.
        let label_y = match nest_tex.as_ref() {
            Some(t) => {
                let h = NEST_SPRITE_H * cam.zoom;
                let aspect = if t.height() > 0.0 { t.width() / t.height() } else { 1.0 };
                let w = h * aspect;
                let bottom = p.y + ry * 0.35; // plant the sprite base on the shadow
                let top = bottom - h;
                let tint = if unlocked { WHITE } else { color_u8!(120, 120, 128, 210) };
                draw_texture_ex(
                    t,
                    p.x - w * 0.5,
                    top,
                    tint,
                    DrawTextureParams {
                        dest_size: Some(vec2(w, h)),
                        ..Default::default()
                    },
                );
                top - 6.0 * cam.zoom
            }
            None => {
                let (rim, bowl) = if unlocked {
                    (color_u8!(120, 86, 54, 255), color_u8!(86, 60, 38, 255))
                } else {
                    (color_u8!(70, 64, 58, 200), color_u8!(48, 44, 40, 200))
                };
                draw_ellipse(p.x, p.y, rx, ry, 0.0, rim);
                draw_ellipse(p.x, p.y - ry * 0.18, rx * 0.78, ry * 0.7, 0.0, bowl);
                p.y - ry - 8.0
            }
        };

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
                text_shadow("[E] Nest", p.x - 30.0, label_y, 18.0, TEXT);
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
                text_shadow(&lbl, p.x - w * 0.5, label_y, 18.0, col);
            } else {
                text_shadow("Locked", p.x - 24.0, label_y, 16.0, TEXT_DIM);
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
    let cam = app.camera;
    let apos = app.session.my_avatar().pos;
    let owned = app.zoo.structures.len();
    for i in 0..MAX_FOOD_STRUCTURES {
        let world = app.zoo.food_structure_pos(i);
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

/// Draw every placed pedestal as a small pillar + slab, and — while a placement
/// is in progress — a translucent ghost snapped to the tile under the cursor
/// (green when the tile is valid, red when it's off-plot or already taken).
fn draw_pedestals(app: &GameApp, now: DateTime<Utc>) {
    let cam = app.camera;
    let apos = app.session.my_avatar().pos;

    for ped in &app.zoo.pedestals {
        let world = app.zoo.tile_to_world(ped.tile);
        let p = view::world_to_screen(world, &cam);
        let near = (world - apos).length() <= crate::app::INTERACT_RANGE;
        draw_pedestal_shape(p, cam.zoom, color_u8!(150, 150, 165, 255));

        // A dedicated animal at cap pulses a gold pip (income waiting to sweep).
        if let Some(a) = ped.animal.and_then(|id| app.zoo.animals.get(&id)) {
            let col = if a.is_at_cap(now) {
                let pulse = (get_time() as f32 * 3.0).sin() * 0.5 + 0.5;
                Color::new(1.0, 0.823, 0.353, 0.6 + 0.4 * pulse)
            } else {
                color_u8!(123, 207, 167, 255)
            };
            draw_circle(p.x, p.y - 30.0 * cam.zoom, 4.0 * cam.zoom, col);
        }
        if near {
            text_shadow("[E] Pedestal", p.x - 42.0, p.y - 40.0 * cam.zoom, 18.0, TEXT);
        }
    }

    // Placement ghost.
    if let Some(placement) = app.placing {
        let (mx, my) = mouse_position();
        let world = view::screen_to_world(vec2(mx, my), &cam);
        let tile = app.zoo.world_to_tile(world);
        let ignore = match placement {
            crate::app::Placement::Move(id) => Some(id),
            crate::app::Placement::Hotbar => None,
        };
        let valid = app.zoo.tile_in_bounds(tile) && app.zoo.pedestal_tile_free(tile, ignore);
        let p = view::world_to_screen(app.zoo.tile_to_world(tile), &cam);
        let tint = if valid {
            color_u8!(120, 235, 160, 160)
        } else {
            color_u8!(235, 110, 110, 160)
        };
        draw_pedestal_shape(p, cam.zoom, tint);
    }
}

/// Draw every placed NPC as a grounded billboard: idle vertical bob, a scale-pop
/// on the speaking transition, and a `{id}_speaking` sprite swap while its panel
/// is open. Falls back to the base sprite if no speaking art exists, and to a
/// placeholder figure if no sprite exists at all. Shows an `[E] <label>` prompt
/// when the player is in interaction range.
fn draw_npcs(app: &mut GameApp) {
    let cam = app.camera;
    let apos = app.session.my_avatar().pos;
    // Snapshot the render data so the per-NPC texture lookups below can borrow
    // the texture cache mutably without conflicting with the `npcs` borrow.
    let items: Vec<(String, &'static str, Vec2, f32, f32, &'static str)> = app
        .npcs
        .iter()
        .map(|n| (n.sprite_id(), n.id, n.world, n.bob_offset(), n.pop, n.label))
        .collect();

    for (sprite_id, base_id, world, bob_off, pop, label) in items {
        let near = (world - apos).length() <= crate::app::INTERACT_RANGE;
        // Prefer the resolved sprite (e.g. `{id}_speaking`); fall back to the
        // base sprite when the variant art isn't bundled.
        let tex = app
            .textures
            .npc(&sprite_id)
            .or_else(|| app.textures.npc(base_id));
        draw_npc_billboard(world, tex.as_ref(), bob_off, pop, near, label, &cam);
    }
}

/// Render a single NPC billboard + interaction prompt. `bob_off` is the idle bob
/// offset in pre-zoom px (lifts the body, not the shadow); `pop` drives a
/// feet-anchored scale-pop.
fn draw_npc_billboard(
    world: Vec2,
    tex: Option<&Texture2D>,
    bob_off: f32,
    pop: f32,
    near: bool,
    label: &str,
    cam: &Camera,
) {
    let p = view::world_to_screen(world, cam);
    let z = cam.zoom;
    let bob = bob_off * z; // body lift in screen px
    let scale = view::pop_scale(pop);

    // Soft contact shadow stays planted on the ground point (no bob/pop).
    draw_ellipse(p.x, p.y, 26.0 * z, 8.0 * z, 0.0, color_u8!(0, 0, 0, 60));

    let base = p.y + bob; // feet line, lifted by the idle bob
    let top_y = match tex {
        Some(t) => {
            // Billboard a touch taller than a critter; pop grows it about the feet.
            let h = CRITTER_H * 1.35 * z * scale;
            let aspect = if t.height() > 0.0 { t.width() / t.height() } else { 1.0 };
            let w = h * aspect;
            let top = base - h;
            draw_texture_ex(
                t,
                p.x - w * 0.5,
                top,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(w, h)),
                    ..Default::default()
                },
            );
            top
        }
        None => {
            // Placeholder: stall awning behind a rounded figure.
            let s = z * scale;
            draw_rectangle(p.x - 26.0 * s, base - 54.0 * s, 52.0 * s, 12.0 * s, color_u8!(196, 84, 92, 255));
            draw_rectangle(p.x - 26.0 * s, base - 42.0 * s, 52.0 * s, 4.0 * s, color_u8!(232, 224, 210, 255));
            draw_rectangle(p.x - 9.0 * s, base - 36.0 * s, 18.0 * s, 30.0 * s, color_u8!(96, 120, 180, 255));
            draw_circle(p.x, base - 40.0 * s, 8.0 * s, color_u8!(226, 188, 156, 255));
            base - 54.0 * s
        }
    };

    let prompt;
    let lbl: &str = if near {
        prompt = format!("[E] {label}");
        &prompt
    } else {
        label
    };
    let col = if near { TEXT } else { TEXT_DIM };
    let w = measure_text(lbl, None, 18, 1.0).width;
    text_shadow(lbl, p.x - w * 0.5, top_y - 10.0 * z, 18.0, col);
}

/// Shared pedestal sprite: a soft shadow, a short pillar, and a flat top slab.
fn draw_pedestal_shape(p: Vec2, zoom: f32, body: Color) {
    let hw = 18.0 * zoom;
    let h = 26.0 * zoom;
    draw_ellipse(p.x, p.y, hw * 1.2, 7.0 * zoom, 0.0, color_u8!(0, 0, 0, 60));
    // Pillar.
    draw_rectangle(p.x - hw * 0.55, p.y - h, hw * 1.1, h, body);
    // Top slab.
    draw_ellipse(p.x, p.y - h, hw, 6.0 * zoom, 0.0, body);
    draw_ellipse(
        p.x,
        p.y - h - 2.0 * zoom,
        hw * 0.78,
        4.5 * zoom,
        0.0,
        Color::new(0.823, 0.823, 0.882, body.a),
    );
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
    let hint = "WASD move · Shift sprint · Space dash · E inspect / nest · 1–5 hotbar · F6 expedition · U Upgrades O Settings M Waypoints";
    text_shadow(hint, 16.0, 62.0, 18.0, TEXT_DIM);

    // While hosting, always show our own join code so it's readable without
    // opening Settings (and never confused with the "join a friend" field).
    if app.is_hosting() {
        if let Some(code) = app.session.join_code() {
            let peers = app.session.peer_count();
            let line = format!("● Online — share code  {}   ·   {peers}/3 visitors", code.as_str());
            text_shadow(&line, 16.0, 84.0, 18.0, color_u8!(123, 207, 167, 255));
        }
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
    // In an expedition the bottom-centre slot row is replaced by the catch +
    // stamina bars; otherwise it's the normal item hotbar.
    if app.expedition.is_some() {
        draw_expedition_bars(app);
    } else {
        draw_hotbar(app);
    }
    draw_inspect_panel(app, now_utc);
}

/// The bottom-center hotbar: 5 spaced squares. The selected slot uses the
/// `slot_container_selected` sprite (else a highlighted rounded rect); item icons
/// are drawn centered in the slot. Empty slots read as faint outlines.
fn draw_hotbar(app: &mut GameApp) {
    use crate::app::HOTBAR_SLOTS;
    let slots = app.hotbar_slots();
    let selected_slot = app.selected_slot;
    let size = 56.0;
    let gap = 12.0;
    let total = HOTBAR_SLOTS as f32 * size + (HOTBAR_SLOTS as f32 - 1.0) * gap;
    let x0 = (screen_width() - total) * 0.5;
    let y = screen_height() - size - 24.0;

    // Container sprites (owned clones, so the per-slot texture lookups below can
    // still borrow the cache mutably). `slot_container_selected` falls back to
    // `slot_container` when only the base art is provided.
    let container = app.textures.hotbar("slot_container");
    let container_sel = app.textures.hotbar("slot_container_selected");

    for i in 0..HOTBAR_SLOTS {
        let x = x0 + i as f32 * (size + gap);
        let selected = i == selected_slot;

        // Slot background: sprite if available, else the legacy rounded rect.
        let bg_tex = if selected {
            container_sel.as_ref().or(container.as_ref())
        } else {
            container.as_ref()
        };
        match bg_tex {
            Some(t) => {
                draw_texture_ex(
                    t,
                    x,
                    y,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(size, size)),
                        ..Default::default()
                    },
                );
            }
            None => {
                let bg = if selected {
                    color_u8!(44, 50, 62, 235)
                } else {
                    color_u8!(24, 27, 33, 200)
                };
                ui::rrect(x, y, size, size, 8.0, bg);
                let edge = if selected {
                    color_u8!(123, 207, 167, 255)
                } else {
                    color_u8!(64, 72, 86, 220)
                };
                ui::rrect_outline(x, y, size, size, 8.0, edge);
            }
        }

        // Slot number (1-based) in the corner.
        text_shadow(&format!("{}", i + 1), x + 5.0, y + 16.0, 13.0, TEXT_DIM);

        if let Some((item, count)) = slots[i] {
            // Item icon, centered in the slot (sprite if provided, else the
            // in-world pedestal shape as a placeholder).
            match app.textures.hotbar(item.icon_id()) {
                Some(t) => draw_centered_icon(&t, x, y, size),
                None => draw_pedestal_shape(
                    vec2(x + size * 0.5, y + size * 0.72),
                    0.5,
                    color_u8!(150, 150, 165, 255),
                ),
            }
            // Stack-count badge, bottom-right.
            let badge = format!("×{count}");
            let w = measure_text(&badge, None, 16, 1.0).width;
            text_shadow(&badge, x + size - w - 6.0, y + size - 7.0, 16.0, TEXT);
        }
    }
}

/// Draw `tex` centered inside the slot at `(x, y)` of side `size`, fit within an
/// inner padded box while preserving aspect ratio.
fn draw_centered_icon(tex: &Texture2D, x: f32, y: f32, size: f32) {
    let pad = size * 0.16;
    let box_sz = size - pad * 2.0;
    let aspect = if tex.height() > 0.0 { tex.width() / tex.height() } else { 1.0 };
    let (iw, ih) = if aspect >= 1.0 {
        (box_sz, box_sz / aspect)
    } else {
        (box_sz * aspect, box_sz)
    };
    draw_texture_ex(
        tex,
        x + (size - iw) * 0.5,
        y + (size - ih) * 0.5,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(iw, ih)),
            ..Default::default()
        },
    );
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
    // Visitors may only sell if the host granted the permission (the host always
    // can). Permissions live in the shared `visitors` map under our session id.
    let may_sell = if app.is_guest() {
        app.zoo
            .visitors
            .get(&app.session.local_player_id)
            .is_some_and(|r| r.permissions.has(crate::game::visitor::PermissionSet::SELL))
    } else {
        true
    };
    let can_sell = !breeding && !in_nest && may_sell;

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
        use crate::game::action::{Action, ActionOutcome};
        match app.dispatch(Action::LevelUp(animal_id), now) {
            Some(ActionOutcome::Leveled(l)) => app.set_status(format!("fed — now level {l}")),
            None if app.is_guest() => app.set_status("feeding…"),
            _ => {}
        }
    } else if do_sell {
        use crate::game::action::{Action, ActionOutcome};
        match app.dispatch(Action::Sell(animal_id), now) {
            Some(ActionOutcome::Sold(coins)) => {
                app.stop_following(animal_id);
                app.inspect = None;
                app.set_status(format!("sold for {coins} coins"));
            }
            None if app.is_guest() => {
                app.stop_following(animal_id);
                app.inspect = None;
                app.set_status("sell requested");
            }
            _ => {}
        }
    }
}

/// Convert the engine-free biome colour (`game::biome::Rgba`) into macroquad's
/// `Color` at the draw boundary.
#[inline]
fn biome_color(c: crate::game::biome::Rgba) -> Color {
    Color::new(c.r, c.g, c.b, c.a)
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
