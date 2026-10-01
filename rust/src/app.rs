//! Application layer: window, menu, level loading, input and HUD.
//!
//! Implements the controls from specification.md, section "Sterowanie":
//! view pan/zoom, source selection (RMB/LMB), vehicle sending, route
//! preview, Esc and pause.
//!
//! # States
//!
//! The application is a small state machine over [`State`]: the level menu, a
//! loading pause, the game, and the editor. Pause is a flag of the game state
//! rather than a state of its own; the editor has no pause, because nothing
//! simulates while editing. The game and the editor share the renderer, the
//! camera and the picking helpers, so a change in one is visible in the other
//! immediately.
//!
//! # Window and resizing
//!
//! The window is created with macroquad's default configuration: no size is
//! hardcoded, so the user can resize it, and the render size is read fresh
//! every frame from `screen_width()`/`screen_height()` rather than cached at
//! startup. A resize therefore invalidates the rasterisation buffers and the
//! cached terrain mesh, which `ensure_buffers` and the terrain fingerprint
//! detect.
//!
//! # Text and HUD
//!
//! UI text (menu, unit badges, floating combat numbers) is drawn last, on top
//! of the isometric scene, with macroquad's built-in font. Unit badges
//! ([`Application::draw_badges`]) are anchored to the lower right of the object
//! they belong to and are deliberately exempt from the depth buffer, so a
//! number is never hidden by terrain; the badge disc holds the unit count, a
//! `MAX` label sits under it for a full building, and a base draws a spawn
//! progress ring beside it. Badges outside the viewport are skipped, so a
//! large board costs no HUD work off-screen.
//!
//! # Level menu
//!
//! [`Application::draw_menu`] lays the levels of the `maps` directory out in
//! a compact grid ([`constants::MENU_COLUMNS`] columns) and reports the cell
//! rectangles it built, so input and drawing agree on what is clickable. When
//! the rows do not fit on screen the grid scrolls with the mouse wheel or the
//! arrow / page keys, clamped to the content height, and a scrollbar is drawn
//! on the right while it is scrollable. Long level names are ellipsised to the
//! column width.

use std::path::PathBuf;

use macroquad::prelude::*;

use crate::ai::AiController;
use crate::camera::Camera;
use crate::constants::{self};
use crate::editor::{EditorOverlay, EditorState};
use crate::entities::{Player, vehicle_kind_of};
use crate::game::Game;
use crate::hexgrid::Tile;
use crate::iso::IsoCamera;
use crate::mapfile::{self, level_seed};
use crate::math;
use crate::mesh::{self, DynamicMesh, TerrainMesh};
use crate::render::Renderer;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Menu,
    Loading,
    Playing,
    Editor,
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
    /// Map editor state (entered from the menu via `add map` / RMB).
    editor: Option<EditorState>,
    /// True while playing the edited level as a test (`p` in the editor):
    /// Esc then returns to the editor instead of the level menu.
    playtest: bool,
    /// Editor preview wrapped as a game so the shared renderer draws it.
    editor_game: Option<Game>,
    /// True when the editor terrain buffers match `editor_game`.
    editor_clean: bool,
    /// Fingerprint of the board the terrain buffers were built from, so
    /// building-only edits (units, owner, kind) skip the expensive static
    /// mesh rebuild; terrain-affecting edits change the fingerprint.
    editor_terrain_fp: u64,
    /// Last frame's hovered editor tile (for click edge detection).
    editor_tile: Option<Tile>,
    /// Explosion particles of destroyed vehicles (presentation only, see
    /// [`crate::fx`]). Filled from the wrecks the simulation reports.
    fx: crate::fx::Fx,
    /// Sound playback (presentation only, see [`crate::audio`]). Filled from
    /// the sounds the simulation reports next to the explosion particles.
    audio: crate::audio::Audio,
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
            editor: None,
            editor_game: None,
            editor_clean: true,
            editor_terrain_fp: 0,
            editor_tile: None,
            fx: crate::fx::Fx::new(),
            audio: crate::audio::Audio::new(),
            playtest: false,
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
        // Synthesise and decode the sound effects once, before the first
        // frame. A failure here only disables sound (see `Audio::load`).
        self.audio.load().await;
        loop {
            let dt = get_frame_time().min(0.1);
            self.handle_input(dt);
            self.update(dt);
            if std::env::var("HEXFRONT_TIMING").is_ok()
                && (self.state == State::Playing || self.state == State::Editor)
            {
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
            self.draw(dt);
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
        // macroquad queues one char per pressed key and never drops them on
        // its own; only the editor's Save overlay reads that queue. Drain it
        // in every other state so keys pressed while playing or editing never
        // pile up and then spill into the map-name field.
        let save_open = self.state == State::Editor
            && self
                .editor
                .as_ref()
                .map(|e| crate::editor::consumes_text(e.overlay))
                .unwrap_or(false);
        if !save_open {
            while get_char_pressed().is_some() {}
        }
        let wheel = mouse_wheel();
        if wheel.1 != 0.0 {
            if self.state == State::Menu {
                self.menu_scroll -= wheel.1 * constants::MENU_SCROLL_STEP;
            } else if self.state == State::Playing || self.state == State::Editor {
                let (mx, my) = mouse_position();
                let f = if wheel.1 > 0.0 {
                    constants::ZOOM_STEP
                } else {
                    1.0 / constants::ZOOM_STEP
                };
                self.camera.zoom_at(f, mx, my);
            }
        }
        if self.state == State::Editor {
            self.handle_editor_input(dt);
            return;
        }
        if self.state == State::Playing {
            self.pan_view(dt, true);
            let (mx, my) = mouse_position();
            if is_mouse_button_down(MouseButton::Left) {
                if self.down_pos.is_some() {
                    if self.drag_started(mx, my) {
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
                } else if self.playtest {
                    // A test run of the edited level always goes back to the
                    // editor (specification_rust.md, "Edytor plansz").
                    self.return_to_editor();
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
    /// View panning shared by the game and the editor: arrow keys, screen
    /// edges and LMB drag. The editor skips WASD (key `s` saves) and RMB pan
    /// (RMB deletes objects).
    fn pan_view(&mut self, dt: f32, wasd: bool) {
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
        if is_key_down(KeyCode::Left) || (wasd && is_key_down(KeyCode::A)) {
            dx += constants::PAN_SPEED as f32 * dt;
        }
        if is_key_down(KeyCode::Right) || (wasd && is_key_down(KeyCode::D)) {
            dx -= constants::PAN_SPEED as f32 * dt;
        }
        if is_key_down(KeyCode::Up) || (wasd && is_key_down(KeyCode::W)) {
            dy += constants::PAN_SPEED as f32 * dt;
        }
        if is_key_down(KeyCode::Down) || (wasd && is_key_down(KeyCode::S)) {
            dy -= constants::PAN_SPEED as f32 * dt;
        }
        if dx != 0.0 || dy != 0.0 {
            self.camera.pan(dx, dy);
        }
    }
    /// True once the held left button has moved further than
    /// [`constants::DRAG_THRESHOLD`] from the point it went down on.
    ///
    /// A press that stays inside the threshold counts as a click, not as a
    /// view pan, so the game and the editor both have to ask this question
    /// before they pan. Latched on the first frame that answers yes, which is
    /// why the caller only asks while `dragging` is still false.
    fn drag_started(&mut self, mx: f32, my: f32) -> bool {
        if self.dragging {
            return true;
        }
        if let Some((dx0, dy0)) = self.down_pos {
            // Compared squared, so the per-frame threshold test needs no root.
            let d2 = math::sqr(f64::from(mx - dx0)) + math::sqr(f64::from(my - dy0));
            if d2 > math::sqr(f64::from(constants::DRAG_THRESHOLD)) {
                self.dragging = true;
            }
        }
        self.dragging
    }
    fn enter_menu(&mut self) {
        self.state = State::Menu;
        self.maps = mapfile::list_maps(None);
        self.menu_scroll = 0.0;
        self.game = None;
        self.ai.clear();
        self.selection = None;
        self.editor = None;
        self.editor_game = None;
        self.playtest = false;
        self.fx.clear();
        self.audio.stop_all();
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
                self.camera.focus_board(&game.board);
                self.terrain = mesh::build_terrain(&game.board);
                self.renderer.set_terrain(&self.terrain);
                self.terrain_board_key = Some((game.board.cols, game.board.rows));
                self.game = Some(game);
                self.ai = ai;
                self.playtest = false;
                self.paused = false;
                self.selection = None;
                self.preview_tile = None;
                self.preview_path = None;
                self.load_timer = 0.0;
                self.sim_acc = 0.0;
                // The explosion particles of a level are drawn from a stream
                // seeded with the level seed: random on screen, repeatable
                // when the same level is played again.
                self.fx.reseed(seed);
                // Same for the choice between the recordings of one event.
                self.audio.reseed(seed);
                self.state = State::Loading;
            }
            Err(e) => eprintln!("cannot load {}: {}", path.display(), e),
        }
    }
    /// Start a test run (`p` in the editor) of the map being edited.
    ///
    /// The playtest builds a fresh [`Game`] from a copy of the editor board
    /// (players from the placed building owners, player 0 human, AI controls
    /// the rest), keeps the current view and remembers that Esc must return
    /// to the editor; the edited map itself is never touched by the run.
    fn start_playtest(&mut self) {
        // A pending units entry is accepted so the test plays the shown value.
        if let Some(ed) = self.editor.as_mut() {
            ed.commit_digits();
        }
        let Some(ed) = self.editor.as_ref() else {
            return;
        };
        let game = ed.playtest_game();
        let seed = ed.playtest_seed();
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
        self.terrain = mesh::build_terrain(&game.board);
        self.renderer.set_terrain(&self.terrain);
        self.terrain_board_key = Some((game.board.cols, game.board.rows));
        self.camera.limit_to_board(&game.board);
        self.game = Some(game);
        self.ai = ai;
        self.playtest = true;
        self.paused = false;
        self.selection = None;
        self.preview_tile = None;
        self.preview_path = None;
        self.load_timer = 0.0;
        self.sim_acc = 0.0;
        self.fx.reseed(seed);
        self.audio.reseed(seed);
        self.state = State::Playing;
    }
    /// Leave a playtest run and keep editing the same map (Esc in playtest).
    fn return_to_editor(&mut self) {
        self.game = None;
        self.ai.clear();
        self.playtest = false;
        self.paused = false;
        self.selection = None;
        self.preview_tile = None;
        self.preview_path = None;
        self.state = State::Editor;
        // Rebuild the editor preview from the (unchanged) edited board; the
        // terrain fingerprint matches, so the static mesh is not rebuilt.
        self.editor_clean = false;
    }
    fn update(&mut self, dt: f32) {
        // Retrigger limiting runs on wall-clock time, not simulation time, so
        // it keeps the same pace while the game is paused.
        self.audio.update(dt);
        if self.state == State::Loading {
            self.load_timer += dt as f64;
            if self.load_timer >= constants::LOADING_TIME {
                self.state = State::Playing;
            }
            return;
        }
        if self.state == State::Editor {
            // Digit-entry commit timer; the terrain/game preview rebuild
            // happens lazily in draw (macroquad context).
            if let Some(ed) = self.editor.as_mut() {
                ed.tick(dt as f64);
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
        // Vehicles that fell to zero units (rules.md sections 4, 9, 10) leave a
        // wreck behind; the effect is started here, so it runs on the same
        // frame the wreck disappears. The effects themselves are advanced with
        // the wall-clock delta in `draw`, like the rotor phase.
        self.spawn_wreck_effects();
        self.play_sounds();
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
    /// Turn the wrecks reported by the simulation into explosion effects.
    ///
    /// The rendered height of each wreck is taken from the same geometry the
    /// vehicle itself was drawn with ([`mesh::vehicle_z`]), so a blast starts
    /// exactly where the hull was: on the ground for a ground vehicle, at
    /// flight altitude for a helicopter.
    fn spawn_wreck_effects(&mut self) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        for wreck in game.take_wrecks() {
            let z = mesh::wreck_z(game, &wreck);
            self.fx.explode(&wreck, z);
        }
    }
    /// Play the sounds reported by the simulation since the last frame.
    ///
    /// Runs next to [`Application::spawn_wreck_effects`] and drains the same
    /// kind of one-shot event list the explosion particles come from
    /// ([`Game::take_sounds`](crate::game::Game::take_sounds)), so a shot is
    /// heard on the same frame the simulation fired it. The events carry only
    /// *where* they happened; [`crate::audio`] turns that into a volume using
    /// the view centre, which is why the camera is read here.
    fn play_sounds(&mut self) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        let events = game.take_sounds();
        if events.is_empty() {
            return;
        }
        // Invert the projection to get the world point the view is centred on
        // (the same inverse the picking code uses, at zero elevation).
        let (sx, sy) = (
            self.camera.screen_size.0 / 2.0,
            self.camera.screen_size.1 / 2.0,
        );
        let centre = self.camera.screen_to_world(sx, sy, 0.0);
        self.audio.play_events(centre, &events);
    }
    fn draw(&mut self, dt: f32) {
        self.ensure_buffers();
        match self.state {
            State::Menu => self.draw_menu(),
            State::Loading => self.draw_loading(),
            State::Playing => self.draw_game(dt),
            State::Editor => self.draw_editor(dt),
        }
    }
    fn draw_game(&mut self, dt: f32) {
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
            // Explosion particles live in the presentation layer, not in the
            // simulation, so they are advanced with the wall-clock delta
            // (exactly like the rotor phase) and rebuilt into the same
            // per-frame mesh.
            self.fx.update(f64::from(dt));
            self.fx.build(&mut self.dynamic);
            // Advance the shared rotor phase by wall-clock time, so the
            // spin speed does not depend on the frame rate; the next frame
            // uses it for the airframe blades and their shadow alike.
            self.renderer.rotor_phase = (self.renderer.rotor_phase
                + constants::ROTOR_SPIN_RAD_PER_S * f64::from(dt))
                % std::f64::consts::TAU;
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
                // Spawn progress ring: a base
                // shows a thin white arc completing one full circle over
                // the spawn interval.
                let frac = (b.production_timer / constants::BASE_SPAWN_INTERVAL).clamp(0.0, 1.0);
                if frac > 0.0 {
                    let segs = ((frac * 24.0).ceil() as u8).max(1);
                    // macroquad draws the arc from `rotation` to
                    // `rotation + arc`; the call below used to pass the
                    // thickness and the sweep swapped, which drew a fat
                    // short bar instead of a thin ring (the "horizontal
                    // tick" next to bases).
                    draw_arc(
                        cx,
                        cy,
                        segs,
                        r + 4.0,
                        -90.0,
                        2.0,
                        360.0 * frac as f32,
                        WHITE,
                    );
                }
            }
        }
        for v in game.vehicles.iter() {
            if v.dead {
                continue;
            }
            let gz = mesh::vehicle_z(game, v);
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
            let gz = mesh::vehicle_z(game, v);
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
        let lines = [if self.playtest {
            "TEST of the edited map   Esc: back to the editor   P: pause   wheel/+/-: zoom"
        } else {
            "Drag/WASD/arrows/edge: pan   wheel/+/-: zoom   P: pause   Alt: flat pick   Esc: menu"
        }];
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
        // "add map" button opens a fresh editor board (specification_rust.md).
        let add_rect = Rect::new(w / 2.0 - 90.0, h * 0.08 + 118.0, 180.0, 40.0);
        draw_rectangle(
            add_rect.x,
            add_rect.y,
            add_rect.w,
            add_rect.h,
            Color::new(0.16, 0.35, 0.18, 1.0),
        );
        draw_rectangle_lines(
            add_rect.x,
            add_rect.y,
            add_rect.w,
            add_rect.h,
            2.0,
            Color::new(0.45, 0.85, 0.5, 1.0),
        );
        {
            let t = "add map";
            let dim = measure_text(t, None, 22, 1.0);
            draw_text(
                t,
                w / 2.0 - dim.width / 2.0,
                h * 0.08 + 118.0 + 27.0,
                22.0,
                WHITE,
            );
        }
        if add_rect.contains(vec2(mx, my)) && is_mouse_button_pressed(MouseButton::Left) {
            self.enter_editor_new();
            return;
        }
        // RMB on a map cell edits that map (specification_rust.md).
        if is_mouse_button_pressed(MouseButton::Right) {
            for (rect, path) in self.menu_rects.clone() {
                if rect.contains(vec2(mx, my)) {
                    self.enter_editor_path(&path);
                    return;
                }
            }
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
            "click a level to play - RMB edits - wheel/Up/Down scrolls"
        } else {
            "click a level to play - RMB edits"
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

impl Application {
    /// Ctrl held on either side (quick save / new map).
    fn ctrl_down() -> bool {
        is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
    }
    /// Tile under the cursor in the editor (Alt picks flat, like the game).
    fn editor_hover(&self) -> Option<Tile> {
        let ed = self.editor.as_ref()?;
        let (mx, my) = mouse_position();
        ed.board.pick_tile(&self.camera, mx, my, Self::flat())
    }
    /// Rebuild the editor preview game from the editor board.
    ///
    /// The `Game` wrapper (buildings, obstacles, ranges) is rebuilt on every
    /// sync, but the static terrain mesh only when the terrain fingerprint
    /// changed (height, ramp or bridge edit) — a full 256x256 rebuild costs
    /// ~50 ms and must not hitch digit typing or owner cycling.
    fn sync_editor_game(&mut self) {
        let Some(ed) = self.editor.as_ref() else {
            return;
        };
        let players = vec![
            Player::new(0, true),
            Player::new(1, false),
            Player::new(2, false),
            Player::new(3, false),
        ];
        let fp = ed.terrain_fingerprint();
        let game = Game::new(ed.board.clone(), players, ed.buildings.clone(), 0);
        self.editor_game = Some(game);
        // NOTE: unlike the game (immutable board per level), the editor board
        // mutates in place (terrain height, ramps, bridges), so the (cols,
        // rows) key is not enough to skip the static mesh rebuild
        // (specification_rust.md: rebuild on each terrain/ramp/bridge change).
        // `build_dynamic` below still runs every frame for objects.
        if fp != self.editor_terrain_fp {
            if let Some(g) = self.editor_game.as_ref() {
                self.terrain = mesh::build_terrain(&g.board);
                self.renderer.set_terrain(&self.terrain);
                self.terrain_board_key = Some((g.board.cols, g.board.rows));
            }
            self.editor_terrain_fp = fp;
        }
        self.editor_clean = true;
    }
    /// Enter the editor with a fresh board, centred on the land rectangle.
    fn enter_editor_new(&mut self) {
        let ed = EditorState::new_board();
        self.camera = Camera::new((screen_width(), screen_height()));
        self.camera.focus_board(&ed.board);
        self.editor = Some(ed);
        self.editor_game = None;
        self.editor_clean = false;
        self.editor_terrain_fp = 0;
        self.editor_tile = None;
        self.down_pos = None;
        self.dragging = false;
        self.state = State::Editor;
        self.sync_editor_game();
    }
    /// Enter the editor editing the map at `path`.
    fn enter_editor_path(&mut self, path: &std::path::Path) {
        let mut ed = EditorState::new_board();
        match ed.load_path(path) {
            Ok(()) => {
                self.camera = Camera::new((screen_width(), screen_height()));
                self.camera.focus_board(&ed.board);
                self.editor = Some(ed);
                self.editor_game = None;
                self.editor_clean = false;
                self.editor_terrain_fp = 0;
                self.editor_tile = None;
                self.down_pos = None;
                self.dragging = false;
                self.state = State::Editor;
                self.sync_editor_game();
            }
            Err(e) => eprintln!("cannot edit {}: {}", path.display(), e),
        }
    }
}

impl Application {
    /// Editor keyboard/mouse handling (see specification_rust.md, "Edytor plansz").
    fn handle_editor_input(&mut self, dt: f32) {
        // View: arrows/edges/LMB drag/wheel/+- like the game, but no WASD and
        // no RMB pan (RMB deletes objects).
        self.pan_view(dt, false);
        let (mx, my) = mouse_position();
        // LMB drag pans; a click without drag edits via Delete/RMB path below
        // (keys do the editing, clicks only delete with RMB).
        if is_mouse_button_down(MouseButton::Left) {
            if self.down_pos.is_some() {
                if self.drag_started(mx, my) {
                    self.camera
                        .pan(mx - self.last_mouse.0, my - self.last_mouse.1);
                }
            } else {
                self.down_pos = Some((mx, my));
                self.dragging = false;
            }
        } else if let Some(_down) = self.down_pos.take() {
            self.dragging = false;
        }
        self.last_mouse = (mx, my);
        if is_mouse_button_pressed(MouseButton::Right) {
            let tile = self.editor_hover();
            if let Some(ed) = self.editor.as_mut()
                && ed.overlay == EditorOverlay::None
                && ed.delete_at(tile)
            {
                self.editor_clean = false;
            }
        }
        if is_key_pressed(KeyCode::Equal) || is_key_pressed(KeyCode::KpAdd) {
            self.camera.zoom_at(constants::ZOOM_STEP, mx, my);
        }
        if is_key_pressed(KeyCode::Minus) || is_key_pressed(KeyCode::KpSubtract) {
            self.camera.zoom_at(1.0 / constants::ZOOM_STEP, mx, my);
        }
        self.handle_editor_keys();
        // Track hover for digit acceptance display.
        self.editor_tile = self.editor_hover();
    }
    /// Editor key handling split out for readability.
    fn handle_editor_keys(&mut self) {
        let overlay = self
            .editor
            .as_ref()
            .map(|e| e.overlay)
            .unwrap_or(EditorOverlay::None);
        // Overlay input first.
        if overlay != EditorOverlay::None {
            self.handle_editor_overlay_keys(overlay);
            return;
        }
        let ctrl = Self::ctrl_down();
        // ctrl+n: new map, ctrl+s: quick save (both work even with overlays closed).
        if ctrl && is_key_pressed(KeyCode::N) {
            if let Some(ed) = self.editor.as_mut() {
                ed.new_map();
                self.camera.focus_board(&ed.board);
                self.editor_clean = false;
            }
            return;
        }
        if ctrl && is_key_pressed(KeyCode::S) {
            let opened = {
                let ed = self.editor.as_mut().unwrap();
                match ed.quick_save() {
                    Ok(true) => false,
                    Ok(false) => true,
                    Err(e) => {
                        eprintln!("cannot save: {}", e);
                        false
                    }
                }
            };
            let _ = opened;
            return;
        }
        if is_key_pressed(KeyCode::Escape) {
            let dirty = self.editor.as_ref().map(|e| e.dirty).unwrap_or(false);
            if dirty {
                if let Some(ed) = self.editor.as_mut() {
                    ed.commit_digits();
                    ed.overlay = EditorOverlay::Exit;
                }
            } else {
                self.enter_menu();
            }
            return;
        }
        // `p`: play the edited level as a test; Esc there returns to editing.
        if is_key_pressed(KeyCode::P) {
            self.start_playtest();
            return;
        }
        let tile = self.editor_hover();
        // Digit keys: Key0-Key9 + keypad.
        let mut digit: Option<char> = None;
        for (code, ch) in [
            (KeyCode::Key0, '0'),
            (KeyCode::Key1, '1'),
            (KeyCode::Key2, '2'),
            (KeyCode::Key3, '3'),
            (KeyCode::Key4, '4'),
            (KeyCode::Key5, '5'),
            (KeyCode::Key6, '6'),
            (KeyCode::Key7, '7'),
            (KeyCode::Key8, '8'),
            (KeyCode::Key9, '9'),
        ] {
            if is_key_pressed(code) {
                digit = Some(ch);
                break;
            }
        }
        if digit.is_none() {
            for (code, ch) in [
                (KeyCode::Kp0, '0'),
                (KeyCode::Kp1, '1'),
                (KeyCode::Kp2, '2'),
                (KeyCode::Kp3, '3'),
                (KeyCode::Kp4, '4'),
                (KeyCode::Kp5, '5'),
                (KeyCode::Kp6, '6'),
                (KeyCode::Kp7, '7'),
                (KeyCode::Kp8, '8'),
                (KeyCode::Kp9, '9'),
            ] {
                if is_key_pressed(code) {
                    digit = Some(ch);
                    break;
                }
            }
        }
        if let Some(ch) = digit {
            if let Some(ed) = self.editor.as_mut()
                && ed.type_digit(tile, ch)
            {
                self.editor_clean = false;
            }
            return;
        }
        // Typed letters arrive via chars too, but B/O/T/M/P/R/L/S brackets are
        // physical keys; check them explicitly (layout-independent enough for
        // the Latin keys the spec names).
        if is_key_pressed(KeyCode::B) {
            if let Some(ed) = self.editor.as_mut()
                && ed.press_b(tile)
            {
                self.editor_clean = false;
            }
        } else if is_key_pressed(KeyCode::O) {
            if let Some(ed) = self.editor.as_mut()
                && ed.press_o(tile)
            {
                self.editor_clean = false;
            }
        } else if is_key_pressed(KeyCode::T) {
            if let Some(ed) = self.editor.as_mut()
                && ed.press_t(tile)
            {
                self.editor_clean = false;
            }
        } else if is_key_pressed(KeyCode::M) {
            if let Some(ed) = self.editor.as_mut()
                && ed.press_m(tile)
            {
                self.editor_clean = false;
            }
        } else if is_key_pressed(KeyCode::R) {
            // Ramp placement/rotation (`r`).
            if let Some(ed) = self.editor.as_mut()
                && ed.press_r(tile)
            {
                self.editor_clean = false;
            }
        } else if is_key_pressed(KeyCode::LeftBracket) {
            if let Some(ed) = self.editor.as_mut()
                && ed.change_height(tile, -1)
            {
                self.editor_clean = false;
            }
        } else if is_key_pressed(KeyCode::RightBracket) {
            if let Some(ed) = self.editor.as_mut()
                && ed.change_height(tile, 1)
            {
                self.editor_clean = false;
            }
        } else if is_key_pressed(KeyCode::Delete) || is_key_pressed(KeyCode::Backspace) {
            if let Some(ed) = self.editor.as_mut()
                && ed.delete_at(tile)
            {
                self.editor_clean = false;
            }
        } else if is_key_pressed(KeyCode::L) {
            if let Some(ed) = self.editor.as_mut() {
                ed.open_load();
            }
        } else if is_key_pressed(KeyCode::S)
            && !ctrl
            && let Some(ed) = self.editor.as_mut()
        {
            ed.open_save();
        }
    }
}

impl Application {
    /// Keys inside the load/save/exit overlays.
    fn handle_editor_overlay_keys(&mut self, overlay: EditorOverlay) {
        if is_key_pressed(KeyCode::Escape) {
            if let Some(ed) = self.editor.as_mut() {
                ed.overlay = EditorOverlay::None;
            }
            return;
        }
        match overlay {
            EditorOverlay::Load => {
                let n = self
                    .editor
                    .as_ref()
                    .map(|e| e.overlay_items.len())
                    .unwrap_or(0);
                if is_key_pressed(KeyCode::Up)
                    && let Some(ed) = self.editor.as_mut()
                    && ed.overlay_cursor > 0
                {
                    ed.overlay_cursor -= 1;
                }
                if is_key_pressed(KeyCode::Down)
                    && let Some(ed) = self.editor.as_mut()
                    && ed.overlay_cursor + 1 < n
                {
                    ed.overlay_cursor += 1;
                }
                if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
                    let path = self
                        .editor
                        .as_ref()
                        .and_then(|e| e.overlay_items.get(e.overlay_cursor).cloned());
                    if let Some(path) = path {
                        let ok = {
                            let ed = self.editor.as_mut().unwrap();
                            ed.load_path(&path).is_ok()
                        };
                        if ok {
                            if let Some(ed) = self.editor.as_ref() {
                                self.camera.focus_board(&ed.board);
                            }
                            self.editor_clean = false;
                        }
                    }
                }
                // Click selection happens in draw (rects need layout); see draw_editor_overlay.
            }
            EditorOverlay::Save => {
                // Type the name with printable chars; Backspace deletes.
                // The char that opened this overlay was already drained in
                // handle_input (the queue is only left alone while Save is
                // open), so everything read here is real typing.
                while let Some(ch) = get_char_pressed() {
                    if ch == '\n' || ch == '\r' {
                        continue;
                    }
                    if let Some(ed) = self.editor.as_mut() {
                        if ch.is_control() {
                            continue;
                        }
                        ed.input_text.push(ch);
                    }
                }
                // get_char_pressed already consumed text keys; Backspace
                // arrives as a key press (not a char) on most layouts.
                if is_key_pressed(KeyCode::Backspace)
                    && let Some(ed) = self.editor.as_mut()
                {
                    ed.input_text.pop();
                }
                let n = self
                    .editor
                    .as_ref()
                    .map(|e| e.overlay_items.len())
                    .unwrap_or(0);
                if is_key_pressed(KeyCode::Up)
                    && let Some(ed) = self.editor.as_mut()
                    && ed.overlay_cursor > 0
                {
                    ed.overlay_cursor -= 1;
                }
                if is_key_pressed(KeyCode::Down)
                    && let Some(ed) = self.editor.as_mut()
                    && ed.overlay_cursor + 1 < n
                {
                    ed.overlay_cursor += 1;
                }
                if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
                    // Empty field + cursor on a list entry: reuse that name.
                    let pick = self.editor.as_ref().and_then(|e| {
                        if e.input_text.trim().is_empty() {
                            e.overlay_items.get(e.overlay_cursor).cloned()
                        } else {
                            None
                        }
                    });
                    if let Some(path) = pick
                        && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                        && let Some(ed) = self.editor.as_mut()
                    {
                        ed.input_text = stem.to_string();
                    }
                    let res = self.editor.as_mut().map(|ed| ed.save());
                    if let Some(Err(e)) = res {
                        eprintln!("cannot save: {}", e);
                    }
                }
            }
            EditorOverlay::Exit => {
                // S: save and exit, N: discard and exit, Esc: keep editing.
                if is_key_pressed(KeyCode::S) {
                    let saved = self.editor.as_mut().map(|ed| ed.save());
                    match saved {
                        Some(Ok(_)) => self.enter_menu(),
                        Some(Err(_)) => {
                            // No name yet: switch to the save overlay.
                            if let Some(ed) = self.editor.as_mut() {
                                ed.open_save();
                            }
                        }
                        None => {}
                    }
                }
                if is_key_pressed(KeyCode::N) {
                    self.enter_menu();
                }
            }
            EditorOverlay::None => {}
        }
    }
}

impl Application {
    /// Draw the editor: shared 3D preview plus legend, errors and overlays.
    fn draw_editor(&mut self, dt: f32) {
        if !self.editor_clean {
            self.sync_editor_game();
        }
        let hover = self.editor_hover();
        if let Some(game) = self.editor_game.as_ref() {
            let (lo, hi) = mesh::depth_span(&game.board);
            let iso = IsoCamera::from_camera(&self.camera, lo, hi);
            let view_bounds = mesh::visible_world_bounds(
                &self.camera,
                mesh::max_height(&game.board),
                game.board.side,
            );
            mesh::build_dynamic(game, self.renderer.rotor_phase, &mut self.dynamic);
            self.fx.update(f64::from(dt));
            self.fx.build(&mut self.dynamic);
            self.renderer.rotor_phase = (self.renderer.rotor_phase
                + constants::ROTOR_SPIN_RAD_PER_S * f64::from(dt))
                % std::f64::consts::TAU;
            self.renderer.draw_gpu(
                &iso,
                &self.camera,
                view_bounds,
                &self.dynamic,
                game,
                hover,
                None,
                None,
            );
        }
        self.draw_editor_badges(hover);
        self.draw_editor_hud();
        let overlay = self
            .editor
            .as_ref()
            .map(|e| e.overlay)
            .unwrap_or(EditorOverlay::None);
        match overlay {
            EditorOverlay::Load => self.draw_editor_list("load map", false),
            EditorOverlay::Save => self.draw_editor_save(),
            EditorOverlay::Exit => self.draw_editor_exit(),
            EditorOverlay::None => {}
        }
    }
    /// Unit badges for the editor preview (typed digits show live).
    fn draw_editor_badges(&self, hover: Option<Tile>) {
        let Some(game) = self.editor_game.as_ref() else {
            return;
        };
        for b in game.buildings.iter() {
            let (wx, wy) = b.pos(game.board.side);
            let wz = mesh::tile_top_z(&game.board, b.tile);
            let (cx, cy) = self.badge_anchor(wx, wy, wz);
            if !Self::on_screen(cx, cy) {
                continue;
            }
            let fs = ((17.0 * self.camera.zoom as f32).max(10.0)) as u16;
            // Pending digit entry shows the typed buffer on its tile.
            let txt = if let Some(ed) = self.editor.as_ref() {
                if ed.digit_tile == Some(b.tile) && !ed.digit_buf.is_empty() {
                    ed.digit_buf.clone()
                } else {
                    format!("{}", b.units.round() as i64)
                }
            } else {
                format!("{}", b.units.round() as i64)
            };
            let dim = measure_text(&txt, None, fs, 1.0);
            draw_text(&txt, cx - dim.width / 2.0, cy, fs as f32, WHITE);
            let _ = hover;
        }
    }
    /// Legend (bottom-left) and validation errors (top-left, red).
    fn draw_editor_hud(&self) {
        let Some(ed) = self.editor.as_ref() else {
            return;
        };
        let errors = ed.validate();
        for (i, e) in errors.iter().enumerate() {
            let ec = constants::EDITOR_ERROR_COLOR;
            draw_text(
                e,
                10.0,
                24.0 + i as f32 * 22.0,
                18.0,
                Color::from_rgba(ec[0], ec[1], ec[2], 255),
            );
        }
        let h = screen_height();
        for (i, line) in crate::editor::LEGEND.iter().enumerate() {
            draw_text(
                line,
                10.0,
                h - (crate::editor::LEGEND.len() - i) as f32 * 20.0 - 8.0,
                16.0,
                Color::new(0.75, 0.78, 0.85, 1.0),
            );
        }
        let name = ed.map_name.as_deref().unwrap_or("(unnamed)");
        let dirty = if ed.dirty { " *" } else { "" };
        let title = format!("editor: {}{}", name, dirty);
        draw_text(
            &title,
            10.0,
            h - crate::editor::LEGEND.len() as f32 * 20.0 - 32.0,
            20.0,
            WHITE,
        );
    }
    /// Generic overlay panel with a centred title box.
    fn editor_panel(&self, title: &str, height: f32) {
        let (w, h) = (screen_width(), screen_height());
        draw_rectangle(0.0, 0.0, w, h, Color::new(0.0, 0.0, 0.0, 0.55));
        let (pw, ph) = (520.0, height);
        let (px, py) = ((w - pw) / 2.0, (h - ph) / 2.0);
        draw_rectangle(px, py, pw, ph, Color::new(0.10, 0.11, 0.15, 1.0));
        draw_rectangle_lines(px, py, pw, ph, 2.0, Color::new(0.47, 0.55, 0.78, 1.0));
        draw_text(
            title,
            w / 2.0 - measure_text(title, None, 26, 1.0).width / 2.0,
            py + 34.0,
            26.0,
            WHITE,
        );
    }
    /// Load/save list overlay; clicks pick entries (current name highlighted first).
    fn draw_editor_list(&mut self, title: &str, is_save: bool) {
        let items = self
            .editor
            .as_ref()
            .map(|e| {
                (
                    e.overlay_items.clone(),
                    e.overlay_cursor,
                    e.map_name.clone(),
                )
            })
            .unwrap_or_default();
        let (paths, cursor, current) = items;
        self.editor_panel(title, 560.0);
        let (w, h) = (screen_width(), screen_height());
        // Clickable rows.
        let mut clicked: Option<PathBuf> = None;
        let (mx, my) = mouse_position();
        let mut y = h / 2.0 - 210.0;
        for (i, path) in paths.iter().enumerate().take(12) {
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
            let rect = Rect::new(w / 2.0 - 230.0, y, 460.0, 30.0);
            let hl = Some(name.to_string()) == current || i == cursor;
            let bg = if hl {
                Color::new(0.20, 0.23, 0.31, 1.0)
            } else {
                Color::new(0.13, 0.14, 0.18, 1.0)
            };
            draw_rectangle(rect.x, rect.y, rect.w, rect.h, bg);
            if hl {
                draw_rectangle_lines(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    2.0,
                    Color::new(1.0, 1.0, 0.47, 1.0),
                );
            }
            draw_text(name, rect.x + 10.0, rect.y + 22.0, 20.0, WHITE);
            if rect.contains(vec2(mx, my)) {
                if is_mouse_button_pressed(MouseButton::Left) {
                    clicked = Some(path.clone());
                }
                if let Some(ed) = self.editor.as_mut() {
                    ed.overlay_cursor = i;
                }
            }
            y += 34.0;
        }
        if paths.len() > 12 {
            draw_text(
                format!("... and {} more", paths.len() - 12),
                w / 2.0 - 200.0,
                y + 10.0,
                18.0,
                GRAY,
            );
        }
        draw_text(
            "Up/Down + Enter, click, or Esc",
            w / 2.0 - 200.0,
            h / 2.0 + 250.0,
            18.0,
            GRAY,
        );
        if let Some(path) = clicked {
            if is_save
                && let Some(stem) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                && let Some(ed) = self.editor.as_mut()
            {
                if ed.input_text.trim().is_empty() {
                    ed.input_text = stem;
                } else {
                    // Non-empty field: save under the typed name right away.
                    let res = ed.save();
                    if let Err(e) = res {
                        eprintln!("cannot save: {}", e);
                    }
                }
            } else if !is_save {
                let ok = {
                    let ed = self.editor.as_mut().unwrap();
                    ed.load_path(&path).is_ok()
                };
                if ok {
                    if let Some(ed) = self.editor.as_ref() {
                        self.camera.focus_board(&ed.board);
                    }
                    self.editor_clean = false;
                }
            }
        }
    }
    /// Save overlay: name field plus the existing-map list.
    fn draw_editor_save(&mut self) {
        self.draw_editor_list("save map", true);
        let (w, h) = (screen_width(), screen_height());
        let txt = self
            .editor
            .as_ref()
            .map(|e| e.input_text.clone())
            .unwrap_or_default();
        let field = Rect::new(w / 2.0 - 230.0, h / 2.0 - 252.0, 460.0, 34.0);
        draw_rectangle(
            field.x,
            field.y,
            field.w,
            field.h,
            Color::new(0.07, 0.08, 0.10, 1.0),
        );
        draw_rectangle_lines(
            field.x,
            field.y,
            field.w,
            field.h,
            1.0,
            Color::new(1.0, 1.0, 0.47, 1.0),
        );
        draw_text(
            format!("{}_", txt),
            field.x + 10.0,
            field.y + 24.0,
            20.0,
            Color::new(1.0, 1.0, 0.47, 1.0),
        );
    }
    /// Unsaved-changes prompt on exit.
    fn draw_editor_exit(&self) {
        self.editor_panel("unsaved changes", 190.0);
        let (w, h) = (screen_width(), screen_height());
        for (i, line) in [
            "S - save and exit",
            "N - discard changes and exit",
            "Esc - keep editing",
        ]
        .iter()
        .enumerate()
        {
            draw_text(
                line,
                w / 2.0 - 140.0,
                h / 2.0 - 10.0 + i as f32 * 32.0,
                22.0,
                WHITE,
            );
        }
    }
}
