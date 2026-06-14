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
use crate::render::ui::{
    self, ACCENT, PANEL, PANEL_EDGE, STATUS_AMBER, TEXT, TEXT_DIM, ease_out_back, fade,
};
use crate::render::view;

const BTN: Color = color_u8!(46, 52, 64, 255);
const BTN_HOVER: Color = color_u8!(70, 80, 98, 255);
const BTN_DISABLED: Color = color_u8!(30, 34, 41, 255);

/// Per-frame draw context carrying the scale/fade transform, cursor state, and
/// the gamepad focus frame. Holds only `Copy`/shared-ref fields so it stays cheap
/// to pass by value; `focus` is a shared `&` with interior mutability.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    center: Vec2,
    scale: f32,
    alpha: f32,
    mouse: Vec2,
    click: bool,
    interactive: bool,
    focus: &'a crate::render::focus::FocusFrame,
}

impl Ctx<'_> {
    /// Transform a logical (full-size) point into the animated screen point.
    fn pt(&self, x: f32, y: f32) -> Vec2 {
        self.center + (vec2(x, y) - self.center) * self.scale
    }
}

/// Shift a menu's pivot to one side so the panel pops up on the screen edge
/// *opposite* the player (mirroring the inspect panel), keeping the NPC the
/// player is talking to visible. Used by the NPC interaction shops.
fn side_ctx<'a>(app: &GameApp, mut ctx: Ctx<'a>) -> Ctx<'a> {
    let apos = app.session.my_avatar().pos;
    let screen = view::world_to_screen(apos, &app.camera);
    // Player on the left half → panel on the right, and vice versa.
    let to_right = screen.x < screen_width() * 0.5;
    let off = screen_width() * 0.22;
    ctx.center.x = screen_width() * 0.5 + if to_right { off } else { -off };
    ctx
}

pub fn draw(app: &mut GameApp, now: DateTime<Utc>) {
    if app.menu_t <= 0.001 {
        return;
    }
    let t = app.menu_t.clamp(0.0, 1.0);
    let eased = 0.6 + 0.4 * ease_out_back(t); // slight pop past 1.0 mid-open
    let interactive = app.screen != Screen::World && app.menu_t > 0.9;
    // Gamepad focus frame for this draw: rings shown when the pad is active and
    // the panel is interactive; confirm activates the focused widget.
    let focus = app
        .focus_nav
        .frame(interactive && app.gamepad_active(), interactive && app.ui_confirm());
    let ctx = Ctx {
        center: vec2(screen_width() * 0.5, screen_height() * 0.5),
        scale: eased,
        alpha: t,
        mouse: {
            let (mx, my) = mouse_position();
            vec2(mx, my)
        },
        click: is_mouse_button_pressed(MouseButton::Left),
        interactive,
        focus: &focus,
    };
    match app.shown_menu {
        Screen::Shop => draw_shop(app, now, ctx),
        Screen::Upgrades => draw_upgrades(app, now, ctx),
        Screen::Settings => draw_settings(app, now, ctx),
        Screen::Waypoints => draw_waypoints(app, now, ctx),
        Screen::Nest => draw_nest(app, now, ctx),
        Screen::Structure => draw_structure(app, now, ctx),
        Screen::Pedestal => draw_pedestal(app, now, ctx),
        Screen::Merchant => draw_merchant(app, now, ctx),
        Screen::ExoticShop => draw_exotic_shop(app, now, ctx),
        Screen::Disconnected => draw_disconnected(app, now, ctx),
        Screen::Player => draw_player(app, now, ctx),
        Screen::ExpeditionBoard => draw_expedition_board(app, now, ctx),
        Screen::Collections => draw_collections(app, now, ctx),
        Screen::World => {}
    }
    // Store this frame's widget rects for next-frame directional navigation.
    app.focus_nav.commit(focus.take_rects());
}

// ─────────────────────────── Player (co-op) ───────────────────────────

/// Press-E-on-a-player panel. The host can grant/revoke a visitor's sell
/// permission here; a visitor sees a read-only view of another player.
fn draw_player(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
    use crate::game::action::Action;
    use crate::game::visitor::PermissionSet;

    let Some(target) = app.active_player else {
        app.set_screen(Screen::World);
        return;
    };

    let pw = 440.0;
    let ph = 200.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);
    panel(&ctx, px, py, pw, ph);

    // Look up the target's name + permissions from the (shared) visitors map.
    let rec = app.zoo.visitors.get(&target);
    let name = rec.map(|r| r.display_name.clone()).unwrap_or_else(|| "Player".to_string());
    let has_sell = rec.is_some_and(|r| r.permissions.has(PermissionSet::SELL));

    title(&ctx, px + 26.0, py + 34.0, &name);

    // Only the host (not a guest) can manage permissions, and only for a player
    // that has a visitor record (i.e. an actual visitor, not the host itself).
    if !app.is_guest() && rec.is_some() {
        label(&ctx, px + 26.0, py + 70.0, "Permissions", 16.0, ACCENT);
        let lbl = if has_sell { "Revoke sell permission" } else { "Grant sell permission" };
        if button(&ctx, px + 26.0, py + 84.0, pw - 52.0, 32.0, lbl, true) {
            app.dispatch(Action::GrantPermission { target, bit: PermissionSet::SELL, grant: !has_sell }, now);
        }
        label(
            &ctx,
            px + 26.0,
            py + 128.0,
            "Visitors can do everything except sell unless granted.",
            13.0,
            TEXT_DIM,
        );
    } else {
        let status = if has_sell { "Can sell here." } else { "Cannot sell here." };
        label(&ctx, px + 26.0, py + 74.0, status, 15.0, TEXT_DIM);
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.active_player = None;
        app.set_screen(Screen::World);
    }
}

// ─────────────────────────── Disconnected ───────────────────────────

/// Shown to a visitor when the host's zoo goes away. Offers to return to the
/// player's own zoo (the only action — the world behind is a frozen mirror).
fn draw_disconnected(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
    let pw = 460.0;
    let ph = 180.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);
    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 34.0, "DISCONNECTED");
    let msg = app
        .disconnect_reason
        .map(|r| r.message())
        .unwrap_or("Lost connection to the host.");
    label(&ctx, px + 26.0, py + 70.0, msg, 16.0, TEXT_DIM);
    label(&ctx, px + 26.0, py + 92.0, "Your own zoo kept running while you were away.", 14.0, TEXT_DIM);
    if button(&ctx, px + (pw - 240.0) * 0.5, py + ph - 50.0, 240.0, 34.0, "Return to your zoo", true) {
        app.end_visiting(now);
    }
}

// ─────────────────────────── Waypoints ───────────────────────────

fn draw_waypoints(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {

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

    // Default destination: the home zoo (this player's plot origin).
    if button(&ctx, px + 26.0, y, tp_w, 32.0, "Home Zoo", true) {
        let home = app.zoo.plot_origin;
        app.teleport_to(home);
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

fn draw_settings(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
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
    // +56 for the Grass quality row.
    let ph = 130.0 + effects.len() as f32 * 40.0 + 56.0 + online_block_h + 30.0;
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
    // While visiting someone else's zoo, the only online control is "Leave".
    if app.is_guest() {
        label(&ctx, px + 26.0, y + 6.0, "You're visiting another player's zoo.", 15.0, TEXT_DIM);
        y += 28.0;
        if button(&ctx, px + 26.0, y, pw - 52.0, 30.0, "Leave zoo", true) {
            app.end_visiting(now);
            return;
        }
        if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
            app.set_screen(Screen::World);
        }
        return;
    }
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
        // Prominent: "Your code" caption + the code itself large in accent, so
        // the host reads off the code to share — visually distinct from the
        // "join a friend" input box below.
        label(&ctx, px + 26.0, y + 10.0, "Your code (share to invite):", 14.0, TEXT_DIM);
        label(&ctx, px + 26.0, y + 36.0, &code, 26.0, ACCENT);
        label(&ctx, px + 26.0 + 200.0, y + 36.0, &format!("{peers}/3 visitors"), 16.0, TEXT_DIM);
        y += 18.0; // a touch more room for the larger code line
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
    let code_len = crate::net::protocol::CODE_LEN;
    let shown = format!("{:_<width$}", app.join_code_buffer, width = code_len);
    draw_text(
        &shown,
        p.x + 10.0,
        p.y + s.y * 0.5 + 6.0,
        18.0 * ctx.scale,
        fade(TEXT, ctx.alpha),
    );
    let join_enabled = app.join_code_buffer.len() == code_len && !hosting;
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

// ─────────────────────────── Expedition board ───────────────────────────

/// The hub's expedition board: pick a biome to launch a bounded, freshly-seeded
/// expedition into. Every biome is launchable (expeditions are how you catch
/// fauna — especially the catch-only new-biome species); the vendor roster gives
/// a stable order + flavour names. Selecting one closes the board and drops the
/// avatar into the instance.
fn draw_expedition_board(app: &mut GameApp, _now: DateTime<Utc>, ctx: Ctx<'_>) {
    use crate::game::vendor::VENDORS;

    // Two-column grid of biome buttons.
    let pw = 720.0;
    let rows = VENDORS.len().div_ceil(2);
    let ph = 110.0 + rows as f32 * 36.0 + 56.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    let power = app.power_score();

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "EXPEDITION BOARD");
    label(
        &ctx,
        px + 26.0,
        py + 62.0,
        &format!("Choose a biome — locked regions need a higher Power Score.   Your Power: {power}"),
        16.0,
        TEXT_DIM,
    );

    let col_w = (pw - 52.0 - 12.0) * 0.5;
    let mut y = py + 84.0;
    let mut launch: Option<crate::game::species::HabitatTheme> = None;
    for (i, v) in VENDORS.iter().enumerate() {
        let col = (i % 2) as f32;
        let bx = px + 26.0 + col * (col_w + 12.0);
        if i % 2 == 0 && i != 0 {
            y += 36.0;
        }
        let req = crate::game::power::required_power(v.theme);
        let locked = !crate::game::power::can_enter(power, v.theme);
        let lbl = if locked {
            format!("{}  ·  [LOCKED] Power {req}", v.theme.name())
        } else if req == 0 {
            format!("{}  ·  {}", v.theme.name(), v.npc_name)
        } else {
            format!("{}  ·  Power {req}", v.theme.name())
        };
        // Disabled button blocks the click on locked regions (server re-checks too).
        if button(&ctx, bx, y, col_w, 30.0, &lbl, !locked) {
            launch = Some(v.theme);
        }
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }

    // Defer the launch until after the panel is drawn; `launch_expedition`
    // closes this menu and teleports the avatar into the instance.
    if let Some(theme) = launch {
        app.launch_expedition(theme);
    }
}

/// Collections panel (hotkey C): own every animal in a set to claim its reward
/// (currency or an exclusive max-rank animal). Claims are permanent.
fn draw_collections(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
    use crate::game::collection::{self, Reward};
    use crate::game::species;

    let list = collection::all();
    let pw = 760.0;
    let row_h = 52.0;
    let ph = 96.0 + list.len() as f32 * row_h + 50.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "COLLECTIONS");
    label(
        &ctx,
        px + 26.0,
        py + 62.0,
        "Own every animal in a set to claim its reward — rewards are one-time.",
        16.0,
        TEXT_DIM,
    );

    // The reward as a short string + a notification icon.
    let reward_text = |r: Reward| match r {
        Reward::Coins(n) => format!("+{n} coins"),
        Reward::Dna(n) => format!("+{n} DNA"),
        Reward::Animal(sp) => format!("{} (Neon)", species::get(sp).display_name),
    };

    let mut claim: Option<&'static str> = None;
    let mut y = py + 84.0;
    for c in list {
        let claimed = app.zoo.claimed_collections.contains(c.id);
        let complete = app.zoo.collection_complete(c);
        let owned = c.required.iter().filter(|s| app.zoo.owns_species(s)).count();

        label(&ctx, px + 26.0, y + 18.0, &format!("{}  —  {}", c.name, reward_text(c.reward)), 18.0, TEXT);
        // Requirement line: each species marked owned (✓) / missing (·).
        let reqs: Vec<String> = c
            .required
            .iter()
            .map(|s| {
                let mark = if app.zoo.owns_species(s) { "✓" } else { "·" };
                format!("{} {mark}", species::get(s).display_name)
            })
            .collect();
        label(
            &ctx,
            px + 26.0,
            y + 40.0,
            &format!("{}/{}   {}", owned, c.required.len(), reqs.join("   ")),
            14.0,
            if complete { ACCENT } else { TEXT_DIM },
        );

        // Claim button on the right.
        let (lbl, enabled) = if claimed {
            ("Claimed", false)
        } else if complete {
            ("Claim", true)
        } else {
            ("Incomplete", false)
        };
        if button(&ctx, px + pw - 26.0 - 130.0, y + 10.0, 130.0, 32.0, lbl, enabled) {
            claim = Some(c.id);
        }
        y += row_h;
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }

    // Defer the claim until after drawing (it mutates the zoo).
    if let Some(id) = claim {
        if let Some(c) = collection::get(id) {
            if app.dispatch(crate::game::action::Action::ClaimCollection { id: id.to_string() }, now).is_some() {
                let icon = match c.reward {
                    Reward::Animal(sp) => crate::app::NotifIcon::Animal(sp),
                    Reward::Dna(_) => crate::app::NotifIcon::Currency("dna_helix"),
                    Reward::Coins(_) => crate::app::NotifIcon::Currency("coin"),
                };
                app.push_notification(c.name, "Collection complete!", icon);
            }
        }
    }
}

// ───────────────────────────── Shop ─────────────────────────────

fn draw_shop(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
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
            let outcome = app.dispatch(crate::game::action::Action::Purchase(id.to_string()), now);
            if outcome.is_some() {
                // Host applied it (visitor sees the buy via the next snapshot).
                app.push_notification(
                    species::get(*id).display_name,
                    "Purchased",
                    crate::app::NotifIcon::Animal(*id),
                );
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
            if app.dispatch(crate::game::action::Action::SkipExoticWait, now).is_some() {
                app.set_status("opened the exotic shop early!");
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

// ───────────────────────────── Upgrades ─────────────────────────────

/// The UPGRADES menu (key 1). Currently houses the zoo expansion; more
/// player/zoo upgrades will join it here. The animal Shop is reached via NPCs.
fn draw_upgrades(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
    use crate::game::plot::zoo_tiles_for_level;
    use crate::game::zoo::{MAX_ZOO_LEVEL, ZOO_CAPACITY_PER_LEVEL, zoo_upgrade_duration};

    let coins = app.zoo.coins;

    let pw = 560.0;
    let ph = 260.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "UPGRADES");
    right_text(&ctx, px + pw - 26.0, py + 30.0, &format!("{coins} coins"));

    // ── Zoo expansion ──────────────────────────────────────────────────────
    let lvl = app.zoo.zoo_level;
    let used = app.zoo.animals.len();
    let cap = app.zoo.max_animal_capacity();
    let tiles = zoo_tiles_for_level(lvl);

    let sx = px + 26.0;
    label(&ctx, sx, py + 74.0, "Zoo Expansion", 18.0, ACCENT);
    label(
        &ctx,
        sx,
        py + 98.0,
        &format!("{used}/{cap} animals  ·  plot {tiles}×{tiles} tiles  ·  level {lvl}"),
        15.0,
        TEXT_DIM,
    );

    let bw = pw - 52.0;
    let by = py + 120.0;
    if let Some(ends_at) = app.zoo.zoo_upgrade_finishes_at {
        let remaining = (ends_at - now).num_seconds().max(0);
        if remaining == 0 {
            if button(&ctx, sx, by, bw, 32.0, "Claim expansion", true)
                && app.dispatch(crate::game::action::Action::ClaimZooUpgrade, now).is_some()
            {
                app.set_status("Zoo expanded!");
            }
        } else {
            label(
                &ctx,
                sx,
                by + 16.0,
                &format!("building · {} remaining", fmt_secs(remaining)),
                16.0,
                TEXT_DIM,
            );
        }
    } else if lvl >= MAX_ZOO_LEVEL {
        label(&ctx, sx, by + 16.0, "Zoo is at its maximum size.", 16.0, TEXT_DIM);
    } else {
        let cost = app.zoo.zoo_upgrade_cost().unwrap_or(0);
        let secs = zoo_upgrade_duration(lvl).map(|d| d.num_seconds()).unwrap_or(0);
        let next_tiles = zoo_tiles_for_level(lvl + 1);
        label(
            &ctx,
            sx,
            by - 2.0,
            &format!("Next: plot {next_tiles}×{next_tiles}  ·  +{ZOO_CAPACITY_PER_LEVEL} capacity  ·  builds in {}",
                fmt_secs(secs)),
            14.0,
            TEXT_DIM,
        );
        let lbl = format!("Expand  ·  {cost} coins");
        if button(&ctx, sx, by + 18.0, bw, 32.0, &lbl, coins >= cost)
            && app.dispatch(crate::game::action::Action::StartZooUpgrade, now).is_some()
        {
            app.set_status("Zoo expansion under way");
        }
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }
}

fn buy_exotic(app: &mut GameApp, sp: SpeciesId, price: Price, now: DateTime<Utc>) {
    // The exotic shop charges a window-specific price, which isn't expressible as
    // a plain `Action` yet — so for co-op v1 it stays host/solo only. (Visitors
    // can still buy from the regular shop.)
    if app.is_guest() {
        app.set_status("the exotic shop is host-only in co-op for now");
        return;
    }
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
        app.after_zoo_mutation(now);
        app.push_notification(
            species::get(sp).display_name,
            "Purchased",
            crate::app::NotifIcon::Animal(sp),
        );
    }
}

// ───────────────────────────── Nest ─────────────────────────────

fn draw_nest(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
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
            use crate::game::action::{Action, ActionOutcome};
            match app.dispatch(Action::NestCollect(nest_id), now) {
                Some(ActionOutcome::Offspring { species, is_hybrid }) => {
                    let dname = species::get(species).display_name;
                    let amount = if is_hybrid { "Hybrid! +1 DNA" } else { "Collected" };
                    app.push_notification(dname, amount, crate::app::NotifIcon::Animal(species));
                    app.set_screen(Screen::World);
                }
                // Visitor: forwarded; the offspring lands via the host snapshot.
                None if app.is_guest() => app.set_screen(Screen::World),
                _ => {}
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
    use crate::game::action::Action;
    if let Some(slot) = remove_slot {
        if app.dispatch(Action::RemoveFromNest { nest: nest_id, slot }, now).is_some() {
            app.set_status("removed from nest");
        }
    } else if do_deposit {
        // Hand off to the full-screen spotlight picker.
        app.enter_deposit_mode(nest_id);
    } else if do_breed && app.dispatch(Action::NestBreed(nest_id), now).is_some() {
        app.set_status("breeding started");
    }
}

// ────────────────────────── Food structure ──────────────────────────

fn draw_structure(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
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

    use crate::game::action::{Action, ActionOutcome};
    if do_collect {
        match app.dispatch(Action::CollectFoodStructure(sid), now) {
            Some(ActionOutcome::Food(g)) => app.set_status(format!("collected {g} food")),
            None if app.is_guest() => app.set_status("collecting…"),
            _ => {}
        }
    } else if do_upgrade && app.dispatch(Action::UpgradeStructure(sid), now).is_some() {
        app.set_status("upgraded the structure");
    }
}

// ─────────────────────────── Pedestal ───────────────────────────

/// Panel for a placed pedestal: shows its dedicated animal's auto-income (if
/// any) and offers Dedicate / Release / Move / Remove. Unlike the host-gated
/// Sell, every co-op player may manage pedestals.
fn draw_pedestal(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
    use crate::game::action::Action;
    use crate::game::species::{self, IncomeKind};

    let Some(pid) = app.active_pedestal else {
        app.set_screen(Screen::World);
        return;
    };
    let Some(ped) = app.zoo.pedestals.iter().find(|p| p.id == pid) else {
        app.set_screen(Screen::World);
        return;
    };
    let dedicated = ped.animal;
    // Lock/cooldown remaining seconds (None when neither applies).
    let lock_secs = ped.lock_until().filter(|_| ped.is_locked(now)).map(|t| (t - now).num_seconds());
    let cooldown_secs = ped.cooldown_until.filter(|_| ped.on_cooldown(now)).map(|t| (t - now).num_seconds());

    let pw = 440.0;
    let ph = 250.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "PEDESTAL");

    let mut y = py + 72.0;
    match dedicated.and_then(|aid| app.zoo.animals.get(&aid)) {
        Some(a) => {
            let def = species::get(a.species);
            let cur = match def.income_kind {
                IncomeKind::Coin => "coins",
                IncomeKind::DnaHelix => "DNA",
            };
            right_text(&ctx, px + pw - 26.0, py + 30.0, &format!("Lv {}", a.level));
            for (lbl, val) in [
                ("Dedicated".to_string(), def.display_name.to_string()),
                (format!("{cur} / sec"), format!("{:.2}", a.rate_per_sec())),
                ("Stored".to_string(), format!("{} / {}", a.stored_at(now), a.storage_cap())),
            ] {
                label(&ctx, px + 26.0, y, &lbl, 18.0, TEXT_DIM);
                right_text(&ctx, px + pw - 26.0, y, &val);
                y += 30.0;
            }
            let note = match lock_secs {
                Some(s) => format!("Locked to pedestal · {} left", fmt_secs(s)),
                None => "Auto-collects when full (10× offline). Income only.".to_string(),
            };
            label(&ctx, px + 26.0, y, &note, 15.0, TEXT_DIM);
        }
        None => {
            label(
                &ctx,
                px + 26.0,
                y,
                "No animal yet. Dedicate one of your",
                17.0,
                TEXT_DIM,
            );
            label(
                &ctx,
                px + 26.0,
                y + 24.0,
                "followers to auto-collect its income.",
                17.0,
                TEXT_DIM,
            );
            if let Some(s) = cooldown_secs {
                label(&ctx, px + 26.0, y + 50.0, &format!("Cooling down · {} left", fmt_secs(s)), 15.0, STATUS_AMBER);
            }
        }
    }

    let act_y = py + ph - 42.0;
    let mut do_dedicate = false;
    let mut do_release = false;
    let mut do_move = false;
    let mut do_remove = false;
    if dedicated.is_some() {
        // Release is locked out during the 48h lock.
        if button(&ctx, px + 26.0, act_y, 110.0, 30.0, "Release", lock_secs.is_none()) {
            do_release = true;
        }
    } else if button(
        &ctx,
        px + 26.0,
        act_y,
        140.0,
        30.0,
        "Dedicate animal",
        !app.following.is_empty() && cooldown_secs.is_none(),
    ) {
        do_dedicate = true;
    }
    if button(&ctx, px + pw - 26.0 - 290.0, act_y, 90.0, 30.0, "Move", true) {
        do_move = true;
    }
    // Removing is blocked while a locked animal sits on the pedestal.
    if button(&ctx, px + pw - 26.0 - 190.0, act_y, 90.0, 30.0, "Remove", lock_secs.is_none()) {
        do_remove = true;
    }
    if button(&ctx, px + pw - 26.0 - 90.0, act_y, 90.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }

    if do_dedicate {
        app.enter_dedicate_mode(pid);
    } else if do_release && app.dispatch(Action::UndedicateAnimal(pid), now).is_some() {
        app.set_status("released the animal");
    } else if do_move {
        app.begin_move_pedestal(pid);
    } else if do_remove && app.dispatch(Action::RemovePedestal(pid), now).is_some() {
        app.set_status("removed the pedestal");
        app.set_screen(Screen::World);
    }
}

// ─────────────────────────── Merchant ───────────────────────────

/// Structure-merchant shop: buys placeable structures into the hotbar. Today
/// one offer (pedestals); the catalog is future-proofed via `merchant::offers`.
fn draw_merchant(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
    use crate::game::action::Action;
    use crate::game::merchant::{self, StructureItemKind};
    use crate::game::pedestal::{MAX_PEDESTALS, pedestal_cost};

    // NPC shop: pop up on the side opposite the player, like the inspect panel.
    let ctx = side_ctx(app, ctx);
    let pw = 480.0;
    let ph = 250.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "STRUCTURE MERCHANT");
    right_text(&ctx, px + pw - 26.0, py + 30.0, &format!("{} DNA", app.zoo.dna_helix));

    let mut y = py + 78.0;
    let mut buy_pedestal = false;
    for offer in merchant::offers() {
        label(&ctx, px + 26.0, y, offer.name, 19.0, ACCENT);
        label(&ctx, px + 26.0, y + 22.0, offer.blurb, 14.0, TEXT_DIM);
        match offer.kind {
            StructureItemKind::Pedestal => {
                let owned = app.zoo.pedestals_owned();
                label(
                    &ctx,
                    px + 26.0,
                    y + 42.0,
                    &format!("Owned {owned}/{MAX_PEDESTALS}"),
                    14.0,
                    TEXT_DIM,
                );
                match pedestal_cost(owned) {
                    Some(cost) => {
                        let lbl = format!("Buy · {cost} DNA");
                        if button(&ctx, px + pw - 26.0 - 150.0, y + 4.0, 150.0, 32.0, &lbl, app.zoo.dna_helix >= cost) {
                            buy_pedestal = true;
                        }
                    }
                    None => {
                        label(&ctx, px + pw - 26.0 - 150.0, y + 24.0, "Max owned", 16.0, TEXT_DIM);
                    }
                }
            }
        }
        y += 76.0;
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }

    if buy_pedestal {
        match app.dispatch(Action::BuyPedestalItem, now) {
            Some(_) => app.set_status("bought a pedestal — find it in your hotbar"),
            None if app.is_guest() => app.set_status("buying…"),
            None => {}
        }
    }
}

/// Exotic-merchant shop: the time-windowed exotic-animal catalog (reuses the
/// `exotic_shop` window logic + the shared `buy_exotic` path). When the window is
/// closed, offers the DNA "skip wait" to open it early. Pops up on the side
/// opposite the player, like the other NPC shops.
fn draw_exotic_shop(app: &mut GameApp, now: DateTime<Utc>, ctx: Ctx<'_>) {
    let ctx = side_ctx(app, ctx);
    let coins = app.zoo.coins;
    let dna = app.zoo.dna_helix;

    // Resolve the current (or paid-skip) window's offerings.
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

    let pw = 480.0;
    let rows = exotics.len().max(1) as f32;
    let ph = 150.0 + rows * 34.0;
    let (px, py) = (ctx.center.x - pw * 0.5, ctx.center.y - ph * 0.5);

    panel(&ctx, px, py, pw, ph);
    title(&ctx, px + 26.0, py + 30.0, "EXOTIC MERCHANT");
    right_text(&ctx, px + pw - 26.0, py + 30.0, &format!("{coins} coins    {dna} DNA"));

    let mut y = py + 74.0;
    if let Some(secs) = closed_secs {
        label(
            &ctx,
            px + 26.0,
            y + 6.0,
            &format!("Sold out · restocks in {}", fmt_secs(secs)),
            16.0,
            TEXT_DIM,
        );
        y += 36.0;
        if button(
            &ctx,
            px + 26.0,
            y,
            pw - 52.0,
            32.0,
            &format!("Restock now · {skip_cost} DNA"),
            dna >= skip_cost,
        ) {
            if app.dispatch(crate::game::action::Action::SkipExoticWait, now).is_some() {
                app.set_status("restocked the exotic merchant early!");
            }
        }
    } else {
        for (id, lbl, price, afford) in &exotics {
            if button(&ctx, px + 26.0, y, pw - 52.0, 30.0, lbl, *afford) {
                buy_exotic(app, *id, *price, now);
            }
            y += 34.0;
        }
    }

    if button(&ctx, px + pw - 26.0 - 120.0, py + ph - 42.0, 120.0, 30.0, "Close  [Esc]", true) {
        app.set_screen(Screen::World);
    }
}

// ──────────────────────────── widgets ───────────────────────────

fn panel(ctx: &Ctx<'_>, x: f32, y: f32, w: f32, h: f32) {
    let p = ctx.pt(x, y);
    let s = vec2(w, h) * ctx.scale;
    let r = 16.0 * ctx.scale;
    ui::rrect(p.x, p.y, s.x, s.y, r, fade(PANEL, ctx.alpha));
    ui::rrect_outline(p.x, p.y, s.x, s.y, r, fade(PANEL_EDGE, ctx.alpha));
}

fn title(ctx: &Ctx<'_>, x: f32, y: f32, text: &str) {
    let p = ctx.pt(x, y);
    draw_text(text, p.x, p.y, 26.0 * ctx.scale, fade(TEXT, ctx.alpha));
}

fn right_text(ctx: &Ctx<'_>, right_x: f32, y: f32, text: &str) {
    let fs = 18.0 * ctx.scale;
    let dim = measure_text(text, None, fs as u16, 1.0);
    let p = ctx.pt(right_x, y);
    draw_text(text, p.x - dim.width, p.y, fs, fade(TEXT_DIM, ctx.alpha));
}

fn label(ctx: &Ctx<'_>, x: f32, y: f32, text: &str, size: f32, color: Color) {
    let p = ctx.pt(x, y);
    draw_text(text, p.x, p.y, size * ctx.scale, fade(color, ctx.alpha));
}

/// A rounded button. Returns true if clicked this frame (when interactive).
fn button(ctx: &Ctx<'_>, x: f32, y: f32, w: f32, h: f32, text: &str, enabled: bool) -> bool {
    let p = ctx.pt(x, y);
    let s = vec2(w, h) * ctx.scale;
    // Register with the gamepad focus navigator (in draw order) — returns true
    // when this is the focused widget and the pad is the active device.
    let focused = ctx.focus.register(Rect::new(p.x, p.y, s.x, s.y)) && enabled;
    let over = ctx.interactive
        && enabled
        && ctx.mouse.x >= p.x
        && ctx.mouse.x <= p.x + s.x
        && ctx.mouse.y >= p.y
        && ctx.mouse.y <= p.y + s.y;
    let hot = over || focused;
    let bg = if !enabled {
        BTN_DISABLED
    } else if hot {
        BTN_HOVER
    } else {
        BTN
    };
    ui::rrect(p.x, p.y, s.x, s.y, 7.0 * ctx.scale, fade(bg, ctx.alpha));
    // Focus ring while the gamepad drives the UI.
    if focused {
        ui::rrect_outline(p.x, p.y, s.x, s.y, 7.0 * ctx.scale, fade(ACCENT, ctx.alpha));
    }
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
    (over && ctx.click) || (focused && ctx.focus.confirm())
}

fn fmt_secs(secs: i64) -> String {
    let s = secs.max(0);
    let (d, h, m, sec) = (s / 86_400, (s % 86_400) / 3600, (s % 3600) / 60, s % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Pre-game main menu (Solo vs Online)
// ════════════════════════════════════════════════════════════════════════════

/// Screen-space square draw rects for the two play-mode cards (Solo left, Online
/// right). The PNGs are 500×500 with the card art centred inside a transparent
/// margin; callers hit-test an inset of these (see `app::inset_rect`). Shared by
/// the renderer and the input handler so layout stays in lock-step.
pub fn main_menu_card_rects() -> (Rect, Rect) {
    let (w, h) = (screen_width(), screen_height());
    let side = (h * 0.74).min(w * 0.45); // square card side, clamped for narrow windows
    let cy = h * 0.56;
    let gap = side * 0.56; // half the centre-to-centre spacing of the two cards
    let cx = w * 0.5;
    let r = |center_x: f32| Rect::new(center_x - side * 0.5, cy - side * 0.5, side, side);
    (r(cx - gap), r(cx + gap))
}

/// Full-screen pre-game title menu: background art + vignette overlay, the two
/// play-mode cards (darkened until hovered), and the pointer-arm that reveals
/// over whichever card is hovered. All motion is driven by `app.main_menu`'s
/// smoothed tweens (updated in `GameApp::handle_main_menu`).
pub fn draw_main_menu(app: &mut GameApp) {
    let (w, h) = (screen_width(), screen_height());
    let m = app.main_menu.unwrap_or_default();
    let appear = ease_out_back(m.appear.clamp(0.0, 1.0));

    // 1. Background, cover-fit so it always fills the window.
    match app.textures.ui("main_menu_bg") {
        Some(bg) => draw_cover(&bg, w, h, fade(WHITE, 1.0)),
        None => {
            clear_background(color_u8!(22, 24, 28, 255));
        }
    }

    // 2. Vignette / mood overlay on top of the background. Drawn at partial alpha
    //    so it deepens the edges without washing the art out.
    if let Some(ov) = app.textures.ui("main_menu_overlay") {
        draw_cover(&ov, w, h, fade(WHITE, 0.55 * m.appear.clamp(0.0, 1.0)));
    }

    // 3. Title + hint text.
    let title = "WELCOME TO THE ZOO";
    let tfs = (h * 0.075).round();
    let tw = measure_text(title, app.font.as_ref(), tfs as u16, 1.0).width;
    let ty = h * 0.16 * appear.min(1.0);
    text_dropshadow(app, title, (w - tw) * 0.5, ty, tfs, color_u8!(252, 246, 230, 255));

    // 4. The two cards.
    let (solo, online) = main_menu_card_rects();
    draw_play_card(app, "solo_play", solo, m.solo_hover, appear);
    draw_play_card(app, "online_play", online, m.online_hover, appear);

    // 5. The pointer-paw over whichever card is hovered (fades/slides in). Solo's
    //    paw rises from the bottom of the screen; Online's enters from the right.
    draw_paw_from_bottom(app, "cat_paw_1", solo, m.solo_hover, m.arm_phase);
    draw_paw_from_right(app, "cat_paw_2", online, m.online_hover, m.arm_phase);

    // 6. Footer hint.
    let hint = "Click a card to begin";
    let hfs = (h * 0.03).round();
    let hw = measure_text(hint, app.font.as_ref(), hfs as u16, 1.0).width;
    text_dropshadow(app, hint, (w - hw) * 0.5, h * 0.95, hfs, fade(color_u8!(240, 236, 224, 255), 0.85 * appear.min(1.0)));
}

/// Draw one play-mode card centred in its square rect. The card is darkened and
/// sits slightly lower/smaller when not hovered; on hover it brightens, scales
/// up a touch, and lifts. `appear` fades + slides the whole card in on entry.
fn draw_play_card(app: &mut GameApp, id: &str, sq: Rect, hover: f32, appear: f32) {
    let Some(t) = app.textures.ui(id) else { return };
    let cx = sq.x + sq.w * 0.5;
    let cy = sq.y + sq.h * 0.5 - 14.0 * hover + (1.0 - appear) * 40.0;
    let scale = (1.0 + 0.06 * hover) * (0.9 + 0.1 * appear.min(1.0));
    let side = sq.w * scale;
    // Brightness: darkened (0.52) at rest → full (1.0) when hovered.
    let b = 0.52 + 0.48 * hover;
    let tint = Color::new(b, b, b, appear.min(1.0));
    draw_texture_ex(
        &t,
        cx - side * 0.5,
        cy - side * 0.5,
        tint,
        DrawTextureParams { dest_size: Some(vec2(side, side)), ..Default::default() },
    );
}

/// Pointer-paw for the Solo card: a cat paw whose sleeve enters from the bottom
/// of the screen, paw pointing up into the card. Hidden until hovered; on hover
/// it fades in, slides up from below, and bobs vertically. The art (`cat_paw_1`)
/// is vertical with the toes near the top, so we anchor the toes inside the
/// card's lower half and let the arm run off the bottom edge.
fn draw_paw_from_bottom(app: &mut GameApp, id: &str, sq: Rect, hover: f32, phase: f32) {
    if hover < 0.01 {
        return;
    }
    let Some(t) = app.textures.ui(id) else { return };
    let h = hover.clamp(0.0, 1.0);
    let pw = sq.w * 0.80;
    let ph = pw * aspect(&t);
    let bob = (phase * 3.0).sin() * 7.0;
    // A little left of centre, flipped horizontally, anchored lower on the card.
    let cx = sq.x + sq.w * 0.44;
    let top = sq.y + sq.h * 0.60 - ph * 0.18 - bob + (1.0 - h) * 70.0;
    draw_texture_ex(
        &t,
        cx - pw * 0.5,
        top,
        fade(WHITE, h),
        DrawTextureParams { dest_size: Some(vec2(pw, ph)), flip_x: true, ..Default::default() },
    );
}

/// Pointer-paw for the Online card: a cat paw whose watch-arm enters from the
/// right of the screen, paw pointing left into the card. Hidden until hovered;
/// on hover it fades in, slides in from the right, and bobs horizontally. The art
/// (`cat_paw_2`) has the toes near the left edge, so we anchor those inside the
/// card and let the arm run off the right edge.
fn draw_paw_from_right(app: &mut GameApp, id: &str, sq: Rect, hover: f32, phase: f32) {
    if hover < 0.01 {
        return;
    }
    let Some(t) = app.textures.ui(id) else { return };
    let h = hover.clamp(0.0, 1.0);
    let pw = sq.w * 0.88;
    let ph = pw * aspect(&t);
    let bob = (phase * 3.0).sin() * 7.0;
    let cy = sq.y + sq.h * 0.5;
    // Toes (image left) reach to ~47% across the card; slide in from the right.
    let left = sq.x + sq.w * 0.47 - pw * 0.05 + bob + (1.0 - h) * 70.0;
    draw_texture_ex(
        &t,
        left,
        cy - ph * 0.5,
        fade(WHITE, h),
        DrawTextureParams { dest_size: Some(vec2(pw, ph)), ..Default::default() },
    );
}

/// Height-to-width ratio of a texture (1.0 if degenerate).
fn aspect(t: &Texture2D) -> f32 {
    if t.width() > 0.0 { t.height() / t.width() } else { 1.0 }
}

/// Cover-fit a texture to fill `w`×`h` (centre-crop, preserving aspect).
fn draw_cover(t: &Texture2D, w: f32, h: f32, tint: Color) {
    let (tw, th) = (t.width(), t.height());
    if tw <= 0.0 || th <= 0.0 {
        return;
    }
    let scale = (w / tw).max(h / th);
    let (dw, dh) = (tw * scale, th * scale);
    draw_texture_ex(
        t,
        (w - dw) * 0.5,
        (h - dh) * 0.5,
        tint,
        DrawTextureParams { dest_size: Some(vec2(dw, dh)), ..Default::default() },
    );
}

/// Title-style text with a soft drop shadow, using the bundled UI font.
fn text_dropshadow(app: &GameApp, s: &str, x: f32, y: f32, fs: f32, color: Color) {
    let font = app.font.as_ref();
    let params = |c: Color| TextParams { font, font_size: fs as u16, color: c, ..Default::default() };
    draw_text_ex(s, x + 2.0, y + 3.0, params(fade(BLACK, color.a * 0.5)));
    draw_text_ex(s, x, y, params(color));
}
