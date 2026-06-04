//! Hand-drawn menu overlays (Shop, Breeding). Plain but tidy: a rounded, wide
//! panel that scales/fades in on open and out on close, drawn over the blurred
//! + darkened world (the blur/darken backdrop is handled in `GameApp::draw`).
//! Immediate-mode: buttons hit-test the cursor inline and call domain methods.

use chrono::{DateTime, Utc};
use macroquad::prelude::*;

use crate::app::{GameApp, PostEffect, Screen};
use crate::game::exotic_shop::{self, Price};
use crate::game::species::{self, IncomeKind, SpeciesId};
use crate::game::vendor;
use crate::game::zoo::NestStatus;
use crate::render::ui::{self, ACCENT, PANEL, PANEL_EDGE, TEXT, TEXT_DIM, ease_out_back, fade};

const BTN: Color = color_u8!(46, 52, 64, 255);
const BTN_HOVER: Color = color_u8!(70, 80, 98, 255);
const BTN_DISABLED: Color = color_u8!(30, 34, 41, 255);

/// Per-frame draw context carrying the scale/fade transform + cursor state.
#[derive(Clone, Copy)]
struct Ctx {
    center: Vec2,
    scale: f32,
    alpha: f32,
    mouse: Vec2,
    click: bool,
    interactive: bool,
}

impl Ctx {
    /// Transform a logical (full-size) point into the animated screen point.
    fn pt(&self, x: f32, y: f32) -> Vec2 {
        self.center + (vec2(x, y) - self.center) * self.scale
    }
}

pub fn draw(app: &mut GameApp, now: DateTime<Utc>) {
    if app.menu_t <= 0.001 {
        return;
    }
    let t = app.menu_t.clamp(0.0, 1.0);
    let eased = 0.6 + 0.4 * ease_out_back(t); // slight pop past 1.0 mid-open
    let ctx = Ctx {
        center: vec2(screen_width() * 0.5, screen_height() * 0.5),
        scale: eased,
        alpha: t,
        mouse: {
            let (mx, my) = mouse_position();
            vec2(mx, my)
        },
        click: is_mouse_button_pressed(MouseButton::Left),
        interactive: app.screen != Screen::World && app.menu_t > 0.9,
    };
    match app.shown_menu {
        Screen::Shop => draw_shop(app, now, ctx),
        Screen::Settings => draw_settings(app, now, ctx),
        Screen::Waypoints => draw_waypoints(app, now, ctx),
        Screen::Nest => draw_nest(app, now, ctx),
        Screen::Structure => draw_structure(app, now, ctx),
        Screen::World => {}
    }
}

// ─────────────────────────── Waypoints ───────────────────────────

fn draw_waypoints(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx) {
    use crate::game::world_chunks::zoo_center;

    // Snapshot the list so we can call &mut app methods while iterating.
    let waypoints: Vec<(uuid::Uuid, String, Vec2)> = app
        .zoo
        .waypoints
        .iter()
        .map(|w| (w.id, w.name.clone(), w.pos))
        .collect();
    let cap = crate::game::zoo::Zoo::MAX_WAYPOINTS;

    let pw = 480.0;
    // rows: home + each waypoint, then the "add" button + close.
    let rows = 1 + waypoints.len();
    let ph = 150.0 + rows as f32 * 38.0 + 36.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "WAYPOINTS");
    label(&ctx, px + 26.0, py + 64.0, "Teleport to", 18.0, ACCENT);

    let tp_w = pw - 52.0 - 40.0 - 8.0;
    let mut y = py + 78.0;

    // Default destination: the home zoo.
    if button(&ctx, px + 26.0, y, tp_w, 32.0, "Home Zoo", true) {
        app.teleport_to(zoo_center());
    }
    y += 38.0;

    // Player-placed waypoints, each with a teleport + remove control.
    for (id, name, pos) in &waypoints {
        if button(&ctx, px + 26.0, y, tp_w, 32.0, name, true) {
            app.teleport_to(*pos);
        }
        if button(&ctx, px + 26.0 + tp_w + 8.0, y, 40.0, 32.0, "X", true) {
            app.remove_waypoint(*id, now);
        }
        y += 38.0;
    }

    y += 6.0;
    let can_add = waypoints.len() < cap;
    let add_lbl = if can_add {
        "+ Add waypoint here".to_string()
    } else {
        format!("Waypoint limit reached ({cap})")
    };
    if button(&ctx, px + 26.0, y, pw - 52.0, 34.0, &add_lbl, can_add) {
        app.add_waypoint_here(now);
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }
}

// ─────────────────────────── Settings ───────────────────────────

fn draw_settings(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx) {
    let pw = 560.0;
    let effects = PostEffect::ALL;
    // Layout: effects, then Online (host toggle, join, gifts).
    // Pre-compute gift counts so the panel can grow to fit.
    let visiting = matches!(
        app.session.role,
        crate::net::SessionRole::Visiting { .. }
    );
    let pending_gifts: Vec<(uuid::Uuid, uuid::Uuid, String, String, u8)> = app
        .zoo
        .visitors
        .values()
        .flat_map(|v| {
            let name = v.display_name.clone();
            let vid = v.player_id;
            v.gift_inbox.iter().map(move |g| {
                (
                    vid,
                    g.id,
                    name.clone(),
                    g.species.to_string(),
                    g.level,
                )
            })
        })
        .collect();
    let gifts_block_h = if !pending_gifts.is_empty() {
        24.0 + pending_gifts.len().min(4) as f32 * 30.0 + 6.0
    } else if visiting {
        56.0
    } else {
        0.0
    };
    let online_block_h = 170.0 + gifts_block_h;
    let ph = 130.0 + effects.len() as f32 * 40.0 + online_block_h + 30.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "SETTINGS");
    label(&ctx, px + 26.0, py + 64.0, "Fullscreen effect", 18.0, ACCENT);

    let mut y = py + 80.0;
    for eff in effects {
        let selected = app.effect == eff;
        let txt = if selected {
            format!("● {}", eff.label())
        } else {
            format!("  {}", eff.label())
        };
        if button(&ctx, px + 26.0, y, pw - 52.0, 32.0, &txt, true) {
            app.effect = eff;
            app.set_status(format!("effect: {}", eff.label()));
        }
        y += 40.0;
    }

    // ── Online (Steam relay, app ID 480) ────────────────────────────────
    y += 8.0;
    label(&ctx, px + 26.0, y, "Online", 18.0, ACCENT);
    y += 18.0;
    let hosting = app.is_hosting();
    let toggle_label = if hosting { "Stop hosting" } else { "Open Zoo (Steam)" };
    if button(&ctx, px + 26.0, y, pw - 52.0, 30.0, toggle_label, true) {
        if hosting { app.stop_hosting(); } else { app.start_hosting(); }
    }
    y += 36.0;
    if hosting {
        let code = app
            .session
            .join_code()
            .map(|c| c.as_str().to_string())
            .unwrap_or_else(|| "—".to_string());
        let peers = app.session.peer_count();
        label(
            &ctx,
            px + 26.0,
            y + 14.0,
            &format!("join code: {code}     visitors: {peers}/3"),
            16.0,
            TEXT_DIM,
        );
    } else {
        label(
            &ctx,
            px + 26.0,
            y + 14.0,
            "Requires Steam (app 480 — Spacewar test key).",
            14.0,
            TEXT_DIM,
        );
    }
    y += 28.0;

    // ── Join a friend ───────────────────────────────────────────────────
    label(&ctx, px + 26.0, y, "Join a friend", 16.0, ACCENT);
    y += 12.0;
    // Capture chars only while the panel is fully open + interactive, so
    // game input doesn't double-up while we're typing.
    if ctx.interactive {
        app.pump_join_code_input();
    }
    let field_w = pw - 52.0 - 110.0 - 8.0;
    let field_h = 30.0;
    let field_x = px + 26.0;
    // Draw the input field as a panel-like rect.
    let p = ctx.pt(field_x, y);
    let s = vec2(field_w, field_h) * ctx.scale;
    ui::rrect(p.x, p.y, s.x, s.y, 6.0 * ctx.scale, fade(BTN, ctx.alpha));
    let shown = if app.join_code_buffer.is_empty() {
        "______".to_string()
    } else {
        format!("{:_<6}", app.join_code_buffer)
    };
    draw_text(
        &shown,
        p.x + 10.0,
        p.y + s.y * 0.5 + 6.0,
        18.0 * ctx.scale,
        fade(TEXT, ctx.alpha),
    );
    let join_enabled = app.join_code_buffer.len() == 6 && !hosting;
    if button(
        &ctx,
        field_x + field_w + 8.0,
        y,
        110.0,
        field_h,
        "Join",
        join_enabled,
    ) {
        let code = app.join_code_buffer.clone();
        if app.try_join_by_code(&code) {
            app.join_code_buffer.clear();
        }
    }
    y += 40.0;

    // ── Gifts: visitor-side drop / host-side inbox ──────────────────────
    if visiting {
        label(&ctx, px + 26.0, y, "Bring a gift", 16.0, ACCENT);
        y += 14.0;
        if button(
            &ctx,
            px + 26.0,
            y,
            220.0,
            28.0,
            "Drop a Field Mouse",
            true,
        ) {
            app.drop_gift("field_mouse", 1);
        }
    } else if !pending_gifts.is_empty() {
        label(
            &ctx,
            px + 26.0,
            y,
            &format!("Gifts received ({})", pending_gifts.len()),
            16.0,
            ACCENT,
        );
        y += 18.0;
        for (vid, gid, sender, species, level) in pending_gifts.iter().take(4) {
            let lbl = format!("{species} L{level} from {sender}");
            label(&ctx, px + 26.0, y + 18.0, &lbl, 14.0, TEXT);
            if button(
                &ctx,
                px + pw - 26.0 - 110.0,
                y,
                110.0,
                26.0,
                "Claim",
                true,
            ) {
                app.claim_gift(*vid, *gid, now);
            }
            y += 30.0;
        }
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }
}

// ───────────────────────────── Shop ─────────────────────────────

fn draw_shop(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx) {
    let coins = app.zoo.coins;
    let dna = app.zoo.dna_helix;

    // STUB: the shop currently shows the stock of every *open* biome vendor in
    // one window (see `game::vendor`). The new-biome merchants are "coming
    // soon", so their fauna stay catch-only and are teased at the bottom.
    let animals: Vec<(SpeciesId, String, bool)> = vendor::open_shop_stock()
        .into_iter()
        .map(|d| {
            let (afford, cur) = match d.purchase_currency {
                IncomeKind::Coin => (coins >= d.purchase_cost, "c"),
                IncomeKind::DnaHelix => (dna >= d.purchase_cost, "DNA"),
            };
            (d.id, format!("{}  ·  {} {}", d.display_name, d.purchase_cost, cur), afford)
        })
        .collect();

    let window = exotic_shop::effective_window(now, app.zoo.exotic_skip_window);
    let exotics: Vec<(SpeciesId, String, Price, bool)> = window
        .as_ref()
        .map(|w| {
            w.offerings
                .iter()
                .map(|o| {
                    let def = species::get(o.species);
                    let (label, afford) = match o.price {
                        Price::Coins(c) => (format!("{}  ·  {} c", def.display_name, c), coins >= c),
                        Price::Dna(d) => (format!("{}  ·  {} DNA", def.display_name, d), dna >= d),
                    };
                    (o.species, label, o.price, afford)
                })
                .collect()
        })
        .unwrap_or_default();
    let closed_secs = window
        .is_none()
        .then(|| exotic_shop::seconds_until_state_change(now));
    let skip_cost = exotic_shop::SKIP_WAIT_DNA_COST;

    // Wide panel; 2-column animal grid keeps it short.
    let pw = 760.0;
    let rows = animals.len().div_ceil(2);
    let ph = 96.0 + rows as f32 * 34.0 + 180.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "SHOP");
    right_text(
        &ctx,
        px + pw - 26.0,
        py + 30.0,
        &format!("{coins} coins    {dna} DNA"),
    );

    label(&ctx, px + 26.0, py + 64.0, "Animals", 18.0, ACCENT);
    let col_w = (pw - 52.0 - 12.0) * 0.5;
    let mut y = py + 78.0;
    for (i, (id, lbl, afford)) in animals.iter().enumerate() {
        let col = (i % 2) as f32;
        let bx = px + 26.0 + col * (col_w + 12.0);
        if i % 2 == 0 && i != 0 {
            y += 34.0;
        }
        if button(&ctx, bx, y, col_w, 28.0, lbl, *afford) {
            match app.zoo.purchase_animal(*id, now) {
                Ok(_) => {
                    app.sync_critters();
                    app.save_under_lock(now);
                    app.push_notification(
                        species::get(*id).display_name,
                        "Purchased",
                        crate::app::NotifIcon::Animal(*id),
                    );
                }
                Err(e) => app.set_status(format!("{e}")),
            }
        }
    }
    let mut y = y + 44.0;

    label(&ctx, px + 26.0, y, "Exotic", 18.0, ACCENT);
    y += 14.0;
    if let Some(secs) = closed_secs {
        label(
            &ctx,
            px + 26.0,
            y + 12.0,
            &format!("closed · opens in {}", fmt_secs(secs)),
            16.0,
            TEXT_DIM,
        );
        if button(
            &ctx,
            px + pw - 26.0 - 220.0,
            y,
            220.0,
            28.0,
            &format!("Skip wait · {skip_cost} DNA"),
            dna >= skip_cost,
        ) {
            match app.zoo.skip_exotic_wait(now) {
                Ok(()) => app.set_status("opened the exotic shop early!"),
                Err(e) => app.set_status(format!("{e}")),
            }
        }
    } else {
        for (id, lbl, price, afford) in &exotics {
            if button(&ctx, px + 26.0, y, pw - 52.0, 28.0, lbl, *afford) {
                buy_exotic(app, *id, *price, now);
            }
            y += 32.0;
        }
    }
    let _ = y;

    // STUB teaser: biome merchants that will sell the new-biome fauna once the
    // segmented (per-NPC) shop ships. Their animals are catch-only for now.
    let soon: Vec<&str> = vendor::coming_soon_vendors().take(5).map(|v| v.npc_name).collect();
    if !soon.is_empty() {
        label(
            &ctx,
            px + 26.0,
            py + ph - 50.0,
            &format!("Biome merchants coming soon: {} …", soon.join(", ")),
            13.0,
            TEXT_DIM,
        );
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }
}

fn buy_exotic(app: &mut GameApp, sp: SpeciesId, price: Price, now: DateTime<Utc>) {
    let ok = match price {
        Price::Coins(c) if app.zoo.coins >= c => {
            app.zoo.coins -= c;
            true
        }
        Price::Dna(d) if app.zoo.dna_helix >= d => {
            app.zoo.dna_helix -= d;
            true
        }
        _ => false,
    };
    if !ok {
        app.set_status("can't afford that");
        return;
    }
    if app.zoo.spawn_animal_freeform(sp, 1, now).is_ok() {
        app.sync_critters();
        app.save_under_lock(now);
        app.push_notification(
            species::get(sp).display_name,
            "Purchased",
            crate::app::NotifIcon::Animal(sp),
        );
    }
}

// ───────────────────────────── Nest ─────────────────────────────

fn draw_nest(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx) {
    use uuid::Uuid;

    let Some(nest_id) = app.active_nest else {
        app.set_screen(Screen::World);
        return;
    };
    let Some(nest) = app.zoo.nests.iter().find(|n| n.id == nest_id).cloned() else {
        app.set_screen(Screen::World);
        return;
    };

    let status = app.zoo.nest_status(nest_id, now);

    // ── Pending offspring: a finished nest holds a single critter to collect.
    // Parents were auto-released on completion, so the panel is collect-only.
    if let Some(off_id) = nest.offspring {
        let (sp, name) = app
            .zoo
            .animals
            .get(&off_id)
            .map(|a| (a.species, species::get(a.species).display_name.to_string()))
            .unwrap_or(("field_mouse", "?".to_string()));

        let pw = 420.0;
        let ph = 320.0;
        let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);
        panel(&ctx, px, py, pw, ph);
        title(&ctx, px + 26.0, py + 30.0, "NEST");
        label(&ctx, px + 26.0, py + 62.0, "A new arrival hatched!", 18.0, ACCENT);

        // Centered offspring thumbnail.
        let thumb = 130.0;
        if let Some(tx) = app.textures.animal(sp) {
            let aspect = if tx.height() > 0.0 { tx.width() / tx.height() } else { 1.0 };
            let w = thumb * aspect;
            let tp = ctx.pt(px + (pw - w) * 0.5, py + 86.0);
            draw_texture_ex(
                &tx,
                tp.x,
                tp.y,
                fade(WHITE, ctx.alpha),
                DrawTextureParams { dest_size: Some(vec2(w, thumb) * ctx.scale), ..Default::default() },
            );
        }
        let nd = measure_text(&name, None, (20.0 * ctx.scale) as u16, 1.0);
        label(&ctx, px + (pw - nd.width / ctx.scale) * 0.5, py + 86.0 + thumb + 26.0, &name, 20.0, TEXT);

        let act_y = py + ph - 42.0;
        if button(&ctx, px + 26.0, act_y, 180.0, 30.0, "Collect", true) {
            match app.zoo.nest_collect(nest_id, now) {
                Ok((species, is_hybrid)) => {
                    app.sync_critters();
                    app.save_under_lock(now);
                    let dname = species::get(species).display_name;
                    let amount = if is_hybrid { "Hybrid! +1 DNA" } else { "Collected" };
                    app.push_notification(dname, amount, crate::app::NotifIcon::Animal(species));
                    app.set_screen(Screen::World);
                }
                Err(e) => app.set_status(format!("{e}")),
            }
        }
        if button(&ctx, px + pw - 26.0 - 120.0, act_y, 120.0, 30.0, "Close  [Esc]", true) {
            app.set_screen(Screen::World);
        }
        return;
    }

    // Per-slot occupant info: (animal id, species, name, level, breeding).
    let mut slots: [Option<(Uuid, SpeciesId, String, u8, bool)>; 2] = [None, None];
    for (i, s) in nest.slots.iter().enumerate() {
        if let Some(id) = s {
            if let Some(a) = app.zoo.animals.get(id) {
                let breeding = matches!(a.state, crate::game::AnimalState::Breeding { .. });
                slots[i] = Some((
                    *id,
                    a.species,
                    species::get(a.species).display_name.to_string(),
                    a.level,
                    breeding,
                ));
            }
        }
    }
    let occupied = slots.iter().filter(|s| s.is_some()).count();
    let has_free = occupied < 2;
    let outcomes = app.zoo.nest_outcomes(nest_id);
    // How many valid animals are on the follow chain (the deposit pool).
    let follower_count = app
        .following
        .iter()
        .filter(|id| app.zoo.animals.contains_key(id))
        .count();
    let show_chooser = has_free && follower_count > 0;

    let pw = 540.0;
    let out_rows = outcomes.len().max(1);
    let ph = 340.0 + out_rows as f32 * 24.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "NEST");

    // Two slot thumbnails near the top.
    let thumb = 92.0;
    let slot_w = 150.0;
    let gap = 30.0;
    let row_x = px + (pw - (slot_w * 2.0 + gap)) * 0.5;
    let row_y = py + 52.0;
    for i in 0..2 {
        let sx = row_x + i as f32 * (slot_w + gap);
        // Slot frame.
        let fp = ctx.pt(sx, row_y);
        let fs = vec2(slot_w, thumb + 16.0) * ctx.scale;
        ui::rrect(fp.x, fp.y, fs.x, fs.y, 10.0 * ctx.scale, fade(BTN_DISABLED, ctx.alpha));
        if let Some((_, sp, name, level, breeding)) = &slots[i] {
            // Sprite, centered in the frame.
            if let Some(tx) = app.textures.animal(sp) {
                let aspect = if tx.height() > 0.0 { tx.width() / tx.height() } else { 1.0 };
                let w = thumb * aspect;
                let tp = ctx.pt(sx + (slot_w - w) * 0.5, row_y + 8.0);
                draw_texture_ex(
                    &tx,
                    tp.x,
                    tp.y,
                    fade(WHITE, ctx.alpha),
                    DrawTextureParams {
                        dest_size: Some(vec2(w, thumb) * ctx.scale),
                        ..Default::default()
                    },
                );
            }
            label(&ctx, sx + 10.0, row_y + thumb + 8.0, &format!("{name} L{level}"), 16.0, TEXT);
            let _ = breeding;
        } else {
            label(&ctx, sx + slot_w * 0.5 - 24.0, row_y + thumb * 0.5 + 8.0, "empty", 16.0, TEXT_DIM);
        }
    }

    // Remove buttons under each occupied, non-breeding slot.
    let mut remove_slot: Option<usize> = None;
    let rm_y = row_y + thumb + 22.0;
    for i in 0..2 {
        if let Some((_, _, _, _, breeding)) = &slots[i] {
            let sx = row_x + i as f32 * (slot_w + gap);
            let enabled = !breeding;
            if button(&ctx, sx, rm_y, slot_w, 26.0, "Remove", enabled) {
                remove_slot = Some(i);
            }
        }
    }

    // Status line.
    let status_y = rm_y + 40.0;
    let status_txt = match status {
        NestStatus::Empty => "Empty — deposit an animal".to_string(),
        NestStatus::Partial => "Add a second animal to breed".to_string(),
        NestStatus::ReadyToBreed => "Ready to breed".to_string(),
        NestStatus::Incompatible => "These two can't crossbreed".to_string(),
        NestStatus::Breeding(ends) => format!("Breeding — {}", fmt_secs((ends - now).num_seconds())),
        NestStatus::ReadyToCollect => "Offspring ready to collect!".to_string(),
    };
    let status_col = match status {
        NestStatus::ReadyToBreed | NestStatus::ReadyToCollect => ACCENT,
        NestStatus::Incompatible => color_u8!(211, 165, 92, 255),
        _ => TEXT,
    };
    label(&ctx, px + 26.0, status_y, &status_txt, 18.0, status_col);

    // Possible outcomes.
    let mut oy = status_y + 28.0;
    label(&ctx, px + 26.0, oy, "Possible outcomes", 16.0, ACCENT);
    oy += 22.0;
    if outcomes.is_empty() {
        label(&ctx, px + 40.0, oy, "—", 16.0, TEXT_DIM);
        oy += 24.0;
    }
    for (sp, pct, discovered) in &outcomes {
        let name = if *discovered { species::get(*sp).display_name } else { "????" };
        label(&ctx, px + 40.0, oy, name, 16.0, if *discovered { TEXT } else { TEXT_DIM });
        right_text(&ctx, px + pw - 26.0, oy, &format!("{pct}%"));
        oy += 24.0;
    }

    // Action buttons across the bottom. (Collecting is handled by the dedicated
    // offspring panel above, which returns early.)
    let act_y = py + ph - 42.0;
    let mut do_deposit = false;
    let mut do_breed = false;
    let mut bx = px + 26.0;
    if show_chooser {
        let lbl = if follower_count == 1 { "Deposit animal" } else { "Deposit…" };
        if button(&ctx, bx, act_y, 150.0, 30.0, lbl, true) {
            do_deposit = true;
        }
        bx += 160.0;
    }
    if matches!(status, NestStatus::ReadyToBreed) && button(&ctx, bx, act_y, 120.0, 30.0, "Breed", true) {
        do_breed = true;
    }
    if button(&ctx, px + pw - 26.0 - 120.0, act_y, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }

    // ── Apply the chosen action (after all draws / hit-tests). ──
    if let Some(slot) = remove_slot {
        match app.zoo.remove_from_nest(nest_id, slot) {
            Ok(_) => {
                app.sync_critters();
                app.save_under_lock(now);
                app.set_status("removed from nest");
            }
            Err(e) => app.set_status(format!("{e}")),
        }
    } else if do_deposit {
        // Hand off to the full-screen spotlight picker.
        app.enter_deposit_mode(nest_id);
    } else if do_breed {
        match app.zoo.nest_breed(nest_id, now) {
            Ok(_) => {
                app.save_under_lock(now);
                app.set_status("breeding started");
            }
            Err(e) => app.set_status(format!("{e}")),
        }
    }
}

// ────────────────────────── Food structure ──────────────────────────

fn draw_structure(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx) {
    use crate::game::structure::{MAX_STRUCTURE_LEVEL, structure_upgrade_cost};

    let Some(sid) = app.active_structure else {
        app.set_screen(Screen::World);
        return;
    };
    let Some(s) = app.zoo.structures.iter().find(|s| s.id == sid) else {
        app.set_screen(Screen::World);
        return;
    };
    let level = s.level;
    let rate = s.food_rate();
    let stored = s.stored_at(now);
    let cap = s.food_cap();
    let at_max = level >= MAX_STRUCTURE_LEVEL;
    let upgrade_cost = structure_upgrade_cost(level);

    let pw = 440.0;
    let ph = 250.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "FOOD STRUCTURE");
    right_text(&ctx, px + pw - 26.0, py + 30.0, &format!("Lv {level} / {MAX_STRUCTURE_LEVEL}"));

    let mut y = py + 72.0;
    for (lbl, val) in [
        ("Food / sec".to_string(), format!("{rate:.2}")),
        ("Stored".to_string(), format!("{stored} / {cap}")),
        ("Banked food".to_string(), format!("{}", app.zoo.food)),
    ] {
        label(&ctx, px + 26.0, y, &lbl, 18.0, TEXT_DIM);
        right_text(&ctx, px + pw - 26.0, y, &val);
        y += 30.0;
    }

    let act_y = py + ph - 42.0;
    let mut do_collect = false;
    let mut do_upgrade = false;
    if button(&ctx, px + 26.0, act_y, 150.0, 30.0, "Collect food", stored > 0) {
        do_collect = true;
    }
    let up_label = if at_max {
        "Max level".to_string()
    } else {
        format!("Upgrade · {upgrade_cost} c")
    };
    if button(&ctx, px + 26.0 + 160.0, act_y, 170.0, 30.0, &up_label, !at_max && app.zoo.coins >= upgrade_cost) {
        do_upgrade = true;
    }
    if button(&ctx, px + pw - 26.0 - 90.0, act_y, 90.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }

    if do_collect {
        let g = app.zoo.collect_food_structure(sid, now);
        app.save_under_lock(now);
        app.set_status(format!("collected {g} food"));
    } else if do_upgrade {
        match app.zoo.upgrade_structure(sid, now) {
            Ok(l) => {
                app.save_under_lock(now);
                app.set_status(format!("upgraded to level {l}"));
            }
            Err(e) => app.set_status(format!("{e}")),
        }
    }
}

// ──────────────────────────── widgets ───────────────────────────

fn panel(ctx: &Ctx, x: f32, y: f32, w: f32, h: f32) {
    let p = ctx.pt(x, y);
    let s = vec2(w, h) * ctx.scale;
    let r = 16.0 * ctx.scale;
    ui::rrect(p.x, p.y, s.x, s.y, r, fade(PANEL, ctx.alpha));
    ui::rrect_outline(p.x, p.y, s.x, s.y, r, fade(PANEL_EDGE, ctx.alpha));
}

fn title(ctx: &Ctx, x: f32, y: f32, text: &str) {
    let p = ctx.pt(x, y);
    draw_text(text, p.x, p.y, 26.0 * ctx.scale, fade(TEXT, ctx.alpha));
}

fn right_text(ctx: &Ctx, right_x: f32, y: f32, text: &str) {
    let fs = 18.0 * ctx.scale;
    let dim = measure_text(text, None, fs as u16, 1.0);
    let p = ctx.pt(right_x, y);
    draw_text(text, p.x - dim.width, p.y, fs, fade(TEXT_DIM, ctx.alpha));
}

fn label(ctx: &Ctx, x: f32, y: f32, text: &str, size: f32, color: Color) {
    let p = ctx.pt(x, y);
    draw_text(text, p.x, p.y, size * ctx.scale, fade(color, ctx.alpha));
}

/// A rounded button. Returns true if clicked this frame (when interactive).
fn button(ctx: &Ctx, x: f32, y: f32, w: f32, h: f32, text: &str, enabled: bool) -> bool {
    let p = ctx.pt(x, y);
    let s = vec2(w, h) * ctx.scale;
    let over = ctx.interactive
        && enabled
        && ctx.mouse.x >= p.x
        && ctx.mouse.x <= p.x + s.x
        && ctx.mouse.y >= p.y
        && ctx.mouse.y <= p.y + s.y;
    let bg = if !enabled {
        BTN_DISABLED
    } else if over {
        BTN_HOVER
    } else {
        BTN
    };
    ui::rrect(p.x, p.y, s.x, s.y, 7.0 * ctx.scale, fade(bg, ctx.alpha));
    let fs = 17.0 * ctx.scale;
    let dim = measure_text(text, None, fs as u16, 1.0);
    let tc = if enabled { TEXT } else { TEXT_DIM };
    draw_text(
        text,
        p.x + (s.x - dim.width) * 0.5,
        p.y + s.y * 0.5 + dim.offset_y * 0.35,
        fs,
        fade(tc, ctx.alpha),
    );
    over && ctx.click
}

fn fmt_secs(secs: i64) -> String {
    let s = secs.max(0);
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}
