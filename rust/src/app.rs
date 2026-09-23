//! Application layer: window, menu, level loading, input and HUD.
//!
//! Implements the controls from specification.md, section "Sterowanie":
//! view pan/zoom, source selection (RMB/LMB), vehicle sending, route
//! preview, Esc and pause.

use std::path::PathBuf;

use macroquad::prelude::*;

use crate::ai::AiController;
use crate::camera::Camera;
use crate::constants::{self};
use crate::entities::vehicle_kind_of;
use crate::game::Game;
use crate::hexgrid::Tile;
use crate::iso::IsoCamera;
use crate::mapfile::{self, level_seed};
use crate::mesh::{self, DynamicMesh, TerrainMesh};
use crate::render::Renderer;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Menu,
    Loading,
    Playing,
}

/// Owns the window and drives the whole program.
pub struct Application {
    game: Option<Game>,
    camera: Camera,
    renderer: Renderer,
    terrain: TerrainMesh,
    dynamic: DynamicMesh,
    terrain_board_key: Option<(i32, i32)>,
    state: State,
    maps: Vec<PathBuf>,
    menu_scroll: f32,
    menu_rects: Vec<(Rect, PathBuf)>,
    ai: Vec<AiController>,
    paused: bool,
    selection: Option<Tile>,
    preview_tile: Option<Tile>,
    preview_path: Option<Vec<Tile>>,
    load_timer: f64,
    sim_acc: f64,
    down_pos: Option<(f32, f32)>,
    last_mouse: (f32, f32),
    dragging: bool,
    /// TEMPORARY debug hook output path (`HEXFRONT_CAPTURE`).
    capture: Option<String>,
    /// TEMPORARY debug hook frame countdown.
    capture_frames: u32,
    /// TEMPORARY frame-time samples (`HEXFRONT_TIMING`).
    slow_frames: Vec<f32>,
}

impl Application {
    /// Create the application (window is owned by macroquad).
    pub fn new() -> Self {
        let mut app = Self {
            game: None,
            camera: Camera::new((screen_width(), screen_height())),
            renderer: Renderer::new(),
            terrain: TerrainMesh::default(),
            dynamic: DynamicMesh::default(),
            terrain_board_key: None,
            state: State::Menu,
            maps: mapfile::list_maps(None),
            menu_scroll: 0.0,
            menu_rects: Vec::new(),
            ai: Vec::new(),
            paused: false,
            selection: None,
            preview_tile: None,
            preview_path: None,
            load_timer: 0.0,
            sim_acc: 0.0,
            down_pos: None,
            last_mouse: mouse_position(),
            dragging: false,
            capture: None,
            capture_frames: 0,
            slow_frames: Vec::new(),
        };
        // TEMPORARY debug hook: render a level headlessly and dump a PNG.
        if let Ok(path) = std::env::var("HEXFRONT_MAP") {
            if let Ok(png) = std::env::var("HEXFRONT_CAPTURE") {
                app.capture = Some(png);
                app.capture_frames = 20;
            }
            app.start_map(std::path::Path::new(&path));
            app.state = State::Playing;
            app.paused = false;
        }
        if std::env::var("HEXFRONT_TIMING").is_ok() {
            app.sim_acc = 0.0;
        }
        app
    }
    /// Main loop; exits when the window is closed.
    pub async fn run(mut self) {
        loop {
            let dt = get_frame_time().min(0.1);
            self.handle_input(dt);
            self.update(dt);
            if std::env::var("HEXFRONT_TIMING").is_ok() && self.state == State::Playing {
                self.slow_frames.push(dt);
                if self.slow_frames.len() >= 120 {
                    let mut sorted = self.slow_frames.clone();
                    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let med = sorted[sorted.len() / 2] * 1000.0;
                    println!(
                        "TIMING frames={} median={:.1} ms max={:.1} ms",
                        sorted.len(),
                        med,
                        sorted[sorted.len() - 1] * 1000.0
                    );
                    self.slow_frames.clear();
                }
            }
            self.draw();
            if self.capture.is_some() {
                if self.capture_frames > 0 {
                    self.capture_frames -= 1;
                } else {
                    let path = self.capture.clone().unwrap();
                    let img = get_screen_data();
                    img.export_png(&path);
                    println!("CAPTURED {}", path);
                    std::process::exit(0);
                }
            }
            next_frame().await;
        }
    }
    fn ensure_buffers(&mut self) {
        self.camera.screen_size = (screen_width(), screen_height());
    }
}
impl Application {
    fn alt_down() -> bool {
        is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt)
    }
    fn handle_input(&mut self, dt: f32) {
        let wheel = mouse_wheel();
        if wheel.1 != 0.0 {
            if self.state == State::Menu {
                self.menu_scroll -= wheel.1 * constants::MENU_SCROLL_STEP;
            } else if self.state == State::Playing {
                let (mx, my) = mouse_position();
                let f = if wheel.1 > 0.0 {
                    constants::ZOOM_STEP
                } else {
                    1.0 / constants::ZOOM_STEP
                };
                self.camera.zoom_at(f, mx, my);
            }
        }
        if self.state == State::Playing {
            let (mx, my) = mouse_position();
            let (w, h) = (screen_width(), screen_height());
            let mut dx = 0.0f32;
            let mut dy = 0.0f32;
            if mx < constants::EDGE_PAN_MARGIN {
                dx += constants::PAN_SPEED as f32 * dt;
            }
            if mx > w - constants::EDGE_PAN_MARGIN {
                dx -= constants::PAN_SPEED as f32 * dt;
            }
            if my < constants::EDGE_PAN_MARGIN {
                dy += constants::PAN_SPEED as f32 * dt;
            }
            if my > h - constants::EDGE_PAN_MARGIN {
                dy -= constants::PAN_SPEED as f32 * dt;
            }
            if is_key_down(KeyCode::Left) || is_key_down(KeyCode::A) {
                dx += constants::PAN_SPEED as f32 * dt;
            }
            if is_key_down(KeyCode::Right) || is_key_down(KeyCode::D) {
                dx -= constants::PAN_SPEED as f32 * dt;
            }
            if is_key_down(KeyCode::Up) || is_key_down(KeyCode::W) {
                dy += constants::PAN_SPEED as f32 * dt;
            }
            if is_key_down(KeyCode::Down) || is_key_down(KeyCode::S) {
                dy -= constants::PAN_SPEED as f32 * dt;
            }
            if dx != 0.0 || dy != 0.0 {
                self.camera.pan(dx, dy);
            }
            if is_mouse_button_down(MouseButton::Left) {
                if let Some((dx0, dy0)) = self.down_pos {
                    if !self.dragging
                        && ((mx - dx0).powi(2) + (my - dy0).powi(2)).sqrt()
                            > constants::DRAG_THRESHOLD
                    {
                        self.dragging = true;
                    }
                    if self.dragging {
                        self.camera
                            .pan(mx - self.last_mouse.0, my - self.last_mouse.1);
                    }
                } else {
                    self.down_pos = Some((mx, my));
                    self.dragging = false;
                }
            } else if let Some(_down) = self.down_pos.take() {
                if !self.dragging {
                    self.click(mx, my);
                }
                self.dragging = false;
            }
            self.last_mouse = (mx, my);
            if is_mouse_button_pressed(MouseButton::Right) {
                self.select_rmb(mx, my);
            }
        } else if self.state == State::Menu {
            if is_mouse_button_pressed(MouseButton::Left) {
                let (mx, my) = mouse_position();
                for (rect, path) in self.menu_rects.clone() {
                    if rect.contains(vec2(mx, my)) {
                        self.start_map(&path);
                        break;
                    }
                }
            }
            if is_key_pressed(KeyCode::Up) {
                self.menu_scroll -= constants::MENU_SCROLL_STEP;
            }
            if is_key_pressed(KeyCode::Down) {
                self.menu_scroll += constants::MENU_SCROLL_STEP;
            }
            if is_key_pressed(KeyCode::PageUp) {
                self.menu_scroll -= constants::MENU_SCROLL_STEP * 4.0;
            }
            if is_key_pressed(KeyCode::PageDown) {
                self.menu_scroll += constants::MENU_SCROLL_STEP * 4.0;
            }
            if is_key_pressed(KeyCode::Escape) {
                std::process::exit(0);
            }
        }
        if self.state == State::Playing {
            if is_key_pressed(KeyCode::Escape) {
                if self.selection.is_some() {
                    self.set_selection(None);
                } else {
                    self.enter_menu();
                }
            }
            if is_key_pressed(KeyCode::P) {
                self.paused = !self.paused;
            }
            if is_key_pressed(KeyCode::Equal) || is_key_pressed(KeyCode::KpAdd) {
                let (mx, my) = mouse_position();
                self.camera.zoom_at(constants::ZOOM_STEP, mx, my);
            }
            if is_key_pressed(KeyCode::Minus) || is_key_pressed(KeyCode::KpSubtract) {
                let (mx, my) = mouse_position();
                self.camera.zoom_at(1.0 / constants::ZOOM_STEP, mx, my);
            }
        }
        if self.state == State::Loading && is_key_pressed(KeyCode::Escape) {
            self.enter_menu();
        }
    }
    fn enter_menu(&mut self) {
        self.state = State::Menu;
        self.maps = mapfile::list_maps(None);
        self.menu_scroll = 0.0;
        self.game = None;
        self.ai.clear();
        self.selection = None;
    }
    fn set_selection(&mut self, src: Option<Tile>) {
        self.preview_tile = None;
        self.preview_path = None;
        self.selection = src;
    }
    fn flat() -> bool {
        Self::alt_down()
    }
    fn hover_tile(&self, pos: (f32, f32)) -> Option<Tile> {
        let game = self.game.as_ref()?;
        game.board
            .snap_to_building(&self.camera, pos.0, pos.1, &game.buildings, Self::flat())
    }
    fn click(&mut self, mx: f32, my: f32) {
        // LMB: select own building, or send to the hovered building.
        let hover = self.hover_tile((mx, my));
        if let Some(sel) = self.selection
            && let Some(h) = hover
        {
            if h != sel {
                if let Some(game) = self.game.as_mut() {
                    let human = game.human_id;
                    if game.try_send(human, sel, h) {
                        self.set_selection(None);
                        return;
                    }
                }
            } else {
                self.set_selection(None);
                return;
            }
        }
        // Click elsewhere: fall through to (re)select.
        // Select own building with units.
        if let Some(h) = hover {
            let select = if let Some(game) = self.game.as_ref() {
                if let Some(b) = game.building_at_tile(h) {
                    b.owner == Some(game.human_id) && b.units > 0.0
                } else {
                    false
                }
            } else {
                false
            };
            if select {
                self.set_selection(Some(h));
            } else {
                self.set_selection(None);
            }
        } else {
            self.set_selection(None);
        }
    }
    fn select_rmb(&mut self, mx: f32, my: f32) {
        // RMB always (re)selects the clicked own building with units.
        let hover = self.hover_tile((mx, my));
        if let Some(h) = hover {
            let ok = if let Some(game) = self.game.as_ref() {
                if let Some(b) = game.building_at_tile(h) {
                    b.owner == Some(game.human_id) && b.units > 0.0
                } else {
                    false
                }
            } else {
                false
            };
            if ok {
                self.set_selection(Some(h));
            } else {
                self.set_selection(None);
            }
        } else {
            self.set_selection(None);
        }
    }
    fn start_map(&mut self, path: &std::path::Path) {
        match mapfile::load_game(path) {
            Ok(game) => {
                let seed = level_seed(path);
                let mut ai = Vec::new();
                for p in game.players.iter() {
                    if !p.is_human {
                        ai.push(AiController::new(
                            p.id,
                            *constants::ai_difficulty(constants::MAP_DEFAULT_AI_DIFFICULTY),
                            seed.wrapping_add(p.id as u64),
                        ));
                    }
                }
                self.camera = Camera::new((screen_width(), screen_height()));
                self.camera.limit_to_board(&game.board);
                // Center on the board.
                let cx = 1.5 * game.board.side * (game.board.cols - 1) as f64 / 2.0;
                let cy =
                    crate::hexgrid::SQRT3 * game.board.side * (game.board.rows - 1) as f64 / 2.0;
                self.camera.center_on_world(cx, cy, 0.0);
                self.terrain = mesh::build_terrain(&game.board);
                self.renderer.set_terrain(&self.terrain);
                self.terrain_board_key = Some((game.board.cols, game.board.rows));
                self.game = Some(game);
                self.ai = ai;
                self.paused = false;
                self.selection = None;
                self.preview_tile = None;
                self.preview_path = None;
                self.load_timer = 0.0;
                self.sim_acc = 0.0;
                self.state = State::Loading;
            }
            Err(e) => eprintln!("cannot load {}: {}", path.display(), e),
        }
    }
    fn update(&mut self, dt: f32) {
        if self.state == State::Loading {
            self.load_timer += dt as f64;
            if self.load_timer >= constants::LOADING_TIME {
                self.state = State::Playing;
            }
            return;
        }
        if self.state != State::Playing || self.paused {
            return;
        }
        // Fixed-step simulation.
        self.sim_acc += dt as f64;
        let mut steps = 0;
        while self.sim_acc >= constants::SIM_DT && steps < 8 {
            if let Some(game) = self.game.as_mut() {
                game.update(constants::SIM_DT);
                let mut ai = std::mem::take(&mut self.ai);
                for a in ai.iter_mut() {
                    a.update(game, constants::SIM_DT);
                }
                self.ai = ai;
            }
            self.sim_acc -= constants::SIM_DT;
            steps += 1;
        }
        // Refresh route preview.
        if let Some(sel) = self.selection {
            let (mx, my) = mouse_position();
            let hover = self.hover_tile((mx, my));
            if hover != self.preview_tile {
                self.preview_tile = hover;
                self.preview_path = None;
                if let (Some(h), Some(game)) = (hover, self.game.as_ref())
                    && h != sel
                    && game.building_at_tile(h).is_some()
                    && let Some(src) = game.building_at_tile(sel)
                {
                    let kind = vehicle_kind_of(src.kind);
                    self.preview_path = game.board.find_path(sel, h, kind);
                }
            }
        }
    }
    fn draw(&mut self) {
        self.ensure_buffers();
        match self.state {
            State::Menu => self.draw_menu(),
            State::Loading => self.draw_loading(),
            State::Playing => self.draw_game(),
        }
    }
    fn draw_game(&mut self) {
        let (mx, my) = mouse_position();
        let hover = self.hover_tile((mx, my));
        // Update preview path rendering state.
        let preview = self.preview_path.clone();
        if let Some(game) = self.game.as_ref() {
            // Rebuild static terrain when the board identity changed.
            let key = Some((game.board.cols, game.board.rows));
            if self.terrain_board_key != key {
                self.terrain = mesh::build_terrain(&game.board);
                self.renderer.set_terrain(&self.terrain);
                self.terrain_board_key = key;
            }
            let (lo, hi) = mesh::depth_span(&game.board);
            let iso = IsoCamera::from_camera(&self.camera, lo, hi);
            let view_bounds = mesh::visible_world_bounds(
                &self.camera,
                mesh::max_height(&game.board),
                game.board.side,
            );
            mesh::build_dynamic(game, self.renderer.rotor_phase, &mut self.dynamic);
            self.renderer.rotor_phase += 0.2;
            let sel = self.selection;
            self.renderer.draw_gpu(
                &iso,
                &self.camera,
                view_bounds,
                &self.dynamic,
                game,
                sel,
                hover,
                preview.as_ref(),
            );
        }
        // HUD text on top (badges, floating texts, help).
        self.draw_badges();
        self.draw_float_texts();
        self.draw_hud();
    }
    fn badge_anchor(&self, wx: f64, wy: f64, wz: f64) -> (f32, f32) {
        let (sx, sy) = self.camera.world_to_screen(wx, wy, wz);
        let r = (12.0 * self.camera.zoom as f32).max(8.0);
        (sx + r * 1.5, sy + r * 1.1)
    }
    /// True when a screen position is inside the viewport (HUD culling).
    fn on_screen(sx: f32, sy: f32) -> bool {
        let margin = 48.0;
        sx >= -margin
            && sy >= -margin
            && sx <= screen_width() + margin
            && sy <= screen_height() + margin
    }
    fn draw_badges(&self) {
        let game = match self.game.as_ref() {
            Some(g) => g,
            None => return,
        };
        for b in game.buildings.iter() {
            let (wx, wy) = b.pos(game.board.side);
            let z = b.units;
            let gz = mesh::tile_top_z(&game.board, b.tile);
            let (cx, cy) = self.badge_anchor(wx, wy, gz);
            if !Self::on_screen(cx, cy) {
                continue;
            }
            let r = (12.0 * self.camera.zoom as f32).max(8.0);
            draw_circle(cx, cy, r, Color::new(0.11, 0.11, 0.13, 1.0));
            draw_circle_lines(cx, cy, r, 2.0, Color::new(0.96, 0.96, 0.96, 1.0));
            let fs = ((17.0 * self.camera.zoom as f32).max(10.0)) as u16;
            let txt = format!("{}", z.round() as i64);
            let dim = measure_text(&txt, None, fs, 1.0);
            draw_text(
                &txt,
                cx - dim.width / 2.0,
                cy + dim.height / 2.5,
                fs as f32,
                WHITE,
            );
            if z >= b.capacity - 1e-6 {
                let dim2 = measure_text("MAX", None, (fs / 2).max(8), 1.0);
                draw_text(
                    "MAX",
                    cx - dim2.width / 2.0,
                    cy + r + 10.0,
                    (fs / 2).max(8) as f32,
                    WHITE,
                );
            }
            if crate::entities::is_base(b.kind) && b.owner.is_some() && b.units < b.capacity {
                // Spawn ring progress.
                let frac =
                    (b.production_timer / constants::BASE_SPAWN_INTERVAL).clamp(0.0, 1.0) as f32;
                draw_circle_lines(cx, cy, r + 4.0, 2.0, Color::new(1.0, 1.0, 1.0, 0.25));
                draw_arc(cx, cy, 12, r + 4.0, 0.0, 360.0 * frac, 3.0, WHITE);
            }
        }
        for v in game.vehicles.iter() {
            if v.dead {
                continue;
            }
            let gz = mesh::vehicle_ground_z(game, v.x, v.y);
            let (cx, cy) = self.badge_anchor(v.x, v.y, gz);
            if !Self::on_screen(cx, cy) {
                continue;
            }
            let r = (12.0 * self.camera.zoom as f32).max(8.0);
            draw_circle(cx, cy, r, Color::new(0.11, 0.11, 0.13, 1.0));
            draw_circle_lines(cx, cy, r, 2.0, Color::new(0.96, 0.96, 0.96, 1.0));
            let fs = ((17.0 * self.camera.zoom as f32).max(10.0)) as u16;
            let txt = format!("{}", v.units.round() as i64);
            let dim = measure_text(&txt, None, fs, 1.0);
            draw_text(
                &txt,
                cx - dim.width / 2.0,
                cy + dim.height / 2.5,
                fs as f32,
                WHITE,
            );
        }
    }
    fn draw_float_texts(&self) {
        let game = match self.game.as_ref() {
            Some(g) => g,
            None => return,
        };
        for b in game.buildings.iter() {
            if b.texts.is_empty() {
                continue;
            }
            let (wx, wy) = b.pos(game.board.side);
            let gz = mesh::tile_top_z(&game.board, b.tile);
            let (cx, cy) = self.badge_anchor(wx, wy, gz);
            if !Self::on_screen(cx, cy) {
                continue;
            }
            for t in b.texts.iter() {
                let frac = (t.age / constants::FLOAT_TEXT_LIFETIME).clamp(0.0, 1.0);
                let y = cy as f64 - constants::FLOAT_TEXT_SPEED as f64 * t.age;
                let col = if t.amount < 0.0 {
                    WHITE
                } else {
                    Color::new(0.5, 1.0, 0.5, 1.0)
                };
                let txt = format!("{:+.0}", t.amount);
                draw_text(
                    &txt,
                    cx - 10.0,
                    y as f32,
                    16.0,
                    Color::new(col.r, col.g, col.b, 1.0 - frac as f32 * 0.5),
                );
            }
        }
        for v in game.vehicles.iter() {
            if v.dead {
                continue;
            }
            let gz = game
                .board
                .height(game.board.world_to_tile(v.x, v.y).unwrap_or((0, 0)))
                as f64
                * constants::ELEVATION_PX;
            let (cx, cy) = self.badge_anchor(v.x, v.y, gz);
            for t in v.texts.iter() {
                let frac = (t.age / constants::FLOAT_TEXT_LIFETIME).clamp(0.0, 1.0);
                let y = cy as f64 - constants::FLOAT_TEXT_SPEED as f64 * t.age;
                let col = if t.amount < 0.0 {
                    WHITE
                } else {
                    Color::new(0.5, 1.0, 0.5, 1.0)
                };
                let txt = format!("{:+.0}", t.amount);
                draw_text(
                    &txt,
                    cx - 10.0,
                    y as f32,
                    16.0,
                    Color::new(col.r, col.g, col.b, 1.0 - frac as f32 * 0.5),
                );
            }
            if v.wall_target.is_some() {
                let txt = format!("{}", v.wall_shots);
                draw_text(&txt, cx - 6.0, cy - 18.0, 14.0, WHITE);
            }
        }
    }
    fn draw_hud(&self) {
        let lines = [
            "Drag/WASD/arrows/edge: pan   wheel/+/-: zoom   P: pause   Alt: flat pick   Esc: menu",
        ];
        for (i, line) in lines.iter().enumerate() {
            draw_text(
                line,
                10.0,
                20.0 + i as f32 * 20.0,
                18.0,
                Color::new(0.92, 0.92, 0.92, 1.0),
            );
        }
        if self.paused {
            let txt = "PAUSED - press P to resume";
            let dim = measure_text(txt, None, 30, 1.0);
            draw_text(
                txt,
                screen_width() / 2.0 - dim.width / 2.0,
                60.0,
                30.0,
                YELLOW,
            );
        }
        if let Some(game) = self.game.as_ref()
            && game.over
        {
            let txt = if game.winner.as_deref() == Some("human") {
                "VICTORY!"
            } else {
                "DEFEAT"
            };
            let dim = measure_text(txt, None, 48, 1.0);
            draw_text(
                txt,
                screen_width() / 2.0 - dim.width / 2.0,
                screen_height() / 2.0,
                48.0,
                YELLOW,
            );
        }
    }
    fn draw_loading(&self) {
        clear_background(Color::new(0.09, 0.10, 0.13, 1.0));
        let txt = "showing the map...";
        let dim = measure_text(txt, None, 36, 1.0);
        draw_text(
            txt,
            screen_width() / 2.0 - dim.width / 2.0,
            screen_height() / 2.0,
            36.0,
            WHITE,
        );
    }
    fn draw_menu(&mut self) {
        clear_background(Color::new(0.09, 0.10, 0.13, 1.0));
        let (w, h) = (screen_width(), screen_height());
        let title = "HEXFRONT";
        draw_text(
            title,
            w / 2.0 - measure_text(title, None, 72, 1.0).width / 2.0,
            h * 0.08 + 60.0,
            72.0,
            WHITE,
        );
        let sub = "choose a level";
        draw_text(
            sub,
            w / 2.0 - measure_text(sub, None, 24, 1.0).width / 2.0,
            h * 0.08 + 100.0,
            24.0,
            GRAY,
        );
        // Three-column grid.
        let cols = (constants::MENU_COLUMNS).min(self.maps.len().max(1));
        let col_w = (w - 2.0 * constants::MENU_SIDE_MARGIN) / cols as f32;
        let grid_top = h * constants::MENU_GRID_TOP_FRACTION;
        let grid_bottom = h - constants::MENU_GRID_BOTTOM_MARGIN;
        let cell_h = 40.0;
        let stride = cell_h + constants::MENU_ROW_GAP;
        let rows = self.maps.len().div_ceil(cols);
        let content_h = rows as f32 * stride;
        let visible_h = (grid_bottom - grid_top).max(1.0);
        let max_scroll = (content_h - visible_h).max(0.0);
        self.menu_scroll = self.menu_scroll.clamp(0.0, max_scroll);
        let scroll = self.menu_scroll;
        let (mx, my) = mouse_position();
        self.menu_rects.clear();
        for (i, path) in self.maps.clone().iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let cx = constants::MENU_SIDE_MARGIN + col as f32 * col_w + col_w / 2.0;
            let cy = grid_top + row as f32 * stride + cell_h / 2.0 - scroll;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            let fs = constants::MENU_FONT_SIZE;
            let dim = measure_text(&name, None, fs, 1.0);
            let max_w = (col_w - 2.0 * constants::MENU_CELL_PAD_X - 12.0).max(20.0);
            let shown = if dim.width > max_w {
                // Truncate with ellipsis.
                let mut s = name.clone();
                while measure_text(&(s.clone() + "..."), None, fs, 1.0).width > max_w
                    && !s.is_empty()
                {
                    s.pop();
                }
                s + "..."
            } else {
                name
            };
            let dim2 = measure_text(&shown, None, fs, 1.0);
            let rect = Rect::new(
                cx - dim2.width / 2.0 - constants::MENU_CELL_PAD_X,
                cy - cell_h / 2.0,
                dim2.width + 2.0 * constants::MENU_CELL_PAD_X,
                cell_h,
            );
            self.menu_rects.push((rect, path.clone()));
            if cy + cell_h / 2.0 < grid_top || cy - cell_h / 2.0 > grid_bottom {
                continue;
            }
            if rect.contains(vec2(mx, my)) {
                draw_rectangle(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    Color::new(0.20, 0.23, 0.31, 1.0),
                );
                draw_rectangle_lines(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    2.0,
                    Color::new(0.47, 0.55, 0.78, 1.0),
                );
            }
            draw_text(
                &shown,
                cx - dim2.width / 2.0,
                cy + dim2.height / 2.5,
                fs as f32,
                WHITE,
            );
        }
        if max_scroll > 0.0 {
            let bar_h = (visible_h * visible_h / content_h.max(1.0)).max(24.0);
            let bar_y = grid_top + (visible_h - bar_h) * scroll / max_scroll;
            draw_rectangle(
                w - 14.0,
                grid_top,
                6.0,
                visible_h,
                Color::new(0.20, 0.23, 0.31, 1.0),
            );
            draw_rectangle(
                w - 14.0,
                bar_y,
                6.0,
                bar_h,
                Color::new(0.47, 0.55, 0.78, 1.0),
            );
        }
        let hint = if max_scroll > 0.0 {
            "click a level to play - wheel/Up/Down scrolls"
        } else {
            "click a level to play"
        };
        draw_text(
            hint,
            w / 2.0 - measure_text(hint, None, 20, 1.0).width / 2.0,
            h - 40.0,
            20.0,
            GRAY,
        );
    }
}
