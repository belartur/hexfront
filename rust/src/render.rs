//! Isometric renderer: every graphic is drawn from code (no raster assets).
//!
//! Objects of individual players differ by colour (specification.md,
//! section "Grafika i interfejs uzytkownika"); all mesh builders accept
//! the colour as an argument so swapping in raster art later stays easy.
//!
//! The scene is built from GPU triangle meshes ([`crate::mesh`]) projected
//! by the orthographic [`crate::iso`] camera; occlusion is resolved by the
//! hardware depth buffer. UI text is drawn on top with macroquad's
//! built-in font (see [`crate::app`]).
//!
//! # Pass order
//!
//! [`Renderer::draw_gpu`] walks a fixed list of passes. Only the opaque ones
//! write depth, so the list is what decides what may cover what:
//!
//! 1. opaque terrain chunks (tile tops, cliff skirts, ramps, bridge decks),
//! 2. translucent bridge shadows on the fields below the decks,
//! 3. terrain grid lines,
//! 4. opaque dynamic objects (buildings, obstacles, vehicles, projectiles),
//! 5. translucent helicopter shadows,
//! 6. translucent explosion particles,
//! 7. 3D strokes (routes, detail lines),
//! 8. range fills, composited from offscreen masks,
//! 9. range outlines,
//! 10. 2D overlays (selection outline, hovered route preview),
//! 11. text.
//!
//! Coplanar fragments are resolved by this order: terrain first, then details
//! and outlines, so a stroke never z-fights with the surface it lies on. No
//! depth key is ever fudged and no pass is repeated at the end to force ramps
//! or badges in front of everything.
//!
//! The three translucent vehicle-shadow passes (2, 5) and the particle pass
//! (6) draw without a depth write, so they are painted from the farthest to
//! the nearest translucent fragment and never dim what stands in front of
//! them. The two range passes (8, 9) run with the depth test switched off
//! entirely, so nothing can hide a range.
//!
//! # Why the ranges need their own buffers
//!
//! Range fills and range outlines live apart from the ordinary geometry
//! ([`crate::mesh::DynamicMesh::range_turret`], `range_heal`, `range_lines`)
//! only because of the two passes above: the fills are coverage masks that get
//! composited once, and the outlines have to land *after* that composite so a
//! stroke stays readable where ranges overlap. See [`Renderer::draw_range_fills`].

use crate::camera::Camera;
use crate::constants;
use crate::game::Game;
use crate::hexgrid::Tile;

/// Draws a whole game state with the GPU.
///
/// The rotor animation phase ([`Renderer::rotor_phase`]) is shared by the
/// airframe blades and the rotor blades of the shadow
/// ([`crate::mesh::build_dynamic`]), so a helicopter and its shadow always
/// spin together; [`crate::app`] advances it by
/// [`crate::constants::ROTOR_SPIN_RAD_PER_S`] every frame.
pub struct Renderer {
    /// Phase of the helicopter rotor animation.
    pub rotor_phase: f64,
    /// Converted static terrain buffers, uploaded once per level.
    terrain: Option<GpuTerrain>,
    /// Offscreen masks holding the range fills, see
    /// [`Renderer::draw_range_fills`].
    range_masks: Option<RangeMasks>,
}

/// The two offscreen masks range fills are rendered into.
///
/// Turret ranges and heal ranges are masked separately and composited one
/// after the other, so a pixel covered by two ranges of the **same** kind
/// keeps the coverage of a single range, while a turret range overlapping a
/// heal range shows both tints at once. The contract asks for exactly this:
/// overlapping ranges of one kind must not stack their transparency.
struct RangeMasks {
    /// Mask of the turret range discs.
    turret: macroquad::prelude::RenderTarget,
    /// Mask of the heal (tower and buffer) range discs.
    heal: macroquad::prelude::RenderTarget,
    /// Window size the masks were allocated for, so a resize recreates them.
    width: u32,
    /// Window size the masks were allocated for, so a resize recreates them.
    height: u32,
}

/// Static terrain already converted into macroquad meshes.
struct GpuTerrain {
    /// One draw batch per terrain chunk plus its world bounding box.
    chunks: Vec<(macroquad::models::Mesh, (f64, f64, f64, f64))>,
    /// Translucent bridge shadows, one batch per terrain chunk.
    shadows: Vec<(macroquad::models::Mesh, (f64, f64, f64, f64))>,
    /// Grid strokes of every chunk plus its world bounding box.
    grids: Vec<(macroquad::models::Mesh, (f64, f64, f64, f64))>,
}

impl Renderer {
    /// Create a renderer.
    pub fn new() -> Self {
        Self {
            rotor_phase: 0.0,
            terrain: None,
            range_masks: None,
        }
    }
    /// Convert a freshly built terrain mesh into GPU buffers.
    ///
    /// Called by [`crate::app`] right after rebuilding the static terrain,
    /// so the per-frame path never touches per-vertex conversion.
    pub fn set_terrain(&mut self, terrain: &crate::mesh::TerrainMesh) {
        let mut chunks = Vec::with_capacity(terrain.chunks.len());
        let mut shadows = Vec::with_capacity(terrain.chunks.len());
        let mut grids = Vec::with_capacity(terrain.chunks.len());
        for chunk in terrain.chunks.iter() {
            let mut verts: Vec<macroquad::models::Vertex> =
                Vec::with_capacity(chunk.soup.vertices.len());
            for v in chunk.soup.vertices.iter() {
                verts.push(mq_vertex(v));
            }
            let indices: Vec<u16> = (0..verts.len() as u16).collect();
            chunks.push((
                macroquad::models::Mesh {
                    vertices: verts,
                    indices,
                    texture: None,
                },
                chunk.bbox,
            ));
            // Bridge shadows are a translucent soup, so they go through the
            // same per-vertex alpha path as the range fills and the helicopter
            // shadows; chunks without a bridge contribute an empty batch.
            let mut sverts: Vec<macroquad::models::Vertex> =
                Vec::with_capacity(chunk.shadows.vertices.len());
            for v in chunk.shadows.vertices.iter() {
                sverts.push(mq_range_vertex(v));
            }
            let sidx: Vec<u16> = (0..sverts.len() as u16).collect();
            shadows.push((
                macroquad::models::Mesh {
                    vertices: sverts,
                    indices: sidx,
                    texture: None,
                },
                chunk.bbox,
            ));
            let mut gverts: Vec<macroquad::models::Vertex> =
                Vec::with_capacity(chunk.grid_lines.len() * 2);
            let mut gidx: Vec<u16> = Vec::with_capacity(chunk.grid_lines.len() * 2);
            for (a, b) in chunk.grid_lines.iter() {
                let base = gverts.len() as u16;
                gverts.push(mq_vertex(a));
                gverts.push(mq_vertex(b));
                gidx.push(base);
                gidx.push(base + 1);
            }
            grids.push((
                macroquad::models::Mesh {
                    vertices: gverts,
                    indices: gidx,
                    texture: None,
                },
                chunk.bbox,
            ));
        }
        self.terrain = Some(GpuTerrain {
            chunks,
            shadows,
            grids,
        });
    }
    /// Render one frame of the running game.
    ///
    /// Walks the pass order listed in the module docs. The two range passes
    /// run last and with the depth test off, so a range is never clipped by
    /// terrain, a bridge deck, a building or a vehicle. Unit badges and
    /// floating texts are drawn by [`crate::app`] on top, never occluded.
    ///
    /// Terrain chunks outside the viewport are culled before they are
    /// submitted (their world boxes are tested against the visible world box),
    /// which keeps huge boards bounded by the view size. Nothing is culled per
    /// primitive inside a chunk and nothing is culled on the CPU before
    /// projection: macroquad's own clipping does the rest.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_gpu(
        &mut self,
        iso: &crate::iso::IsoCamera,
        camera: &Camera,
        view_bounds: (f64, f64, f64, f64),
        dynamic: &crate::mesh::DynamicMesh,
        game: &Game,
        selection: Option<Tile>,
        hover_tile: Option<Tile>,
        preview: Option<&Vec<Tile>>,
    ) {
        use macroquad::prelude as mq;
        let bg = constants::WATER_COLOR;
        mq::clear_background(mq::Color::from_rgba(bg[0], bg[1], bg[2], 255));
        // Orthographic GPU camera reproducing `Camera::world_to_screen`.
        // `IsoCamera` stores the matrices in the layout glam expects from
        // `from_cols_array_2d` (each array row is one column/axis), so the
        // arrays are passed through unchanged.
        let view = mq::glam::Mat4::from_cols_array_2d(&iso.view);
        let proj = mq::glam::Mat4::from_cols_array_2d(&iso.proj);
        let matrix = proj * view;
        mq::set_camera(&GpuIsoCamera::screen(matrix));
        if let Some(terrain) = self.terrain.as_ref() {
            for (mesh, bbox) in terrain.chunks.iter() {
                if !bbox_hits(bbox, &view_bounds) {
                    continue;
                }
                mq::draw_mesh(mesh);
            }
            // Bridge shadows on the fields below the decks: translucent, right
            // after the opaque terrain so the depth test trims them against
            // nearer cliffs, and before the dynamic objects so a vehicle
            // standing under the bridge is drawn on top of its own shadow
            // (pass 2 of the module docs).
            for (mesh, bbox) in terrain.shadows.iter() {
                if !bbox_hits(bbox, &view_bounds) {
                    continue;
                }
                draw_range_soup_mesh(mesh);
            }
            for (mesh, bbox) in terrain.grids.iter() {
                if !bbox_hits(bbox, &view_bounds) {
                    continue;
                }
                draw_line_mesh(mesh);
            }
        }
        draw_soup(&dynamic.opaque.vertices, &dynamic.opaque.indices);
        // Translucent helicopter shadows: flat dark discs just above the
        // receiving surface, drawn right after the opaque pass
        // (pass 5 of the module docs). The depth test keeps them from
        // darkening hulls, buildings or nearer cliffs.
        draw_range_soup(&dynamic.shadow.vertices, &dynamic.shadow.indices);
        // Explosion particles: camera-facing billboards, a flat ground wave and
        // shards, all translucent and drawn without a depth write, so nearer
        // puffs blend over farther ones. They go after the helicopter shadows
        // and before the range fills, so a blast is never dimmed by a range
        // disc lying over the same tile (pass 6 of the module docs).
        draw_range_soup(&dynamic.fx.vertices, &dynamic.fx.indices);
        // 3D strokes (routes, details). Range outlines live in a separate
        // buffer, because they are drawn after the range fills instead.
        draw_line_soup(&dynamic.lines);
        // Range fills: masked offscreen and composited, then the outlines on
        // top of them. Both ignore the depth buffer, so a range stays fully
        // visible.
        self.draw_range_fills(matrix, dynamic);
        mq::set_camera(&GpuIsoCamera::flat(matrix));
        draw_line_soup(&dynamic.range_lines);
        // Selection outline + hovered route preview as 2D overlays.
        mq::set_default_camera();
        draw_selection_2d(camera, game, selection, hover_tile, preview);
    }

    /// Draw the range fills: offscreen masks, then one composite per kind.
    ///
    /// Each mask is a binary coverage map — the discs are written fully opaque
    /// ([`constants::RANGE_MASK_ALPHA`]), so a disc covering pixels another
    /// disc already covered overwrites them instead of stacking alpha. Drawing
    /// them with the depth test off also means no terrain, bridge deck,
    /// building or vehicle clips a range. The masks are composited once each
    /// with their presentation colour and alpha, which is what makes two
    /// overlapping ranges of one kind look like a single range while a turret
    /// range overlapping a heal range shows both tints.
    fn draw_range_fills(
        &mut self,
        matrix: macroquad::prelude::glam::Mat4,
        dynamic: &crate::mesh::DynamicMesh,
    ) {
        use macroquad::prelude as mq;
        if dynamic.range_turret.vertices.is_empty() && dynamic.range_heal.vertices.is_empty() {
            return;
        }
        let width = mq::screen_width().max(1.0) as u32;
        let height = mq::screen_height().max(1.0) as u32;
        let stale = match self.range_masks.as_ref() {
            Some(masks) => masks.width != width || masks.height != height,
            None => true,
        };
        if stale {
            // A window resize invalidates the masks: they are screen sized, so
            // the old pair is dropped and allocated again at the new size.
            self.range_masks = Some(RangeMasks {
                turret: mq::render_target(width, height),
                heal: mq::render_target(width, height),
                width,
                height,
            });
        }
        let masks = self.range_masks.as_ref().expect("masks just created");
        for (target, soup, color, alpha) in [
            (
                &masks.turret,
                &dynamic.range_turret,
                constants::RANGE_TURRET_FILL_COLOR,
                constants::RANGE_TURRET_FILL_ALPHA,
            ),
            (
                &masks.heal,
                &dynamic.range_heal,
                constants::RANGE_HEAL_FILL_COLOR,
                constants::RANGE_HEAL_FILL_ALPHA,
            ),
        ] {
            if soup.vertices.is_empty() {
                continue;
            }
            mq::set_camera(&GpuIsoCamera::offscreen(matrix, target.render_pass.clone()));
            mq::clear_background(mq::Color::new(0.0, 0.0, 0.0, 0.0));
            draw_range_soup(&soup.vertices, &soup.indices);
            mq::set_default_camera();
            draw_mask_overlay(&target.texture, color, alpha);
        }
    }
    /// True when the renderer holds buffers for `terrain` already.
    #[allow(dead_code)]
    pub fn has_terrain(&self) -> bool {
        self.terrain.is_some()
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

/// Custom 3D camera reproducing the isometric projection on the GPU.
///
/// The same matrix serves three passes, so depth testing and the render
/// target are part of the camera: the scene is drawn with `depth` on, while
/// the offscreen range masks and the range outlines are drawn with `depth` off
/// so nothing occludes them.
struct GpuIsoCamera {
    /// Combined projection * view matrix.
    matrix: macroquad::prelude::glam::Mat4,
    /// Whether fragments are depth tested and depth writing.
    depth: bool,
    /// Offscreen pass to render into, `None` for the screen.
    render_pass: Option<macroquad::prelude::RenderPass>,
}

impl GpuIsoCamera {
    /// Screen pass with the hardware depth buffer (the opaque scene).
    fn screen(matrix: macroquad::prelude::glam::Mat4) -> Self {
        Self {
            matrix,
            depth: true,
            render_pass: None,
        }
    }

    /// Pass without the depth test, so every fragment survives.
    fn flat(matrix: macroquad::prelude::glam::Mat4) -> Self {
        Self {
            matrix,
            depth: false,
            render_pass: None,
        }
    }

    /// Offscreen pass without the depth test, used for the range masks.
    fn offscreen(
        matrix: macroquad::prelude::glam::Mat4,
        render_pass: macroquad::prelude::RenderPass,
    ) -> Self {
        Self {
            matrix,
            depth: false,
            render_pass: Some(render_pass),
        }
    }
}

impl macroquad::camera::Camera for GpuIsoCamera {
    fn matrix(&self) -> macroquad::prelude::glam::Mat4 {
        self.matrix
    }
    fn depth_enabled(&self) -> bool {
        self.depth
    }
    fn render_pass(&self) -> Option<macroquad::prelude::RenderPass> {
        self.render_pass.clone()
    }
    fn viewport(&self) -> Option<(i32, i32, i32, i32)> {
        None
    }
}

/// True when the chunk box `bbox` intersects the visible world box `view`.
fn bbox_hits(bbox: &(f64, f64, f64, f64), view: &(f64, f64, f64, f64)) -> bool {
    bbox.0 <= view.2 && bbox.2 >= view.0 && bbox.1 <= view.3 && bbox.3 >= view.1
}

/// Draw one prepared line mesh (grid strokes) with the current camera.
fn draw_line_mesh(mesh: &macroquad::models::Mesh) {
    use macroquad::prelude as mq;
    for pair in mesh.vertices.chunks(2) {
        if pair.len() < 2 {
            break;
        }
        let color = mq::Color::from_rgba(
            pair[0].color[0],
            pair[0].color[1],
            pair[0].color[2],
            pair[0].color[3],
        );
        mq::draw_line_3d(pair[0].position, pair[1].position, color);
    }
}

/// Convert one mesh vertex into a macroquad vertex.
fn mq_vertex(v: &crate::mesh::GpuVertex) -> macroquad::models::Vertex {
    macroquad::models::Vertex {
        position: macroquad::prelude::glam::vec3(v.x, v.y, v.z),
        uv: macroquad::prelude::glam::vec2(0.0, 0.0),
        color: [v.color[0], v.color[1], v.color[2], 255],
        normal: macroquad::prelude::glam::vec4(0.0, 0.0, 0.0, 0.0),
    }
}

/// Convert one translucent range vertex (carries its own alpha).
fn mq_range_vertex(v: &crate::mesh::RangeVertex) -> macroquad::models::Vertex {
    macroquad::models::Vertex {
        position: macroquad::prelude::glam::vec3(v.x, v.y, v.z),
        uv: macroquad::prelude::glam::vec2(0.0, 0.0),
        color: [v.color[0], v.color[1], v.color[2], v.color[3]],
        normal: macroquad::prelude::glam::vec4(0.0, 0.0, 0.0, 0.0),
    }
}

/// Draw one pre-converted translucent soup (a static bridge-shadow batch).
///
/// A plain [`macroquad::prelude::draw_mesh`] call: the buffer already carries
/// per-vertex RGBA, so no conversion and no batching is needed. An empty batch
/// (a chunk without a bridge) simply draws nothing.
fn draw_range_soup_mesh(mesh: &macroquad::models::Mesh) {
    use macroquad::prelude as mq;
    if mesh.indices.is_empty() {
        return;
    }
    mq::draw_mesh(mesh);
}

/// Convert one 3D line endpoint (carries its own alpha).
fn mq_line_vertex(v: &crate::mesh::LineVertex) -> macroquad::models::Vertex {
    macroquad::models::Vertex {
        position: macroquad::prelude::glam::vec3(v.x, v.y, v.z),
        uv: macroquad::prelude::glam::vec2(0.0, 0.0),
        color: [v.color[0], v.color[1], v.color[2], v.color[3]],
        normal: macroquad::prelude::glam::vec4(0.0, 0.0, 0.0, 0.0),
    }
}

/// Maximum vertices per single `draw_mesh` batch: one mesh chunk
/// (`CHUNK_VERTICES`) must fit a single macroquad draw call raised via
/// `window_conf`, so no further slicing happens at draw time.
pub const DRAW_BATCH_VERTICES: usize = 16_000;

/// End index of the next draw batch that starts at `vi`.
///
/// The batch limit is not a multiple of three, so a full batch is pulled back
/// to a triangle boundary: a batch cutting a triangle in half would draw a
/// broken triangle at the seam and a stray one at the start of the next batch.
/// Only a full batch is rounded, so the tail batch keeps every vertex it has
/// and the walk always makes progress.
fn batch_end(vi: usize, len: usize) -> usize {
    let mut vend = (vi + DRAW_BATCH_VERTICES).min(len);
    if vend < len {
        vend -= (vend - vi) % 3;
    }
    vend
}

/// Draw one triangle soup in chunks fitting the u16 batch limits.
fn draw_soup(vertices: &[crate::mesh::GpuVertex], indices: &[u16]) {
    use macroquad::prelude as mq;
    let mut vi = 0;
    while vi < vertices.len() {
        let vend = batch_end(vi, vertices.len());
        let mut verts: Vec<macroquad::models::Vertex> = Vec::with_capacity(vend - vi);
        for v in &vertices[vi..vend] {
            verts.push(mq_vertex(v));
        }
        let mut idx: Vec<u16> = Vec::with_capacity(vend - vi);
        for i in 0..(vend - vi) {
            idx.push(i as u16);
        }
        let _ = &indices;
        mq::draw_mesh(&macroquad::models::Mesh {
            vertices: verts,
            indices: idx,
            texture: None,
        });
        vi = vend;
    }
}

/// Draw one range soup with its own per-vertex alpha.
///
/// Turret fills and heal fills use separate draw calls (white vs.
/// light-green transparency); inside one kind
/// every disc sits at a deterministic index-based lift, so coplanar
/// blends no longer flicker while panning. Lines of one call always
/// share one depth value, which keeps macroquad's draw batching from
/// splitting the batch by depth (draw_line_3d state).
fn draw_range_soup(vertices: &[crate::mesh::RangeVertex], indices: &[u16]) {
    use macroquad::prelude as mq;
    let mut vi = 0;
    while vi < vertices.len() {
        // Split on a triangle boundary, like [`draw_soup`].
        let vend = batch_end(vi, vertices.len());
        let mut verts: Vec<macroquad::models::Vertex> = Vec::with_capacity(vend - vi);
        for v in &vertices[vi..vend] {
            verts.push(mq_range_vertex(v));
        }
        let mut idx: Vec<u16> = Vec::with_capacity(vend - vi);
        for i in 0..(vend - vi) {
            idx.push(i as u16);
        }
        let _ = &indices;
        mq::draw_mesh(&macroquad::models::Mesh {
            vertices: verts,
            indices: idx,
            texture: None,
        });
        vi = vend;
    }
}

/// Draw 3D line segments in u16-sized batches.
fn chunked_lines(vertices: &[macroquad::models::Vertex], indices: &[u16]) {
    use macroquad::prelude as mq;
    let mut vi = 0;
    let mut ii = 0;
    while ii < indices.len() {
        let iend = (ii + 6000).min(indices.len());
        let vcount = iend - ii;
        let vend = (vi + vcount).min(vertices.len());
        for pair in vertices[vi..vend].chunks(2) {
            if pair.len() < 2 {
                break;
            }
            let a = pair[0].position;
            let b = pair[1].position;
            let col = mq::Color::from_rgba(
                pair[0].color[0],
                pair[0].color[1],
                pair[0].color[2],
                pair[0].color[3],
            );
            mq::draw_line_3d(a, b, col);
        }
        vi = vend;
        ii = iend;
    }
}

/// Draw a list of 3D line segments (range outlines, routes, details).
///
/// Segments of one call always share one depth value, which keeps macroquad's
/// draw batching from splitting the batch by depth (draw_line_3d state).
fn draw_line_soup(lines: &[(crate::mesh::LineVertex, crate::mesh::LineVertex)]) {
    let mut verts: Vec<macroquad::models::Vertex> = Vec::with_capacity(lines.len() * 2);
    let mut idx: Vec<u16> = Vec::with_capacity(lines.len() * 2);
    for (a, b) in lines.iter() {
        let base = verts.len() as u16;
        verts.push(mq_line_vertex(a));
        verts.push(mq_line_vertex(b));
        idx.push(base);
        idx.push(base + 1);
    }
    chunked_lines(&verts, &idx);
}

/// Composite one range mask over the whole screen in `color` and `alpha`.
///
/// The mask is drawn 1:1 with the window it was rendered at, so the range
/// disc keeps exactly the position and size it had in the scene. Mask pixels
/// are white and opaque, so macroquad's `color * texture` shader gives the
/// requested tint there and a fully transparent fragment everywhere else,
/// which keeps the scene behind the mask untouched.
fn draw_mask_overlay(texture: &macroquad::prelude::Texture2D, color: [u8; 3], alpha: u8) {
    use macroquad::prelude as mq;
    mq::draw_texture_ex(
        texture,
        0.0,
        0.0,
        mq::Color::from_rgba(color[0], color[1], color[2], alpha),
        mq::DrawTextureParams {
            dest_size: Some(mq::glam::vec2(mq::screen_width(), mq::screen_height())),
            // A render target is stored bottom-up, so its rows have to be
            // flipped to land the mask the right way up on the screen.
            flip_y: true,
            ..Default::default()
        },
    );
}

/// 2D selection outline and route preview (never occluded).
fn draw_selection_2d(
    camera: &Camera,
    game: &Game,
    selection: Option<Tile>,
    hover_tile: Option<Tile>,
    preview: Option<&Vec<Tile>>,
) {
    use macroquad::prelude as mq;
    let _ = hover_tile;
    if let Some(sel) = selection {
        let z = crate::mesh::tile_top_z(&game.board, sel);
        let corners = crate::hexgrid::hex_corners(sel.0, sel.1, game.board.side);
        let pts: Vec<(f32, f32)> = corners
            .iter()
            .map(|(x, y)| camera.world_to_screen(*x, *y, z))
            .collect();
        for w in pts.windows(2) {
            mq::draw_line(w[0].0, w[0].1, w[1].0, w[1].1, 2.0, mq::YELLOW);
        }
        if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
            mq::draw_line(a.0, a.1, b.0, b.1, 2.0, mq::YELLOW);
        }
    }
    if let Some(path) = preview {
        let mut prev: Option<(f32, f32)> = None;
        if let Some(sel) = selection {
            let (wx, wy) = game.board.center_world(sel);
            let z = crate::mesh::tile_top_z(&game.board, sel);
            prev = Some(camera.world_to_screen(wx, wy, z));
        }
        for t in path.iter() {
            let (wx, wy) = game.board.center_world(*t);
            let z = crate::mesh::tile_top_z(&game.board, *t);
            let cur = camera.world_to_screen(wx, wy, z);
            if let Some(p) = prev {
                mq::draw_line(p.0, p.1, cur.0, cur.1, 3.0, mq::YELLOW);
            }
            prev = Some(cur);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DRAW_BATCH_VERTICES, batch_end};

    #[test]
    fn draw_batches_end_on_triangle_boundaries() {
        // The batch limit (u16 index batches) is not a multiple of three, so a
        // naive split would cut a triangle in half at every seam. The walk has
        // to stay on triangle boundaries, make progress and keep the tail.
        let len = DRAW_BATCH_VERTICES * 3;
        let mut vi = 0;
        let mut batches = 0;
        while vi < len {
            let end = batch_end(vi, len);
            assert!(end > vi, "batch does not progress: {vi}..{end}");
            assert_eq!((end - vi) % 3, 0, "batch cuts a triangle: {vi}..{end}");
            vi = end;
            batches += 1;
        }
        assert!(batches >= 3, "expected several full batches, got {batches}");
        // A soup shorter than one batch is drawn in a single call, untouched.
        let short = 3 * 7;
        assert_eq!(batch_end(0, short), short);
    }
}
