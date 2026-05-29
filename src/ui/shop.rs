//! Shop tab — three sub-tabs (Habitats / Structures / Animals). Each sub-tab
//! lists catalog items with affordability-aware buy buttons. Animals
//! auto-place into the first compatible habitat with room.

use chrono::{DateTime, Utc};
use egui::{RichText, Ui};

use super::{theme, widgets};
use crate::app::{EguiApp, ShopTab};
use crate::game::habitat::habitat_purchase_cost;
use crate::game::species::{self, HabitatTheme};
use crate::game::structure::{STRUCTURE_TOTAL_CAP, structure_purchase_cost};
use crate::game::structure_kind;

pub fn draw(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) {
    let mut want_save = false;

    ui.horizontal(|ui| {
        for tab in [
            ShopTab::Habitats,
            ShopTab::Structures,
            ShopTab::Animals,
            ShopTab::Exotic,
        ] {
            let selected = app.shop_tab == tab;
            let label = match tab {
                ShopTab::Habitats => "1  Habitats".to_string(),
                ShopTab::Structures => "2  Structures".to_string(),
                ShopTab::Animals => "3  Animals".to_string(),
                ShopTab::Exotic => {
                    if crate::game::exotic_shop::is_open(now) {
                        let left =
                            crate::game::exotic_shop::seconds_until_state_change(now);
                        format!("4  Exotic  ({} left)", super::fmt_duration_short(left))
                    } else {
                        let until =
                            crate::game::exotic_shop::seconds_until_state_change(now);
                        format!(
                            "4  Exotic  (opens {})",
                            super::fmt_duration_short(until)
                        )
                    }
                }
            };
            let color = if selected {
                theme::ACCENT
            } else if tab == ShopTab::Exotic {
                // Always pink-tinted so the rotating shop stays glanceable.
                theme::SPECIAL
            } else {
                theme::TEXT_DIM
            };
            let text = RichText::new(label).color(color);
            if ui
                .add(
                    egui::Button::new(text)
                        .fill(if selected { theme::SURFACE_ACTIVE } else { theme::SURFACE }),
                )
                .clicked()
            {
                app.shop_tab = tab;
            }
        }
    });

    ui.add_space(theme::PAD_M);

    egui::ScrollArea::vertical().show(ui, |ui| match app.shop_tab {
        ShopTab::Habitats => want_save |= draw_habitats(ui, app, now),
        ShopTab::Structures => want_save |= draw_structures(ui, app, now),
        ShopTab::Animals => want_save |= draw_animals(ui, app, now),
        ShopTab::Exotic => want_save |= draw_exotic(ui, app, now),
    });

    if want_save {
        app.save_under_lock(now);
    }
}

fn draw_habitats(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) -> bool {
    let mut dirty = false;
    let themes = [
        HabitatTheme::Forest,
        HabitatTheme::Wetland,
        HabitatTheme::Arctic,
        HabitatTheme::Jungle,
        HabitatTheme::Savanna,
        HabitatTheme::Ocean,
        HabitatTheme::Farmland,
    ];
    let cost = habitat_purchase_cost();
    for theme_kind in themes {
        let owned = app.zoo.count_habitats_with_theme(theme_kind) > 0;
        let affordable = !owned && app.zoo.coins >= cost;

        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(theme_kind.name())
                        .color(theme::TEXT_PRIMARY)
                        .strong(),
                );
                ui.label(
                    RichText::new(if owned { "owned" } else { "not owned" })
                        .color(theme::TEXT_DIM)
                        .size(11.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if owned {
                        // One per theme — extra purchases are gated by the
                        // domain. Capacity now grows via timed upgrades on
                        // the Habitats tab.
                        widgets::disabled_button(ui, "Owned");
                    } else {
                        let label = format!("Buy  ·  {} coins", super::fmt_int(cost));
                        if widgets::cost_button(ui, &label, affordable).clicked() {
                            match app.zoo.buy_habitat(theme_kind) {
                                Ok(_) => {
                                    app.set_status(
                                        format!("bought a {} habitat", theme_kind.name()),
                                        false,
                                        now,
                                    );
                                    dirty = true;
                                }
                                Err(e) => app.set_status(format!("{e}"), true, now),
                            }
                        }
                    }
                });
            });
        });
        ui.add_space(theme::PAD_S);
    }
    dirty
}

fn draw_structures(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) -> bool {
    let mut dirty = false;
    let next_slot_cost = structure_purchase_cost(app.zoo.structures.len());
    let at_cap = app.zoo.structures.len() >= STRUCTURE_TOTAL_CAP;
    let mut offerings: Vec<_> = structure_kind::all().collect();
    offerings.sort_by_key(|d| d.purchase_cost);

    for def in offerings {
        let cost = next_slot_cost.max(def.purchase_cost);
        let affordable = !at_cap && app.zoo.coins >= cost;
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(def.display_name)
                        .color(theme::TEXT_PRIMARY)
                        .strong(),
                );
                ui.label(
                    RichText::new(format!(
                        "{:.1}/s  ·  cap {}",
                        def.base_food_per_sec, def.base_food_cap
                    ))
                    .color(theme::TEXT_DIM)
                    .size(11.0)
                    .family(egui::FontFamily::Monospace),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if at_cap {
                        widgets::disabled_button(ui, "CAP");
                    } else {
                        let label = format!("Buy  ·  {} coins", super::fmt_int(cost));
                        if widgets::cost_button(ui, &label, affordable).clicked() {
                            match app.zoo.buy_structure(def.id, now) {
                                Ok(_) => {
                                    app.set_status(
                                        format!("bought a {}", def.display_name),
                                        false,
                                        now,
                                    );
                                    dirty = true;
                                }
                                Err(e) => app.set_status(format!("{e}"), true, now),
                            }
                        }
                    }
                });
            });
        });
        ui.add_space(theme::PAD_S);
    }
    dirty
}

fn draw_animals(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) -> bool {
    let mut dirty = false;
    let mut offerings: Vec<_> = species::all_purchasable().collect();
    offerings.sort_by_key(|d| d.purchase_cost);

    for def in offerings {
        let has_compatible = app
            .zoo
            .habitats
            .iter()
            .any(|h| h.theme == def.theme && h.animal_ids.len() < h.capacity());
        let (affordable, currency_label) = match def.purchase_currency {
            species::IncomeKind::Coin => (app.zoo.coins >= def.purchase_cost, "coins"),
            species::IncomeKind::DnaHelix => {
                (app.zoo.dna_helix >= def.purchase_cost, "DNA")
            }
        };
        let buyable = has_compatible && affordable;

        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(def.display_name)
                        .color(theme::TEXT_PRIMARY)
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("→ {}", def.theme.name()))
                        .color(theme::TEXT_DIM)
                        .size(11.0),
                );
                ui.label(
                    RichText::new(format!(
                        "{:.1}/s · cap {}",
                        def.base_rate_per_sec, def.base_storage_cap
                    ))
                    .color(theme::TEXT_DIM)
                    .size(11.0)
                    .family(egui::FontFamily::Monospace),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = format!(
                        "Buy  ·  {} {}",
                        super::fmt_int(def.purchase_cost),
                        currency_label
                    );
                    if widgets::cost_button(ui, &label, buyable).clicked() {
                        match app.zoo.buy_animal(def.id, now) {
                            Ok((hid, _)) => {
                                let where_ = app
                                    .zoo
                                    .habitats
                                    .iter()
                                    .find(|h| h.id == hid)
                                    .map(|h| h.theme.name())
                                    .unwrap_or("?");
                                app.set_status(
                                    format!(
                                        "bought a {} → {} habitat",
                                        def.display_name, where_
                                    ),
                                    false,
                                    now,
                                );
                                dirty = true;
                            }
                            Err(e) => app.set_status(format!("{e}"), true, now),
                        }
                    }
                });
            });
            if !has_compatible {
                ui.label(
                    RichText::new(format!("needs a {} habitat with space", def.theme.name()))
                        .color(theme::ERROR)
                        .size(11.0),
                );
            }
        });
        ui.add_space(theme::PAD_S);
    }
    dirty
}

/// Exotic shop sub-tab. Shows the current window's offerings when open, or
/// a "next opens in {countdown}" hint when in the 45-minute gap.
fn draw_exotic(ui: &mut Ui, app: &mut EguiApp, now: DateTime<Utc>) -> bool {
    use crate::game::exotic_shop::{self, Price};

    let mut dirty = false;
    let Some(window) = exotic_shop::effective_window(now, app.zoo.exotic_skip_window) else {
        let until = exotic_shop::seconds_until_state_change(now);
        let skip_cost = exotic_shop::SKIP_WAIT_DNA_COST;
        let can_skip = app.zoo.dna_helix >= skip_cost;
        widgets::card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Exotic shop is closed")
                        .color(theme::TEXT_DIM)
                        .size(13.0)
                        .strong(),
                );
                ui.add_space(theme::PAD_S);
                ui.label(
                    RichText::new(format!(
                        "next window opens in {}",
                        super::fmt_duration_short(until)
                    ))
                    .color(theme::TEXT_DIM)
                    .size(11.0),
                );
                ui.add_space(theme::PAD_S);
                let btn = RichText::new(format!("Skip wait  ·  {skip_cost} DNA"))
                    .color(theme::SPECIAL)
                    .strong();
                let resp = ui.add_enabled(
                    can_skip,
                    egui::Button::new(btn).fill(theme::SURFACE),
                );
                if resp.clicked() {
                    match app.zoo.skip_exotic_wait(now) {
                        Ok(()) => {
                            app.set_status("opened the exotic shop early!", false, now);
                            dirty = true;
                        }
                        Err(e) => app.set_status(format!("{e}"), true, now),
                    }
                }
            });
        });
        return dirty;
    };

    // Forced open via a paid skip during the closed gap.
    let opened_early = !exotic_shop::is_open(now);

    // Open window header with time-remaining strip.
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            let header = if opened_early {
                format!("Window #{} — open (early)", window.index)
            } else {
                format!("Window #{} — open", window.index)
            };
            ui.label(
                RichText::new(header)
                    .color(theme::SPECIAL)
                    .size(13.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let left = (window.closes_at - now).num_seconds().max(0);
                ui.label(
                    RichText::new(format!(
                        "closes in {}",
                        super::fmt_duration_short(left)
                    ))
                    .color(theme::TIMER)
                    .family(egui::FontFamily::Monospace),
                );
            });
        });
    });
    ui.add_space(theme::PAD_S);

    // Offerings (typically 3). Each is a card; price colored to its currency.
    for off in &window.offerings {
        let def = species::get(off.species);
        let (label, affordable, color) = match off.price {
            Price::Coins(c) => (
                format!("{} coins", super::fmt_int(c)),
                app.zoo.coins >= c,
                theme::ACCENT,
            ),
            Price::Dna(d) => (
                format!("{} DNA", super::fmt_int(d)),
                app.zoo.dna_helix >= d,
                theme::SPECIAL,
            ),
        };
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(def.display_name)
                        .color(theme::TEXT_PRIMARY)
                        .strong(),
                );
                ui.label(
                    RichText::new(def.theme.name())
                        .color(theme::TEXT_DIM)
                        .size(11.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_text = RichText::new(label.clone()).color(color).strong();
                    let resp = ui.add_enabled(
                        affordable,
                        egui::Button::new(btn_text).fill(theme::SURFACE),
                    );
                    if resp.clicked() {
                        if let Some(msg) = try_buy_exotic(app, off, now) {
                            app.set_status(msg, false, now);
                            dirty = true;
                        }
                    }
                });
            });
        });
        ui.add_space(theme::PAD_S);
    }
    dirty
}

/// Attempt to buy `offering`. Re-checks window-open state and affordability
/// before mutating. Returns a status string on success, or None on a
/// silently-handled failure (status is set by the caller for errors).
fn try_buy_exotic(
    app: &mut EguiApp,
    offering: &crate::game::exotic_shop::ExoticOffering,
    now: DateTime<Utc>,
) -> Option<String> {
    use crate::game::exotic_shop::{self, Price};
    if !exotic_shop::is_open_with_skip(now, app.zoo.exotic_skip_window) {
        app.set_status("shop just closed — try the next window", true, now);
        return None;
    }
    // Affordability + currency drain.
    match offering.price {
        Price::Coins(c) => {
            if app.zoo.coins < c {
                app.set_status("not enough coins", true, now);
                return None;
            }
            app.zoo.coins -= c;
        }
        Price::Dna(d) => {
            if app.zoo.dna_helix < d {
                app.set_status("not enough DNA Helix", true, now);
                return None;
            }
            app.zoo.dna_helix -= d;
        }
    }
    // Place the animal.
    match app.zoo.auto_place_animal(offering.species, 1, now) {
        Ok(_) => {
            let def = species::get(offering.species);
            Some(format!("bought a {}!", def.display_name))
        }
        Err(e) => {
            // Refund on failure (otherwise we'd silently eat their currency).
            match offering.price {
                Price::Coins(c) => app.zoo.coins = app.zoo.coins.saturating_add(c),
                Price::Dna(d) => app.zoo.dna_helix = app.zoo.dna_helix.saturating_add(d),
            }
            app.set_status(format!("{e}"), true, now);
            None
        }
    }
}
