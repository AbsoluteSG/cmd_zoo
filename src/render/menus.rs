//! Hand-drawn menu overlays (Shop, Breeding). Plain but tidy: a rounded, wide
//! panel that scales/fades in on open and out on close, drawn over the blurred
//! + darkened world (the blur/darken backdrop is handled in `GameApp::draw`).
//! Immediate-mode: buttons hit-test the cursor inline and call domain methods.

use chrono::{DateTime, Utc};
use macroquad::prelude::*;

use crate::app::{GameApp, PostEffect, Screen};
use crate::game::exotic_shop::{self, Price};
use crate::game::npc::{NpcRole, ShopListingKind};
use crate::game::species::{self, IncomeKind, SpeciesId};
use crate::game::structure_kind;
use crate::game::zoo::{MAX_NESTS, nest_purchase_cost};

const PANEL: Color = color_u8!(24, 27, 33, 250);
const PANEL_EDGE: Color = color_u8!(64, 72, 86, 255);
const BTN: Color = color_u8!(46, 52, 64, 255);
const BTN_HOVER: Color = color_u8!(70, 80, 98, 255);
const BTN_DISABLED: Color = color_u8!(30, 34, 41, 255);
const ACCENT: Color = color_u8!(123, 207, 167, 255);
const TEXT: Color = color_u8!(231, 233, 236, 255);
const TEXT_DIM: Color = color_u8!(150, 156, 166, 255);

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
        Screen::Breeding => draw_breeding(app, now, ctx, None),
        Screen::Settings => draw_settings(app, now, ctx),
        Screen::NpcShop(idx) => draw_npc_shop(app, now, ctx, idx),
        Screen::NpcBreeder(idx) => draw_npc_breeder(app, now, ctx, idx),
        Screen::World => {}
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

    // ── Online (M2 stub: loopback demo until Steam transport lands) ──────
    y += 8.0;
    label(&ctx, px + 26.0, y, "Online", 18.0, ACCENT);
    y += 18.0;
    let hosting = app.is_hosting();
    let toggle_label = if hosting {
        "Stop hosting"
    } else {
        "Open zoo (local demo)"
    };
    if button(&ctx, px + 26.0, y, pw - 52.0, 30.0, toggle_label, true) {
        if hosting {
            app.stop_hosting();
        } else {
            app.start_local_demo();
        }
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
            "(local demo spawns an in-process visitor.)",
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
    rrect(p.x, p.y, s.x, s.y, 6.0 * ctx.scale, fade(BTN, ctx.alpha));
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

// ───────────────────────────── NPC Shop ─────────────────────────

fn draw_npc_shop(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx, npc_idx: usize) {
    // Extract NPC data before borrowing app mutably in button handlers.
    let (greeting, animal_ids, struct_ids, sells_nests) = {
        let npc = &app.npcs[npc_idx];
        let NpcRole::ShopKeeper { inventory, dialog } = &npc.role else { return };
        let a: Vec<_> = inventory
            .listings
            .iter()
            .filter_map(|l| match l.kind {
                ShopListingKind::Animal(id) => Some(id),
                _ => None,
            })
            .collect();
        let s: Vec<_> = inventory
            .listings
            .iter()
            .filter_map(|l| match l.kind {
                ShopListingKind::Structure(id) => Some(id),
                _ => None,
            })
            .collect();
        let nests = inventory.listings.iter().any(|l| matches!(l.kind, ShopListingKind::NestSlot));
        (dialog.greeting, a, s, nests)
    };

    let coins = app.zoo.coins;
    let nest_cost = nest_purchase_cost(app.zoo.nest_count);

    let animals: Vec<(SpeciesId, String, bool)> = animal_ids
        .iter()
        .map(|&id| {
            let d = species::get(id);
            let (afford, cur) = match d.purchase_currency {
                IncomeKind::Coin => (coins >= d.purchase_cost, "c"),
                IncomeKind::DnaHelix => (app.zoo.dna_helix >= d.purchase_cost, "DNA"),
            };
            (id, format!("{}  ·  {} {}", d.display_name, d.purchase_cost, cur), afford)
        })
        .collect();

    let structures: Vec<(&str, String, bool)> = struct_ids
        .iter()
        .map(|&id| {
            let d = structure_kind::get(id);
            (id, format!("{}  ·  {} c", d.display_name, d.purchase_cost), coins >= d.purchase_cost)
        })
        .collect();

    let pw = 760.0;
    let anim_rows = animals.len().div_ceil(2);
    let ph = 30.0 + 96.0 + anim_rows as f32 * 34.0
        + 24.0 + structures.len() as f32 * 34.0
        + if sells_nests { 44.0 } else { 0.0 }
        + 50.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    label(&ctx, px + 26.0, py + 24.0, greeting, 15.0, TEXT_DIM);
    title(&ctx, px + 26.0, py + 50.0, "SHOP");
    right_text(
        &ctx,
        px + pw - 26.0,
        py + 50.0,
        &format!("{coins} coins    {} DNA", app.zoo.dna_helix),
    );

    label(&ctx, px + 26.0, py + 82.0, "Animals", 18.0, ACCENT);
    let col_w = (pw - 52.0 - 12.0) * 0.5;
    let mut y = py + 96.0;
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
                    app.set_status(format!("bought {}", species::get(*id).display_name));
                }
                Err(e) => app.set_status(format!("{e}")),
            }
        }
    }

    y += 44.0;
    label(&ctx, px + 26.0, y, "Food Structures", 18.0, ACCENT);
    y += 14.0;
    for (id, lbl, afford) in &structures {
        if button(&ctx, px + 26.0, y, pw - 52.0, 28.0, lbl, *afford) {
            match app.zoo.buy_structure(id, now) {
                Ok(_) => {
                    app.save_under_lock(now);
                    app.set_status(format!("built {}", structure_kind::get(id).display_name));
                }
                Err(e) => app.set_status(format!("{e}")),
            }
        }
        y += 34.0;
    }

    if sells_nests && app.zoo.nest_count < MAX_NESTS {
        y += 6.0;
        if button(
            &ctx,
            px + 26.0,
            y,
            pw - 52.0,
            28.0,
            &format!("Buy Breeding Nest  ·  {nest_cost} c"),
            coins >= nest_cost,
        ) {
            match app.zoo.buy_nest() {
                Ok(_) => {
                    app.save_under_lock(now);
                    app.set_status("bought a nest");
                }
                Err(e) => app.set_status(format!("{e}")),
            }
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

    let mut animals: Vec<(SpeciesId, String, bool)> = species::all_purchasable()
        .map(|d| {
            let (afford, cur) = match d.purchase_currency {
                IncomeKind::Coin => (coins >= d.purchase_cost, "c"),
                IncomeKind::DnaHelix => (dna >= d.purchase_cost, "DNA"),
            };
            (d.id, format!("{}  ·  {} {}", d.display_name, d.purchase_cost, cur), afford)
        })
        .collect();
    animals.sort_by_key(|(id, _, _)| species::get(*id).purchase_cost);

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
    let ph = 96.0 + rows as f32 * 34.0 + 150.0;
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
                    app.set_status(format!("bought {}", species::get(*id).display_name));
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
        app.set_status(format!("bought {}", species::get(sp).display_name));
    }
}

// ─────────────────────────── Breeding ───────────────────────────

fn draw_npc_breeder(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx, npc_idx: usize) {
    let greeting = match &app.npcs[npc_idx].role {
        NpcRole::Breeder { dialog } => dialog.greeting,
        _ => return,
    };
    draw_breeding(app, now, ctx, Some(greeting));
}

fn draw_breeding(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx, greeting: Option<&str>) {
    let owned = app.zoo.nest_count;
    let busy = app.zoo.active_breeding_pair_count() as u8;
    let nest_cost = nest_purchase_cost(owned);

    let gestations: Vec<(uuid::Uuid, String, bool)> = app
        .active_gestations()
        .iter()
        .map(|(a, b, ends_at)| {
            let na = species::get(a.species).display_name;
            let nb = species::get(b.species).display_name;
            let ready = *ends_at <= now;
            let label = if ready {
                format!("{na} + {nb}  —  READY")
            } else {
                format!("{na} + {nb}  —  {}", fmt_secs((*ends_at - now).num_seconds()))
            };
            (a.id, label, ready)
        })
        .collect();

    let candidates: Vec<(uuid::Uuid, String)> = app
        .breeding_candidates(app.breeding_first_pick)
        .iter()
        .map(|a| (a.id, format!("{} L{}", species::get(a.species).display_name, a.level)))
        .collect();

    let first = app.breeding_first_pick;
    let second = app.breeding_second_pick;
    let name_of = |id: Option<uuid::Uuid>| -> String {
        match id.and_then(|i| app.zoo.animals.get(&i)) {
            Some(a) => format!("{} L{}", species::get(a.species).display_name, a.level),
            None => "—".to_string(),
        }
    };
    let a_label = name_of(first);
    let b_label = name_of(second);
    let can_breed = first.is_some() && second.is_some();

    let pw = 760.0;
    let cand_rows = candidates.len().div_ceil(2).max(1);
    let greeting_h = if greeting.is_some() { 28.0 } else { 0.0 };
    let ph = 150.0 + gestations.len() as f32 * 34.0 + cand_rows as f32 * 32.0 + 130.0 + greeting_h;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    if let Some(g) = greeting {
        label(&ctx, px + 26.0, py + 24.0, g, 15.0, TEXT_DIM);
    }
    title(&ctx, px + 26.0, py + 30.0 + greeting_h, "BREEDING");
    right_text(
        &ctx,
        px + pw - 26.0,
        py + 30.0 + greeting_h,
        &format!("nests {busy}/{owned}  (cap {MAX_NESTS})"),
    );
    if owned < MAX_NESTS
        && button(
            &ctx,
            px + pw - 26.0 - 200.0,
            py + 44.0 + greeting_h,
            200.0,
            26.0,
            &format!("Buy nest · {nest_cost} c"),
            app.zoo.coins >= nest_cost,
        )
    {
        match app.zoo.buy_nest() {
            Ok(_) => {
                app.save_under_lock(now);
                app.set_status("bought a nest");
            }
            Err(e) => app.set_status(format!("{e}")),
        }
    }

    let mut y = py + 82.0 + greeting_h;
    label(&ctx, px + 26.0, y, "Active", 18.0, ACCENT);
    y += 14.0;
    if gestations.is_empty() {
        label(&ctx, px + 26.0, y + 12.0, "(none)", 16.0, TEXT_DIM);
        y += 28.0;
    }
    for (id, lbl, ready) in &gestations {
        label(&ctx, px + 26.0, y + 18.0, lbl, 16.0, if *ready { ACCENT } else { TEXT });
        let action = if *ready { "Redeem" } else { "Cancel" };
        if button(&ctx, px + pw - 26.0 - 120.0, y, 120.0, 26.0, action, true) {
            if *ready {
                match app.zoo.claim_completed_breeding(*id, now) {
                    Ok(c) => {
                        app.sync_critters();
                        app.save_under_lock(now);
                        let name = species::get(c.offspring_species).display_name;
                        app.set_status(if c.is_hybrid_drop {
                            format!("{name}!  +1 DNA")
                        } else {
                            format!("a {name} hatched")
                        });
                    }
                    Err(e) => app.set_status(format!("{e}")),
                }
            } else {
                let _ = app.zoo.cancel_breeding(*id, now);
                app.save_under_lock(now);
                app.set_status("breeding cancelled");
            }
        }
        y += 34.0;
    }

    y += 8.0;
    label(&ctx, px + 26.0, y, &format!("A: {a_label}     B: {b_label}"), 16.0, TEXT);
    if can_breed && button(&ctx, px + pw - 26.0 - 200.0, y - 18.0, 96.0, 26.0, "BREED", true) {
        let (a, b) = (first.unwrap(), second.unwrap());
        match app.zoo.start_breeding(a, b, now) {
            Ok(_) => {
                app.breeding_first_pick = None;
                app.breeding_second_pick = None;
                app.save_under_lock(now);
                app.set_status("breeding started");
            }
            Err(e) => app.set_status(format!("{e}")),
        }
    }
    if (first.is_some() || second.is_some())
        && button(&ctx, px + pw - 26.0 - 96.0, y - 18.0, 96.0, 26.0, "Clear", true)
    {
        app.breeding_first_pick = None;
        app.breeding_second_pick = None;
    }
    y += 22.0;

    label(&ctx, px + 26.0, y, "Pick a cross-species pair", 18.0, ACCENT);
    y += 14.0;
    if candidates.is_empty() {
        label(&ctx, px + 26.0, y + 12.0, "(no eligible idle animals)", 16.0, TEXT_DIM);
    }
    let col_w = (pw - 52.0 - 12.0) * 0.5;
    let start_y = y;
    for (i, (id, lbl)) in candidates.iter().enumerate() {
        let col = (i % 2) as f32;
        let bx = px + 26.0 + col * (col_w + 12.0);
        let by = start_y + (i / 2) as f32 * 32.0;
        let tag = if first.is_none() { "A" } else { "B" };
        if button(&ctx, bx, by, col_w, 28.0, &format!("Pick {tag}:  {lbl}"), true) {
            if app.breeding_first_pick.is_none() {
                app.breeding_first_pick = Some(*id);
            } else if app.breeding_second_pick.is_none() {
                app.breeding_second_pick = Some(*id);
            }
        }
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }
}

// ──────────────────────────── widgets ───────────────────────────

fn panel(ctx: &Ctx, x: f32, y: f32, w: f32, h: f32) {
    let p = ctx.pt(x, y);
    let s = vec2(w, h) * ctx.scale;
    let r = 16.0 * ctx.scale;
    rrect(p.x, p.y, s.x, s.y, r, fade(PANEL, ctx.alpha));
    rrect_outline(p.x, p.y, s.x, s.y, r, fade(PANEL_EDGE, ctx.alpha));
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
    rrect(p.x, p.y, s.x, s.y, 7.0 * ctx.scale, fade(bg, ctx.alpha));
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

/// Filled rounded rectangle (edges + corner discs).
fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    draw_rectangle(x + r, y, w - 2.0 * r, h, color);
    draw_rectangle(x, y + r, w, h - 2.0 * r, color);
    draw_circle(x + r, y + r, r, color);
    draw_circle(x + w - r, y + r, r, color);
    draw_circle(x + r, y + h - r, r, color);
    draw_circle(x + w - r, y + h - r, r, color);
}

fn rrect_outline(x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    draw_line(x + r, y, x + w - r, y, 1.5, color);
    draw_line(x + r, y + h, x + w - r, y + h, 1.5, color);
    draw_line(x, y + r, x, y + h - r, 1.5, color);
    draw_line(x + w, y + r, x + w, y + h - r, 1.5, color);
}

fn fade(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a)
}

/// easeOutBack — overshoots slightly before settling, for a subtle pop.
fn ease_out_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    let u = t - 1.0;
    1.0 + c3 * u * u * u + c1 * u * u
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
