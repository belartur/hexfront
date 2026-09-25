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

use crate::camera::Camera;
use crate::constants;
use crate::game::Game;
use crate::hexgrid::Tile;

/// Draws a whole game state with the GPU.
pub struct Renderer {
    /// Phase of the helicopter rotor animation.
    pub rotor_phase: f64,
    /// Converted static terrain buffers, uploaded once per level.
    terrain: Option<GpuTerrain>,
}

/// Static terrain already converted into macroquad meshes.
struct GpuTerrain {
    /// One draw batch per terrain chunk plus its world bounding box.
    chunks: Vec<(macroquad::models::Mesh, (f64, f64, f64, f64))>,
    /// Grid strokes of every chunk plus its world bounding box.
    grids: Vec<(macroquad::models::Mesh, (f64, f64, f64, f64))>,
}

impl Renderer {
    /// Create a renderer.
    pub fn new() -> Self {
        Self {
            rotor_phase: 0.0,
            terrain: None,
        }
    }
    /// Convert a freshly built terrain mesh into GPU buffers.
    ///
    /// Called by [`crate::app`] right after rebuilding the static terrain,
    /// so the per-frame path never touches per-vertex conversion.
    pub fn set_terrain(&mut self, terrain: &crate::mesh::TerrainMesh) {
        let mut chunks = Vec::with_capacity(terrain.chunks.len());
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
        self.terrain = Some(GpuTerrain { chunks, grids });
    }
    /// Render one frame of the running game.
    ///
    /// Pass order: opaque terrain chunks -> terrain grid lines -> opaque
    /// dynamic objects -> translucent helicopter shadows -> translucent range
    /// discs -> 3D strokes -> 2D overlays. Unit badges and floating texts are
    /// drawn by [`crate::app`] on top, never occluded. Terrain chunks outside
    /// the viewport are culled before they are submitted (their world boxes
    /// are tested against the visible world box), which keeps huge boards
    /// bounded by the view size.
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
        let cam3d = GpuIsoCamera {
            matrix: proj * view,
        };
        mq::set_camera(&cam3d);
        if let Some(terrain) = self.terrain.as_ref() {
            for (mesh, bbox) in terrain.chunks.iter() {
                if !bbox_hits(bbox, &view_bounds) {
                    continue;
                }
                mq::draw_mesh(mesh);
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
        // receiving surface, drawn right after the opaque pass and before the
        // range fills (specification_rust.md pass order). The depth test keeps
        // them from darkening hulls, buildings or nearer cliffs.
        draw_range_soup(&dynamic.shadow.vertices, &dynamic.shadow.indices);
        // Translucent range discs: one draw call per fill kind (white
        // turret vs. light-green heal), so overlapping fills of the same
        // kind share one depth value per disc and blend in a stable order
        // (specification_rust.md pass order); no depth write, depth test on.
        draw_range_soup(
            &dynamic.range_turret.vertices,
            &dynamic.range_turret.indices,
        );
        draw_range_soup(&dynamic.range_heal.vertices, &dynamic.range_heal.indices);
        // 3D strokes (range rings, routes, details).
        {
            let mut verts: Vec<macroquad::models::Vertex> =
                Vec::with_capacity(dynamic.lines.len() * 2);
            let mut idx: Vec<u16> = Vec::with_capacity(dynamic.lines.len() * 2);
            for (a, b) in dynamic.lines.iter() {
                let base = verts.len() as u16;
                verts.push(mq_line_vertex(a));
                verts.push(mq_line_vertex(b));
                idx.push(base);
                idx.push(base + 1);
            }
            chunked_lines(&verts, &idx);
        }
        // Selection outline + hovered route preview as 2D overlays.
        mq::set_default_camera();
        draw_selection_2d(camera, game, selection, hover_tile, preview);
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
struct GpuIsoCamera {
    /// Combined projection * view matrix.
    matrix: macroquad::prelude::glam::Mat4,
}

impl macroquad::camera::Camera for GpuIsoCamera {
    fn matrix(&self) -> macroquad::prelude::glam::Mat4 {
        self.matrix
    }
    fn depth_enabled(&self) -> bool {
        true
    }
    fn render_pass(&self) -> Option<macroquad::prelude::RenderPass> {
        None
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

/// Draw one triangle soup in chunks fitting the u16 batch limits.
fn draw_soup(vertices: &[crate::mesh::GpuVertex], indices: &[u16]) {
    use macroquad::prelude as mq;
    let mut vi = 0;
    while vi < vertices.len() {
        let vend = (vi + DRAW_BATCH_VERTICES).min(vertices.len());
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
/// light-green transparency from the Python version); inside one kind
/// every disc sits at a deterministic index-based lift, so coplanar
/// blends no longer flicker while panning. Lines of one call always
/// share one depth value, which keeps macroquad's draw batching from
/// splitting the batch by depth (draw_line_3d state).
fn draw_range_soup(vertices: &[crate::mesh::RangeVertex], indices: &[u16]) {
    use macroquad::prelude as mq;
    let mut vi = 0;
    while vi < vertices.len() {
        let vend = (vi + DRAW_BATCH_VERTICES).min(vertices.len());
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
