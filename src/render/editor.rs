//! Dev-only level editor (compiled only under `--features editor`).
//!
//! Edits the location the player is currently in: pick a palette item, place it
//! with the mouse, erase with right-click, Ctrl+S to write the blueprint to
//! `assets/levels/<key>.json`. State lives in [`EditorState`] on `GameApp`;
//! placement/erase logic and the F8 toggle live in `app.rs`. This module owns the
//! editor's own types + its overlay/palette drawing.

use macroquad::prelude::*;

use crate::app::GameApp;
use crate::level::Level;
use crate::render::view;

/// What the editor is currently placing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    Tiles,
    Props,
}

impl EditMode {
    pub fn label(self) -> &'static str {
        match self {
            EditMode::Tiles => "Tiles",
            EditMode::Props => "Props",
        }
    }
}

/// Live editor state: the working level being authored plus the current tool.
pub struct EditorState {
    pub mode: EditMode,
    pub palette_index: usize,
    /// Snap props to tile centres (tiles always snap).
    pub snap: bool,
    pub dirty: bool,
    pub working: Level,
}

impl EditorState {
    pub fn new(key: String, existing: Option<&Level>) -> Self {
        let working = existing.cloned().unwrap_or_else(|| Level::new(key));
        Self { mode: EditMode::Tiles, palette_index: 0, snap: true, dirty: false, working }
    }
}

/// The palette ids for the current mode: tile ids, or the current biome's
/// terrain-prop ids (the hub falls back to Forest props as a starter set).
pub fn palette(app: &GameApp) -> Vec<String> {
    match app.editor.as_ref().map(|e| e.mode) {
        Some(EditMode::Tiles) => crate::render::textures::tile_ids().iter().map(|s| s.to_string()).collect(),
        Some(EditMode::Props) => {
            let theme = app
                .expedition
                .as_ref()
                .map(|e| e.instance.theme.name().to_string())
                .unwrap_or_else(|| "Forest".to_string());
            crate::render::textures::terrain_prop_ids(&theme).iter().map(|s| s.to_string()).collect()
        }
        None => Vec::new(),
    }
}

/// The currently-selected palette id, if any.
pub fn selected_id(app: &GameApp) -> Option<String> {
    let idx = app.editor.as_ref()?.palette_index;
    palette(app).into_iter().nth(idx)
}

/// Draw the editor overlay: the placement ghost (scene space) + the palette and
/// status (HUD). Called from `GameApp::draw` after the world, before the cursor.
pub fn draw(app: &mut GameApp) {
    let Some(mode) = app.editor.as_ref().map(|e| e.mode) else { return };
    let cam = app.camera;
    let (mx, my) = mouse_position();
    let mouse = vec2(mx, my);

    // Ghost preview at the cursor.
    match mode {
        EditMode::Tiles => {
            let (tx, ty) = view::screen_to_tile(mouse, 128.0, &cam);
            let (pos, size) = view::tile_rect(tx, ty, 128.0, &cam);
            draw_rectangle_lines(pos.x, pos.y, size.x, size.y, 2.0, color_u8!(120, 230, 140, 230));
        }
        EditMode::Props => {
            if let Some(id) = selected_id(app) {
                let world = ghost_world(app, mouse);
                let p = view::world_to_screen(world, &cam);
                if let Some(tex) = app.textures.terrain(&id) {
                    let z = cam.zoom;
                    let w = 100.0 / tex.width().max(tex.height()).max(1.0) * tex.width() * z;
                    let h = 100.0 / tex.width().max(tex.height()).max(1.0) * tex.height() * z;
                    draw_texture_ex(
                        &tex,
                        p.x - w * 0.5,
                        p.y - h,
                        Color::new(1.0, 1.0, 1.0, 0.6),
                        DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() },
                    );
                }
            }
        }
    }

    // Palette + status (HUD text).
    let pal = palette(app);
    let ed = app.editor.as_ref().unwrap();
    let font = app.font.clone();
    let font = font.as_ref();

    let x = 16.0;
    let mut y = screen_height() * 0.30;
    let header = format!(
        "EDITOR · {}  [1]Tiles [2]Props · snap {} · [/] cycle · Ctrl+S save · F8 exit{}",
        ed.mode.label(),
        if ed.snap { "on" } else { "off" },
        if ed.dirty { " · *unsaved" } else { "" },
    );
    text_line(&header, x, y, 18.0, color_u8!(245, 240, 220, 255), font);
    y += 26.0;
    for (i, id) in pal.iter().enumerate() {
        let sel = i == ed.palette_index;
        let col = if sel { color_u8!(255, 230, 130, 255) } else { color_u8!(200, 200, 210, 220) };
        let marker = if sel { "▶ " } else { "  " };
        text_line(&format!("{marker}{id}"), x, y, 16.0, col, font);
        y += 20.0;
        if y > screen_height() - 20.0 {
            break;
        }
    }
}

/// Snapped world position for prop placement (tile-centre when snap is on).
pub fn ghost_world(app: &GameApp, mouse: Vec2) -> Vec2 {
    let world = view::screen_to_world(mouse, &app.camera);
    let snap = app.editor.as_ref().map(|e| e.snap).unwrap_or(false);
    if snap {
        let (tx, ty) = view::screen_to_tile(mouse, 128.0, &app.camera);
        vec2((tx as f32 + 0.5) * 128.0, (ty as f32 + 0.5) * 128.0)
    } else {
        world
    }
}

fn text_line(s: &str, x: f32, y: f32, size: f32, color: Color, font: Option<&Font>) {
    let params = |c: Color| TextParams { font, font_size: size as u16, color: c, ..Default::default() };
    draw_text_ex(s, x + 1.0, y + 1.0, params(fade_black(color.a)));
    draw_text_ex(s, x, y, params(color));
}

fn fade_black(a: f32) -> Color {
    Color::new(0.0, 0.0, 0.0, a * 0.5)
}
