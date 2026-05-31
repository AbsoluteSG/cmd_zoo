//! Macroquad presentation layer: isometric grid math, texture cache, and the
//! world scene. Replaces the retired egui `ui` module. Pure rendering + input
//! translation — all game rules live in `crate::game`.

pub mod menus;
pub mod textures;
pub mod view;
pub mod world;
