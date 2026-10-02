//! Static terrain geometry: field tops, cliff skirts, ramps, bridge decks and
//! the bridge shadows they cast, built once per board into [`TerrainMesh`].

use super::{
    GpuVertex, RangeSoup, TriangleSoup, grow_bbox, push_box, push_quad, push_range_rect, push_tri,
    vert,
};
use crate::board::{Board, Bridge};
use crate::constants;
use crate::hexgrid::{self, Tile};

/// Maximum vertices per GPU chunk: macroquad batches draw calls into
/// `u16` index buffers, so large terrains are split into chunks. It is the
/// draw-call limit itself, so one chunk is one draw call.
pub(super) const CHUNK_VERTICES: usize = constants::DRAW_BATCH_VERTICES;

/// Tiles per chunk edge: the terrain is split into spatial chunks so view
/// culling can skip whole regions (16 x 16 tiles stays well below
/// [`CHUNK_VERTICES`] per chunk).
pub(super) const CHUNK_TILES: i32 = 16;

/// One spatially bounded terrain chunk; the renderer culls it by `bbox`.
#[derive(Clone, Debug, Default)]
pub struct TerrainChunk {
    /// Opaque triangles (tops, skirts, ramps, decks).
    pub soup: TriangleSoup,
    /// Translucent bridge shadows cast on the fields below the decks.
    pub shadows: RangeSoup,
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
///
/// Bridge decks are included: they float [`constants::BRIDGE_DECK_LIFT`]
/// above the nominal height of the bridge, so a board with a bridge has to
/// cover that extra depth or the deck would be clipped by the far plane.
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
    for br in board.bridges.iter() {
        let z = bridge_deck_z(br);
        for frag in br.fragments.iter() {
            let (cx, cy) = board.center_world(*frag);
            let d = (cx + cy) * constants::ISO_SIN + z;
            lo = lo.min(d);
            hi = hi.max(d);
        }
    }
    if lo > hi { (0.0, 1.0) } else { (lo, hi) }
}

/// Highest rendered elevation of a board in px (view-culling padding and the
/// fixed helicopter altitude). A deck can sit a
/// [`crate::constants::BRIDGE_DECK_LIFT`] above a board's highest field, so
/// the lift is added whenever a bridge exists.
pub fn max_height(board: &Board) -> f64 {
    let h = board.tiles.values().map(|t| t.height).max().unwrap_or(0);
    let terrain = h as f64 * constants::ELEVATION_PX;
    if board.bridges.is_empty() {
        terrain
    } else {
        terrain + constants::BRIDGE_DECK_LIFT
    }
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

/// Base tile colour (checkerboard for land, blue for water).
pub(super) fn tile_color(tile: Tile, height: i32) -> [u8; 3] {
    if height == 0 {
        constants::WATER_COLOR
    } else if (tile.0 + tile.1) & 1 == 0 {
        constants::LAND_COLOR
    } else {
        constants::LAND_VARIANT
    }
}

/// Height rendered for a tile top (ramps sit at the lower end for picking).
pub fn tile_top_z(board: &Board, tile: Tile) -> f64 {
    if let Some((a, b)) = board.ramps.get(&tile) {
        return board.height(*a).min(board.height(*b)) as f64 * constants::ELEVATION_PX;
    }
    board.height(tile) as f64 * constants::ELEVATION_PX
}

/// Height of the ramp strip on `tile` at its centre (mid-slope).
///
/// Vehicles drive centre to centre, so a waypoint standing on the ramp sits
/// halfway between the two joined heights; see [`Board::ramp_center_z`].
/// Route lines and previews share this value, which keeps the whole travel
/// on one continuous slope instead of jumping at the tile border.
pub fn ramp_waypoint_z(board: &Board, tile: Tile) -> Option<f64> {
    board.ramp_center_z(tile)
}

/// Half-length of one deck segment along the bridge axis, as a fraction of
/// the distance between neighbouring field centres: 0.52 makes consecutive
/// fragments overlap slightly, so the deck runs from one field edge to the
/// next as one continuous causeway.
pub(super) const BRIDGE_DECK_LEN_FRAC: f64 = 0.52;
/// Half-width of a deck segment across the bridge axis, as a fraction of the
/// hex side. Much narrower than a field, so the deck reads as a bridge with a
/// visible direction instead of a second field glued onto the water.
pub(super) const BRIDGE_DECK_WID_FRAC: f64 = 0.34;

/// Rendered elevation of the driveable surface of a bridge in px.
///
/// Rules.md section 8: travelling along a bridge happens on its deck, which
/// floats [`constants::BRIDGE_DECK_LIFT`] above the nominal bridge height, so
/// the deck hides vehicles passing underneath and leaves the ones driving on
/// it visible.
pub(super) fn bridge_deck_z(br: &Bridge) -> f64 {
    br.w as f64 * constants::ELEVATION_PX + constants::BRIDGE_DECK_LIFT
}

/// One bridge deck fragment in world space (rules.md section 8).
///
/// The deck is a narrow rectangle running edge to edge along
/// [`Bridge::direction`], so a bridge reads as one straight causeway with a
/// visible direction. A plain hexagon covering the whole field would look
/// like land instead, and would hide both the water below and the way the
/// vehicles travel over it.
#[derive(Clone, Copy, Debug)]
pub(super) struct DeckQuad {
    /// Four corners, wound around the rectangle (two per long edge).
    pub corners: [(f64, f64); 4],
    /// Centre of the fragment.
    pub center: (f64, f64),
    /// Unit vector along the bridge axis.
    pub axis: (f64, f64),
    /// Half length along [`DeckQuad::axis`].
    pub half_len: f64,
    /// Half width across [`DeckQuad::axis`].
    pub half_wid: f64,
    /// Rendered elevation of the deck surface in px.
    pub z: f64,
}

/// World-space rectangle of one bridge deck fragment: the deck spans from
/// one field edge towards the next, so
/// consecutive fragments overlap slightly and the bridge has no gaps.
pub(super) fn bridge_deck_quad(board: &Board, br: &Bridge, frag: Tile) -> DeckQuad {
    let (cx, cy) = board.center_world(frag);
    let nxt = hexgrid::neighbor(frag.0, frag.1, br.direction);
    let (nx, ny) = if board.contains(nxt) {
        board.center_world(nxt)
    } else {
        board.center_world(br.b)
    };
    let (ax, ay) = (nx - cx, ny - cy);
    let len = (ax * ax + ay * ay).sqrt().max(1e-6);
    let (ux, uy) = (ax / len, ay / len);
    let (px, py) = (-uy, ux);
    let half_len = BRIDGE_DECK_LEN_FRAC * len;
    let half_wid = BRIDGE_DECK_WID_FRAC * board.side;
    DeckQuad {
        corners: [
            (
                cx + ux * half_len + px * half_wid,
                cy + uy * half_len + py * half_wid,
            ),
            (
                cx + ux * half_len - px * half_wid,
                cy + uy * half_len - py * half_wid,
            ),
            (
                cx - ux * half_len - px * half_wid,
                cy - uy * half_len - py * half_wid,
            ),
            (
                cx - ux * half_len + px * half_wid,
                cy - uy * half_len + py * half_wid,
            ),
        ],
        center: (cx, cy),
        axis: (ux, uy),
        half_len,
        half_wid,
        z: bridge_deck_z(br),
    }
}

/// Deck of one bridge fragment: a slab with a visible thickness, raised on
/// pillars over the field below.
///
/// Rules.md section 8 lets vehicles pass *under* a bridge, so the deck must
/// not be drawn as a solid block down to the ground: only the slab and a few
/// thin pillars are built, leaving the space below open and readable. The
/// narrow rectangle still shows which way the bridge runs.
pub(super) fn push_bridge_deck(board: &Board, soup: &mut TriangleSoup, br: &Bridge, frag: Tile) {
    let deck = bridge_deck_quad(board, br, frag);
    let ground = board.height(frag) as f64 * constants::ELEVATION_PX;
    // Slab top plus a short side band of `BRIDGE_DECK_THICKNESS`, so the deck
    // reads as a plate with an edge rather than a paper-thin sheet.
    let slab_z = (deck.z - constants::BRIDGE_DECK_THICKNESS).max(ground);
    let top = deck
        .corners
        .map(|(x, y)| vert(x, y, deck.z, constants::BRIDGE_DECK_COLOR));
    push_quad(soup, top[0], top[1], top[2], top[3]);
    for k in 0..4 {
        let a = deck.corners[k];
        let b = deck.corners[(k + 1) % 4];
        let p0 = vert(a.0, a.1, deck.z, constants::BRIDGE_DECK_SIDE_COLOR);
        let p1 = vert(b.0, b.1, deck.z, constants::BRIDGE_DECK_SIDE_COLOR);
        let p2 = vert(b.0, b.1, slab_z, constants::BRIDGE_DECK_SIDE_COLOR);
        let p3 = vert(a.0, a.1, slab_z, constants::BRIDGE_DECK_SIDE_COLOR);
        push_quad(soup, p0, p1, p2, p3);
    }
    // Pillars at the four corners, dropped to the field below. They carry the
    // deck visually while leaving the span open underneath.
    if slab_z - ground > 0.0 {
        for (x, y) in deck.corners.iter() {
            push_box(
                soup,
                *x,
                *y,
                ground,
                constants::BRIDGE_PILLAR_WID,
                constants::BRIDGE_PILLAR_WID,
                slab_z - ground,
                constants::BRIDGE_PILLAR_COLOR,
            );
        }
    }
}

/// Shadow cast by one bridge fragment onto the field below it.
///
/// A raised deck darkens what stands under it: the same rectangle as the deck
/// itself, projected straight down onto the surface that takes it (water or
/// low land) and lifted by [`constants::SHADOW_LIFT`] so it never fights that
/// surface for depth. Vertical projection matches the helicopter shadow
/// ([`push_helicopter_shadow`]), so the light in the scene reads as coming
/// from straight above; the colour and alpha are the shared shadow values of
/// the contract in specification.md. The shadow is static like the deck, so it
/// is built once with the terrain instead of every frame.
pub(super) fn push_bridge_shadow(board: &Board, shadows: &mut RangeSoup, br: &Bridge, frag: Tile) {
    let deck = bridge_deck_quad(board, br, frag);
    let ground = board.height(frag) as f64 * constants::ELEVATION_PX;
    let z = ground + constants::SHADOW_LIFT;
    // Only an elevated deck shades the ground; a deck flush with the field
    // (or one over water level zero that would sit inside the surface) has
    // nothing to darken.
    if deck.z - ground > constants::SHADOW_LIFT {
        push_range_rect(
            shadows,
            deck.center.0,
            deck.center.1,
            z,
            2.0 * deck.half_len,
            2.0 * deck.half_wid,
            deck.axis.0,
            deck.axis.1,
            constants::SHADOW_COLOR,
            constants::SHADOW_ALPHA,
        );
    }
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
    // chunk of that tile (a bridge may span more than one chunk). The shadow
    // it casts on the field below goes into the same chunk.
    for br in board.bridges.iter() {
        for f in br.fragments.iter().copied() {
            let key = (f.0 / CHUNK_TILES, f.1 / CHUNK_TILES);
            let Some(idx) = groups.keys().position(|k| *k == key) else {
                continue;
            };
            push_bridge_deck(board, &mut mesh.chunks[idx].soup, br, f);
            push_bridge_shadow(board, &mut mesh.chunks[idx].shadows, br, f);
        }
    }
    debug_assert!(
        mesh.chunks
            .iter()
            .all(|c| c.shadows.vertices.len() <= CHUNK_VERTICES),
        "bridge shadows exceed the u16 draw batch"
    );
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

/// Rendered ramp: a narrower earth-coloured strip across the tile, with no
/// arrows painted on it.
///
/// The strip runs edge to edge along the a->b axis of the ramp, its two short
/// edges sitting on the midpoints of the hex edges that face the two joined
/// neighbours, at those neighbours' heights. The long edges stay parallel to
/// the a->b axis, so the tilt of the top face is proportional to the height
/// difference of the joined tiles (a ramp between two tiles of equal height is
/// flat, even though the game rules forbid it). The body is solid: both sides
/// are filled from the tilted top edges down to the base elevation in a darker
/// earth tone, so no empty space is visible underneath.
///
/// Rendering it as a full hexagon would hide the two edges of the tile that
/// still carry ordinary terrain, and the strip is what tells the player which
/// way a vehicle can leave this tile.
fn push_ramp(board: &Board, soup: &mut TriangleSoup, tile: Tile) {
    let (a, b) = match board.ramps.get(&tile) {
        Some(v) => *v,
        None => return,
    };
    // The strip runs edge to edge, its short edges lying on the midpoints of
    // the hex edges facing the two joined neighbours, tilted by their
    // height difference. The edge lookup is shared with the drive height
    // (Board::ramp_edges), so vehicles interpolate along the very strip
    // drawn here.
    let (edge_a, edge_b) = match board.ramp_edges(tile) {
        Some(e) => e,
        None => return,
    };
    let ha = board.height(a) as f64 * constants::ELEVATION_PX;
    let hb = board.height(b) as f64 * constants::ELEVATION_PX;
    let (dx, dy) = (edge_b.0 - edge_a.0, edge_b.1 - edge_a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (ux, uy) = (dx / len, dy / len);
    // Strip half-width: just under half the hex side, so the strip runs
    // edge to edge without spilling past the hex.
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
