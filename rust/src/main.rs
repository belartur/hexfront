//! Hexfront game entry point (`cargo run --release`).
//!
//! Only [`crate::app::Application`] runs here; see
//! specification_rust.md for the module layout.

mod ai;
mod app;
mod board;
mod camera;
mod constants;
mod entities;
mod game;
mod hexgrid;
mod iso;
mod mapfile;
mod mesh;
mod render;
#[cfg(test)]
mod render_baseline;
mod rng;

#[allow(unused_imports)]
use macroquad::prelude::*;

/// Window configuration: macroquad window, user-resizable.
///
/// The draw-call buffers are raised above macroquad's defaults (10k verts /
/// 5k indices): a single static terrain chunk holds up to `CHUNK_VERTICES`
/// vertices and must fit one draw call without clamping. This is
/// `macroquad::conf::Conf` (not `miniquad::conf::Conf`): `#[macroquad::main]`
/// calls `Window::from_config`, which forwards these fields.
fn window_conf() -> macroquad::conf::Conf {
    macroquad::conf::Conf {
        miniquad_conf: macroquad::miniquad::conf::Conf {
            window_title: "Hexfront".to_owned(),
            window_width: 1180,
            window_height: 720,
            window_resizable: true,
            ..Default::default()
        },
        draw_call_vertex_capacity: 16_384,
        draw_call_index_capacity: 16_384,
        ..Default::default()
    }
}

/// Launch the game.
#[macroquad::main(window_conf)]
async fn main() {
    let app = app::Application::new();
    app.run().await;
}
