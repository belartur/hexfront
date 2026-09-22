//! Isometric renderer: every graphic is drawn from code (no raster assets).
//!
//! Objects of individual players differ by colour (specification.md,
//! section "Grafika i interfejs uzytkownika"); all drawing helpers accept
//! the colour as an argument so swapping in raster art later stays easy.
//!
//! The scene is rasterised in software into CPU colour/depth buffers
//! ([`crate::depth`]) uploaded each frame as a texture; UI text is drawn
//! on top with macroquad's built-in font (see [`crate::app`]).

use std::collections::HashMap;

use crate::board::{Board, ObstacleKind};
use crate::camera::Camera;
use crate::constants::{self, TurretKind, VehicleKind};
use crate::depth::{DepthBuffer, DepthCamera, ProjectedPoint};
use crate::entities::{Building, BuildingKind, Vehicle, turret_kind_of};
use crate::game::Game;
use crate::hexgrid::{self, Tile};

/// Draws a whole game state into a software framebuffer.
pub struct Renderer {
    /// Waste heat of the helicopter rotor animation.
    pub rotor_phase: f64,
    terrain_key: Option<Vec<u8>>,
    terrain_color: Option<Vec<u8>>,
    terrain_depth: Option<Vec<f64>>,
    terrain_size: (usize, usize),
}

impl Renderer {
    /// Create a renderer with an empty terrain cache.
    pub fn new() -> Self {
        Self {
            rotor_phase: 0.0,
            terrain_key: None,
            terrain_color: None,
            terrain_depth: None,
            terrain_size: (0, 0),
        }
    }
    /// Render one frame of the running game into `buf`.
    ///
    /// Pass order (specification.md, graphics): ground (tiles, skirts,
    /// ramps, bridges) -> objects (buildings, obstacles, vehicles) ->
    /// shadows -> ranges (fills first, then outlines) -> paths ->
    /// projectiles. Unit badges and floating texts are drawn by
    /// [`crate::app`] on top, never occluded.
    pub fn draw_world(
        &mut self,
        buf: &mut DepthBuffer,
        game: &Game,
        camera: &Camera,
        selection: Option<Tile>,
        hover_tile: Option<Tile>,
    ) {
        self.rotor_phase += 0.2;
        buf.clear(constants::WATER_COLOR);
        let dc = DepthCamera::new(camera.clone());
        // Terrain cache: reuse the static ground when nothing changed.
        let key = self.terrain_key_for(game, camera, buf.w, buf.h);
        let use_cache = self.terrain_key.as_ref() == Some(&key)
            && self.terrain_size == (buf.w, buf.h)
            && self.terrain_color.as_ref().map(|c| c.len()) == Some(buf.color.len());
        if use_cache {
            if let (Some(c), Some(d)) = (self.terrain_color.clone(), self.terrain_depth.clone()) {
                buf.color.copy_from_slice(&c);
                buf.depth.copy_from_slice(&d);
            }
        } else {
            self.draw_tiles(game, &dc, buf, camera);
            self.terrain_color = Some(buf.color.clone());
            self.terrain_depth = Some(buf.depth.clone());
            self.terrain_key = Some(key);
            self.terrain_size = (buf.w, buf.h);
        }
        self.draw_objects(game, &dc, buf, camera, selection, hover_tile);
        self.draw_shadows(game, &dc, buf);
        self.draw_ranges(game, &dc, buf);
        self.draw_paths(game, &dc, buf, selection);
        self.draw_projectiles(game, &dc, buf);
    }
    /// Invalidate the static terrain cache.
    pub fn invalidate(&mut self) {
        self.terrain_key = None;
    }
}
// ---------------------------------------------------------------------------
// Terrain
// ---------------------------------------------------------------------------
impl Renderer {
    fn terrain_key_for(&self, game: &Game, camera: &Camera, w: usize, h: usize) -> Vec<u8> {
        // Camera (exact bits: even subpixel moves invalidate), window size,
        // board heights, ramps and bridges.
        let mut key = Vec::new();
        for v in [camera.x, camera.y, camera.zoom] {
            key.extend_from_slice(&v.to_le_bytes());
        }
        key.extend_from_slice(&(w as u64).to_le_bytes());
        key.extend_from_slice(&(h as u64).to_le_bytes());
        key.extend_from_slice(&(game.board.cols as i64).to_le_bytes());
        key.extend_from_slice(&(game.board.rows as i64).to_le_bytes());
        for r in 0..game.board.rows {
            for q in 0..game.board.cols {
                key.push(game.board.height((q, r)) as u8);
            }
        }
        type RampEntry = (Tile, (Tile, Tile));
        let mut ramps: Vec<RampEntry> = game.board.ramps.iter().map(|(k, v)| (*k, *v)).collect();
        ramps.sort();
        for (t, (a, b)) in ramps {
            for v in [t.0, t.1, a.0, a.1, b.0, b.1] {
                key.extend_from_slice(&v.to_le_bytes());
            }
        }
        for br in game.board.bridges.iter() {
            for v in [br.a.0, br.a.1, br.b.0, br.b.1, br.w, br.direction as i32] {
                key.extend_from_slice(&v.to_le_bytes());
            }
            for f in br.fragments.iter() {
                key.extend_from_slice(&f.0.to_le_bytes());
                key.extend_from_slice(&f.1.to_le_bytes());
            }
        }
        let _ = self;
        key
    }
    fn tile_top_z(&self, game: &Game, tile: Tile) -> f64 {
        // Ramp tiles render at the height of their lower end.
        if let Some((a, b)) = game.board.ramps.get(&tile) {
            let h = game.board.height(*a).min(game.board.height(*b));
            return h as f64 * constants::ELEVATION_PX;
        }
        game.board.height(tile) as f64 * constants::ELEVATION_PX
    }
    fn draw_tiles(&self, game: &Game, dc: &DepthCamera, buf: &mut DepthBuffer, camera: &Camera) {
        // Back-to-front: sort by (x + y) of the tile centre.
        let mut tiles: Vec<Tile> = game.board.tiles.keys().copied().collect();
        tiles.sort_by(|a, b| {
            let ca = game.board.center_world(*a);
            let cb = game.board.center_world(*b);
            (ca.0 + ca.1).partial_cmp(&(cb.0 + cb.1)).unwrap()
        });
        for t in tiles {
            self.draw_skirts(game, dc, buf, camera, t);
        }
        for t in game.board.tiles.keys().copied().collect::<Vec<_>>() {
            self.draw_tile_top(game, dc, buf, t);
        }
        // Ramps and bridges above the ground.
        let mut ramps: Vec<Tile> = game.board.ramps.keys().copied().collect();
        ramps.sort_by(|a, b| {
            let ca = game.board.center_world(*a);
            let cb = game.board.center_world(*b);
            (ca.0 + ca.1).partial_cmp(&(cb.0 + cb.1)).unwrap()
        });
        for t in ramps {
            self.draw_ramp(game, dc, buf, t);
        }
        let mut bridges: Vec<usize> = (0..game.board.bridges.len()).collect();
        bridges.sort_by(|a, b| {
            let ca = game.board.center_world(game.board.bridges[*a].a);
            let cb = game.board.center_world(game.board.bridges[*b].a);
            (ca.0 + ca.1).partial_cmp(&(cb.0 + cb.1)).unwrap()
        });
        for i in bridges {
            self.draw_bridge(game, dc, buf, i);
        }
    }
    fn draw_tile_top(&self, game: &Game, dc: &DepthCamera, buf: &mut DepthBuffer, tile: Tile) {
        let h = game.board.height(tile);
        // Open water outside tiles is the clear colour; water tiles get a top.
        let base = if h == 0 {
            constants::WATER_COLOR
        } else if (tile.0 + tile.1) & 1 == 0 {
            constants::LAND_COLOR
        } else {
            constants::LAND_VARIANT
        };
        let z = self.tile_top_z(game, tile);
        let corners = hexgrid::hex_corners(tile.0, tile.1, game.board.side);
        let pts: Vec<ProjectedPoint> = corners
            .iter()
            .map(|(x, y)| dc.world_to_screen(*x, *y, z))
            .collect();
        // Cull: skip fully off-screen tops.
        let (w, hh) = (buf.w as i32, buf.h as i32);
        if pts
            .iter()
            .all(|p| p.sx < -50 || p.sy < -50 || p.sx > w + 50 || p.sy > hh + 50)
        {
            return;
        }
        buf.polygon(base, &pts, 0);
        // Grid stroke sampled on the supporting plane (never cut by its own top).
        for i in 0..6 {
            let a = &pts[i];
            let b = &pts[(i + 1) % 6];
            let edge = if h == 0 {
                constants::WATER_EDGE
            } else {
                constants::LAND_EDGE
            };
            buf.line(edge, a, b, constants::GRID_LINE_WIDTH as i32);
        }
    }
    fn draw_skirts(
        &self,
        game: &Game,
        dc: &DepthCamera,
        buf: &mut DepthBuffer,
        camera: &Camera,
        tile: Tile,
    ) {
        let board = &game.board;
        let h = board.height(tile);
        if h <= 0 {
            return;
        }
        let z_top = h as f64 * constants::ELEVATION_PX;
        let corners = hexgrid::hex_corners(tile.0, tile.1, board.side);
        for k in 0..6 {
            let dir = hexgrid::edge_dir_index(tile.0, k);
            let n = hexgrid::neighbor(tile.0, tile.1, dir);
            let nh = board.height(n);
            if nh >= h {
                continue;
            }
            let z_bot = nh as f64 * constants::ELEVATION_PX;
            let c0 = corners[k];
            let c1 = corners[(k + 1) % 6];
            // Cull skirts fully off-screen (with full wall height).
            let p0 = dc.world_to_screen(c0.0, c0.1, z_top);
            let p1 = dc.world_to_screen(c1.0, c1.1, z_top);
            let p2 = dc.world_to_screen(c1.0, c1.1, z_bot);
            let p3 = dc.world_to_screen(c0.0, c0.1, z_bot);
            let (w, hh) = (buf.w as i32, buf.h as i32);
            let xs = [p0.sx, p1.sx, p2.sx, p3.sx];
            let ys = [p0.sy, p1.sy, p2.sy, p3.sy];
            let xmax = xs.iter().max().copied().unwrap_or(0);
            let xmin = xs.iter().min().copied().unwrap_or(0);
            let ymax = ys.iter().max().copied().unwrap_or(0);
            let ymin = ys.iter().min().copied().unwrap_or(0);
            if xmax < 0 || xmin > w || ymax < 0 || ymin > hh {
                continue;
            }
            let _ = camera;
            let shade = 0.82;
            let base = if (tile.0 + tile.1) & 1 == 0 {
                constants::LAND_COLOR
            } else {
                constants::LAND_VARIANT
            };
            let col = constants::shade(base, shade);
            buf.polygon(col, &[p0, p1, p2, p3], 0);
        }
    }
    fn draw_ramp(&self, game: &Game, dc: &DepthCamera, buf: &mut DepthBuffer, tile: Tile) {
        // Narrow soil band through the tile centre along the ramp axis.
        let (a, b) = match game.board.ramps.get(&tile) {
            Some(v) => *v,
            None => return,
        };
        // Axis: direction a -> tile.
        let mut axis = 0usize;
        for d in 0..6 {
            if hexgrid::neighbor(a.0, a.1, d) == tile {
                axis = d % 3;
                break;
            }
        }
        let ha = game.board.height(a) as f64 * constants::ELEVATION_PX;
        let hb = game.board.height(b) as f64 * constants::ELEVATION_PX;
        let corners = hexgrid::hex_corners(tile.0, tile.1, game.board.side);
        // Short edges lie on the mid-edge points facing a and b.
        let ea = hexgrid::neighbor(tile.0, tile.1, axis);
        let eb = hexgrid::neighbor(tile.0, tile.1, (axis + 3) % 6);
        let _ = (ea, eb);
        // Edge midpoints of the hex facing a and b: average of the two
        // corners adjacent to that edge.
        let mid = |dir: usize| -> (f64, f64) {
            // Edge facing `dir` is edge `dir` (see edge_dir_index).
            let k = dir % 6;
            let c0 = corners[k];
            let c1 = corners[(k + 1) % 6];
            ((c0.0 + c1.0) / 2.0, (c0.1 + c1.1) / 2.0)
        };
        let ma = mid(axis);
        let mb = mid((axis + 3) % 6);
        // Narrow the band: interpolate towards the centre.
        let narrow = 0.35;
        let (cx, cy) = game.board.center_world(tile);
        let pa = (cx + (ma.0 - cx) * narrow, cy + (ma.1 - cy) * narrow);
        let pb = (cx + (mb.0 - cx) * narrow, cy + (mb.1 - cy) * narrow);
        // Widen along the axis direction: offset perpendicular.
        let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (nx, ny) = (-dy / len, dx / len);
        let half = game.board.side * 0.35;
        let q0 = (pa.0 + nx * half, pa.1 + ny * half);
        let q1 = (pa.0 - nx * half, pa.1 - ny * half);
        let q2 = (pb.0 - nx * half, pb.1 - ny * half);
        let q3 = (pb.0 + nx * half, pb.1 + ny * half);
        // Top surface: heights of the respective neighbours.
        let p0 = dc.world_to_screen(q0.0, q0.1, ha);
        let p1 = dc.world_to_screen(q1.0, q1.1, ha);
        let p2 = dc.world_to_screen(q2.0, q2.1, hb);
        let p3 = dc.world_to_screen(q3.0, q3.1, hb);
        buf.polygon([168, 150, 110], &[p0, p1, p2, p3], 0);
    }
    fn draw_bridge(&self, game: &Game, dc: &DepthCamera, buf: &mut DepthBuffer, idx: usize) {
        let br = match game.board.bridges.get(idx) {
            Some(b) => b,
            None => return,
        };
        let z = br.w as f64 * constants::ELEVATION_PX + constants::BRIDGE_DECK_LIFT;
        // Deck: chain of hex tops at deck height.
        for f in br.fragments.iter() {
            let corners = hexgrid::hex_corners(f.0, f.1, game.board.side);
            let pts: Vec<ProjectedPoint> = corners
                .iter()
                .map(|(x, y)| dc.world_to_screen(*x, *y, z))
                .collect();
            buf.polygon([150, 120, 90], &pts, 0);
            for i in 0..6 {
                buf.line([100, 80, 60], &pts[i], &pts[(i + 1) % 6], 1);
            }
        }
    }
}
// ---------------------------------------------------------------------------
// Objects
// ---------------------------------------------------------------------------
impl Renderer {
    pub(crate) fn vehicle_z(&self, game: &Game, v: &Vehicle) -> f64 {
        // Ground vehicles rest on the tile top (or ramp/bridge deck);
        // helicopters hover one elevation step above the ground below.
        let ground = if let Some(deck) = game.bridge_deck_height_at(
            v.x,
            v.y,
            if v.route_index > 0 {
                v.route.get(v.route_index - 1).copied()
            } else {
                None
            },
            v.route.get(v.route_index).copied(),
            v.src_tile,
        ) {
            deck as f64 * constants::ELEVATION_PX + constants::BRIDGE_DECK_LIFT
        } else if let Some(t) = game.board.world_to_tile(v.x, v.y) {
            if let Some((a, b)) = game.board.ramps.get(&t) {
                game.board.height(*a).min(game.board.height(*b)) as f64 * constants::ELEVATION_PX
            } else {
                game.board.height(t) as f64 * constants::ELEVATION_PX
            }
        } else {
            0.0
        };
        if v.kind == VehicleKind::Helicopter {
            ground + constants::ELEVATION_PX
        } else {
            ground
        }
    }
    fn draw_objects(
        &mut self,
        game: &Game,
        dc: &DepthCamera,
        buf: &mut DepthBuffer,
        _camera: &Camera,
        selection: Option<Tile>,
        hover_tile: Option<Tile>,
    ) {
        // Buildings ordered back-to-front.
        let mut order: Vec<usize> = (0..game.buildings.len()).collect();
        order.sort_by(|a, b| {
            let pa = game.buildings[*a].pos(game.board.side);
            let pb = game.buildings[*b].pos(game.board.side);
            (pa.0 + pa.1).partial_cmp(&(pb.0 + pb.1)).unwrap()
        });
        for i in order {
            let selected = Some(game.buildings[i].tile) == selection;
            let hovered = Some(game.buildings[i].tile) == hover_tile;
            self.draw_building(game, dc, buf, &game.buildings[i], selected, hovered);
        }
        // Obstacles.
        let mut tiles: Vec<Tile> = game.board.tiles.keys().copied().collect();
        tiles.sort_by(|a, b| {
            let ca = game.board.center_world(*a);
            let cb = game.board.center_world(*b);
            (ca.0 + ca.1).partial_cmp(&(cb.0 + cb.1)).unwrap()
        });
        for t in tiles {
            let has = game
                .board
                .tiles
                .get(&t)
                .and_then(|x| x.obstacle.as_ref())
                .is_some();
            if has {
                self.draw_obstacle(game, dc, buf, t);
            }
        }
        // Vehicles back-to-front.
        let mut vs: Vec<usize> = (0..game.vehicles.len()).collect();
        vs.sort_by(|a, b| {
            let pa = game.vehicles[*a].pos();
            let pb = game.vehicles[*b].pos();
            (pa.0 + pa.1).partial_cmp(&(pb.0 + pb.1)).unwrap()
        });
        for i in vs {
            if !game.vehicles[i].dead {
                self.draw_vehicle(game, dc, buf, &game.vehicles[i]);
            }
        }
    }
    fn building_color(b: &Building) -> [u8; 3] {
        match b.owner {
            Some(id) => constants::player_color(id),
            None => constants::NEUTRAL_COLOR,
        }
    }
    fn draw_building(
        &self,
        game: &Game,
        dc: &DepthCamera,
        buf: &mut DepthBuffer,
        b: &Building,
        selected: bool,
        hovered: bool,
    ) {
        let (cx, cy) = b.pos(game.board.side);
        let z = self.tile_top_z(game, b.tile);
        let color = Self::building_color(b);
        let dark = constants::shade(color, 0.7);
        match b.kind {
            BuildingKind::BaseTank | BuildingKind::BaseBuffer => {
                self.iso_box(dc, buf, cx, cy, z, 26.0, 26.0, 14.0, color);
                // Turret dome / cross on top.
                if b.kind == BuildingKind::BaseBuffer {
                    self.draw_cross(dc, buf, cx, cy, z + 14.0, [130, 235, 140]);
                } else {
                    self.iso_box(dc, buf, cx, cy, z + 14.0, 12.0, 12.0, 6.0, dark);
                }
            }
            BuildingKind::BaseHelicopter => {
                // Landing pad: flat disc + H mark.
                let pts = dc.screen_circle_poly(cx, cy, 20.0, z, 20);
                buf.polygon(dark, &pts, 0);
                let pts2 = dc.screen_circle_poly(cx, cy, 15.0, z + 1.0, 20);
                buf.polygon(color, &pts2, 0);
            }
            BuildingKind::BaseHovercraft => {
                self.iso_box(dc, buf, cx, cy, z, 30.0, 20.0, 10.0, color);
            }
            BuildingKind::TurretNormal | BuildingKind::TurretRapid | BuildingKind::TurretRocket => {
                // Round base + barrel towards the last target.
                let pts = dc.screen_circle_poly(cx, cy, 14.0, z, 16);
                buf.polygon(dark, &pts, 0);
                let pts2 = dc.screen_circle_poly(cx, cy, 10.0, z + 8.0, 14);
                buf.polygon(color, &pts2, 0);
                let (dx, dy) = match b.last_target_pos {
                    Some((tx, ty)) => {
                        let d = ((tx - cx).powi(2) + (ty - cy).powi(2)).sqrt().max(1e-6);
                        ((tx - cx) / d, (ty - cy) / d)
                    }
                    None => (1.0, 0.0),
                };
                let tk = turret_kind_of(b.kind).unwrap_or(TurretKind::Normal);
                let len = if tk == TurretKind::Rapid { 10.0 } else { 18.0 };
                let base = dc.world_to_screen(cx, cy, z + 12.0);
                let tip = dc.world_to_screen(cx + dx * len, cy + dy * len, z + 14.0);
                buf.line(dark, &base, &tip, 3);
            }
            BuildingKind::HealTower => {
                self.iso_box(dc, buf, cx, cy, z, 16.0, 16.0, 26.0, color);
                self.draw_cross(dc, buf, cx, cy, z + 26.0, [200, 255, 200]);
            }
        }
        if selected || hovered {
            let pts = dc.screen_circle_poly(cx, cy, 30.0, z, 24);
            let col = if selected {
                [255, 240, 120]
            } else {
                [255, 255, 255]
            };
            for i in 0..pts.len() {
                let a = &pts[i];
                let bb = &pts[(i + 1) % pts.len()];
                buf.line(col, a, bb, 2);
            }
        }
    }
    fn draw_obstacle(&self, game: &Game, dc: &DepthCamera, buf: &mut DepthBuffer, tile: Tile) {
        let t = match game.board.tiles.get(&tile) {
            Some(t) => t,
            None => return,
        };
        let o = match t.obstacle.as_ref() {
            Some(o) => o,
            None => return,
        };
        let (cx, cy) = game.board.center_world(tile);
        let z = t.height as f64 * constants::ELEVATION_PX;
        match o.kind {
            ObstacleKind::Wall => {
                self.iso_box(dc, buf, cx, cy, z, 30.0, 26.0, 18.0, [120, 100, 80]);
            }
            ObstacleKind::Mine | ObstacleKind::MineWater => {
                let pts = dc.screen_circle_poly(cx, cy, 8.0, z, 10);
                buf.polygon([40, 40, 40], &pts, 0);
            }
            ObstacleKind::TrapFire => {
                let pts = dc.screen_circle_poly(cx, cy, 12.0, z, 12);
                buf.polygon([230, 120, 60], &pts, 0);
            }
            ObstacleKind::TrapIce => {
                let pts = dc.screen_circle_poly(cx, cy, 12.0, z, 12);
                buf.polygon([150, 210, 250], &pts, 0);
            }
        }
    }
    fn draw_vehicle(&self, game: &Game, dc: &DepthCamera, buf: &mut DepthBuffer, v: &Vehicle) {
        let color = constants::player_color(v.owner);
        let z = self.vehicle_z(game, v);
        let (x, y) = (v.x, v.y);
        match v.kind {
            VehicleKind::Tank => {
                self.iso_box(dc, buf, x, y, z, 16.0, 22.0, 9.0, color);
                self.iso_box(
                    dc,
                    buf,
                    x,
                    y,
                    z + 9.0,
                    10.0,
                    10.0,
                    5.0,
                    constants::shade(color, 0.7),
                );
            }
            VehicleKind::Helicopter => {
                let pts = dc.screen_circle_poly(x, y, 10.0, z, 12);
                buf.polygon(color, &pts, 0);
                let tail = dc.world_to_screen(x - 16.0, y, z);
                let body = dc.world_to_screen(x, y, z);
                buf.line(constants::shade(color, 0.7), &body, &tail, 3);
                let ang = self.rotor_phase;
                let r = 22.0;
                let p1 = dc.world_to_screen(x + r * ang.cos(), y + r * ang.sin(), z + 5.0);
                let p2 = dc.world_to_screen(x - r * ang.cos(), y - r * ang.sin(), z + 5.0);
                buf.line([210, 210, 210], &p1, &p2, 2);
            }
            VehicleKind::Hovercraft => {
                let pts = dc.screen_circle_poly(x, y, 13.0, z, 14);
                buf.polygon(color, &pts, 0);
                let inner = dc.screen_circle_poly(x, y, 7.0, z + 4.0, 12);
                buf.polygon(constants::shade(color, 0.7), &inner, 0);
            }
            VehicleKind::Buffer => {
                self.iso_box(dc, buf, x, y, z, 16.0, 20.0, 9.0, color);
                self.draw_cross(dc, buf, x, y, z + 9.0, [130, 235, 140]);
            }
        }
    }
    fn draw_cross(
        &self,
        dc: &DepthCamera,
        buf: &mut DepthBuffer,
        x: f64,
        y: f64,
        z: f64,
        color: [u8; 3],
    ) {
        let h = 6.0;
        let p1 = dc.world_to_screen(x - h, y, z);
        let p2 = dc.world_to_screen(x + h, y, z);
        let p3 = dc.world_to_screen(x, y - h, z);
        let p4 = dc.world_to_screen(x, y + h, z);
        buf.line(color, &p1, &p2, 3);
        buf.line(color, &p3, &p4, 3);
    }
    /// Axis-aligned isometric box centred at `(x, y, z)` with footprint
    /// `(sx, sy)`, height `sz` and base `color`.
    #[allow(clippy::too_many_arguments)]
    fn iso_box(
        &self,
        dc: &DepthCamera,
        buf: &mut DepthBuffer,
        x: f64,
        y: f64,
        z: f64,
        sx: f64,
        sy: f64,
        sz: f64,
        color: [u8; 3],
    ) {
        // 8 corners.
        let c000 = dc.world_to_screen(x - sx / 2.0, y - sy / 2.0, z);
        let c100 = dc.world_to_screen(x + sx / 2.0, y - sy / 2.0, z);
        let c110 = dc.world_to_screen(x + sx / 2.0, y + sy / 2.0, z);
        let c010 = dc.world_to_screen(x - sx / 2.0, y + sy / 2.0, z);
        let c001 = dc.world_to_screen(x - sx / 2.0, y - sy / 2.0, z + sz);
        let c101 = dc.world_to_screen(x + sx / 2.0, y - sy / 2.0, z + sz);
        let c111 = dc.world_to_screen(x + sx / 2.0, y + sy / 2.0, z + sz);
        let c011 = dc.world_to_screen(x - sx / 2.0, y + sy / 2.0, z + sz);
        // Visible faces: top + two sides facing the viewer.
        buf.polygon(color, &[c001, c101, c111, c011], 0);
        buf.polygon(constants::shade(color, 0.85), &[c010, c110, c111, c011], 0);
        buf.polygon(constants::shade(color, 0.7), &[c100, c110, c111, c101], 0);
        let _ = c000;
    }
}
// ---------------------------------------------------------------------------
// Shadows, ranges, paths, projectiles
// ---------------------------------------------------------------------------
impl Renderer {
    fn draw_shadows(&self, game: &Game, dc: &DepthCamera, buf: &mut DepthBuffer) {
        // Transparent decals on the receiving surface (same plane test).
        for v in game.vehicles.iter() {
            if v.dead {
                continue;
            }
            // Ground point under the vehicle.
            let z = if let Some(deck) = game.bridge_deck_height_at(
                v.x,
                v.y,
                if v.route_index > 0 {
                    v.route.get(v.route_index - 1).copied()
                } else {
                    None
                },
                v.route.get(v.route_index).copied(),
                v.src_tile,
            ) {
                deck as f64 * constants::ELEVATION_PX + constants::BRIDGE_DECK_LIFT
            } else if let Some(t) = game.board.world_to_tile(v.x, v.y) {
                game.board.height(t) as f64 * constants::ELEVATION_PX
            } else {
                0.0
            };
            // Shadow only for flying vehicles (helicopters hover above).
            if v.kind != VehicleKind::Helicopter {
                continue;
            }
            let pts = dc.screen_circle_poly(
                v.x,
                v.y,
                constants::SHADOW_RADIUS,
                z,
                constants::SHADOW_SEGMENTS,
            );
            buf.decal([0, 0, 0, 70], &pts);
        }
    }
    fn draw_ranges(&self, game: &Game, dc: &DepthCamera, buf: &mut DepthBuffer) {
        // Fills first, outlines second (specification.md, graphics).
        let mut outlines: Vec<(Vec<ProjectedPoint>, [u8; 4], f64)> = Vec::new();
        for b in game.buildings.iter() {
            if let Some(tk) = turret_kind_of(b.kind) {
                let (cx, cy) = b.pos(game.board.side);
                let z = self.tile_top_z(game, b.tile);
                let r = constants::turret_range(tk);
                let pts = dc.screen_circle_poly(cx, cy, r, z, 48);
                buf.polygon_blend(constants::RANGE_TURRET_FILL, &pts, 0);
                outlines.push((pts, constants::RANGE_TURRET_OUTLINE, z));
            } else if b.kind == BuildingKind::HealTower {
                let (cx, cy) = b.pos(game.board.side);
                let z = self.tile_top_z(game, b.tile);
                let r = b.units * constants::HEAL_TOWER_RANGE_PER_UNIT;
                if r > 1.0 {
                    let pts = dc.screen_circle_poly(cx, cy, r, z, 48);
                    buf.polygon_blend(constants::RANGE_HEAL_FILL, &pts, 0);
                    outlines.push((pts, constants::RANGE_HEAL_OUTLINE, z));
                }
            }
        }
        for v in game.vehicles.iter() {
            if v.dead || v.kind != VehicleKind::Buffer {
                continue;
            }
            let z = self.vehicle_z(game, v);
            // Buffer aura moves with the vehicle; drawn on its plane.
            let pts = dc.screen_circle_poly(
                v.x,
                v.y,
                constants::BUFFER_HEAL_RADIUS,
                z - constants::ELEVATION_PX,
                48,
            );
            buf.polygon_blend(constants::RANGE_HEAL_FILL, &pts, 0);
            outlines.push((pts, constants::RANGE_HEAL_OUTLINE, z));
        }
        for (pts, col, _z) in outlines {
            for i in 0..pts.len() {
                let a = &pts[i];
                let b = &pts[(i + 1) % pts.len()];
                buf.line_blend(col, a, b, constants::RANGE_OUTLINE_WIDTH as i32);
            }
        }
    }
    fn draw_paths(
        &self,
        game: &Game,
        dc: &DepthCamera,
        buf: &mut DepthBuffer,
        selection: Option<Tile>,
    ) {
        // Vehicle routes: dashed line fading behind the vehicle.
        for v in game.vehicles.iter() {
            if v.dead || v.route.is_empty() {
                continue;
            }
            let z = self.vehicle_z(game, v);
            let mut pts: Vec<ProjectedPoint> = Vec::new();
            pts.push(dc.world_to_screen(v.x, v.y, z));
            for t in v.route.iter().skip(v.route_index) {
                let (wx, wy) = game.board.center_world(*t);
                pts.push(dc.world_to_screen(wx, wy, z));
            }
            dashed_line(buf, &pts, [255, 255, 255], 2);
        }
        // Selection preview.
        if let Some(src) = selection
            && let Some(b) = game.building_at_tile(src)
        {
            let kind = crate::entities::vehicle_kind_of(b.kind);
            // Preview to hovered building is drawn by app via preview path.
            let _ = kind;
        }
    }
    /// Route preview for the selected source to `dst`.
    pub fn draw_preview(
        &self,
        game: &Game,
        dc: &DepthCamera,
        buf: &mut DepthBuffer,
        path: &[Tile],
    ) {
        if path.is_empty() {
            return;
        }
        let z = 20.0;
        let mut pts: Vec<ProjectedPoint> = Vec::new();
        for t in path.iter() {
            let (wx, wy) = game.board.center_world(*t);
            pts.push(dc.world_to_screen(wx, wy, z));
        }
        dashed_line(buf, &pts, constants::PATH_PREVIEW_COLOR, 2);
    }
    fn draw_projectiles(&self, game: &Game, dc: &DepthCamera, buf: &mut DepthBuffer) {
        for p in game.projectiles.iter() {
            let t = (p.t / p.dur).clamp(0.0, 1.0);
            let x = p.from_pos.0 + (p.to.0 - p.from_pos.0) * t;
            let y = p.from_pos.1 + (p.to.1 - p.from_pos.1) * t;
            // Arc height: peak in the middle.
            let arc = 40.0 * (1.0 - (2.0 * t - 1.0).powi(2));
            let z = 30.0 + arc;
            let c = dc.world_to_screen(x, y, z);
            let r = if p.kind == TurretKind::Rocket {
                constants::ROCKET_RADIUS
            } else {
                constants::PROJECTILE_RADIUS
            };
            // Dark outline 1 px, then fill.
            buf.disc([25, 25, 30], &c, r + 1.0);
            let col = if p.kind == TurretKind::Rocket {
                [255, 120, 60]
            } else {
                [250, 250, 250]
            };
            buf.disc(col, &c, r);
        }
    }
    #[allow(dead_code)]
    /// Depth-aware picking shared with the editor contract: tile under the
    /// cursor, refined against terrain elevation (specification.md).
    ///
    /// Kept next to the renderer for the picking tests; delegates to the
    /// single board implementation in
    /// [`Board::pick_tile`](crate::board::Board::pick_tile).
    pub fn pick_tile(
        &self,
        game: &Game,
        camera: &Camera,
        sx: f32,
        sy: f32,
        flat: bool,
    ) -> Option<Tile> {
        let _ = self;
        game.board.pick_tile(camera, sx, sy, flat)
    }
    /// Nearest building tile within HOVER_SNAP_RADIUS of the cursor.
    ///
    /// Shared helper kept next to the renderer: delegates to the board
    /// geometry in [`Board::snap_to_building`](crate::board::Board::snap_to_building),
    /// so the game input and the picking tests use exactly one code path.
    #[allow(dead_code)]
    pub fn snap_to_building(
        &self,
        game: &Game,
        camera: &Camera,
        sx: f32,
        sy: f32,
        flat: bool,
    ) -> Option<Tile> {
        game.board
            .snap_to_building(camera, sx, sy, &game.buildings, flat)
    }
}

fn dashed_line(buf: &mut DepthBuffer, pts: &[ProjectedPoint], color: [u8; 3], width: i32) {
    // 8 px dash, 6 px gap along the polyline.
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let len = ((b.projected.0 - a.projected.0).powi(2)
            + (b.projected.1 - a.projected.1).powi(2))
        .sqrt();
        let steps = (len / 4.0).ceil().max(1.0) as usize;
        for i in 0..steps {
            let t0 = i as f64 / steps as f64;
            let t1 = (i + 1) as f64 / steps as f64;
            let seg_len = len / steps as f64;
            // Dash pattern in screen space.
            let s0 = acc + t0 * len;
            let s1 = acc + t1 * len;
            let _ = seg_len;
            // Sample sub-segment midpoint.
            let mid = (s0 + s1) / 2.0;
            if (mid % 14.0) < 8.0 {
                let ax = a.projected.0 + (b.projected.0 - a.projected.0) * t0;
                let ay = a.projected.1 + (b.projected.1 - a.projected.1) * t0;
                let bx = a.projected.0 + (b.projected.0 - a.projected.0) * t1;
                let by = a.projected.1 + (b.projected.1 - a.projected.1) * t1;
                let pa = ProjectedPoint {
                    sx: ax.round() as i32,
                    sy: ay.round() as i32,
                    projected: (ax, ay),
                    depth: a.depth + (b.depth - a.depth) * t0,
                    ground_plane: a.ground_plane,
                };
                let pb = ProjectedPoint {
                    sx: bx.round() as i32,
                    sy: by.round() as i32,
                    projected: (bx, by),
                    depth: a.depth + (b.depth - a.depth) * t1,
                    ground_plane: b.ground_plane,
                };
                buf.line(color, &pa, &pb, width);
            }
        }
        acc += len;
    }
}

#[allow(dead_code)]
fn point_in_poly(sx: f32, sy: f32, pts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = pts.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if ((yi > sy) != (yj > sy)) && (sx < (xj - xi) * (sy - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[allow(dead_code)]
fn _use_board(_b: &Board) {}
#[allow(dead_code)]
fn _use_hashmap(_m: &HashMap<Tile, usize>) {}
