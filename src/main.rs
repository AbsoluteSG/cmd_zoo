#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{Context, Result};
use chrono::Utc;

use cmd_zoo::app::EguiApp;
use cmd_zoo::game::{Zoo, economy};
use cmd_zoo::persistence::json_file::JsonFileRepository;

fn main() -> Result<()> {
    // Repo + initial load under lock; identical pattern to the old ratatui main.
    let repo = Arc::new(
        JsonFileRepository::at_default_path().context("preparing save file")?,
    );
    let now = Utc::now();
    let (mut zoo, initial_warnings) = {
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

    // Offline catch-up so any breeding that completed while the app was closed
    // is reflected before the window opens.
    economy::advance(&mut zoo, now);
    zoo.last_saved_at = now;
    let last_modtime = {
        let access = repo.lock().context("post catch-up save lock")?;
        access.save(&zoo)?
    };

    let mut app = EguiApp::new(zoo, repo, last_modtime);
    // Surface load-time warnings (e.g. "dropped 1 animal(s) of unknown species
    // 'frog'") as a status banner so the user knows why their zoo shrank.
    if !initial_warnings.is_empty() {
        for w in &initial_warnings {
            eprintln!("load warning: {w}");
        }
        let summary = if initial_warnings.len() == 1 {
            initial_warnings[0].clone()
        } else {
            format!("{} load warnings — first: {}", initial_warnings.len(), initial_warnings[0])
        };
        app.set_status(summary, true, now);
    }

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("cmd_zoo"),
        ..Default::default()
    };
    eframe::run_native(
        "cmd_zoo",
        opts,
        Box::new(|cc| {
            cmd_zoo::ui::theme::install(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}
