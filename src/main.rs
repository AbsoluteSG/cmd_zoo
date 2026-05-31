#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{Context, Result};
use chrono::Utc;
use macroquad::prelude::*;

use cmd_zoo::app::GameApp;
use cmd_zoo::game::animal::Animal;
use cmd_zoo::game::{Zoo, economy};
use cmd_zoo::persistence::json_file::JsonFileRepository;

fn window_conf() -> Conf {
    Conf {
        window_title: "cmd_zoo".to_owned(),
        window_width: 1100,
        window_height: 720,
        high_dpi: true,
        ..Default::default()
    }
}

/// Repo + initial load + offline catch-up, then build the app. Renderer-agnostic
/// — identical dance to the old egui/ratatui boot, just feeds a `GameApp`.
fn boot() -> Result<(GameApp, Vec<String>)> {
    let repo = Arc::new(JsonFileRepository::at_default_path().context("preparing save file")?);
    let now = Utc::now();
    let (mut zoo, warnings) = {
        let access = repo.lock().context("initial save lock")?;
        match access.load_if_newer(SystemTime::UNIX_EPOCH)? {
            Some((zoo, _, warnings)) => (zoo, warnings),
            None => {
                let fresh = Zoo::new(now);
                access.save(&fresh)?;
                (fresh, Vec::new())
            }
        }
    };

    economy::advance(&mut zoo, now);
    // Seed a starter critter so a fresh save has something to collect. The
    // animal lives in `zoo.animals` directly (no habitat needed in the
    // freeform model); it persists and reloads like any other animal.
    if zoo.animals.is_empty() {
        let frog = Animal::new("blue_frog", now);
        zoo.animals.insert(frog.id, frog);
    }
    zoo.last_saved_at = now;
    let last_modtime = {
        let access = repo.lock().context("post catch-up save lock")?;
        access.save(&zoo)?
    };

    Ok((GameApp::new(zoo, repo, last_modtime), warnings))
}

#[macroquad::main(window_conf)]
async fn main() {
    let (mut app, warnings) = match boot() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("fatal: failed to start cmd_zoo: {e:?}");
            return;
        }
    };
    if let Some(first) = warnings.first() {
        for w in &warnings {
            eprintln!("load warning: {w}");
        }
        app.set_status(first.clone());
    }

    // Decode embedded sound effects (async) and use a custom in-game cursor.
    app.sounds = cmd_zoo::audio::Sounds::load_all().await;
    show_mouse(false);

    loop {
        let now = Utc::now();
        app.tick(now);
        app.handle_input(now);
        app.draw(now);
        next_frame().await;
    }
}
