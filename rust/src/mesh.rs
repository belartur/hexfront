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
    /// Flat translucent vehicle shadow decals (no depth write, drawn after
    /// the opaque pass and before the range fills; see
    /// [`push_helicopter_shadow`]).
    pub shadow: RangeSoup,
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
        self.shadow.vertices.clear();
        self.shadow.indices.clear();
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

/// Oriented box centred at `(cx, cy)` on base `z`: `len` runs along the
/// unit forward vector `(fx, fy)`, `wid` along its perpendicular, `h` up.
///
/// Same face set as [`push_box`] (top plus sides), so rotated vehicle parts
/// keep the flat-shaded look of the axis-aligned boxes.
#[allow(clippy::too_many_arguments)]
pub fn push_oriented_box(
    mesh: &mut TriangleSoup,
    cx: f64,
    cy: f64,
    z: f64,
    len: f64,
    wid: f64,
    h: f64,
    fx: f64,
    fy: f64,
    color: [u8; 3],
) {
    let (px, py) = (-fy, fx);
    let corner = |along: f64, across: f64, zz: f64, c: [u8; 3]| {
        vert(
            cx + fx * along + px * across,
            cy + fy * along + py * across,
            zz,
            c,
        )
    };
    let (hl, hw) = (len / 2.0, wid / 2.0);
    let top = constants::shade(color, 1.0);
    let t0 = corner(-hl, -hw, z + h, top);
    let t1 = corner(hl, -hw, z + h, top);
    let t2 = corner(hl, hw, z + h, top);
    let t3 = corner(-hl, hw, z + h, top);
    push_quad(mesh, t0, t1, t2, t3);
    let b0 = corner(-hl, -hw, z, color);
    let b1 = corner(hl, -hw, z, color);
    let b2 = corner(hl, hw, z, color);
    let b3 = corner(-hl, hw, z, color);
    let side_a = constants::shade(color, 0.85);
    let side_b = constants::shade(color, 0.7);
    let mut q = [b0, b1, t1, t0];
    for v in q.iter_mut() {
        v.color = side_a;
    }
    push_quad(mesh, q[0], q[1], q[2], q[3]);
    let mut q = [b2, b3, t3, t2];
    for v in q.iter_mut() {
        v.color = side_a;
    }
    push_quad(mesh, q[0], q[1], q[2], q[3]);
    let mut q = [b1, b2, t2, t1];
    for v in q.iter_mut() {
        v.color = side_b;
    }
    push_quad(mesh, q[0], q[1], q[2], q[3]);
    let mut q = [b3, b0, t0, t3];
    for v in q.iter_mut() {
        v.color = side_b;
    }
    push_quad(mesh, q[0], q[1], q[2], q[3]);
}

/// Vertical truncated cone: a faceted side wall closed by a flat top disc.
///
/// Stacked flat discs are enough for a helicopter hull seen from above, but
/// a turret has to read as one solid volume: the side quads close the space
/// between the base ring and the roof, so no terrain shows through while the
/// turret rotates. Alternating facet shades fake a curved surface with two
/// flat colours, like every other part of the scene.
#[allow(clippy::too_many_arguments)]
pub fn push_cylinder(
    mesh: &mut TriangleSoup,
    x: f64,
    y: f64,
    z: f64,
    r0: f64,
    r1: f64,
    h: f64,
    n: usize,
    color: [u8; 3],
) {
    let facets = [constants::shade(color, 0.8), constants::shade(color, 0.65)];
    for i in 0..n {
        let a0 = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        let a1 = 2.0 * std::f64::consts::PI * (i + 1) as f64 / n as f64;
        let c = facets[i % 2];
        let b0 = vert(x + r0 * a0.cos(), y + r0 * a0.sin(), z, c);
        let b1 = vert(x + r0 * a1.cos(), y + r0 * a1.sin(), z, c);
        let t0 = vert(x + r1 * a0.cos(), y + r1 * a0.sin(), z + h, c);
        let t1 = vert(x + r1 * a1.cos(), y + r1 * a1.sin(), z + h, c);
        push_quad(mesh, b0, b1, t1, t0);
    }
    push_disc(mesh, x, y, z + h, r1, n, constants::shade(color, 1.0));
}

/// Oriented wedge: a `len` x `wid` rectangle on base `z` whose top face
/// tilts from `h_back` at its rear edge down to `h_front` at its front edge.
///
/// Both the top face and all four sides are filled, so the wedge reads as a
/// solid sloped plate (the tank's glacis) from any heading.
#[allow(clippy::too_many_arguments)]
pub fn push_oriented_slope(
    mesh: &mut TriangleSoup,
    cx: f64,
    cy: f64,
    z: f64,
    len: f64,
    wid: f64,
    h_back: f64,
    h_front: f64,
    fx: f64,
    fy: f64,
    color: [u8; 3],
) {
    let (px, py) = (-fy, fx);
    let corner = |along: f64, across: f64, zz: f64, c: [u8; 3]| {
        vert(
            cx + fx * along + px * across,
            cy + fy * along + py * across,
            zz,
            c,
        )
    };
    let (hl, hw) = (len / 2.0, wid / 2.0);
    let top = constants::shade(color, 1.0);
    let b0 = corner(-hl, -hw, z, color);
    let b1 = corner(hl, -hw, z, color);
    let b2 = corner(hl, hw, z, color);
    let b3 = corner(-hl, hw, z, color);
    let t0 = corner(-hl, -hw, z + h_back, top);
    let t1 = corner(hl, -hw, z + h_front, top);
    let t2 = corner(hl, hw, z + h_front, top);
    let t3 = corner(-hl, hw, z + h_back, top);
    push_quad(mesh, t0, t1, t2, t3);
    let side_a = constants::shade(color, 0.85);
    let side_b = constants::shade(color, 0.7);
    let mut q = [b0, b1, t1, t0];
    for v in q.iter_mut() {
        v.color = side_a;
    }
    push_quad(mesh, q[0], q[1], q[2], q[3]);
    let mut q = [b2, b3, t3, t2];
    for v in q.iter_mut() {
        v.color = side_a;
    }
    push_quad(mesh, q[0], q[1], q[2], q[3]);
    let mut q = [b1, b2, t2, t1];
    for v in q.iter_mut() {
        v.color = side_b;
    }
    push_quad(mesh, q[0], q[1], q[2], q[3]);
    let mut q = [b3, b0, t0, t3];
    for v in q.iter_mut() {
        v.color = side_b;
    }
    push_quad(mesh, q[0], q[1], q[2], q[3]);
}

/// Flight heading of a vehicle as a unit `(fx, fy)` vector in world space.
///
/// Points at the next route waypoint; when the vehicle has nowhere to go
/// (empty route, arrived, ad-hoc test vehicle) it falls back to the leg it
/// came from, and finally to east (+x), so parked helicopters still face a
/// deterministic direction instead of snapping arbitrarily.
fn vehicle_heading(game: &Game, v: &crate::entities::Vehicle) -> (f64, f64) {
    let aim_at = |tx: f64, ty: f64| {
        let (dx, dy) = (tx - v.x, ty - v.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len > 1e-6 {
            Some((dx / len, dy / len))
        } else {
            None
        }
    };
    if v.route_index < v.route.len() {
        let (wx, wy) = game.board.center_world(v.route[v.route_index]);
        if let Some(h) = aim_at(wx, wy) {
            return h;
        }
    }
    if v.route_index > 0 && v.route_index <= v.route.len() {
        let prev = v.route[v.route_index - 1];
        let (wx, wy) = game.board.center_world(prev);
        // Heading is where we came *from* reversed: from the previous
        // waypoint towards the current position.
        let (dx, dy) = (v.x - wx, v.y - wy);
        let len = (dx * dx + dy * dy).sqrt();
        if len > 1e-6 {
            return (dx / len, dy / len);
        }
    }
    if let Some(src) = v.src_tile {
        let (wx, wy) = game.board.center_world(src);
        let (dx, dy) = (v.x - wx, v.y - wy);
        let len = (dx * dx + dy * dy).sqrt();
        if len > 1e-6 {
            return (dx / len, dy / len);
        }
    }
    (1.0, 0.0)
}

/// Aim direction of a tank turret as a unit `(ax, ay)` vector in world space.
///
/// The gun points at whatever the tank is currently shooting: the enemy
/// vehicle of its duel (rules.md section 9) or, when no duel is running, the
/// wall it shells on its way (section 4). With no target at all the turret
/// stays aligned with `heading` -- the chassis direction from
/// [`vehicle_heading`] -- so a marching column keeps its barrels forward.
/// `heading` is passed in because the caller needs it for the chassis too.
fn tank_aim(game: &Game, v: &crate::entities::Vehicle, heading: (f64, f64)) -> (f64, f64) {
    let aim_at = |tx: f64, ty: f64| {
        let (dx, dy) = (tx - v.x, ty - v.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len > 1e-6 {
            Some((dx / len, dy / len))
        } else {
            None
        }
    };
    if let Some(tid) = v.combat_target
        && let Some(enemy) = game.vehicles.iter().find(|x| x.id == tid && !x.dead)
        && let Some(dir) = aim_at(enemy.x, enemy.y)
    {
        return dir;
    }
    if let Some(tile) = v.wall_target {
        let (wx, wy) = game.board.center_world(tile);
        if let Some(dir) = aim_at(wx, wy) {
            return dir;
        }
    }
    heading
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
        push_vehicle(game, v, rotor_phase, &mut out.opaque, &mut out.lines);
        if v.kind == constants::VehicleKind::Helicopter {
            push_helicopter_shadow(game, v, &mut out.shadow);
        }
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

/// Rendered elevation of a vehicle in px.
///
/// A ground vehicle stands on the walkable surface below it (deck and ramp
/// aware, see [`vehicle_ground_z`]). A helicopter ignores the terrain
/// (rules.md section 5.2), so it flies at a fixed altitude above the
/// *highest* tile of the board ([`helicopter_altitude`]): the altitude is
/// constant for the whole level instead of following every bump, and the
/// shadow disc built by [`push_helicopter_shadow`] still tells which tile
/// the helicopter is over.
pub fn vehicle_z(game: &Game, v: &crate::entities::Vehicle) -> f64 {
    if v.kind == constants::VehicleKind::Helicopter {
        helicopter_altitude(game)
    } else {
        vehicle_ground_z(game, v.x, v.y)
    }
}

/// Fixed flight altitude of the helicopters of `game` in px.
///
/// Measured above the highest terrain of the board, so a helicopter never
/// hides behind a peak no matter where it crosses the map.
fn helicopter_altitude(game: &Game) -> f64 {
    max_height(&game.board) + constants::HELICOPTER_ALTITUDE_PX
}

fn push_vehicle(
    game: &Game,
    v: &crate::entities::Vehicle,
    rotor_phase: f64,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
) {
    let color = constants::player_color(v.owner);
    let z = vehicle_z(game, v);
    let (x, y) = (v.x, v.y);
    match v.kind {
        constants::VehicleKind::Tank => push_tank(game, v, mesh, lines, x, y, z, color),
        constants::VehicleKind::Helicopter => {
            push_helicopter(game, v, mesh, lines, x, y, z, rotor_phase, color);
        }
        constants::VehicleKind::Hovercraft => {
            // Hull sits flat on the ground: lift it like the flat obstacle
            // markers so it does not z-fight with the tile top.
            let dz = z + constants::OBSTACLE_LIFT;
            push_disc(mesh, x, y, dz, 13.0, 14, color);
            push_disc(mesh, x, y, dz + 4.0, 7.0, 12, constants::shade(color, 0.7));
        }
        constants::VehicleKind::Buffer => {
            // Same chassis as a tank (rules.md section 5.4: a buffer drives
            // exactly like one) without a gun: the green healing cross the
            // base uses marks it as the support vehicle instead.
            let (fx, fy) = vehicle_heading(game, v);
            // `deck_top` is already absolute (it includes the vehicle's `z`).
            let deck_top = push_tank_chassis(mesh, lines, x, y, z, color, fx, fy);
            push_cross(mesh, lines, x, y, deck_top + 4.0, [130, 235, 140]);
        }
    }
}

/// Detailed helicopter: slender pod hull with a glazed cockpit, tail boom
/// with a fin and a spinning two-blade main rotor plus a tail rotor.
///
/// The airframe is oriented along the flight heading (see
/// [`vehicle_heading`]): the cockpit faces the next waypoint and the tail
/// boom trails behind it, so the tail always stays at the back of the
/// flight direction. The body stays an opaque box stack (depth-tested like
/// every other vehicle), while the thin rotor blades and skid struts are 3D
/// line strokes: they need no depth fighting on the GPU path and match the
/// Python renderer, which draws the rotor as lines above the body.
/// `rotor_phase` rotates the main blades around the mast, so consecutive
/// frames built with an advancing phase show the spin.
#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
fn push_helicopter(
    game: &Game,
    v: &crate::entities::Vehicle,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
    x: f64,
    y: f64,
    z: f64,
    rotor_phase: f64,
    color: [u8; 3],
) {
    let (fx, fy) = vehicle_heading(game, v);
    push_helicopter_oriented(mesh, lines, x, y, z, rotor_phase, color, fx, fy);
}

// ---------------------------------------------------------------------------
// Helicopter parts (rules.md section 5.2; every value is a rendering-only
// size in px, exactly like the tank constants below -- colours come from the
// owning player, only the cockpit glass has a fixed tint).
// ---------------------------------------------------------------------------

/// Length of the pod hull along the flight heading in px.
const HELI_HULL_LEN: f64 = 26.0;
/// Width of the pod hull across the heading in px; clearly longer than wide,
/// so the hull reads as a slender pod instead of a disc.
const HELI_HULL_WID: f64 = 11.0;
/// Height of the pod hull in px.
const HELI_HULL_H: f64 = 6.0;
/// Clearance of the hull belly above the skid base in px.
const HELI_HULL_LIFT: f64 = 3.0;
/// Length of the glazed cockpit on the forward hull top in px.
const HELI_CANOPY_LEN: f64 = 8.0;
/// Width of the cockpit canopy in px.
const HELI_CANOPY_WID: f64 = 8.0;
/// Height of the cockpit canopy in px.
const HELI_CANOPY_H: f64 = 3.5;
/// Forward shift of the canopy centre from the hull centre in px.
const HELI_CANOPY_SHIFT: f64 = 6.5;
/// Elevation of the canopy base above the skid base in px: it sits on the
/// forward hull top, so the glass reads as a windscreen (not a floating box).
const HELI_CANOPY_BASE: f64 = HELI_HULL_LIFT + HELI_HULL_H - 1.5;
/// Fixed tint of the cockpit glass (a dark canopy, never a white box).
const HELI_CANOPY_COLOR: [u8; 3] = [72, 106, 126];
/// Length of the tail boom behind the hull in px.
const HELI_TAIL_LEN: f64 = 18.0;
/// Width of the tail boom in px.
const HELI_TAIL_WID: f64 = 3.0;
/// Height of the tail boom in px.
const HELI_TAIL_H: f64 = 3.0;
/// Rearward shift of the tail boom centre from the hull centre in px.
const HELI_TAIL_SHIFT: f64 = -20.0;
/// Elevation of the tail boom base above the skid base in px.
const HELI_TAIL_BASE: f64 = 4.5;
/// Length of the vertical tail fin along the heading in px.
const HELI_FIN_LEN: f64 = 3.0;
/// Width of the vertical tail fin in px.
const HELI_FIN_WID: f64 = 2.5;
/// Height of the vertical tail fin in px (it carries the tail rotor).
const HELI_FIN_H: f64 = 9.0;
/// Rearward shift of the tail fin from the hull centre in px.
const HELI_FIN_SHIFT: f64 = -27.0;
/// Elevation of the tail fin base above the skid base in px.
const HELI_FIN_BASE: f64 = 4.0;
/// Length of one landing skid rail in px.
const HELI_SKID_LEN: f64 = 20.0;
/// Thickness of a skid rail in px.
const HELI_SKID_THICK: f64 = 1.6;
/// Lateral offset of each skid rail from the hull centre line in px.
const HELI_SKID_OFFSET: f64 = 6.0;
/// Offset along the heading of the skid struts from the hull centre in px.
const HELI_STRUT_SHIFT: f64 = 7.0;
/// Elevation of the rotor mast base above the skid base in px (hull top).
const HELI_MAST_BASE: f64 = HELI_HULL_LIFT + HELI_HULL_H;
/// Height of the rotor mast above the hull top in px.
const HELI_MAST_H: f64 = 5.0;
/// Thickness of the rotor mast in px.
const HELI_MAST_THICK: f64 = 2.5;
/// Radius of the main rotor in px.
const HELI_ROTOR_R: f64 = 22.0;
/// Radius of the tail rotor in px.
const HELI_TAIL_ROTOR_R: f64 = 5.0;
/// Elevation of the tail rotor axis above the skid base in px: just above
/// the tip of the vertical fin.
const HELI_TAIL_ROTOR_Z: f64 = HELI_FIN_BASE + HELI_FIN_H + 1.0;

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn push_helicopter_oriented(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
    x: f64,
    y: f64,
    z: f64,
    rotor_phase: f64,
    color: [u8; 3],
    fx: f64,
    fy: f64,
) {
    let (px, py) = (-fy, fx);
    let dark = constants::shade(color, 0.7);
    let darker = constants::shade(color, 0.55);
    // Slender pod hull: one long oriented box (much longer than wide and
    // wider than it is tall) replaces the old stack of round discs.
    push_oriented_box(
        mesh,
        x,
        y,
        z + HELI_HULL_LIFT,
        HELI_HULL_LEN,
        HELI_HULL_WID,
        HELI_HULL_H,
        fx,
        fy,
        color,
    );
    // Glazed cockpit on the forward hull top: a small tinted canopy, so the
    // windscreen reads as glass instead of a glaring white box.
    push_oriented_box(
        mesh,
        x + fx * HELI_CANOPY_SHIFT,
        y + fy * HELI_CANOPY_SHIFT,
        z + HELI_CANOPY_BASE,
        HELI_CANOPY_LEN,
        HELI_CANOPY_WID,
        HELI_CANOPY_H,
        fx,
        fy,
        HELI_CANOPY_COLOR,
    );
    // Tail boom trailing behind (-forward) with an end fin (tail rotor
    // mast). The boom overlaps the hull rear, so no gap opens while the
    // helicopter turns.
    push_oriented_box(
        mesh,
        x + fx * HELI_TAIL_SHIFT,
        y + fy * HELI_TAIL_SHIFT,
        z + HELI_TAIL_BASE,
        HELI_TAIL_LEN,
        HELI_TAIL_WID,
        HELI_TAIL_H,
        fx,
        fy,
        dark,
    );
    push_oriented_box(
        mesh,
        x + fx * HELI_FIN_SHIFT,
        y + fy * HELI_FIN_SHIFT,
        z + HELI_FIN_BASE,
        HELI_FIN_LEN,
        HELI_FIN_WID,
        HELI_FIN_H,
        fx,
        fy,
        darker,
    );
    // Landing skids: two thin rails parallel to the hull, one on each side,
    // joined to the belly by four strut strokes. Chunky axis-aligned boxes
    // would read as one flat rectangle from the isometric view.
    for side in [-1.0, 1.0] {
        let rail_cx = x + px * side * HELI_SKID_OFFSET;
        let rail_cy = y + py * side * HELI_SKID_OFFSET;
        push_oriented_box(
            mesh,
            rail_cx,
            rail_cy,
            z,
            HELI_SKID_LEN,
            HELI_SKID_THICK,
            HELI_SKID_THICK,
            fx,
            fy,
            darker,
        );
        for along in [-HELI_STRUT_SHIFT, HELI_STRUT_SHIFT] {
            push_beam(
                lines,
                x + fx * along + px * side * HELI_SKID_OFFSET,
                y + fy * along + py * side * HELI_SKID_OFFSET,
                z + HELI_SKID_THICK,
                x + fx * along + px * side * (HELI_SKID_OFFSET / 2.0),
                y + fy * along + py * side * (HELI_SKID_OFFSET / 2.0),
                z + HELI_HULL_LIFT,
                darker,
            );
        }
    }
    // Mast holding the main rotor above the hull.
    push_box(
        mesh,
        x,
        y,
        z + HELI_MAST_BASE,
        HELI_MAST_THICK,
        HELI_MAST_THICK,
        HELI_MAST_H,
        darker,
    );
    // Main rotor: two opposite blades rotating with `rotor_phase`, drawn as
    // bright strokes like in the Python version (`_draw_vehicle`); the disc
    // sits one px above the mast top.
    let rz = z + HELI_MAST_BASE + HELI_MAST_H + 1.0;
    let (c, s) = (rotor_phase.cos(), rotor_phase.sin());
    let r = HELI_ROTOR_R;
    let blade = [210, 210, 210];
    push_beam(
        lines,
        x - r * c,
        y - r * s,
        rz,
        x + r * c,
        y + r * s,
        rz,
        blade,
    );
    // Second blade pair at 90 degrees fakes motion blur of a fast rotor.
    let faint = [150, 150, 150];
    push_beam(
        lines,
        x - r * s,
        y + r * c,
        rz,
        x + r * s,
        y - r * c,
        rz,
        faint,
    );
    // Tail rotor: short vertical stroke spinning on the fin tip. Its phase
    // runs twice as fast as the main rotor.
    let t_phase = rotor_phase * 2.0;
    let (tc, ts) = (t_phase.cos(), t_phase.sin());
    let tr = HELI_TAIL_ROTOR_R;
    let (tx, ty, tz) = (
        x + fx * HELI_FIN_SHIFT,
        y + fy * HELI_FIN_SHIFT,
        z + HELI_TAIL_ROTOR_Z,
    );
    push_beam(
        lines,
        tx,
        ty - tr * tc,
        tz - tr * ts,
        tx,
        ty + tr * tc,
        tz + tr * ts,
        blade,
    );
}

/// Elevation in px of the surface that receives a helicopter shadow.
///
/// A deck fragment of a bridge shields the water below it, so a helicopter
/// crossing a bridge drops its shadow on the deck; everywhere else the
/// shadow follows the terrain (ramps included, see [`vehicle_ground_z`]).
fn helicopter_shadow_z(game: &Game, x: f64, y: f64) -> f64 {
    if let Some(tile) = game.board.world_to_tile(x, y)
        && let Some(bi) = game.board.tiles.get(&tile).and_then(|t| t.bridge)
        && let Some(bridge) = game.board.bridges.get(bi)
    {
        return bridge.w as f64 * constants::ELEVATION_PX + constants::BRIDGE_DECK_LIFT;
    }
    vehicle_ground_z(game, x, y)
}

/// Translucent shadow disc of a helicopter, drawn below its hull.
///
/// The flight altitude does not follow the terrain (rules.md section 5.2),
/// so the isometric view alone cannot tell which tile a helicopter is over;
/// the dark decal marks it (specification.md, section "Grafika i interfejs
/// użytkownika"). It is one flat disc lifted by [`OBSTACLE_LIFT`] so that it
/// does not z-fight with the receiving surface, and it goes into the
/// translucent pass: the GPU depth test keeps it from darkening the hull,
/// other vehicles or nearer cliffs, and the terrain under the helicopter
/// keeps its own colour everywhere else.
fn push_helicopter_shadow(game: &Game, v: &crate::entities::Vehicle, shadow: &mut RangeSoup) {
    let z = helicopter_shadow_z(game, v.x, v.y) + constants::OBSTACLE_LIFT;
    push_range_disc(
        shadow,
        v.x,
        v.y,
        z,
        constants::SHADOW_RADIUS,
        constants::SHADOW_SEGMENTS,
        constants::SHADOW_COLOR,
        constants::SHADOW_ALPHA,
    );
}

// ---------------------------------------------------------------------------
// Tank rendering (rules.md section 5.1; every value is a rendering-only size
// in px, exactly like the other mesh dimensions -- colours come from the
// owning player, not from here).
// ---------------------------------------------------------------------------

/// Length of the tank hull along its heading in px.
const TANK_HULL_LEN: f64 = 22.0;
/// Width of the tank hull across its heading in px.
const TANK_HULL_WID: f64 = 13.0;
/// Height of the lower hull box in px.
const TANK_HULL_H: f64 = 5.0;
/// Ground clearance of the hull bottom in px (the tracks touch the ground).
const TANK_HULL_LIFT: f64 = 1.0;
/// Length of the upper deck box in px.
const TANK_DECK_LEN: f64 = 15.0;
/// Width of the upper deck box in px.
const TANK_DECK_WID: f64 = 10.0;
/// Height of the upper deck box in px (it sits on the lower hull).
const TANK_DECK_H: f64 = 3.0;
/// Rearward shift of the deck centre in px (the glacis eats the front).
const TANK_DECK_SHIFT: f64 = -1.0;
/// Height of the glacis lip at its front edge in px (just above the hull).
const TANK_GLACIS_LIP: f64 = 0.5;
/// Length of one track in px (it overhangs the hull front and rear).
const TANK_TRACK_LEN: f64 = 26.0;
/// Width of one track in px.
const TANK_TRACK_WID: f64 = 4.5;
/// Height of one track in px (deliberately taller than the lower hull).
const TANK_TRACK_H: f64 = 6.0;
/// Distance of each track centre from the hull centre line in px.
const TANK_TRACK_OFFSET: f64 = 6.5;
/// Tread strokes painted across the top of each track.
const TANK_TREAD_MARKS: usize = 4;
/// Base radius of the rotating turret body in px.
const TANK_TURRET_R: f64 = 7.5;
/// Roof radius of the turret body in px (slightly tapered).
const TANK_TURRET_TOP_R: f64 = 6.5;
/// Height of the turret body in px.
const TANK_TURRET_H: f64 = 4.5;
/// Facets of the turret cylinder (low-poly, flat-shaded look).
const TANK_TURRET_SEGMENTS: usize = 8;
/// Radius of the commander cupola in px.
const TANK_CUPOLA_R: f64 = 2.6;
/// Roof radius of the commander cupola in px.
const TANK_CUPOLA_TOP_R: f64 = 2.2;
/// Height of the commander cupola in px.
const TANK_CUPOLA_H: f64 = 2.0;
/// Offset of the cupola towards the turret rear in px.
const TANK_CUPOLA_SHIFT: f64 = -2.5;
/// Facets of the cupola cylinder.
const TANK_CUPOLA_SEGMENTS: usize = 6;
/// Length of the rear turret bustle (stowage) box in px.
const TANK_BUSTLE_LEN: f64 = 5.0;
/// Width of the rear turret bustle box in px.
const TANK_BUSTLE_WID: f64 = 8.0;
/// Height of the rear turret bustle box in px.
const TANK_BUSTLE_H: f64 = 2.5;
/// Distance from the turret centre where the barrel starts in px; the rear
/// end hides inside the turret, so no gap opens when the turret rotates.
const TANK_BARREL_GAP: f64 = 3.0;
/// Length of the gun barrel in px.
const TANK_BARREL_LEN: f64 = 18.0;
/// Width of the gun barrel in px.
const TANK_BARREL_WID: f64 = 3.0;
/// Height of the gun barrel in px.
const TANK_BARREL_H: f64 = 3.0;
/// Height of the barrel axis above the turret base in px.
const TANK_BARREL_LIFT: f64 = 1.4;
/// Length of the muzzle brake at the barrel tip in px.
const TANK_MUZZLE_LEN: f64 = 3.5;
/// Width of the muzzle brake in px.
const TANK_MUZZLE_WID: f64 = 5.0;
/// Height of the muzzle brake in px.
const TANK_MUZZLE_H: f64 = 3.6;
/// Height of the whip antenna stroke above the turret roof in px.
const TANK_ANTENNA_H: f64 = 8.0;
/// Reach of the barrel tip from the vehicle centre in px (derived: barrel
/// gap + barrel length + muzzle length).
const TANK_BARREL_REACH: f64 = TANK_BARREL_GAP + TANK_BARREL_LEN + TANK_MUZZLE_LEN;

/// Chassis shared by tanks and buffers (rules.md sections 5.1 and 5.4: a
/// buffer drives exactly like a tank, only its gun is replaced by the
/// healing gear the caller adds on top).
///
/// Draws two tracks with tread strokes, the lower hull, the upper deck and
/// the sloped glacis plate, all rotated into the chassis heading `(fx, fy)`.
/// Returns the rendered elevation of the deck, i.e. the surface the turret
/// (or the buffer's cross) stands on.
#[allow(clippy::too_many_arguments)]
fn push_tank_chassis(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
    x: f64,
    y: f64,
    z: f64,
    color: [u8; 3],
    fx: f64,
    fy: f64,
) -> f64 {
    let (px, py) = (-fy, fx);
    let dark = constants::shade(color, 0.7);
    let tread = constants::shade(color, 0.35);
    for side in [-1.0, 1.0] {
        let ox = x + px * side * TANK_TRACK_OFFSET;
        let oy = y + py * side * TANK_TRACK_OFFSET;
        push_oriented_box(
            mesh,
            ox,
            oy,
            z,
            TANK_TRACK_LEN,
            TANK_TRACK_WID,
            TANK_TRACK_H,
            fx,
            fy,
            constants::shade(color, 0.5),
        );
        // Tread strokes lie across the track top, clear of both ends.
        let hw = TANK_TRACK_WID / 2.0;
        for k in 0..TANK_TREAD_MARKS {
            let along =
                ((k as f64 + 0.5) / TANK_TREAD_MARKS as f64 * 2.0 - 1.0) * TANK_TRACK_LEN * 0.36;
            push_beam(
                lines,
                ox + fx * along - px * side * hw,
                oy + fy * along - py * side * hw,
                z + TANK_TRACK_H + 0.15,
                ox + fx * along + px * side * hw,
                oy + fy * along + py * side * hw,
                z + TANK_TRACK_H + 0.15,
                tread,
            );
        }
    }
    let hull_z = z + TANK_HULL_LIFT;
    push_oriented_box(
        mesh,
        x,
        y,
        hull_z,
        TANK_HULL_LEN,
        TANK_HULL_WID,
        TANK_HULL_H,
        fx,
        fy,
        color,
    );
    let hull_top = hull_z + TANK_HULL_H;
    push_oriented_box(
        mesh,
        x + fx * TANK_DECK_SHIFT,
        y + fy * TANK_DECK_SHIFT,
        hull_top,
        TANK_DECK_LEN,
        TANK_DECK_WID,
        TANK_DECK_H,
        fx,
        fy,
        dark,
    );
    // Sloped glacis joining the hull front edge with the deck front edge.
    let hull_front = TANK_HULL_LEN / 2.0;
    let deck_front = TANK_DECK_SHIFT + TANK_DECK_LEN / 2.0;
    push_oriented_slope(
        mesh,
        x + fx * ((hull_front + deck_front) / 2.0),
        y + fy * ((hull_front + deck_front) / 2.0),
        hull_top,
        (hull_front - deck_front).max(0.5),
        TANK_HULL_WID,
        TANK_DECK_H,
        TANK_GLACIS_LIP,
        fx,
        fy,
        color,
    );
    hull_top + TANK_DECK_H
}

/// Detailed tank (rules.md section 5.1): the shared chassis plus a faceted
/// turret, a commander cupola, a rear bustle, a whip antenna and a gun
/// barrel that follows the current target.
///
/// The chassis turns with the travel heading while the turret and its barrel
/// turn independently towards the aim direction (see [`tank_aim`]), so a tank
/// driving north and fighting an enemy to the east shows its hull side-on
/// with the gun trained east.
#[allow(clippy::too_many_arguments)]
fn push_tank(
    game: &Game,
    v: &crate::entities::Vehicle,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
    x: f64,
    y: f64,
    z: f64,
    color: [u8; 3],
) {
    let (fx, fy) = vehicle_heading(game, v);
    let (ax, ay) = tank_aim(game, v, (fx, fy));
    push_tank_oriented(mesh, lines, x, y, z, color, fx, fy, ax, ay);
}

/// Tank parts in an explicit chassis/aim frame (unit tests drive this).
#[allow(clippy::too_many_arguments)]
fn push_tank_oriented(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(LineVertex, LineVertex)>,
    x: f64,
    y: f64,
    z: f64,
    color: [u8; 3],
    fx: f64,
    fy: f64,
    ax: f64,
    ay: f64,
) {
    let deck_top = push_tank_chassis(mesh, lines, x, y, z, color, fx, fy);
    let (tx, ty) = (-ay, ax);
    let dark = constants::shade(color, 0.7);
    let darker = constants::shade(color, 0.5);
    // Turret: one solid faceted cylinder, so the roof keeps a clean outline
    // while the gun swings around it.
    push_cylinder(
        mesh,
        x,
        y,
        deck_top,
        TANK_TURRET_R,
        TANK_TURRET_TOP_R,
        TANK_TURRET_H,
        TANK_TURRET_SEGMENTS,
        color,
    );
    let turret_top = deck_top + TANK_TURRET_H;
    // Rear bustle plus a cupola offset to the same side: both rotate with
    // the gun, so the turret never looks symmetric.
    let bustle_back = TANK_TURRET_TOP_R + TANK_BUSTLE_LEN / 2.0 - 1.0;
    push_oriented_box(
        mesh,
        x - ax * bustle_back,
        y - ay * bustle_back,
        deck_top + 0.8,
        TANK_BUSTLE_LEN,
        TANK_BUSTLE_WID,
        TANK_BUSTLE_H,
        ax,
        ay,
        dark,
    );
    push_cylinder(
        mesh,
        x + ax * TANK_CUPOLA_SHIFT,
        y + ay * TANK_CUPOLA_SHIFT,
        turret_top,
        TANK_CUPOLA_R,
        TANK_CUPOLA_TOP_R,
        TANK_CUPOLA_H,
        TANK_CUPOLA_SEGMENTS,
        dark,
    );
    // Whip antenna on the turret roof, drawn as a stroke like the
    // helicopter rotor: thin parts need no depth fighting on the GPU path.
    push_beam(
        lines,
        x - ax * 4.5 + tx * 3.0,
        y - ay * 4.5 + ty * 3.0,
        turret_top,
        x - ax * 4.5 + tx * 3.0,
        y - ay * 4.5 + ty * 3.0,
        turret_top + TANK_ANTENNA_H,
        darker,
    );
    // Gun: the barrel starts inside the turret (so no gap opens at any
    // turret angle) and ends with a wider muzzle brake.
    let barrel_z = deck_top + TANK_BARREL_LIFT;
    let barrel_mid = TANK_BARREL_GAP + TANK_BARREL_LEN / 2.0;
    push_oriented_box(
        mesh,
        x + ax * barrel_mid,
        y + ay * barrel_mid,
        barrel_z,
        TANK_BARREL_LEN,
        TANK_BARREL_WID,
        TANK_BARREL_H,
        ax,
        ay,
        dark,
    );
    let muzzle_mid = TANK_BARREL_REACH - TANK_MUZZLE_LEN / 2.0;
    push_oriented_box(
        mesh,
        x + ax * muzzle_mid,
        y + ay * muzzle_mid,
        barrel_z - 0.3,
        TANK_MUZZLE_LEN,
        TANK_MUZZLE_WID,
        TANK_MUZZLE_H,
        ax,
        ay,
        darker,
    );
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
        // The first leg starts at the vehicle itself (a helicopter above the
        // terrain), the following waypoints sit on the terrain below them, so
        // an airborne route descends to the ground instead of floating.
        let mut prev = (v.x, v.y, vehicle_z(game, v));
        for t in v.route.iter().skip(v.route_index) {
            let (wx, wy) = game.board.center_world(*t);
            let wz = tile_top_z(&game.board, *t);
            lines.push((
                line_vert(prev.0, prev.1, prev.2, [255, 255, 255], 255),
                line_vert(wx, wy, wz, [255, 255, 255], 255),
            ));
            prev = (wx, wy, wz);
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
    fn helicopter_has_slender_hull_and_spinning_rotor() {
        use crate::constants::VehicleKind;
        use crate::entities::{Player, Vehicle};
        use crate::game::Game;
        let mut board = Board::new(8, 8);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let (sx, sy) = crate::hexgrid::hex_to_world(3, 3, board.side);
        let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
        game.vehicles.push(Vehicle::new(
            VehicleKind::Helicopter,
            0,
            10.0,
            Vec::new(),
            (sx, sy),
            None,
        ));
        let mut first = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut first);
        // Opaque hull + canopy + tail boom + fin + skid rails + mast: clearly
        // more than the old single flat disc (12 triangles = 36 vertices).
        assert!(
            first.opaque.vertices.len() > 36,
            "helicopter body has no details: {} vertices",
            first.opaque.vertices.len()
        );
        // Four skid struts + two main-rotor strokes + tail-rotor stroke.
        assert_eq!(
            first.lines.len(),
            7,
            "rotor/tail lines: {:?}",
            first.lines.len()
        );
        // Slender hull: the airframe is much longer along the heading (east
        // for a parked helicopter) than it is wide across it.
        let (mut lo_x, mut hi_x, mut lo_y, mut hi_y) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for vert in first.opaque.vertices.iter() {
            lo_x = lo_x.min(f64::from(vert.x));
            hi_x = hi_x.max(f64::from(vert.x));
            lo_y = lo_y.min(f64::from(vert.y));
            hi_y = hi_y.max(f64::from(vert.y));
        }
        let (long, wide) = (hi_x - lo_x, hi_y - lo_y);
        assert!(long > 1.6 * wide, "hull not slender: {long} x {wide}");
        // Two main-rotor blades are the highest strokes: they span the full
        // rotor diameter and their midpoint is the mast above the hull.
        let blades = |mesh: &DynamicMesh| -> Vec<(LineVertex, LineVertex)> {
            let top = mesh
                .lines
                .iter()
                .map(|(a, _)| f64::from(a.z))
                .fold(f64::NEG_INFINITY, f64::max);
            mesh.lines
                .iter()
                .filter(|(a, b)| {
                    (f64::from(a.z) - top).abs() < 1e-6 && (f64::from(b.z) - top).abs() < 1e-6
                })
                .copied()
                .collect()
        };
        let first_blades = blades(&first);
        assert_eq!(first_blades.len(), 2, "expected two main-rotor blades");
        for (a, b) in first_blades.iter() {
            let len = (f64::from(a.x) - f64::from(b.x)).hypot(f64::from(a.y) - f64::from(b.y));
            assert!(
                (len - 2.0 * HELI_ROTOR_R).abs() < 1e-3,
                "rotor blade length {len}"
            );
            let (mx, my) = (f64::from(a.x + b.x) / 2.0, f64::from(a.y + b.y) / 2.0);
            assert!(
                (mx - sx).abs() < 1e-3 && (my - sy).abs() < 1e-3,
                "rotor mast off centre: {mx},{my}"
            );
        }
        // Advancing the phase rotates the main blades.
        let mut second = DynamicMesh::default();
        build_dynamic(&game, 0.7, &mut second);
        assert_eq!(second.lines.len(), 7);
        let endpoints = |mesh: &DynamicMesh| -> Vec<(f32, f32, f32, f32)> {
            blades(mesh)
                .iter()
                .map(|(a, b)| (a.x, a.y, b.x, b.y))
                .collect()
        };
        assert_ne!(
            endpoints(&first),
            endpoints(&second),
            "rotor does not spin with the phase"
        );
    }

    #[test]
    fn helicopter_tail_trails_behind_flight_heading() {
        use crate::constants::VehicleKind;
        use crate::entities::{Player, Vehicle};
        use crate::game::Game;
        let mut board = Board::new(10, 10);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let start = (2, 2);
        let dest = (6, 2);
        let (sx, sy) = hexgrid::hex_to_world(start.0, start.1, board.side);
        let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
        game.vehicles.push(Vehicle::new(
            VehicleKind::Helicopter,
            0,
            10.0,
            vec![dest],
            (sx, sy),
            Some(start),
        ));
        let (vx, vy) = (game.vehicles[0].x, game.vehicles[0].y);
        let (fx, fy) = super::vehicle_heading(&game, &game.vehicles[0]);
        let (wx, wy) = game.board.center_world(dest);
        let (dx, dy) = (wx - vx, wy - vy);
        let len = (dx * dx + dy * dy).sqrt();
        assert!(
            (fx - dx / len).abs() < 1e-9 && (fy - dy / len).abs() < 1e-9,
            "heading {fx},{fy} misses waypoint {dx},{dy}"
        );
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        // Distance of a stroke midpoint along the heading.
        let along = |p: &(LineVertex, LineVertex)| {
            let (mx, my) = (
                f64::from(p.0.x + p.1.x) / 2.0,
                f64::from(p.0.y + p.1.y) / 2.0,
            );
            (mx - vx) * fx + (my - vy) * fy
        };
        // The tail rotor is the rearmost stroke: it trails the hull.
        let tail = dynamic
            .lines
            .iter()
            .min_by(|a, b| along(a).partial_cmp(&along(b)).unwrap())
            .expect("tail rotor stroke");
        assert!(
            along(tail) < -10.0,
            "tail not behind the flight heading: {}",
            along(tail)
        );
        // The glazed cockpit (identified by its fixed tint) sits ahead and on
        // top of the hull instead of sticking out as a white box.
        let canopy: Vec<&GpuVertex> = dynamic
            .opaque
            .vertices
            .iter()
            .filter(|vert| vert.color == HELI_CANOPY_COLOR)
            .collect();
        assert!(!canopy.is_empty(), "no cockpit canopy found");
        let n = canopy.len() as f64;
        let (ccx, ccy, ccz) = (
            canopy.iter().map(|p| f64::from(p.x)).sum::<f64>() / n,
            canopy.iter().map(|p| f64::from(p.y)).sum::<f64>() / n,
            canopy.iter().map(|p| f64::from(p.z)).sum::<f64>() / n,
        );
        let forward = (ccx - vx) * fx + (ccy - vy) * fy;
        assert!(
            forward > 0.0,
            "cockpit {ccx},{ccy} not ahead of heading {fx},{fy}"
        );
        assert!(
            forward - along(tail) > 15.0,
            "cockpit not ahead of the tail"
        );
        let hull_top = vehicle_z(&game, &game.vehicles[0]) + HELI_MAST_BASE;
        assert!(
            ccz > hull_top - HELI_HULL_H && ccz < hull_top + HELI_CANOPY_H,
            "canopy {ccz} is not on the hull top {hull_top}"
        );
    }

    #[test]
    fn helicopter_keeps_its_altitude_and_shadows_the_tile_below() {
        use crate::constants::VehicleKind;
        use crate::entities::{Player, Vehicle};
        use crate::game::Game;
        // A board with a low tile and a hill: a helicopter must fly at the
        // same altitude over both, above the highest terrain, while its
        // shadow stays on the receiving surface right below it.
        let mut board = Board::new(10, 10);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let hill = (7, 7);
        board.tiles.get_mut(&hill).unwrap().height = 6;
        let low = (2, 2);
        let (lx, ly) = hexgrid::hex_to_world(low.0, low.1, board.side);
        let (hx, hy) = hexgrid::hex_to_world(hill.0, hill.1, board.side);
        let tank_board = board.clone();
        let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
        for pos in [(lx, ly), (hx, hy)] {
            game.vehicles.push(Vehicle::new(
                VehicleKind::Helicopter,
                0,
                10.0,
                Vec::new(),
                pos,
                Some(low),
            ));
        }
        let alt = vehicle_z(&game, &game.vehicles[0]);
        let hill_top = 6.0 * constants::ELEVATION_PX;
        assert!(
            alt >= hill_top + constants::HELICOPTER_ALTITUDE_PX - 1e-6,
            "helicopter at {alt} does not clear the hill at {hill_top}"
        );
        assert_eq!(
            alt,
            vehicle_z(&game, &game.vehicles[1]),
            "altitude follows the terrain instead of staying fixed"
        );
        // The shadow lands on the surface under the helicopter: on the low
        // tile far below the hull, on the hill closer to it.
        let low_shadow = helicopter_shadow_z(&game, lx, ly) + constants::OBSTACLE_LIFT;
        let hill_shadow = helicopter_shadow_z(&game, hx, hy) + constants::OBSTACLE_LIFT;
        assert!((low_shadow - (constants::ELEVATION_PX + constants::OBSTACLE_LIFT)).abs() < 1e-9);
        assert!((hill_shadow - (hill_top + constants::OBSTACLE_LIFT)).abs() < 1e-9);
        assert!(
            low_shadow < alt && hill_shadow < alt,
            "shadow {low_shadow}/{hill_shadow} not below the hull {alt}"
        );
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        // One dark translucent disc per helicopter, 14 segments each.
        assert_eq!(
            dynamic.shadow.vertices.len(),
            2 * constants::SHADOW_SEGMENTS * 3,
            "helicopter shadows: {}",
            dynamic.shadow.vertices.len()
        );
        for vert in dynamic.shadow.vertices.iter() {
            assert_eq!(&vert.color[..3], &constants::SHADOW_COLOR[..]);
            assert_eq!(vert.color[3], constants::SHADOW_ALPHA);
        }
        let shadow_z: Vec<f64> = dynamic
            .shadow
            .vertices
            .iter()
            .map(|vert| f64::from(vert.z))
            .collect();
        assert!(
            shadow_z.contains(&low_shadow) && shadow_z.contains(&hill_shadow),
            "shadows not on the receiving surfaces: {shadow_z:?}"
        );
        // Ground vehicles hug the terrain and get no shadow disc.
        let mut tank_game = Game::new(tank_board, vec![Player::new(0, true)], Vec::new(), 1);
        tank_game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            10.0,
            Vec::new(),
            (lx, ly),
            None,
        ));
        let mut tank_mesh = DynamicMesh::default();
        build_dynamic(&tank_game, 0.0, &mut tank_mesh);
        assert!(
            tank_mesh.shadow.vertices.is_empty(),
            "a ground vehicle needs no shadow disc"
        );
    }

    #[test]
    fn tank_gun_trains_on_the_fought_enemy() {
        use crate::constants::VehicleKind;
        use crate::entities::{Player, Vehicle};
        use crate::game::Game;
        let mut board = Board::new(12, 12);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        // Tank driving north, enemy to the east: the chassis must keep
        // facing north while the gun turns east onto the duel target.
        let start = (3, 6);
        let north = (3, 5);
        let (sx, sy) = hexgrid::hex_to_world(start.0, start.1, board.side);
        let mut game = Game::new(
            board,
            vec![Player::new(0, true), Player::new(1, false)],
            Vec::new(),
            1,
        );
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            30.0,
            vec![north],
            (sx, sy),
            Some(start),
        ));
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            1,
            30.0,
            Vec::new(),
            (sx + 120.0, sy),
            None,
        ));
        game.vehicles[0].combat_target = Some(game.vehicles[1].id);
        let (vx, vy) = (game.vehicles[0].x, game.vehicles[0].y);
        let (fx, fy) = super::vehicle_heading(&game, &game.vehicles[0]);
        assert!(fx.abs() < 1e-9 && fy < -0.999, "chassis heading {fx},{fy}");
        let (ax, ay) = super::tank_aim(&game, &game.vehicles[0], (fx, fy));
        assert!(ax > 0.999 && ay.abs() < 1e-9, "gun aim {ax},{ay}");
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        // Keep only the parts of the first tank: the enemy sits 120 px away
        // and its own barrel stays outside this radius.
        let (mut along_gun, mut across_gun) = (f64::NEG_INFINITY, 0.0f64);
        for vtx in dynamic.opaque.vertices.iter() {
            let (dx, dy) = (f64::from(vtx.x) - vx, f64::from(vtx.y) - vy);
            if dx * dx + dy * dy > 60.0 * 60.0 {
                continue;
            }
            along_gun = along_gun.max(dx * ax + dy * ay);
            across_gun = across_gun.max((dx * -ay + dy * ax).abs());
        }
        // The muzzle brake ends TANK_BARREL_REACH along the gun axis...
        assert!(
            (along_gun - super::TANK_BARREL_REACH).abs() < 0.05,
            "barrel reach {along_gun} vs {}",
            super::TANK_BARREL_REACH
        );
        // ...while the chassis runs across it: the 26 px long tracks stay
        // perpendicular to the barrel, so the hull is seen side-on.
        assert!(
            across_gun > 12.5,
            "chassis did not follow the travel heading: {across_gun}"
        );
    }

    #[test]
    fn tank_gun_follows_walls_and_travel_heading() {
        use crate::constants::VehicleKind;
        use crate::entities::{Player, Vehicle};
        use crate::game::Game;
        let mut board = Board::new(12, 12);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let start = (3, 6);
        let north = (3, 5);
        let wall_tile = (6, 6);
        let (sx, sy) = hexgrid::hex_to_world(start.0, start.1, board.side);
        let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            30.0,
            vec![north],
            (sx, sy),
            Some(start),
        ));
        let heading = (0.0, -1.0);
        // Nothing in range: the gun rests along the chassis heading.
        let (ax, ay) = super::tank_aim(&game, &game.vehicles[0], heading);
        assert_eq!((ax, ay), heading);
        // Shelling a wall (rules.md section 4): the gun turns onto it.
        game.vehicles[0].wall_target = Some(wall_tile);
        let (wx, wy) = game.board.center_world(wall_tile);
        let (vx, vy) = (game.vehicles[0].x, game.vehicles[0].y);
        let (dx, dy) = (wx - vx, wy - vy);
        let len = (dx * dx + dy * dy).sqrt();
        let (ax, ay) = super::tank_aim(&game, &game.vehicles[0], heading);
        assert!(
            (ax - dx / len).abs() < 1e-9 && (ay - dy / len).abs() < 1e-9,
            "wall aim {ax},{ay} vs {dx},{dy}"
        );
        assert!(ax > 0.5, "gun did not turn east onto the wall: {ax}");
    }

    #[test]
    fn tank_parts_stack_on_the_chassis_with_details() {
        use crate::constants::VehicleKind;
        use crate::entities::{Player, Vehicle};
        use crate::game::Game;
        let mut board = Board::new(10, 10);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let (sx, sy) = hexgrid::hex_to_world(4, 4, board.side);
        let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            30.0,
            Vec::new(),
            (sx, sy),
            None,
        ));
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        // Chassis (two tracks, hull, deck, glacis) plus turret, cupola,
        // bustle, barrel and muzzle brake: far more than the two plain
        // boxes the tank used to be (36 vertices).
        assert!(
            dynamic.opaque.vertices.len() > 240,
            "tank lost its details: {} vertices",
            dynamic.opaque.vertices.len()
        );
        // One tread stroke per mark on each track, plus the whip antenna.
        assert_eq!(
            dynamic.lines.len(),
            2 * super::TANK_TREAD_MARKS + 1,
            "tread/antenna strokes: {}",
            dynamic.lines.len()
        );
        // The cupola roof is the highest opaque point: turret on the deck,
        // cupola on the turret roof, all above the ground the tank stands on.
        let ground = vehicle_ground_z(&game, sx, sy);
        let top = dynamic
            .opaque
            .vertices
            .iter()
            .map(|v| f64::from(v.z))
            .fold(f64::NEG_INFINITY, f64::max);
        let roof = ground
            + super::TANK_HULL_LIFT
            + super::TANK_HULL_H
            + super::TANK_DECK_H
            + super::TANK_TURRET_H
            + super::TANK_CUPOLA_H;
        assert!((top - roof).abs() < 0.05, "roof {top} vs {roof}");
        // The tracks stick out past the hull, so the chassis is wider than
        // the deck and the tank does not read as one flat block.
        let mut wide = 0.0f64;
        for vtx in dynamic.opaque.vertices.iter() {
            wide = wide.max((f64::from(vtx.x) - sx).abs());
        }
        assert!(
            wide > super::TANK_HULL_LEN / 2.0,
            "tracks do not overhang the hull: {wide}"
        );
    }

    #[test]
    fn buffer_shares_the_tank_chassis_without_a_gun() {
        use crate::constants::VehicleKind;
        use crate::entities::{Player, Vehicle};
        use crate::game::Game;
        let mut board = Board::new(10, 10);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let (sx, sy) = hexgrid::hex_to_world(4, 4, board.side);
        let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
        game.vehicles.push(Vehicle::new(
            VehicleKind::Buffer,
            0,
            30.0,
            Vec::new(),
            (sx, sy),
            None,
        ));
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        // The healing cross (rules.md section 5.4) marks the support role
        // and floats where a tank's turret would be, not above the whole
        // vehicle twice over.
        let cross_z = dynamic
            .lines
            .iter()
            .find(|(a, _)| a.color[..3] == [130, 235, 140])
            .map(|(a, _)| f64::from(a.z))
            .expect("buffer has no healing cross");
        let ground = vehicle_ground_z(&game, sx, sy);
        let expected =
            ground + super::TANK_HULL_LIFT + super::TANK_HULL_H + super::TANK_DECK_H + 4.0;
        assert!(
            (cross_z - expected).abs() < 0.05,
            "cross at {cross_z}, expected {expected}"
        );
        // No gun: nothing reaches past the chassis, unlike a tank barrel.
        let mut reach = 0.0f64;
        for vtx in dynamic.opaque.vertices.iter() {
            reach = reach.max((f64::from(vtx.x) - sx).abs());
        }
        assert!(
            reach < super::TANK_BARREL_REACH - 5.0,
            "buffer grew a barrel: {reach}"
        );
    }

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
