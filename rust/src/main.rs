//! Hexfront game entry point (`cargo run --release`).
//!
//! Only [`crate::app::Application`] runs here; see
//! specification_rust.md for the module layout.

mod ai;
mod app;
mod board;
mod camera;
mod constants;
mod depth;
mod entities;
mod game;
mod hexgrid;
mod mapfile;
mod render;
mod rng;

use macroquad::prelude::*;

/// Window configuration: default macroquad window, user-resizable.
fn window_conf() -> Conf {
    Conf {
        window_title: "Hexfront".to_owned(),
        window_width: 1180,
        window_height: 720,
        window_resizable: true,
        ..Default::default()
    }
}

/// Launch the game.
#[macroquad::main(window_conf)]
async fn main() {
    let app = app::Application::new();
    app.run().await;
}
