//! GPU mesh builders for the isometric scene (no macroquad dependency).
//!
//! The shared vocabulary of the mesh layer lives here: the two vertex layouts
//! and the two triangle soups, the primitive solids every model is assembled
//! from, and the per-frame [`DynamicMesh`] that [`build_dynamic`] fills. The
//! models themselves are grouped by what they draw -- static terrain in
//! [`terrain`], walkable elevations in [`surface`], buildings in
//! [`buildings`], obstacles in [`obstacles`], vehicles in [`vehicles`],
//! bonuses and drones in [`bonuses`] and the flat overlays in [`overlays`] --
//! and are re-exported at the end of this file, so
//! `crate::mesh::build_terrain` and friends keep working whatever file a given
//! model sits in.
//!
//! Vertices carry world `(x, y, z)` positions plus a colour; the GPU camera
//! ([`crate::iso`]) projects them and the hardware depth buffer resolves
//! occlusion, so no per-pixel work happens on the CPU.

use crate::constants;
use crate::game::Game;

use bonuses::{push_bonus_markers, push_drones};
use buildings::push_building;
use obstacles::push_obstacle;
use overlays::{push_paths, push_projectiles, push_ranges};
use terrain::DeckQuad;
use vehicles::{push_helicopter_shadow, push_vehicle};

mod bonuses;
mod buildings;
mod obstacles;
mod overlays;
pub(crate) mod surface;
mod terrain;
mod vehicles;

#[cfg(test)]
mod tests;

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

/// One translucent vertex: world position plus RGBA colour.
///
/// Shared by every pass that carries its own transparency, which is why it is
/// a single type: range fills, range outlines, 3D strokes, vehicle shadows and
/// explosion particles all need exactly a position and an RGBA, and all of them
/// reach the GPU through the same conversion in [`crate::render`]. Storing the
/// alpha per vertex is what lets one buffer hold passes that need different
/// transparencies — white turret fills and light-green heal fills, or a range
/// outline that shares the fill hue but is clearly less transparent.
#[derive(Clone, Copy, Debug)]
pub struct AlphaVertex {
    /// World x in distance units (j).
    pub x: f32,
    /// World y in distance units (j).
    pub y: f32,
    /// Rendered elevation in px.
    pub z: f32,
    /// RGBA colour bytes.
    pub color: [u8; 4],
}

/// Triangle soup of translucent [`AlphaVertex`] triangles.
///
/// The name comes from the range fills it was introduced for, and it also
/// carries the other blended passes: bridge and helicopter shadows, range
/// fills of one kind, and the explosion particles of [`crate::fx`]. They are
/// separate fields of the mesh structs only because each one is drawn in its
/// own pass, not because the geometry differs.
#[derive(Clone, Debug, Default)]
pub struct RangeSoup {
    /// All vertices, three per triangle, in draw order.
    pub vertices: Vec<AlphaVertex>,
}

/// Triangle soup with per-vertex colours (flat shading = 3 equal colours).
#[derive(Clone, Debug, Default)]
pub struct TriangleSoup {
    /// All vertices, three per triangle, in draw order.
    pub vertices: Vec<GpuVertex>,
}

fn grow_bbox(bbox: &mut (f64, f64, f64, f64), x: f64, y: f64) {
    bbox.0 = bbox.0.min(x);
    bbox.1 = bbox.1.min(y);
    bbox.2 = bbox.2.max(x);
    bbox.3 = bbox.3.max(y);
}

/// Append one triangle. Vertices are stored in draw order, three per
/// triangle, so the index buffer is always `0..vertices.len()`; the renderer
/// generates it at draw time instead of carrying a second copy per soup.
fn push_tri(soup: &mut TriangleSoup, a: GpuVertex, b: GpuVertex, c: GpuVertex) {
    soup.vertices.push(a);
    soup.vertices.push(b);
    soup.vertices.push(c);
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

/// One translucent vertex from an RGB colour plus a separate alpha.
fn alpha_vert(x: f64, y: f64, z: f64, color: [u8; 3], alpha: u8) -> AlphaVertex {
    AlphaVertex {
        x: x as f32,
        y: y as f32,
        z: z as f32,
        color: [color[0], color[1], color[2], alpha],
    }
}

/// Same as [`alpha_vert`], but with the alpha channel given outright: the
/// explosion particles of [`crate::fx`] fade a colour and its transparency
/// independently, so both components come from the particle itself.
fn alpha_vert_rgba(x: f64, y: f64, z: f64, color: [u8; 4]) -> AlphaVertex {
    AlphaVertex {
        x: x as f32,
        y: y as f32,
        z: z as f32,
        color,
    }
}
fn push_range_tri(soup: &mut RangeSoup, a: AlphaVertex, b: AlphaVertex, c: AlphaVertex) {
    soup.vertices.push(a);
    soup.vertices.push(b);
    soup.vertices.push(c);
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
    let center = alpha_vert(x, y, z, color, alpha);
    let mut prev = alpha_vert(x + r, y, z, color, alpha);
    for i in 1..=n {
        let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        let next = alpha_vert(x + r * a.cos(), y + r * a.sin(), z, color, alpha);
        push_range_tri(mesh, center, prev, next);
        prev = next;
    }
}

/// Axis vectors of a screen-facing (billboard) square in world coordinates.
///
/// The isometric projection of `camera.rs` maps a world delta `(dx, dy, dz)`
/// to the screen delta `((dx - dy) * ISO_COS, (dx + dy) * ISO_SIN - dz)`, so a
/// particle quad has to be spanned by the two world directions that move the
/// screen exactly right and exactly up:
///
/// * `right = (k, -k, 0)`, with `k = 1 / (2 * ISO_COS)`, gives screen
///   `(+1, 0)` and no depth change (`D` is unchanged), so a quad built on it
///   is perfectly flat in the depth buffer -- no z-fighting between its own
///   halves and a stable translucent blend,
/// * `up = (-k, -k, 2 * ISO_SIN / 2)`, with `k = 1 / (4 * ISO_SIN)`, gives
///   screen `(0, -1)` and `dD = 2k * ISO_SIN - 2k * ISO_SIN = 0` as well.
///
/// Both vectors are unit *screen* steps, not unit world vectors, which is why
/// the particle sizes in [`crate::fx`] can be written in plain px.
pub fn billboard_axes() -> ((f64, f64, f64), (f64, f64, f64)) {
    let k = 1.0 / (2.0 * constants::ISO_COS);
    let m = 1.0 / (4.0 * constants::ISO_SIN);
    ((k, -k, 0.0), (-m, -m, constants::ISO_SIN))
}

/// Soft, camera-facing blob (fire, smoke, flash) of half-size `radius` px.
///
/// Built as a square with a **fully transparent centre and opaque edges**, so
/// the flat quad blends into a round puff without any texture: the fade is
/// radial by construction and costs eight triangles per particle. `center` is
/// the current RGBA, `edge` the RGBA of the outer ring.
pub fn push_fx_blob(
    soup: &mut RangeSoup,
    x: f64,
    y: f64,
    z: f64,
    radius: f64,
    center: [u8; 4],
    edge: [u8; 4],
) {
    let (right, up) = billboard_axes();
    let at = |a: f64, b: f64, c: [u8; 4]| {
        alpha_vert_rgba(
            x + a * right.0 + b * up.0,
            y + a * right.1 + b * up.1,
            z + a * right.2 + b * up.2,
            c,
        )
    };
    let mid = at(0.0, 0.0, center);
    // Four edge midpoints carry the colour, the four corners stay transparent:
    // the square therefore reads as a soft round puff.
    let n = at(0.0, -radius, edge);
    let e = at(radius, 0.0, edge);
    let s = at(0.0, radius, edge);
    let w = at(-radius, 0.0, edge);
    for (a, b) in [(n, e), (e, s), (s, w), (w, n)] {
        push_range_tri(soup, mid, a, b);
    }
    // Two opaque triangles fill the middle of the square so the blob has a
    // solid core instead of four thin triangles meeting in one point.
    let top = at(0.0, -radius * constants::FX_CORE_FILL, center);
    let right_mid = at(radius * constants::FX_CORE_FILL, 0.0, center);
    let bottom = at(0.0, radius * constants::FX_CORE_FILL, center);
    let left_mid = at(-radius * constants::FX_CORE_FILL, 0.0, center);
    push_range_tri(soup, mid, top, right_mid);
    push_range_tri(soup, mid, right_mid, bottom);
    push_range_tri(soup, mid, bottom, left_mid);
    push_range_tri(soup, mid, left_mid, top);
}

/// One flat ring lying in the ground plane: the blast shock wave of an
/// explosion. Built as a band of quads between `inner` and `outer` radius,
/// whose inner rim is `inner_alpha` and outer rim `outer_alpha`, so the ring
/// fades out towards its centre. It is a *ground* effect on purpose: it reads
/// as a wave running over the terrain instead of a sprite floating in the air.
#[allow(clippy::too_many_arguments)]
pub fn push_fx_ring(
    soup: &mut RangeSoup,
    x: f64,
    y: f64,
    z: f64,
    inner: f64,
    outer: f64,
    color: [u8; 3],
    inner_alpha: u8,
    outer_alpha: u8,
) {
    let inner_c = [color[0], color[1], color[2], inner_alpha];
    let outer_c = [color[0], color[1], color[2], outer_alpha];
    let n = constants::FX_RING_SEGMENTS;
    let at = |r: f64, a: f64, c: [u8; 4]| alpha_vert_rgba(x + r * a.cos(), y + r * a.sin(), z, c);
    for i in 0..n {
        let a0 = std::f64::consts::TAU * i as f64 / n as f64;
        let a1 = std::f64::consts::TAU * (i + 1) as f64 / n as f64;
        let p0 = at(inner, a0, inner_c);
        let p1 = at(inner, a1, inner_c);
        let p2 = at(outer, a1, outer_c);
        let p3 = at(outer, a0, outer_c);
        push_range_tri(soup, p0, p1, p2);
        push_range_tri(soup, p0, p2, p3);
    }
}

/// Oriented square shard (spark, wreck fragment): a hard-edged billboard,
/// unlike the soft [`push_fx_blob`]. Sparks stay crisp, which is what makes
/// them read as glowing embers rather than more smoke.
pub fn push_fx_shard(
    soup: &mut RangeSoup,
    x: f64,
    y: f64,
    z: f64,
    radius: f64,
    color: [u8; 4],
    angle: f64,
) {
    let (right, up) = billboard_axes();
    let (c, s) = (angle.cos(), angle.sin());
    let at = |along: f64, across: f64| {
        let a = along * c - across * s;
        let b = along * s + across * c;
        alpha_vert_rgba(
            x + a * right.0 + b * up.0,
            y + a * right.1 + b * up.1,
            z + a * right.2 + b * up.2,
            color,
        )
    };
    let a = at(-radius, -radius);
    let b = at(radius, -radius);
    let c = at(radius, radius);
    let d = at(-radius, radius);
    push_range_tri(soup, a, b, c);
    push_range_tri(soup, a, c, d);
}

/// Flat oriented rectangle with one RGBA colour (helicopter shadow parts).
///
/// `len` runs along the unit direction `(fx, fy)`, `wid` across it, exactly
/// like [`push_oriented_box`] draws the part itself, so the shadow keeps the
/// silhouette of the airframe.
#[allow(clippy::too_many_arguments)]
fn push_range_rect(
    soup: &mut RangeSoup,
    cx: f64,
    cy: f64,
    z: f64,
    len: f64,
    wid: f64,
    fx: f64,
    fy: f64,
    color: [u8; 3],
    alpha: u8,
) {
    let (px, py) = (-fy, fx);
    let (hl, hw) = (len / 2.0, wid / 2.0);
    let corner = |along: f64, across: f64| {
        alpha_vert(
            cx + fx * along + px * across,
            cy + fy * along + py * across,
            z,
            color,
            alpha,
        )
    };
    let a = corner(-hl, -hw);
    let b = corner(hl, -hw);
    let c = corner(hl, hw);
    let d = corner(-hl, hw);
    push_range_tri(soup, a, b, c);
    push_range_tri(soup, a, c, d);
}

/// Oriented rectangle of a helicopter shadow clipped to a bridge deck.
///
/// The deck is a narrow strip lying over water, and the silhouette is much
/// wider, so without a clip the parts would hang in mid-air beside the bridge.
/// The rectangle is cut against the four deck edges (Sutherland-Hodgman) and
/// the surviving polygon is triangulated as a fan, which keeps the
/// translucent plane flat (one alpha, no self-overlap) while the hardware
/// depth test still trims it against nearer hulls and cliffs.
#[allow(clippy::too_many_arguments)]
fn push_range_rect_on_deck(
    soup: &mut RangeSoup,
    cx: f64,
    cy: f64,
    z: f64,
    len: f64,
    wid: f64,
    fx: f64,
    fy: f64,
    color: [u8; 3],
    alpha: u8,
    deck: &DeckQuad,
) {
    let (px, py) = (-fy, fx);
    let (hl, hw) = (len / 2.0, wid / 2.0);
    let corner =
        |along: f64, across: f64| (cx + fx * along + px * across, cy + fy * along + py * across);
    let rect = [
        corner(-hl, -hw),
        corner(hl, -hw),
        corner(hl, hw),
        corner(-hl, hw),
    ];
    // Deck-local axes: along the bridge and across it.
    let (ux, uy) = deck.axis;
    let (vx, vy) = (-uy, ux);
    let mut poly = rect.to_vec();
    for (nx, ny, limit) in [
        (ux, uy, deck.half_len),
        (-ux, -uy, deck.half_len),
        (vx, vy, deck.half_wid),
        (-vx, -vy, deck.half_wid),
    ] {
        poly = clip_polygon_half_plane(&poly, deck.center, (nx, ny), limit);
        if poly.len() < 3 {
            return;
        }
    }
    for k in 1..poly.len() - 1 {
        let a = alpha_vert(poly[0].0, poly[0].1, z, color, alpha);
        let b = alpha_vert(poly[k].0, poly[k].1, z, color, alpha);
        let c = alpha_vert(poly[k + 1].0, poly[k + 1].1, z, color, alpha);
        push_range_tri(soup, a, b, c);
    }
}

/// Clip a convex polygon to the half-plane `dot(p - origin, normal) <= limit`
/// (Sutherland-Hodgman, single pass over the edges).
fn clip_polygon_half_plane(
    poly: &[(f64, f64)],
    origin: (f64, f64),
    normal: (f64, f64),
    limit: f64,
) -> Vec<(f64, f64)> {
    let side = |p: (f64, f64)| (p.0 - origin.0) * normal.0 + (p.1 - origin.1) * normal.1 - limit;
    let mut out = Vec::with_capacity(poly.len() + 2);
    for k in 0..poly.len() {
        let a = poly[k];
        let b = poly[(k + 1) % poly.len()];
        let (da, db) = (side(a), side(b));
        if da <= 0.0 {
            out.push(a);
        }
        if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
            let t = da / (da - db);
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    out
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
    /// Range outlines (rings around turret and heal ranges), kept apart from
    /// [`DynamicMesh::lines`] so they can be drawn after the composited range
    /// fills, in the owner colour and without a depth test (see
    /// [`push_ranges`]).
    pub range_lines: Vec<(AlphaVertex, AlphaVertex)>,
    /// Explosion particles: camera-facing billboard quads, flat ground rings
    /// and shards, all with their own RGBA (no depth write, drawn after the
    /// opaque pass; see [`push_fx_blob`] and [`push_fx_ring`]).
    pub fx: RangeSoup,
    /// 3D line segments (grid already in terrain; ranges/routes here).
    pub lines: Vec<(AlphaVertex, AlphaVertex)>,
}

impl DynamicMesh {
    /// Remove all per-frame geometry before rebuilding the frame.
    pub fn clear(&mut self) {
        self.opaque.vertices.clear();
        self.shadow.vertices.clear();
        self.range_turret.vertices.clear();
        self.range_heal.vertices.clear();
        self.range_lines.clear();
        self.fx.vertices.clear();
        self.lines.clear();
    }
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

/// Flat-top hexagonal slab (prism) centred at `(x, y)` on base `z`.
///
/// Buildings stand on one of these instead of on the bare tile top: the slab
/// shares the flat-top orientation of the fields, so a building reads as one
/// structure with the tile below it, and its dark colour cuts the building
/// out of the grey terrain. Six facets with two alternating side shades keep
/// the flat-shaded look of [`push_cylinder`] without rounding the outline,
/// which would blur the hexagon into the field.
pub fn push_hex_prism(
    mesh: &mut TriangleSoup,
    x: f64,
    y: f64,
    z: f64,
    r: f64,
    h: f64,
    color: [u8; 3],
) {
    let facets = [constants::shade(color, 0.85), constants::shade(color, 0.7)];
    let corner = |k: usize| -> (f64, f64) {
        let a = std::f64::consts::FRAC_PI_3 * k as f64;
        (x + r * a.cos(), y + r * a.sin())
    };
    // Top face as a fan around the centre, so the slab stays closed.
    for k in 0..6 {
        let (ax, ay) = corner(k);
        let (bx, by) = corner((k + 1) % 6);
        let c = constants::shade(color, 1.0);
        push_tri(
            mesh,
            vert(x, y, z + h, c),
            vert(ax, ay, z + h, c),
            vert(bx, by, z + h, c),
        );
    }
    for k in 0..6 {
        let (ax, ay) = corner(k);
        let (bx, by) = corner((k + 1) % 6);
        let c = facets[k % 2];
        push_quad(
            mesh,
            vert(ax, ay, z, c),
            vert(bx, by, z, c),
            vert(bx, by, z + h, c),
            vert(ax, ay, z + h, c),
        );
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
    // The two sides facing away from the viewer are not drawn: the isometric
    // camera never sees them, and skipping them halves the triangles of every
    // box in the scene. Only the corners those two sides need are named.
    let c100 = vert(x + sx / 2.0, y - sy / 2.0, z, color);
    let c110 = vert(x + sx / 2.0, y + sy / 2.0, z, color);
    let c010 = vert(x - sx / 2.0, y + sy / 2.0, z, color);
    let top = constants::shade(color, 1.0);
    let c001 = vert(x - sx / 2.0, y - sy / 2.0, z + sz, top);
    let c101 = vert(x + sx / 2.0, y - sy / 2.0, z + sz, top);
    let c111 = vert(x + sx / 2.0, y + sy / 2.0, z + sz, top);
    let c011 = vert(x - sx / 2.0, y + sy / 2.0, z + sz, top);
    // Top plus the two viewer-facing sides.
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
/// Thick 3D segment as a camera-facing box strip (grid-free strokes).
#[allow(clippy::too_many_arguments)]
pub fn push_beam(
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    x0: f64,
    y0: f64,
    z0: f64,
    x1: f64,
    y1: f64,
    z1: f64,
    color: [u8; 3],
) {
    lines.push((
        alpha_vert(x0, y0, z0, color, 255),
        alpha_vert(x1, y1, z1, color, 255),
    ));
}
/// Rebuild the dynamic mesh of one frame (buildings, obstacles, vehicles).
///
/// `max_height_px` is the cached [`terrain::max_height`] of the board: terrain
/// never changes during a match, so the caller computes it once per level and
/// hands it down instead of rescanning the board per helicopter per frame.
pub fn build_dynamic(game: &Game, rotor_phase: f64, max_height_px: f64, out: &mut DynamicMesh) {
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
    // Obstacles come from the game index, not from a full board scan: the
    // board only changes on wall destruction / mine explosion, which keep the
    // index in sync (see `Game::remove_obstacle_tile`).
    for tile in game.obstacle_tiles.iter() {
        push_obstacle(game, *tile, &mut out.opaque, &mut out.lines);
    }
    for v in game.vehicles.iter() {
        if v.dead {
            continue;
        }
        push_vehicle(
            game,
            v,
            rotor_phase,
            max_height_px,
            &mut out.opaque,
            &mut out.lines,
        );
        if v.kind == constants::VehicleKind::Helicopter {
            push_helicopter_shadow(game, v, rotor_phase, &mut out.shadow);
        }
    }
    push_bonus_markers(game, &mut out.opaque);
    push_drones(game, rotor_phase, &mut out.opaque, &mut out.lines);
    push_ranges(
        game,
        &mut out.range_turret,
        &mut out.range_heal,
        &mut out.range_lines,
    );
    push_paths(game, &mut out.lines);
    push_projectiles(game, &mut out.opaque);
}

/// Two crossing strokes in `lines`: the healing cross on a buffer roof and in
/// the middle of a buffer hull.
pub(super) fn push_cross(
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    x: f64,
    y: f64,
    z: f64,
    color: [u8; 3],
) {
    let h = 6.0;
    lines.push((
        alpha_vert(x - h, y, z, color, 255),
        alpha_vert(x + h, y, z, color, 255),
    ));
    lines.push((
        alpha_vert(x, y - h, z, color, 255),
        alpha_vert(x, y + h, z, color, 255),
    ));
}

/// Circle of `n` segments as strokes in `lines` (range outlines, base rings).
#[allow(clippy::too_many_arguments)]
pub(super) fn push_ring(
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
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
            alpha_vert(prev.0, prev.1, z, color, alpha),
            alpha_vert(next.0, next.1, z, color, alpha),
        ));
        prev = next;
    }
}

// ---------------------------------------------------------------------------
// Re-exports: the mesh layer is addressed as `crate::mesh::*`, so the terrain
// and the elevations the rest of the crate ask for keep their names even though
// the file that defines them changed. The per-model builders stay private to
// this module -- `build_dynamic` is the only way in.
// ---------------------------------------------------------------------------

pub use terrain::{
    TerrainMesh, build_terrain, depth_span, max_height, ramp_waypoint_z, tile_top_z,
    visible_world_bounds,
};
pub use vehicles::{vehicle_z, wreck_z};
