//! Obstacle models (rules.md sections 1 and 4).
//!
//! The wall stays one plain block. The ice trap gets a model, and the mine --
//! the only hazard rules.md section 1 allows on land *and* on water -- gets
//! one shape per terrain: ashore a pressure plate a vehicle rolls over,
//! afloat a moored body floating on the surface. The ice trap is land-only, so
//! a water shape for it would be dead code. The fire trap gets no model at
//! all: it is a cluster of living particle emitters
//! ([`crate::fx::campfire_cluster`]), and the fire alone *is* the trap.

use super::{
    AlphaVertex, TriangleSoup, push_beam, push_box, push_cylinder, push_hex_prism,
    push_oriented_slope, tile_top_z,
};
use crate::constants;
use crate::game::Game;
use crate::hexgrid::Tile;

pub(super) fn push_obstacle(
    game: &Game,
    tile: Tile,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
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
        ObstacleKind::Wall => push_wall(mesh, cx, cy, z),
        // A mine stands on land and on water alike (rules.md section 1), so
        // the renderer picks a shape for the terrain underneath: a pressure
        // disc ashore, a moored floating body afloat.
        ObstacleKind::Mine => {
            if game.board.height(tile) == 0 {
                push_mine_water(mesh, lines, cx, cy, z);
            } else {
                push_mine_land(mesh, lines, cx, cy, z);
            }
        }
        // The ice trap is land-only (rules.md section 1), so it needs no water
        // shape. The fire trap needs no shape at all: its cluster of
        // campfires is a particle emitter in `crate::fx`, and the fire alone
        // *is* the trap.
        ObstacleKind::TrapFire => {}
        ObstacleKind::TrapIce => push_ice_trap(mesh, lines, cx, cy, z),
    }
}

// ---------------------------------------------------------------------------
// Obstacle parts (rules.md sections 1 and 4; every value is a rendering-only
// size in px, exactly like the building, tank and helicopter constants). The
// wall stays one plain block; the mine and the ice trap get models, and the
// mine -- the only hazard rules.md section 1 allows on land *and* on water --
// gets one shape per terrain: ashore a pressure plate a vehicle rolls over,
// afloat a moored body floating on the surface. The ice trap is land-only
// (rules.md section 1), so a water shape for it would be dead code.
// ---------------------------------------------------------------------------

/// Facets of the round obstacle parts (mine parts, the ice trap).
pub(super) const OBS_SEGMENTS: usize = 12;
/// Highest point any obstacle part may reach above its field in px: like
/// `BLD_MAX_H` a design limit checked by the mesh tests, not a value the
/// builder reads back, so it is compiled for tests alone.
#[cfg(test)]
pub(super) const OBS_MAX_H: f64 = 20.0;
/// Furthest horizontal distance of an obstacle part from its field centre in
/// px; the wall block is the widest of them. Also a design limit checked by
/// the mesh tests, so it is compiled for tests alone like `BLD_MAX_H`.
#[cfg(test)]
pub(super) const OBS_MAX_REACH: f64 = 27.0;

// Wall (rules.md sections 1 and 4: 20 hp, blocks ground vehicles).
/// Footprint of the wall block along x in px.
pub(super) const OBS_WALL_X: f64 = 30.0;
/// Footprint of the wall block along y in px.
pub(super) const OBS_WALL_Y: f64 = 26.0;
/// Height of the wall block in px.
pub(super) const OBS_WALL_H: f64 = 18.0;
/// Rubble colour of the wall block.
pub(super) const OBS_WALL_COLOR: [u8; 3] = [120, 100, 80];

/// Rendered wall: the plain rubble block that stops ground vehicles (rules.md
/// sections 1 and 4). Anchored at the tile top, not lifted: unlike the flat
/// markers it has a volume, so it cannot lose the depth race against the
/// terrain.
fn push_wall(mesh: &mut TriangleSoup, cx: f64, cy: f64, z: f64) {
    push_box(
        mesh,
        cx,
        cy,
        z,
        OBS_WALL_X,
        OBS_WALL_Y,
        OBS_WALL_H,
        OBS_WALL_COLOR,
    );
}

// Mine (rules.md sections 1 and 4: 25 damage once and then removed; stands on
// land and on water alike).
/// Red danger colour shared by both mine shapes: the cross on the pressure
/// plate ashore, the belt around the floating body afloat. One colour marks a
/// mine of either terrain.
pub(super) const OBS_MINE_MARK_COLOR: [u8; 3] = [200, 60, 50];
/// Radius of the ground mine body at its base in px.
pub(super) const OBS_MINE_BODY_R: f64 = 8.2;
/// Radius of the ground mine body at its top in px.
pub(super) const OBS_MINE_BODY_TOP_R: f64 = 7.4;
/// Height of the ground mine body in px: a low drum, the way a mine that a
/// vehicle rolls over looks from above.
pub(super) const OBS_MINE_BODY_H: f64 = 2.1;
/// Colour of the ground mine body (dark military green-grey).
pub(super) const OBS_MINE_COLOR: [u8; 3] = [78, 82, 70];
/// Radius of the light pressure plate on top of the ground mine in px. It is a
/// flat-top hexagon like the building slabs, so the plate reads as an object of
/// the same world as the fields below it.
pub(super) const OBS_MINE_PLATE_R: f64 = 4.4;
/// Height of the pressure plate in px.
pub(super) const OBS_MINE_PLATE_H: f64 = 1.0;
/// Colour of the steel pressure plate.
pub(super) const OBS_MINE_PLATE_COLOR: [u8; 3] = [150, 150, 142];
/// Half-diagonal of the red cross painted on the pressure plate in px.
pub(super) const OBS_MINE_MARK_R: f64 = 3.0;

/// Rendered ground mine (rules.md sections 1 and 4): a low drum with a steel
/// pressure plate on top. Unlike the flat marker it replaces the drum is a
/// closed volume, so the mine keeps a silhouette in the isometric view.
fn push_mine_land(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    cx: f64,
    cy: f64,
    z: f64,
) {
    // The body floats by `OBSTACLE_LIFT`: exactly coplanar with the tile top
    // it would lose the depth race against the terrain and flicker.
    let base = z + constants::OBSTACLE_LIFT;
    push_cylinder(
        mesh,
        cx,
        cy,
        base,
        OBS_MINE_BODY_R,
        OBS_MINE_BODY_TOP_R,
        OBS_MINE_BODY_H,
        OBS_SEGMENTS,
        OBS_MINE_COLOR,
    );
    let plate = base + OBS_MINE_BODY_H;
    push_hex_prism(
        mesh,
        cx,
        cy,
        plate,
        OBS_MINE_PLATE_R,
        OBS_MINE_PLATE_H,
        OBS_MINE_PLATE_COLOR,
    );
    // The red cross on the plate is what makes a mine readable at any zoom;
    // lines draw after the opaque pass, so the plate never hides it.
    let top = plate + OBS_MINE_PLATE_H + 0.1;
    let r = OBS_MINE_MARK_R;
    push_beam(
        lines,
        cx - r,
        cy - r,
        top,
        cx + r,
        cy + r,
        top,
        OBS_MINE_MARK_COLOR,
    );
    push_beam(
        lines,
        cx - r,
        cy + r,
        top,
        cx + r,
        cy - r,
        top,
        OBS_MINE_MARK_COLOR,
    );
}

/// Radius of the float collar of the floating mine in px.
pub(super) const OBS_MINE_FLOAT_R: f64 = 7.5;
/// Height of the float collar in px.
pub(super) const OBS_MINE_FLOAT_H: f64 = 1.4;
/// Colour of the float collar (steel, darker than the hull above it).
pub(super) const OBS_MINE_FLOAT_COLOR: [u8; 3] = [62, 66, 70];
/// Radius of the floating hull at its belly in px.
pub(super) const OBS_MINE_HULL_R: f64 = 8.5;
/// Radius of the belly cone of the floating hull at its foot in px.
pub(super) const OBS_MINE_HULL_FOOT_R: f64 = 3.5;
/// Height of the belly cone in px; its top ring is the widest part of the hull.
/// The three cones of the hull add up to a roughly round body (that is what a
/// moored mine is), so the water shape reads as a ball, not as a disc.
pub(super) const OBS_MINE_HULL_LOW_H: f64 = 5.0;
/// Radius of the shoulder ring, where the floating hull starts to close in px.
pub(super) const OBS_MINE_HULL_SHOULDER_R: f64 = 6.5;
/// Height of the shoulder cone in px.
pub(super) const OBS_MINE_HULL_HIGH_H: f64 = 4.0;
/// Radius of the small top cap of the floating hull in px.
pub(super) const OBS_MINE_CAP_R: f64 = 3.0;
/// Height of the top cap in px.
pub(super) const OBS_MINE_CAP_H: f64 = 3.0;
/// Radius of the red belt that rings the floating hull at its belly in px; a
/// little proud of the hull, so the belt stays visible all around it.
pub(super) const OBS_MINE_BELT_R: f64 = 9.0;
/// Height of the red belt in px.
pub(super) const OBS_MINE_BELT_H: f64 = 0.8;
/// Number of contact horns around the floating hull.
pub(super) const OBS_MINE_HORNS: usize = 6;
/// Radial distance from the hull centre where a horn starts in px; just inside
/// the belly, so the joint disappears in the hull.
pub(super) const OBS_MINE_HORN_IN: f64 = 7.5;
/// Length of one horn along its own axis in px; its tip reaches
/// `OBS_MINE_HORN_IN + OBS_MINE_HORN_LEN` from the hull centre.
pub(super) const OBS_MINE_HORN_LEN: f64 = 5.5;
/// Cross-section of a horn in px.
pub(super) const OBS_MINE_HORN_WID: f64 = 2.2;
/// Rise of a horn tip above its foot in px: the horns lean clearly upwards, so
/// they read as contact spikes of a moored mine.
pub(super) const OBS_MINE_HORN_RISE: f64 = 4.0;
/// Colour of the horns (pale steel, so they stand out against the hull).
pub(super) const OBS_MINE_HORN_COLOR: [u8; 3] = [206, 202, 190];
/// Half-diagonal of the red cross on the cap of the floating mine in px. Small
/// enough to stay on the cap: the cross is the same mine marker the ground
/// plate carries, this time seen from above.
pub(super) const OBS_MINE_CAP_MARK_R: f64 = 2.0;

/// Rendered floating mine (rules.md section 1 allows a mine on water too): a
/// moored body on a float collar, belted in the same red as the land cross and
/// bristling with contact horns. Both mine shapes share the danger colour and
/// the field, so each reads as "mine", while their silhouettes say whether it
/// waits ashore or afloat.
fn push_mine_water(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    cx: f64,
    cy: f64,
    z: f64,
) {
    let base = z + constants::OBSTACLE_LIFT;
    push_cylinder(
        mesh,
        cx,
        cy,
        base,
        OBS_MINE_FLOAT_R,
        OBS_MINE_FLOAT_R,
        OBS_MINE_FLOAT_H,
        OBS_SEGMENTS,
        OBS_MINE_FLOAT_COLOR,
    );
    let hull = base + OBS_MINE_FLOAT_H;
    // Rounded hull out of three stacked cones: the scene has no sphere
    // primitive, and the alternating facet shades of `push_cylinder` are what
    // makes the stack read as one round body.
    let belly = hull + OBS_MINE_HULL_LOW_H;
    push_cylinder(
        mesh,
        cx,
        cy,
        hull,
        OBS_MINE_HULL_FOOT_R,
        OBS_MINE_HULL_R,
        OBS_MINE_HULL_LOW_H,
        OBS_SEGMENTS,
        OBS_MINE_COLOR,
    );
    push_cylinder(
        mesh,
        cx,
        cy,
        belly,
        OBS_MINE_HULL_R,
        OBS_MINE_HULL_SHOULDER_R,
        OBS_MINE_HULL_HIGH_H,
        OBS_SEGMENTS,
        OBS_MINE_COLOR,
    );
    push_cylinder(
        mesh,
        cx,
        cy,
        belly + OBS_MINE_HULL_HIGH_H,
        OBS_MINE_HULL_SHOULDER_R,
        OBS_MINE_CAP_R,
        OBS_MINE_CAP_H,
        OBS_SEGMENTS,
        OBS_MINE_COLOR,
    );
    // Red belt straddling the widest ring of the hull: the mine marker of the
    // water shape, wider than the hull so it shows all around the belly.
    push_cylinder(
        mesh,
        cx,
        cy,
        belly - OBS_MINE_BELT_H / 2.0,
        OBS_MINE_BELT_R,
        OBS_MINE_BELT_R,
        OBS_MINE_BELT_H,
        OBS_SEGMENTS,
        OBS_MINE_MARK_COLOR,
    );
    // Contact horns leaning out of the belly. Each horn is a solid wedge
    // (`push_oriented_slope`), not a stroke: a horn has a volume of its own, so
    // the depth buffer hides the ones on the far side of the hull instead of
    // letting them show through it.
    let horn_mid = OBS_MINE_HORN_IN + OBS_MINE_HORN_LEN / 2.0;
    for i in 0..OBS_MINE_HORNS {
        let a = std::f64::consts::TAU * i as f64 / OBS_MINE_HORNS as f64;
        let (dx, dy) = (a.cos(), a.sin());
        push_oriented_slope(
            mesh,
            cx + dx * horn_mid,
            cy + dy * horn_mid,
            belly,
            OBS_MINE_HORN_LEN,
            OBS_MINE_HORN_WID,
            0.0,
            OBS_MINE_HORN_RISE,
            dx,
            dy,
            OBS_MINE_HORN_COLOR,
        );
    }
    // Red cross on the cap: the same mine marker the ground plate carries, and
    // the detail stroke of the water shape. It sits on the topmost surface, so
    // nothing can hide it.
    let top = belly + OBS_MINE_HULL_HIGH_H + OBS_MINE_CAP_H + 0.1;
    let r = OBS_MINE_CAP_MARK_R;
    push_beam(
        lines,
        cx - r,
        cy - r,
        top,
        cx + r,
        cy + r,
        top,
        OBS_MINE_MARK_COLOR,
    );
    push_beam(
        lines,
        cx - r,
        cy + r,
        top,
        cx + r,
        cy - r,
        top,
        OBS_MINE_MARK_COLOR,
    );
}

// Fire trap (rules.md section 4: 1 damage per second while a vehicle sits on
// it; never removed; land only, per rules.md section 1). Deliberately
// unmodelled: the trap is a cluster of living campfires in the presentation
// layer (`crate::fx::campfire_cluster`), and the fires alone are the trap --
// a static base would only pin the flames to one plastic prop again.

// Ice trap (rules.md section 4: halves the speed of a ground vehicle while it
// is on the field; never removed; land only, per rules.md section 1).
/// Radius of the ice rim at its base in px.
pub(super) const OBS_ICE_RIM_R: f64 = 12.0;
/// Radius of the ice rim at its top in px.
pub(super) const OBS_ICE_RIM_TOP_R: f64 = 11.0;
/// Height of the ice rim in px.
pub(super) const OBS_ICE_RIM_H: f64 = 1.0;
/// Colour of the frozen sheet (cold pale blue).
pub(super) const OBS_ICE_COLOR: [u8; 3] = [168, 208, 236];
/// Radius of the raised centre sheet at its base in px.
pub(super) const OBS_ICE_SHEET_R: f64 = 7.0;
/// Radius of the raised centre sheet at its top in px.
pub(super) const OBS_ICE_SHEET_TOP_R: f64 = 6.2;
/// Height of the centre sheet above the rim in px: the middle of the field
/// stays frozen solid, the way a fresh patch buckles.
pub(super) const OBS_ICE_SHEET_H: f64 = 1.4;
/// Colour of the raised centre sheet (older, thicker ice).
pub(super) const OBS_ICE_SHEET_COLOR: [u8; 3] = [196, 226, 246];
/// Facets of the centre sheet.
pub(super) const OBS_ICE_SHEET_SEGMENTS: usize = 10;
/// Number of clear ice shards standing on the field.
pub(super) const OBS_ICE_SHARDS: usize = 4;
/// Offset of each shard from the field centre in px.
pub(super) const OBS_ICE_SHARD_OFF: f64 = 7.5;
/// Base radius of an ice shard in px.
pub(super) const OBS_ICE_SHARD_R: f64 = 2.4;
/// Tip radius of an ice shard in px.
pub(super) const OBS_ICE_SHARD_TOP_R: f64 = 0.5;
/// Height of an ice shard above the rim in px.
pub(super) const OBS_ICE_SHARD_H: f64 = 6.5;
/// Facets of a shard cone.
pub(super) const OBS_ICE_SEGMENTS: usize = 6;
/// Colour of the shards (near-white ice; the shaded facets of the cones keep
/// them clearly paler than the sheet below).
pub(super) const OBS_ICE_SHARD_COLOR: [u8; 3] = [242, 250, 255];
/// Half-length of the bright slashes across the centre sheet in px. The
/// slashes are the sign that does not change: the flat disc the trap used to
/// be carried them too.
pub(super) const OBS_ICE_SLASH_R: f64 = 4.5;
/// Colour of the slashes.
pub(super) const OBS_ICE_SLASH_COLOR: [u8; 3] = [240, 250, 255];

/// Rendered ice trap (rules.md section 4): a frozen sheet in a rim, buckled in
/// the middle and bristling with clear shards -- a hazard a rolling vehicle
/// slips on, not a flat blue disc with two strokes on it.
fn push_ice_trap(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    cx: f64,
    cy: f64,
    z: f64,
) {
    let pad = z + constants::OBSTACLE_LIFT;
    push_cylinder(
        mesh,
        cx,
        cy,
        pad,
        OBS_ICE_RIM_R,
        OBS_ICE_RIM_TOP_R,
        OBS_ICE_RIM_H,
        OBS_SEGMENTS,
        OBS_ICE_COLOR,
    );
    let rim = pad + OBS_ICE_RIM_H;
    push_cylinder(
        mesh,
        cx,
        cy,
        rim,
        OBS_ICE_SHEET_R,
        OBS_ICE_SHEET_TOP_R,
        OBS_ICE_SHEET_H,
        OBS_ICE_SHEET_SEGMENTS,
        OBS_ICE_SHEET_COLOR,
    );
    let sheet = rim + OBS_ICE_SHEET_H;
    // Shards on the diagonals, so they never line up with the field grid.
    for i in 0..OBS_ICE_SHARDS {
        let a = std::f64::consts::FRAC_PI_4 + std::f64::consts::FRAC_PI_2 * i as f64;
        push_cylinder(
            mesh,
            cx + OBS_ICE_SHARD_OFF * a.cos(),
            cy + OBS_ICE_SHARD_OFF * a.sin(),
            pad,
            OBS_ICE_SHARD_R,
            OBS_ICE_SHARD_TOP_R,
            OBS_ICE_SHARD_H,
            OBS_ICE_SEGMENTS,
            OBS_ICE_SHARD_COLOR,
        );
    }
    // Two pale slashes across the sheet: the same sign the old flat disc
    // carried, now on top of the ice it belongs to.
    let r = OBS_ICE_SLASH_R;
    push_beam(
        lines,
        cx - r,
        cy - r * 0.5,
        sheet + 0.1,
        cx + r,
        cy + r * 0.5,
        sheet + 0.1,
        OBS_ICE_SLASH_COLOR,
    );
    push_beam(
        lines,
        cx + r * 0.5,
        cy + r,
        sheet + 0.1,
        cx - r * 0.5,
        cy - r,
        sheet + 0.1,
        OBS_ICE_SLASH_COLOR,
    );
}
