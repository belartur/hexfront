//! GPU mesh builders for the isometric scene (no macroquad dependency).
//!
//! [`TerrainMesh`] holds the static world geometry (hex tops, cliff skirts,
//! ramps, bridge decks) as flat-shaded triangles; the dynamic objects are
//! assembled per frame into a [`DynamicMesh`] with the same vertex layout.
//! Vertices carry world `(x, y, z)` positions plus an RGB colour; the GPU
//! camera ([`crate::iso`]) projects them and the hardware depth buffer
//! resolves occlusion, so no per-pixel work happens on the CPU anymore.

use crate::board::Board;
use crate::constants;
use crate::game::Game;
use crate::hexgrid::{self, Tile};

/// One flat-shaded GPU vertex: world position plus RGB colour.
#[derive(Clone, Copy, Debug)]
pub struct GpuVertex {
    /// World x in distance units (j).
    pub x: f32,
    /// World y in distance units (j).
    pub y: f32,
    /// Rendered elevation in px.
    pub z: f32,
    /// RGB colour bytes.
    pub color: [u8; 3],
}

/// One translucent range vertex: world position plus RGBA colour.
///
/// Range fills (and only they) carry their own alpha, so white turret
/// fills and light-green heal fills keep the distinct transparencies
/// from the Python version instead of sharing one value.
#[derive(Clone, Copy, Debug)]
pub struct RangeVertex {
    /// World x in distance units (j).
    pub x: f32,
    /// World y in distance units (j).
    pub y: f32,
    /// Rendered elevation in px.
    pub z: f32,
    /// RGBA colour bytes.
    pub color: [u8; 4],
}

/// One 3D line endpoint: world position plus RGBA colour.
///
/// Range outlines share the fill hue but are clearly less transparent;
/// storing the alpha per line lets the two passes use one buffer.
#[derive(Clone, Copy, Debug)]
pub struct LineVertex {
    /// World x in distance units (j).
    pub x: f32,
    /// World y in distance units (j).
    pub y: f32,
    /// Rendered elevation in px.
    pub z: f32,
    /// RGBA colour bytes.
    pub color: [u8; 4],
}

/// Triangle soup with per-vertex RGBA colours for one range kind.
#[derive(Clone, Debug, Default)]
pub struct RangeSoup {
    /// All vertices, three per triangle.
    pub vertices: Vec<RangeVertex>,
    /// Indices into `vertices` (always `0..len` for a soup).
    pub indices: Vec<u16>,
}

/// Triangle soup with per-vertex colours (flat shading = 3 equal colours).
#[derive(Clone, Debug, Default)]
pub struct TriangleSoup {
    /// All vertices, three per triangle.
    pub vertices: Vec<GpuVertex>,
    /// Indices into `vertices` (always `0..len` for a soup).
    pub indices: Vec<u16>,
}

/// Maximum vertices per GPU chunk: macroquad batches draw calls into
/// `u16` index buffers, so large terrains are split into chunks. Keep the
/// chunk at or below the `draw_call_*_capacity` raised in `window_conf`,
/// so one chunk is one draw call.
pub const CHUNK_VERTICES: usize = 16_000;

/// Tiles per chunk edge: the terrain is split into spatial chunks so view
/// culling can skip whole regions (16 x 16 tiles stays well below
/// [`CHUNK_VERTICES`] per chunk).
pub const CHUNK_TILES: i32 = 16;

/// One spatially bounded terrain chunk; the renderer culls it by `bbox`.
#[derive(Clone, Debug, Default)]
pub struct TerrainChunk {
    /// Opaque triangles (tops, skirts, ramps, decks).
    pub soup: TriangleSoup,
    /// Thin grid strokes over this chunk's hex tops.
    pub grid_lines: Vec<(GpuVertex, GpuVertex)>,
    /// World-space bounds `(x_min, y_min, x_max, y_max)` of the chunk,
    /// padded by one hex so cliffs and strokes stay covered.
    pub bbox: (f64, f64, f64, f64),
}

/// Static terrain geometry, rebuilt only when the board changes.
#[derive(Clone, Debug, Default)]
pub struct TerrainMesh {
    /// Spatial chunks in deterministic order.
    pub chunks: Vec<TerrainChunk>,
}

/// World-space depth span of a board (for the GPU camera setup).
pub fn depth_span(board: &Board) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for tile in board.tiles.keys() {
        let (cx, cy) = board.center_world(*tile);
        let z = board.height(*tile) as f64 * constants::ELEVATION_PX;
        let d = (cx + cy) * constants::ISO_SIN + z;
        lo = lo.min(d);
        hi = hi.max(d);
    }
    if lo > hi { (0.0, 1.0) } else { (lo, hi) }
}

/// Highest rendered elevation of a board in px (view-culling padding).
pub fn max_height(board: &Board) -> f64 {
    let h = board.tiles.values().map(|t| t.height).max().unwrap_or(0);
    h as f64 * constants::ELEVATION_PX
}

/// World box `(x_min, y_min, x_max, y_max)` visible in `camera`.
///
/// The four screen corners are unprojected at zero elevation and at
/// `max_z`, so terrain of any height (and the cliffs hanging below it)
/// stays inside the box; the result is padded by two hexes for strokes.
pub fn visible_world_bounds(
    camera: &crate::camera::Camera,
    max_z: f64,
    side: f64,
) -> (f64, f64, f64, f64) {
    let (w, h) = camera.screen_size;
    let (mut x0, mut y0) = (f64::INFINITY, f64::INFINITY);
    let (mut x1, mut y1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for wz in [0.0, max_z] {
        for (sx, sy) in [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)] {
            let (wx, wy) = camera.screen_to_world(sx, sy, wz);
            x0 = x0.min(wx);
            y0 = y0.min(wy);
            x1 = x1.max(wx);
            y1 = y1.max(wy);
        }
    }
    let pad = 2.0 * side;
    (x0 - pad, y0 - pad, x1 + pad, y1 + pad)
}

fn grow_bbox(bbox: &mut (f64, f64, f64, f64), x: f64, y: f64) {
    bbox.0 = bbox.0.min(x);
    bbox.1 = bbox.1.min(y);
    bbox.2 = bbox.2.max(x);
    bbox.3 = bbox.3.max(y);
}

fn push_tri(soup: &mut TriangleSoup, a: GpuVertex, b: GpuVertex, c: GpuVertex) {
    let base = soup.vertices.len() as u16;
    soup.vertices.push(a);
    soup.vertices.push(b);
    soup.vertices.push(c);
    soup.indices.push(base);
    soup.indices.push(base + 1);
    soup.indices.push(base + 2);
}

fn push_quad(soup: &mut TriangleSoup, a: GpuVertex, b: GpuVertex, c: GpuVertex, d: GpuVertex) {
    push_tri(soup, a, b, c);
    push_tri(soup, a, c, d);
}

fn vert(x: f64, y: f64, z: f64, color: [u8; 3]) -> GpuVertex {
    GpuVertex {
        x: x as f32,
        y: y as f32,
        z: z as f32,
        color,
    }
}

fn range_vert(x: f64, y: f64, z: f64, color: [u8; 3], alpha: u8) -> RangeVertex {
    RangeVertex {
        x: x as f32,
        y: y as f32,
        z: z as f32,
        color: [color[0], color[1], color[2], alpha],
    }
}

fn line_vert(x: f64, y: f64, z: f64, color: [u8; 3], alpha: u8) -> LineVertex {
    LineVertex {
        x: x as f32,
        y: y as f32,
        z: z as f32,
        color: [color[0], color[1], color[2], alpha],
    }
}

fn push_range_tri(soup: &mut RangeSoup, a: RangeVertex, b: RangeVertex, c: RangeVertex) {
    let base = soup.vertices.len() as u16;
    soup.vertices.push(a);
    soup.vertices.push(b);
    soup.vertices.push(c);
    soup.indices.push(base);
    soup.indices.push(base + 1);
    soup.indices.push(base + 2);
}

/// Flat ground disc with one RGBA colour (range fills keep their own alpha).
#[allow(clippy::too_many_arguments)]
fn push_range_disc(
    mesh: &mut RangeSoup,
    x: f64,
    y: f64,
    z: f64,
    r: f64,
    n: usize,
    color: [u8; 3],
    alpha: u8,
) {
    let center = range_vert(x, y, z, color, alpha);
    let mut prev = range_vert(x + r, y, z, color, alpha);
    for i in 1..=n {
        let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        let next = range_vert(x + r * a.cos(), y + r * a.sin(), z, color, alpha);
        push_range_tri(mesh, center, prev, next);
        prev = next;
    }
}

/// Base tile colour (checkerboard for land, blue for water).
pub fn tile_color(tile: Tile, height: i32) -> [u8; 3] {
    if height == 0 {
        constants::WATER_COLOR
    } else if (tile.0 + tile.1) & 1 == 0 {
        constants::LAND_COLOR
    } else {
        constants::LAND_VARIANT
    }
}

/// Height rendered for a tile top (ramps sit at the lower end).
pub fn tile_top_z(board: &Board, tile: Tile) -> f64 {
    if let Some((a, b)) = board.ramps.get(&tile) {
        return board.height(*a).min(board.height(*b)) as f64 * constants::ELEVATION_PX;
    }
    board.height(tile) as f64 * constants::ELEVATION_PX
}

/// Build the static terrain mesh of `board` (tops, skirts, ramps, decks).
pub fn build_terrain(board: &Board) -> TerrainMesh {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(i32, i32), Vec<Tile>> = BTreeMap::new();
    for tile in board.tiles.keys() {
        groups
            .entry((tile.0 / CHUNK_TILES, tile.1 / CHUNK_TILES))
            .or_default()
            .push(*tile);
    }
    let mut mesh = TerrainMesh::default();
    let pad = board.side;
    for tiles in groups.values_mut() {
        tiles.sort();
        let mut chunk = TerrainChunk::default();
        for tile in tiles.iter() {
            push_top(board, &mut chunk.soup, &mut chunk.grid_lines, *tile);
            push_skirts(board, &mut chunk.soup, *tile);
            if board.ramps.contains_key(tile) {
                push_ramp(board, &mut chunk.soup, *tile);
            }
            let (cx, cy) = board.center_world(*tile);
            grow_bbox(&mut chunk.bbox, cx - pad, cy - pad);
            grow_bbox(&mut chunk.bbox, cx + pad, cy + pad);
        }
        debug_assert!(
            chunk.soup.vertices.len() <= CHUNK_VERTICES,
            "terrain chunk exceeds the u16 draw batch ({} vertices)",
            chunk.soup.vertices.len()
        );
        mesh.chunks.push(chunk);
    }
    // Bridge decks sit on their own tiles, so every fragment lands in the
    // chunk of that tile (a bridge may span more than one chunk).
    let mut bridges: Vec<usize> = (0..board.bridges.len()).collect();
    bridges.sort_by_key(|i| board.bridges[*i].a);
    for i in bridges {
        let w = board.bridges[i].w;
        for f in board.bridges[i].fragments.clone() {
            let key = (f.0 / CHUNK_TILES, f.1 / CHUNK_TILES);
            let Some(idx) = groups.keys().position(|k| *k == key) else {
                continue;
            };
            let z = w as f64 * constants::ELEVATION_PX + constants::BRIDGE_DECK_LIFT;
            let corners = hexgrid::hex_corners(f.0, f.1, board.side);
            let c = corners.map(|(x, y)| vert(x, y, z, [150, 120, 90]));
            let soup = &mut mesh.chunks[idx].soup;
            for k in 1..5 {
                push_tri(soup, c[0], c[k], c[k + 1]);
            }
        }
    }
    mesh
}

fn push_top(
    board: &Board,
    soup: &mut TriangleSoup,
    grid: &mut Vec<(GpuVertex, GpuVertex)>,
    tile: Tile,
) {
    let h = board.height(tile);
    let z = tile_top_z(board, tile);
    let corners = hexgrid::hex_corners(tile.0, tile.1, board.side);
    let fill = tile_color(tile, h);
    let c = corners.map(|(x, y)| vert(x, y, z, fill));
    // Fan around corner 0.
    for k in 1..5 {
        push_tri(soup, c[0], c[k], c[k + 1]);
    }
    let edge = if h == 0 {
        constants::WATER_EDGE
    } else {
        constants::LAND_EDGE
    };
    for k in 0..6 {
        let a = vert(corners[k].0, corners[k].1, z, edge);
        let b = vert(corners[(k + 1) % 6].0, corners[(k + 1) % 6].1, z, edge);
        grid.push((a, b));
    }
}

fn push_skirts(board: &Board, soup: &mut TriangleSoup, tile: Tile) {
    let h = board.height(tile);
    if h <= 0 {
        return;
    }
    let z_top = h as f64 * constants::ELEVATION_PX;
    let corners = hexgrid::hex_corners(tile.0, tile.1, board.side);
    let col = constants::shade(tile_color(tile, h), 0.82);
    for k in 0..6 {
        let dir = hexgrid::edge_dir_index(tile.0, k);
        let n = hexgrid::neighbor(tile.0, tile.1, dir);
        if board.height(n) >= h {
            continue;
        }
        let z_bot = board.height(n) as f64 * constants::ELEVATION_PX;
        let p0 = vert(corners[k].0, corners[k].1, z_top, col);
        let p1 = vert(corners[(k + 1) % 6].0, corners[(k + 1) % 6].1, z_top, col);
        let p2 = vert(corners[(k + 1) % 6].0, corners[(k + 1) % 6].1, z_bot, col);
        let p3 = vert(corners[k].0, corners[k].1, z_bot, col);
        push_quad(soup, p0, p1, p2, p3);
    }
}

fn push_ramp(board: &Board, soup: &mut TriangleSoup, tile: Tile) {
    let (a, b) = match board.ramps.get(&tile) {
        Some(v) => *v,
        None => return,
    };
    // Same frame as the Python renderer (`_ramp_frame`/`_draw_ramp`): the
    // strip runs edge to edge, its short edges lying on the midpoints of
    // the hex edges facing the two joined neighbours, tilted by their
    // height difference.
    let corners = hexgrid::hex_corners(tile.0, tile.1, board.side);
    let mut mids = [(0.0, 0.0); 6];
    for k in 0..6 {
        let c1 = corners[k];
        let c2 = corners[(k + 1) % 6];
        mids[k] = ((c1.0 + c2.0) / 2.0, (c1.1 + c2.1) / 2.0);
    }
    let nearest = |target: Tile| -> usize {
        let (tx, ty) = hexgrid::hex_to_world(target.0, target.1, board.side);
        let mut best = 0;
        let mut best_d = f64::INFINITY;
        for (k, m) in mids.iter().enumerate() {
            let d = (m.0 - tx).powi(2) + (m.1 - ty).powi(2);
            if d < best_d {
                best_d = d;
                best = k;
            }
        }
        best
    };
    let edge_a = mids[nearest(a)];
    let edge_b = mids[nearest(b)];
    let ha = board.height(a) as f64 * constants::ELEVATION_PX;
    let hb = board.height(b) as f64 * constants::ELEVATION_PX;
    let (dx, dy) = (edge_b.0 - edge_a.0, edge_b.1 - edge_a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (ux, uy) = (dx / len, dy / len);
    // Strip half-width like the Python renderer: just under half the hex
    // side, so the strip runs edge to edge without spilling past the hex.
    let hw = board.side * 0.45;
    let (px, py) = (-uy, ux);
    let z_lo = ha.min(hb);
    let a1 = (edge_a.0 + px * hw, edge_a.1 + py * hw);
    let a2 = (edge_a.0 - px * hw, edge_a.1 - py * hw);
    let b1 = (edge_b.0 + px * hw, edge_b.1 + py * hw);
    let b2 = (edge_b.0 - px * hw, edge_b.1 - py * hw);
    let col = [168, 150, 110];
    let skirt = [104, 93, 68];
    // Solid body: both sides filled from the tilted top edges down to the
    // base elevation, so no empty space shows under the ramp.
    push_quad(
        soup,
        vert(a1.0, a1.1, ha, skirt),
        vert(b1.0, b1.1, hb, skirt),
        vert(b1.0, b1.1, z_lo, skirt),
        vert(a1.0, a1.1, z_lo, skirt),
    );
    push_quad(
        soup,
        vert(a2.0, a2.1, ha, skirt),
        vert(b2.0, b2.1, hb, skirt),
        vert(b2.0, b2.1, z_lo, skirt),
        vert(a2.0, a2.1, z_lo, skirt),
    );
    // Tilted rectangular top face, edge midpoint to edge midpoint.
    push_quad(
        soup,
        vert(a1.0, a1.1, ha, col),
        vert(a2.0, a2.1, ha, col),
        vert(b2.0, b2.1, hb, col),
        vert(b1.0, b1.1, hb, col),
    );
}

/// Dynamic per-frame geometry: buildings, obstacles, vehicles, effects.
#[derive(Clone, Debug, Default)]
pub struct DynamicMesh {
    /// Opaque boxes/discs (depth-tested, depth-writing).
    pub opaque: TriangleSoup,
    /// Flat translucent white turret range discs (no depth write).
    pub range_turret: RangeSoup,
    /// Flat translucent light-green heal range discs (no depth write).
    pub range_heal: RangeSoup,
    /// Flat translucent range discs (no depth write, drawn after opaque).
    ///
    /// Kept so older callers keep compiling; new code fills
    /// [`DynamicMesh::range_turret`] and [`DynamicMesh::range_heal`].
    pub translucent: TriangleSoup,
    /// 3D line segments (grid already in terrain; ranges/routes here).
    pub lines: Vec<(LineVertex, LineVertex)>,
}

impl DynamicMesh {
    /// Remove all per-frame geometry before rebuilding the frame.
    pub fn clear(&mut self) {
        self.opaque.vertices.clear();
        self.opaque.indices.clear();
        self.range_turret.vertices.clear();
        self.range_turret.indices.clear();
        self.range_heal.vertices.clear();
        self.range_heal.indices.clear();
        self.translucent.vertices.clear();
        self.translucent.indices.clear();
        self.lines.clear();
    }
}

/// Ground elevation under a vehicle (bridge decks included).
pub fn vehicle_ground_z(game: &Game, x: f64, y: f64) -> f64 {
    if let Some(t) = game.board.world_to_tile(x, y) {
        if let Some((a, b)) = game.board.ramps.get(&t) {
            return game.board.height(*a).min(game.board.height(*b)) as f64
                * constants::ELEVATION_PX;
        }
        return game.board.height(t) as f64 * constants::ELEVATION_PX;
    }
    0.0
}

/// Flat ground disc (range indicators, landing pads, mines, traps).
pub fn push_disc(mesh: &mut TriangleSoup, x: f64, y: f64, z: f64, r: f64, n: usize, c: [u8; 3]) {
    let center = vert(x, y, z, c);
    let mut prev = vert(x + r, y, z, c);
    for i in 1..=n {
        let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        let next = vert(x + r * a.cos(), y + r * a.sin(), z, c);
        push_tri(mesh, center, prev, next);
        prev = next;
    }
}

/// Axis-aligned isometric box centred at `(x, y)` on base `z`.
#[allow(clippy::too_many_arguments)]
pub fn push_box(
    mesh: &mut TriangleSoup,
    x: f64,
    y: f64,
    z: f64,
    sx: f64,
    sy: f64,
    sz: f64,
    color: [u8; 3],
) {
    let c000 = vert(x - sx / 2.0, y - sy / 2.0, z, color);
    let c100 = vert(x + sx / 2.0, y - sy / 2.0, z, color);
    let c110 = vert(x + sx / 2.0, y + sy / 2.0, z, color);
    let c010 = vert(x - sx / 2.0, y + sy / 2.0, z, color);
    let top = constants::shade(color, 1.0);
    let c001 = vert(x - sx / 2.0, y - sy / 2.0, z + sz, top);
    let c101 = vert(x + sx / 2.0, y - sy / 2.0, z + sz, top);
    let c111 = vert(x + sx / 2.0, y + sy / 2.0, z + sz, top);
    let c011 = vert(x - sx / 2.0, y + sy / 2.0, z + sz, top);
    // Top plus the two viewer-facing sides (same shading as before).
    push_quad(mesh, c001, c101, c111, c011);
    let side_a = constants::shade(color, 0.85);
    let mut q = [c010, c110, c111, c011];
    for v in q.iter_mut() {
        v.color = side_a;
    }
    push_quad(mesh, q[0], q[1], q[2], q[3]);
    let side_b = constants::shade(color, 0.7);
    let mut q = [c100, c110, c111, c101];
    for v in q.iter_mut() {
        v.color = side_b;
    }
    let _ = (c000, c100, c110, c010);
    push_quad(mesh, q[0], q[1], q[2], q[3]);
}

/// Thick 3D segment as a camera-facing box strip (grid-free strokes).
#[allow(clippy::too_many_arguments)]
pub fn push_beam(
    lines: &mut Vec<(LineVertex, LineVertex)>,
    x0: f64,
    y0: f64,
    z0: f64,
    x1: f64,
    y1: f64,
    z1: f64,
    color: [u8; 3],
) {
    lines.push((
        line_vert(x0, y0, z0, color, 255),
        line_vert(x1, y1, z1, color, 255),
    ));
}

/// Rebuild the dynamic mesh of one frame (buildings, obstacles, vehicles).
pub fn build_dynamic(game: &Game, rotor_phase: f64, out: &mut DynamicMesh) {
    out.clear();
    let mut order: Vec<usize> = (0..game.buildings.len()).collect();
    order.sort_by(|a, b| {
        let pa = game.buildings[*a].pos(game.board.side);
        let pb = game.buildings[*b].pos(game.board.side);
        (pa.0 + pa.1)
            .partial_cmp(&(pb.0 + pb.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for i in order {
        push_building(game, &game.buildings[i], &mut out.opaque, &mut out.lines);
    }
    for (tile, t) in game.board.tiles.iter() {
        if t.obstacle.is_some() {
            push_obstacle(game, *tile, &mut out.opaque, &mut out.lines);
        }
    }
    for v in game.vehicles.iter() {
        if v.dead {
            continue;
        }
        push_vehicle(game, v, rotor_phase, &mut out.opaque);
    }
    push_ranges(
        game,
        &mut out.range_turret,
        &mut out.range_heal,
        &mut out.lines,
    );
    push_paths(game, &mut out.lines);
    push_projectiles(game, &mut out.opaque);
}

fn building_color(b: &crate::entities::Building) -> [u8; 3] {
    match b.owner {
        Some(id) => constants::player_color(id),
        None => constants::NEUTRAL_COLOR,
    }
}

fn push_cross(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
    x: f64,
    y: f64,
    z: f64,
    color: [u8; 3],
) {
    let h = 6.0;
    let _ = mesh;
    lines.push((
        line_vert(x - h, y, z, color, 255),
        line_vert(x + h, y, z, color, 255),
    ));
    lines.push((
        line_vert(x, y - h, z, color, 255),
        line_vert(x, y + h, z, color, 255),
    ));
}

#[allow(clippy::too_many_arguments)]
fn push_ring(
    lines: &mut Vec<(LineVertex, LineVertex)>,
    x: f64,
    y: f64,
    z: f64,
    r: f64,
    n: usize,
    color: [u8; 3],
    alpha: u8,
) {
    let mut prev = (x + r, y);
    for i in 1..=n {
        let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        let next = (x + r * a.cos(), y + r * a.sin());
        lines.push((
            line_vert(prev.0, prev.1, z, color, alpha),
            line_vert(next.0, next.1, z, color, alpha),
        ));
        prev = next;
    }
}

fn push_building(
    game: &Game,
    b: &crate::entities::Building,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
) {
    use crate::entities::BuildingKind;
    let (cx, cy) = b.pos(game.board.side);
    let z = tile_top_z(&game.board, b.tile);
    let color = building_color(b);
    let dark = constants::shade(color, 0.7);
    match b.kind {
        BuildingKind::BaseTank | BuildingKind::BaseBuffer => {
            push_box(mesh, cx, cy, z, 26.0, 26.0, 14.0, color);
            if b.kind == BuildingKind::BaseBuffer {
                push_cross(mesh, lines, cx, cy, z + 14.0, [130, 235, 140]);
            } else {
                push_box(mesh, cx, cy, z + 14.0, 12.0, 12.0, 6.0, dark);
            }
        }
        BuildingKind::BaseHelicopter => {
            // Ground pad floats like the flat obstacle markers: coplanar
            // with the tile top it loses the depth race against terrain.
            push_disc(mesh, cx, cy, z + constants::OBSTACLE_LIFT, 20.0, 20, dark);
            push_disc(
                mesh,
                cx,
                cy,
                z + constants::OBSTACLE_LIFT + 1.0,
                15.0,
                20,
                color,
            );
        }
        BuildingKind::BaseHovercraft => {
            push_box(mesh, cx, cy, z, 30.0, 20.0, 10.0, color);
        }
        BuildingKind::TurretNormal | BuildingKind::TurretRapid | BuildingKind::TurretRocket => {
            // Base ring floats like the flat obstacle markers: coplanar
            // with the tile top it loses the depth race against terrain.
            push_disc(mesh, cx, cy, z + constants::OBSTACLE_LIFT, 14.0, 16, dark);
            push_disc(mesh, cx, cy, z + 8.0, 10.0, 14, color);
            let (dx, dy) = match b.last_target_pos {
                Some((tx, ty)) => {
                    let d = ((tx - cx).powi(2) + (ty - cy).powi(2)).sqrt().max(1e-6);
                    ((tx - cx) / d, (ty - cy) / d)
                }
                None => (1.0, 0.0),
            };
            let tk =
                crate::entities::turret_kind_of(b.kind).unwrap_or(constants::TurretKind::Normal);
            let len = if tk == constants::TurretKind::Rapid {
                10.0
            } else {
                18.0
            };
            push_beam(
                lines,
                cx,
                cy,
                z + 12.0,
                cx + dx * len,
                cy + dy * len,
                z + 14.0,
                dark,
            );
        }
        BuildingKind::HealTower => {
            push_box(mesh, cx, cy, z, 16.0, 16.0, 26.0, color);
            push_cross(mesh, lines, cx, cy, z + 26.0, [200, 255, 200]);
        }
    }
}

fn push_obstacle(
    game: &Game,
    tile: Tile,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
) {
    use crate::board::ObstacleKind;
    let t = match game.board.tiles.get(&tile) {
        Some(t) => t,
        None => return,
    };
    let o = match t.obstacle.as_ref() {
        Some(o) => o,
        None => return,
    };
    let (cx, cy) = game.board.center_world(tile);
    // Ramp tiles render their top at the lower end, so anchors (and the
    // selection overlay in render.rs) use the same helper.
    let z = tile_top_z(&game.board, tile);
    match o.kind {
        ObstacleKind::Wall => push_box(mesh, cx, cy, z, 30.0, 26.0, 18.0, [120, 100, 80]),
        ObstacleKind::Mine | ObstacleKind::MineWater => {
            // Flat marker floats just above the tile top: a coplanar opaque
            // disc loses the depth race against the terrain and flickers.
            let dz = z + constants::OBSTACLE_LIFT;
            push_disc(mesh, cx, cy, dz, 8.0, 10, [40, 40, 40]);
            // Distinct centre colour (Python draws a small red disc here):
            // both diagonals of a small cross, so the marker reads at
            // every zoom. Lines draw after the opaque pass, so they stay
            // visible over the base disc.
            let r = 3.5;
            push_beam(
                lines,
                cx - r,
                cy - r,
                dz + 0.1,
                cx + r,
                cy + r,
                dz + 0.1,
                [200, 60, 50],
            );
            push_beam(
                lines,
                cx - r,
                cy + r,
                dz + 0.1,
                cx + r,
                cy - r,
                dz + 0.1,
                [200, 60, 50],
            );
        }
        ObstacleKind::TrapFire => {
            let dz = z + constants::OBSTACLE_LIFT;
            push_disc(mesh, cx, cy, dz, 12.0, 12, [230, 120, 60]);
            // Flame stub above the centre (Python draws a vertical line).
            push_beam(lines, cx, cy, dz, cx, cy, dz + 10.0, [250, 170, 60]);
        }
        ObstacleKind::TrapIce => {
            let dz = z + constants::OBSTACLE_LIFT;
            push_disc(mesh, cx, cy, dz, 12.0, 12, [150, 210, 250]);
            // Two pale slashes across the disc (Python draws two lines).
            push_beam(
                lines,
                cx - 8.0,
                cy - 4.0,
                dz + 0.1,
                cx + 8.0,
                cy + 4.0,
                dz + 0.1,
                [240, 250, 255],
            );
            push_beam(
                lines,
                cx + 4.0,
                cy + 6.0,
                dz + 0.1,
                cx - 4.0,
                cy - 6.0,
                dz + 0.1,
                [240, 250, 255],
            );
        }
    }
}

fn vehicle_z(game: &Game, v: &crate::entities::Vehicle) -> f64 {
    let ground = vehicle_ground_z(game, v.x, v.y);
    if v.kind == constants::VehicleKind::Helicopter {
        ground + constants::ELEVATION_PX
    } else {
        ground
    }
}

fn push_vehicle(
    game: &Game,
    v: &crate::entities::Vehicle,
    rotor_phase: f64,
    mesh: &mut TriangleSoup,
) {
    let color = constants::player_color(v.owner);
    let z = vehicle_z(game, v);
    let (x, y) = (v.x, v.y);
    match v.kind {
        constants::VehicleKind::Tank => {
            push_box(mesh, x, y, z, 16.0, 22.0, 9.0, color);
            push_box(
                mesh,
                x,
                y,
                z + 9.0,
                10.0,
                10.0,
                5.0,
                constants::shade(color, 0.7),
            );
        }
        constants::VehicleKind::Helicopter => {
            push_disc(mesh, x, y, z, 10.0, 12, color);
        }
        constants::VehicleKind::Hovercraft => {
            // Hull sits flat on the ground: lift it like the flat obstacle
            // markers so it does not z-fight with the tile top.
            let dz = z + constants::OBSTACLE_LIFT;
            push_disc(mesh, x, y, dz, 13.0, 14, color);
            push_disc(mesh, x, y, dz + 4.0, 7.0, 12, constants::shade(color, 0.7));
        }
        constants::VehicleKind::Buffer => {
            push_box(mesh, x, y, z, 16.0, 20.0, 9.0, color);
        }
    }
    // Rotor/tail details become lines below (thin strokes need no depth
    // fighting on the GPU path); keep a rotor stub for animation parity.
    let _ = rotor_phase;
}

fn push_ranges(
    game: &Game,
    turret: &mut RangeSoup,
    heal: &mut RangeSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
) {
    use crate::entities::BuildingKind;
    for (idx, b) in game.buildings.iter().enumerate() {
        // Deterministic lift per disc: coplanar translucent fills share one
        // depth value, so the GPU blend order flips while panning (flicker).
        // A tiny index-based step keeps every disc distinct and stable.
        let lift = idx as f64 * 0.05;
        if let Some(tk) = crate::entities::turret_kind_of(b.kind) {
            let (cx, cy) = b.pos(game.board.side);
            let z = tile_top_z(&game.board, b.tile);
            push_range_disc(
                turret,
                cx,
                cy,
                z + 0.5 + lift,
                constants::turret_range(tk),
                40,
                [255, 255, 255],
                constants::RANGE_TURRET_FILL_ALPHA,
            );
            push_ring(
                lines,
                cx,
                cy,
                z + 0.6 + lift,
                constants::turret_range(tk),
                48,
                [255, 255, 255],
                constants::RANGE_OUTLINE_ALPHA,
            );
        } else if b.kind == BuildingKind::HealTower {
            let (cx, cy) = b.pos(game.board.side);
            let z = tile_top_z(&game.board, b.tile);
            let r = b.units * constants::HEAL_TOWER_RANGE_PER_UNIT;
            if r > 1.0 {
                push_range_disc(
                    heal,
                    cx,
                    cy,
                    z + 0.5 + lift,
                    r,
                    40,
                    [150, 245, 150],
                    constants::RANGE_HEAL_FILL_ALPHA,
                );
                push_ring(
                    lines,
                    cx,
                    cy,
                    z + 0.6 + lift,
                    r,
                    48,
                    [150, 245, 150],
                    constants::RANGE_OUTLINE_ALPHA,
                );
            }
        }
    }
    for (idx, v) in game.vehicles.iter().enumerate() {
        if v.dead || v.kind != constants::VehicleKind::Buffer {
            continue;
        }
        let z = vehicle_z(game, v);
        push_range_disc(
            heal,
            v.x,
            v.y,
            z - constants::ELEVATION_PX + 0.5 + idx as f64 * 0.05,
            constants::BUFFER_HEAL_RADIUS,
            40,
            [150, 245, 150],
            constants::RANGE_HEAL_FILL_ALPHA,
        );
    }
}

fn push_paths(game: &Game, lines: &mut Vec<(LineVertex, LineVertex)>) {
    for v in game.vehicles.iter() {
        if v.dead || v.route.is_empty() {
            continue;
        }
        let z = vehicle_z(game, v);
        let mut prev = (v.x, v.y);
        for t in v.route.iter().skip(v.route_index) {
            let (wx, wy) = game.board.center_world(*t);
            lines.push((
                line_vert(prev.0, prev.1, z, [255, 255, 255], 255),
                line_vert(wx, wy, z, [255, 255, 255], 255),
            ));
            prev = (wx, wy);
        }
    }
}

fn push_projectiles(game: &Game, mesh: &mut TriangleSoup) {
    for p in game.projectiles.iter() {
        let t = (p.t / p.dur).clamp(0.0, 1.0);
        let x = p.from_pos.0 + (p.to.0 - p.from_pos.0) * t;
        let y = p.from_pos.1 + (p.to.1 - p.from_pos.1) * t;
        let arc = 40.0 * (1.0 - (2.0 * t - 1.0).powi(2));
        let z = 30.0 + arc;
        let r = if p.kind == constants::TurretKind::Rocket {
            f64::from(constants::ROCKET_RADIUS)
        } else {
            f64::from(constants::PROJECTILE_RADIUS)
        };
        let col = if p.kind == constants::TurretKind::Rocket {
            [255, 120, 60]
        } else {
            [250, 250, 250]
        };
        push_disc(mesh, x, y, z, r, 10, col);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;

    #[test]
    fn range_fills_keep_distinct_alpha_and_stable_lift() {
        use crate::entities::{Building, BuildingKind, Player};
        use crate::game::Game;
        let board = Board::new(30, 30);
        let mut game = Game::new(
            board,
            vec![Player::new(0, true), Player::new(1, false)],
            Vec::new(),
            1,
        );
        game.buildings.push(Building::new(
            BuildingKind::TurretNormal,
            Some(0),
            2,
            2,
            10.0,
        ));
        game.buildings.push(Building::new(
            BuildingKind::TurretNormal,
            Some(1),
            5,
            5,
            10.0,
        ));
        game.buildings
            .push(Building::new(BuildingKind::HealTower, Some(0), 8, 8, 10.0));
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        assert_eq!(dynamic.range_turret.vertices.len(), 2 * 40 * 3);
        assert!(!dynamic.range_heal.vertices.is_empty());
        // Turret fills are subtler than before (Python parity), heal fills
        // keep their own light-green transparency.
        for v in dynamic.range_turret.vertices.iter() {
            assert_eq!(v.color[3], constants::RANGE_TURRET_FILL_ALPHA);
            assert_eq!(&v.color[..3], &[255, 255, 255]);
        }
        for v in dynamic.range_heal.vertices.iter() {
            assert_eq!(v.color[3], constants::RANGE_HEAL_FILL_ALPHA);
            assert_eq!(&v.color[..3], &[150, 245, 150]);
        }
        // The two turret discs sit at distinct deterministic lifts, so
        // their blend no longer depends on float rounding while panning.
        let z0 = dynamic.range_turret.vertices[0].z;
        let z1 = dynamic.range_turret.vertices[40 * 3].z;
        assert!((z1 - z0 - 0.05).abs() < 1e-6, "{z0} vs {z1}");
        // Outlines share the fill hue but are clearly less transparent.
        let mut turret_ring = false;
        for (a, b) in dynamic.lines.iter() {
            if a.color[3] == constants::RANGE_OUTLINE_ALPHA
                && a.color[..3] == [255, 255, 255]
                && b.color[3] == constants::RANGE_OUTLINE_ALPHA
            {
                turret_ring = true;
            }
        }
        assert!(turret_ring);
    }

    #[test]
    fn obstacles_float_above_terrain_with_details() {
        use crate::board::Obstacle;
        use crate::board::ObstacleKind;
        use crate::entities::Player;
        use crate::game::Game;
        let kinds = [
            ObstacleKind::Mine,
            ObstacleKind::TrapFire,
            ObstacleKind::TrapIce,
        ];
        for (i, kind) in kinds.iter().enumerate() {
            let mut board = Board::new(8, 8);
            let tile = (2 + i as i32, 2);
            for t in board.tiles.clone().keys() {
                board.tiles.get_mut(t).unwrap().height = 1;
            }
            board.tiles.get_mut(&tile).unwrap().obstacle = Some(Obstacle::new(*kind));
            let game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
            let mut dynamic = DynamicMesh::default();
            build_dynamic(&game, 0.0, &mut dynamic);
            let top = tile_top_z(&game.board, tile);
            assert!(
                !dynamic.opaque.vertices.is_empty(),
                "{kind:?} has no opaque marker"
            );
            for v in dynamic.opaque.vertices.iter() {
                assert!(
                    (v.z as f64) >= top + constants::OBSTACLE_LIFT - 1e-6,
                    "{kind:?} marker not lifted: {} vs {top}",
                    v.z
                );
            }
            // Every flat marker carries detail lines (red mine cross, flame
            // stub, ice slashes), so the kind reads even at small zoom.
            assert!(!dynamic.lines.is_empty(), "{kind:?} has no detail lines");
            dynamic.clear();
        }
        // A wall stays a raised box anchored at the tile top (no lift).
        let mut board = Board::new(8, 8);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let tile = (2, 2);
        board.tiles.get_mut(&tile).unwrap().obstacle = Some(Obstacle::new(ObstacleKind::Wall));
        let game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        let top = tile_top_z(&game.board, tile);
        let raised = dynamic
            .opaque
            .vertices
            .iter()
            .any(|v| (v.z as f64) > top + 1.0);
        assert!(raised, "wall box has no height");
    }

    #[test]
    fn flat_building_parts_float_above_terrain() {
        use crate::entities::{Building, BuildingKind, Player};
        use crate::game::Game;
        let mut board = Board::new(10, 10);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let turret = Building::new(BuildingKind::TurretNormal, Some(0), 2, 2, 10.0);
        let pad = Building::new(BuildingKind::BaseHelicopter, Some(0), 5, 5, 10.0);
        let game = Game::new(board, vec![Player::new(0, true)], vec![turret, pad], 1);
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        assert!(!dynamic.opaque.vertices.is_empty());
        // The lowest opaque vertex of the flat parts sits at the shared
        // lift, not coplanar with the tile top (the 1-px upper pads and
        // the raised turret dome sit higher by construction).
        let top = tile_top_z(&game.board, (2, 2));
        let lowest = dynamic
            .opaque
            .vertices
            .iter()
            .map(|v| v.z as f64)
            .fold(f64::INFINITY, f64::min);
        assert!(
            lowest >= top + constants::OBSTACLE_LIFT - 1e-6,
            "flat part not lifted: {lowest} vs {top}"
        );
    }

    #[test]
    fn ramp_strip_runs_edge_to_edge() {
        let mut board = Board::new(6, 6);
        // Neighbours (3, 2) and (5, 2) are opposite across (4, 2).
        board.set_ramp((4, 2), (3, 2), (5, 2));
        let mesh = build_terrain(&board);
        let soup: Vec<&GpuVertex> = mesh
            .chunks
            .iter()
            .flat_map(|c| c.soup.vertices.iter())
            .collect();
        assert!(!soup.is_empty());
        // Edge midpoints of the ramp tile along the a->b axis.
        let corners = crate::hexgrid::hex_corners(4, 2, board.side);
        let mut mids = [(0.0, 0.0); 6];
        for k in 0..6 {
            let c1 = corners[k];
            let c2 = corners[(k + 1) % 6];
            mids[k] = ((c1.0 + c2.0) / 2.0, (c1.1 + c2.1) / 2.0);
        }
        let nearest = |target: (i32, i32)| -> usize {
            let (tx, ty) = crate::hexgrid::hex_to_world(target.0, target.1, board.side);
            let mut best = 0;
            let mut best_d = f64::INFINITY;
            for (k, m) in mids.iter().enumerate() {
                let d = (m.0 - tx).powi(2) + (m.1 - ty).powi(2);
                if d < best_d {
                    best_d = d;
                    best = k;
                }
            }
            best
        };
        let edge_a = mids[nearest((3, 2))];
        let edge_b = mids[nearest((5, 2))];
        // The strip corners (top face + skirts) reach both edge midpoints
        // within the strip half-width, instead of stopping at 35% of the
        // way like the old short strip.
        let hw = board.side * 0.45;
        for edge in [edge_a, edge_b] {
            let mut best = f64::INFINITY;
            for v in soup.iter() {
                let d = ((v.x as f64 - edge.0).powi(2) + (v.y as f64 - edge.1).powi(2)).sqrt();
                best = best.min(d);
            }
            assert!(best <= hw + 1e-3, "edge {edge:?} far: {best}");
        }
    }

    #[test]
    fn terrain_mesh_counts_are_deterministic() {
        let board = Board::new(4, 3);
        let a = build_terrain(&board);
        let b = build_terrain(&board);
        let count = |m: &TerrainMesh| {
            m.chunks
                .iter()
                .map(|c| c.soup.vertices.len())
                .sum::<usize>()
        };
        let indices =
            |m: &TerrainMesh| m.chunks.iter().map(|c| c.soup.indices.len()).sum::<usize>();
        assert_eq!(count(&a), count(&b));
        assert_eq!(indices(&a), indices(&b));
        // One spatial chunk; 12 tiles at height 1 surrounded by water:
        // 12 tops (4 tris each) plus outer skirts (3 edges x 2 tris each).
        assert_eq!(a.chunks.len(), 1);
        assert_eq!(count(&a), 300);
        let grid: usize = a.chunks.iter().map(|c| c.grid_lines.len()).sum();
        assert_eq!(grid, 12 * 6);
        for chunk in a.chunks.iter() {
            assert!(chunk.soup.vertices.len() <= CHUNK_VERTICES);
            for (i, idx) in chunk.soup.indices.iter().enumerate() {
                assert_eq!(*idx as usize, i);
            }
        }
    }

    #[test]
    fn chunks_are_spatial_and_cover_all_vertices() {
        let board = Board::new(40, 40);
        let mesh = build_terrain(&board);
        // 40 / 16 tiles per chunk -> 3 x 3 chunks.
        assert_eq!(mesh.chunks.len(), 9);
        for chunk in mesh.chunks.iter() {
            assert!(chunk.soup.vertices.len() <= CHUNK_VERTICES);
            // Every vertex of the chunk lies inside its (padded) box.
            for v in chunk.soup.vertices.iter() {
                let (x, y) = (v.x as f64, v.y as f64);
                assert!(
                    x >= chunk.bbox.0 - 1e-6
                        && x <= chunk.bbox.2 + 1e-6
                        && y >= chunk.bbox.1 - 1e-6
                        && y <= chunk.bbox.3 + 1e-6,
                    "vertex ({x},{y}) outside {:?}",
                    chunk.bbox
                );
            }
            for (a, b) in chunk.grid_lines.iter() {
                for v in [a, b] {
                    let (x, y) = (v.x as f64, v.y as f64);
                    assert!(x >= chunk.bbox.0 - 1e-6 && x <= chunk.bbox.2 + 1e-6);
                    assert!(y >= chunk.bbox.1 - 1e-6 && y <= chunk.bbox.3 + 1e-6);
                }
            }
        }
    }

    #[test]
    fn visible_bounds_include_the_camera_target() {
        use crate::camera::Camera;
        let mut camera = Camera::new((1180.0, 720.0));
        camera.center_on_world(2000.0, 1000.0, 0.0);
        let (x0, y0, x1, y1) = visible_world_bounds(&camera, 15.0 * constants::ELEVATION_PX, 36.0);
        // The point the camera looks at must be inside the box...
        assert!(x0 < 2000.0 && 2000.0 < x1, "x box {x0}..{x1}");
        assert!(y0 < 1000.0 && 1000.0 < y1, "y box {y0}..{y1}");
        // ...and the box must be tight to the viewport, not the whole world.
        assert!(x1 - x0 < 4000.0, "box too wide: {}", x1 - x0);
        assert!(y1 - y0 < 4000.0, "box too tall: {}", y1 - y0);
        // Panning the camera moves the box with it.
        camera.center_on_world(6000.0, 1000.0, 0.0);
        let (nx0, _, nx1, _) = visible_world_bounds(&camera, 15.0 * constants::ELEVATION_PX, 36.0);
        assert!(nx0 > x0 && nx1 > x1, "box did not follow the pan");
    }

    #[test]
    fn mesh_chunks_fit_u16_indices() {
        for path in crate::mapfile::list_maps(None) {
            let game = crate::mapfile::load_game(&path).expect("repo map loads");
            let mesh = build_terrain(&game.board);
            for chunk in mesh.chunks.iter() {
                assert!(
                    chunk.soup.vertices.len() <= CHUNK_VERTICES,
                    "{} chunk has {} vertices",
                    path.display(),
                    chunk.soup.vertices.len()
                );
            }
        }
    }
}
