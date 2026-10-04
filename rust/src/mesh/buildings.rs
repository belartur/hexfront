//! The eight building models (rules.md section 3).
//!
//! Every kind stands on the same dark hexagonal foundation slab, which is what
//! makes them read as one family; above it each kind has its own model. The
//! owner colour comes from the player, so only the medical green and the
//! landing markings are fixed here.

use super::{
    AlphaVertex, TriangleSoup, push_beam, push_box, push_cross, push_cylinder, push_hex_prism,
    push_oriented_box, push_oriented_slope, push_ring, tile_top_z,
};
use crate::constants;
use crate::game::Game;
use crate::math::dist2;

pub(super) fn building_color(b: &crate::entities::Building) -> [u8; 3] {
    match b.owner {
        Some(id) => constants::player_color(id),
        None => constants::NEUTRAL_COLOR,
    }
}

pub(super) fn push_building(
    game: &Game,
    b: &crate::entities::Building,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
) {
    use crate::entities::BuildingKind;
    let (cx, cy) = b.pos(game.board.side);
    let z = tile_top_z(&game.board, b.tile);
    let color = building_color(b);
    // Every building stands on the same dark hexagonal foundation slab, so
    // the eight kinds share one visual family and none of them floats on the
    // bare grey tile top. The slim healing mast gets a narrower plinth,
    // because a wide empty pad under it would swallow the tower. The slab
    // floats like the flat obstacle markers: coplanar with the tile top it
    // would lose the depth race against the terrain (see
    // `constants::OBSTACLE_LIFT`).
    let found_r = if b.kind == BuildingKind::HealTower {
        BLD_HEAL_FOUND_R
    } else {
        BLD_FOUND_R
    };
    push_hex_prism(
        mesh,
        cx,
        cy,
        z + constants::OBSTACLE_LIFT,
        found_r,
        BLD_FOUND_H,
        constants::shade(color, BLD_DARK),
    );
    let slab = z + constants::OBSTACLE_LIFT + BLD_FOUND_H;
    match b.kind {
        BuildingKind::BaseTank => push_base_tank(mesh, lines, cx, cy, slab, color),
        BuildingKind::BaseHelicopter => push_base_helicopter(mesh, lines, cx, cy, slab, color),
        BuildingKind::BaseHovercraft => push_base_hovercraft(mesh, lines, cx, cy, slab, color),
        BuildingKind::BaseBuffer => push_base_buffer(mesh, lines, cx, cy, slab, color),
        BuildingKind::TurretNormal | BuildingKind::TurretRapid | BuildingKind::TurretRocket => {
            push_turret(mesh, lines, b, cx, cy, slab, color);
        }
        BuildingKind::HealTower => push_heal_tower(mesh, lines, cx, cy, slab, color),
    }
}

// ---------------------------------------------------------------------------
// Building parts (rules.md section 3; every value is a rendering-only size in
// px, exactly like the tank and helicopter constants -- the owner colour comes
// from the player, only the medical green and the landing markings are fixed).
// ---------------------------------------------------------------------------

/// Radius of the hexagonal foundation slab every building stands on in px.
/// Smaller than the field inradius, so the slab never spills into a
/// neighbouring field.
pub(super) const BLD_FOUND_R: f64 = 21.0;
/// Radius of the (smaller) foundation slab of the slim healing mast in px;
/// a wide empty pad under a 6-px shaft would read as a plaza, not a plinth.
pub(super) const BLD_HEAL_FOUND_R: f64 = 14.0;
/// Height of the foundation slab in px.
pub(super) const BLD_FOUND_H: f64 = 2.5;
/// Shade factor of the foundation and other dark structural parts.
pub(super) const BLD_DARK: f64 = 0.55;
/// Thickness of a roof plate in px.
pub(super) const BLD_ROOF_H: f64 = 2.0;
/// Highest point any building part may reach above its field in px: the unit
/// counter and the floating texts sit right next to the structure, and a
/// building must never hide the field behind it. The cap is a design limit
/// checked by the mesh tests, not a value the builder reads back, so it is
/// compiled for tests alone.
#[cfg(test)]
pub(super) const BLD_MAX_H: f64 = 30.0;
/// Furthest horizontal distance of a building part from its field centre in
/// px. A gun barrel overhangs its field (like a tank's), everything else
/// stays inside it. Also a design limit checked by the mesh tests, so like
/// [`BLD_MAX_H`] it is compiled for tests alone.
#[cfg(test)]
pub(super) const BLD_MAX_REACH: f64 = 27.0;
/// Light green of every healing part (tanks of the buffer base, crosses). It
/// matches the healed-range tint, so the support role reads the same way.
pub(super) const BLD_MED_COLOR: [u8; 3] = [150, 245, 150];
/// Bright colour of a healing cross.
pub(super) const BLD_CROSS_COLOR: [u8; 3] = [200, 255, 200];
/// Colour of the landing markings and the mast light of a helicopter base.
pub(super) const BLD_MARK_COLOR: [u8; 3] = [240, 240, 240];

// Tank base (rules.md section 3: the plant that produces tanks).
/// Footprint of the assembly hall along x in px.
pub(super) const BLD_TANK_HALL_X: f64 = 24.0;
/// Footprint of the assembly hall along y in px.
pub(super) const BLD_TANK_HALL_Y: f64 = 21.0;
/// Height of the assembly hall in px.
pub(super) const BLD_TANK_HALL_H: f64 = 11.0;
/// Width of the dark gate along its wall in px.
pub(super) const BLD_TANK_GATE_W: f64 = 9.0;
/// Thickness of the gate plate in px. Just proud of the wall: it reads as an
/// opening, and being this flat its side faces never show.
pub(super) const BLD_TANK_GATE_OUT: f64 = 0.4;
/// Height of the gate in px.
pub(super) const BLD_TANK_GATE_H: f64 = 7.0;
/// Length of the entry ramp in px.
pub(super) const BLD_TANK_RAMP_LEN: f64 = 9.0;
/// Width of the entry ramp in px.
pub(super) const BLD_TANK_RAMP_WID: f64 = 16.0;
/// Height of the ramp where it meets the hall in px.
pub(super) const BLD_TANK_RAMP_H: f64 = 6.5;
/// Radius of a roof vent in px.
pub(super) const BLD_TANK_VENT_R: f64 = 2.8;
/// Roof radius of a roof vent in px.
pub(super) const BLD_TANK_VENT_TOP_R: f64 = 2.3;
/// Height of a roof vent in px.
pub(super) const BLD_TANK_VENT_H: f64 = 5.0;
/// Distance of each roof vent from the hall centre along x in px.
pub(super) const BLD_TANK_VENT_OFF: f64 = 6.0;

// Buffer base (rules.md section 3: the medical plant; section 5.4 makes the
// buffer the healing vehicle, so its base speaks the same colour).
/// Footprint of the buffer hall along x in px.
pub(super) const BLD_BUF_HALL_X: f64 = 22.0;
/// Footprint of the buffer hall along y in px.
pub(super) const BLD_BUF_HALL_Y: f64 = 19.0;
/// Height of the buffer hall in px.
pub(super) const BLD_BUF_HALL_H: f64 = 10.0;
/// Radius of one light-green tank on the roof in px.
pub(super) const BLD_BUF_TANK_R: f64 = 4.0;
/// Height of one tank on the roof in px.
pub(super) const BLD_BUF_TANK_H: f64 = 7.0;
/// Distance of each tank from the hall centre along x in px.
pub(super) const BLD_BUF_TANK_OFF: f64 = 6.5;
/// Side of the square plinth carrying the healing cross in px.
pub(super) const BLD_BUF_PLINTH: f64 = 8.0;
/// Height of the cross plinth in px.
pub(super) const BLD_BUF_PLINTH_H: f64 = 3.0;

// Helicopter base (rules.md section 3: the round landing pad).
/// Radius of the landing pad in px. Small enough that the control shack at
/// its edge still fits on the foundation slab beside it.
pub(super) const BLD_PAD_R: f64 = 14.5;
/// Height of the pad slab in px.
pub(super) const BLD_PAD_H: f64 = 2.5;
/// Facets of the pad cylinder.
pub(super) const BLD_PAD_SEGMENTS: usize = 20;
/// Radius of the thin ring painted on the pad in px.
pub(super) const BLD_PAD_RING_R: f64 = 10.5;
/// Facets of the painted ring (a circle, unlike the hexagonal slab below).
pub(super) const BLD_PAD_RING_SEGMENTS: usize = 24;
/// Half-width of the painted landing H in px.
pub(super) const BLD_MARK_HW: f64 = 4.0;
/// Half-height of the painted landing H in px.
pub(super) const BLD_MARK_HH: f64 = 5.0;
/// Footprint of the control shack at the pad edge in px.
pub(super) const BLD_SHACK_SIDE: f64 = 8.0;
/// Height of the control shack in px.
pub(super) const BLD_SHACK_H: f64 = 6.0;
/// Offset of the shack from the pad centre along x in px; positive, so the
/// shack stands on the side of the pad the camera sees.
pub(super) const BLD_SHACK_X: f64 = 12.0;
/// Offset of the shack from the pad centre along y in px.
pub(super) const BLD_SHACK_Y: f64 = -9.0;
/// Height of the shack mast in px.
pub(super) const BLD_MAST_H: f64 = 5.0;

// Hovercraft base (rules.md section 3: the low dock).
/// Footprint of the dock along x in px.
pub(super) const BLD_DOCK_X: f64 = 24.0;
/// Footprint of the dock along y in px.
pub(super) const BLD_DOCK_Y: f64 = 22.0;
/// Height of the dock slab in px.
pub(super) const BLD_DOCK_H: f64 = 3.0;
/// Length of the sloped bow ramp in px.
pub(super) const BLD_DOCK_BOW_LEN: f64 = 8.0;
/// Distance of each guide rail from the dock centre line in px.
pub(super) const BLD_DOCK_RAIL_OFF: f64 = 5.5;
/// Width of one guide rail in px.
pub(super) const BLD_DOCK_RAIL_WID: f64 = 3.0;
/// Height of one guide rail in px.
pub(super) const BLD_DOCK_RAIL_H: f64 = 2.6;
/// Shrink of the rails relative to the dock length in px, so they stay on it.
pub(super) const BLD_DOCK_RAIL_SHORT: f64 = 6.0;
/// Radius of one blower housing in px.
pub(super) const BLD_DOCK_FAN_R: f64 = 3.4;
/// Roof radius of one blower housing in px.
pub(super) const BLD_DOCK_FAN_TOP_R: f64 = 2.8;
/// Height of one blower housing in px.
pub(super) const BLD_DOCK_FAN_H: f64 = 2.8;
/// Offset of the blower housings from the dock centre in px; outside the
/// guide rails, so they stay visible next to them.
pub(super) const BLD_DOCK_FAN_OFF: f64 = 9.0;
/// Facets of a blower housing.
pub(super) const BLD_DOCK_FAN_SEGMENTS: usize = 6;

// Turrets (rules.md section 10; the weapon on the roof differs per kind).
/// Radius of the parapet base in px. Kept clearly smaller than the reach of
/// any weapon, so the gun always sticks out of the emplacement.
pub(super) const BLD_TUR_PARA_R: f64 = 12.5;
/// Roof radius of the parapet base in px.
pub(super) const BLD_TUR_PARA_TOP_R: f64 = 11.0;
/// Height of the parapet base in px.
pub(super) const BLD_TUR_PARA_H: f64 = 3.5;
/// Radius of the turret body in px.
pub(super) const BLD_TUR_BODY_R: f64 = 9.5;
/// Roof radius of the turret body in px.
pub(super) const BLD_TUR_BODY_TOP_R: f64 = 8.5;
/// Height of the turret body in px.
pub(super) const BLD_TUR_BODY_H: f64 = 4.0;
/// Radius of the armoured dome in px.
pub(super) const BLD_TUR_DOME_R: f64 = 7.5;
/// Roof radius of the armoured dome in px.
pub(super) const BLD_TUR_DOME_TOP_R: f64 = 6.0;
/// Height of the armoured dome in px.
pub(super) const BLD_TUR_DOME_H: f64 = 3.5;
/// Facets of every turret cylinder (low-poly, flat-shaded look).
pub(super) const BLD_TUR_SEGMENTS: usize = 8;
/// Shift of the mantlet towards the aim direction in px.
pub(super) const BLD_TUR_MANTLET_SHIFT: f64 = 5.5;
/// Length of the mantlet wedge along the aim direction in px.
pub(super) const BLD_TUR_MANTLET_LEN: f64 = 5.5;
/// Width of the mantlet in px.
pub(super) const BLD_TUR_MANTLET_WID: f64 = 8.0;
/// Distance from the turret centre where a gun barrel starts in px; the rear
/// end hides inside the turret, so no gap opens when the turret turns.
pub(super) const BLD_TUR_BARREL_GAP: f64 = 3.0;
/// Length of the ordinary turret barrel in px.
pub(super) const BLD_TUR_BARREL_LEN: f64 = 20.0;
/// Cross-section of the ordinary turret barrel in px.
pub(super) const BLD_TUR_BARREL_WID: f64 = 3.0;
/// Height of the barrel axis above the dome roof in px.
pub(super) const BLD_TUR_BARREL_LIFT: f64 = 1.4;
/// Length of the muzzle brake in px.
pub(super) const BLD_TUR_MUZZLE_LEN: f64 = 4.0;
/// Width of the muzzle brake in px.
pub(super) const BLD_TUR_MUZZLE_WID: f64 = 4.0;
/// Height of the muzzle brake in px.
pub(super) const BLD_TUR_MUZZLE_H: f64 = 3.4;
/// Length of one of the two rapid-fire barrels in px.
pub(super) const BLD_TUR_TWIN_LEN: f64 = 12.0;
/// Cross-section of a rapid-fire barrel in px.
pub(super) const BLD_TUR_TWIN_WID: f64 = 2.2;
/// Distance of each twin barrel from the aim axis in px.
pub(super) const BLD_TUR_TWIN_OFF: f64 = 2.6;
/// Radius of the ammo drum of the rapid turret in px.
pub(super) const BLD_TUR_DRUM_R: f64 = 3.2;
/// Height of the ammo drum in px.
pub(super) const BLD_TUR_DRUM_H: f64 = 3.0;
/// Rearward shift of the ammo drum in px.
pub(super) const BLD_TUR_DRUM_SHIFT: f64 = 6.5;
/// Facets of the ammo drum.
pub(super) const BLD_TUR_DRUM_SEGMENTS: usize = 8;
/// Shift of the rocket rack towards the aim direction in px.
pub(super) const BLD_TUR_RACK_SHIFT: f64 = 3.0;
/// Length of the rocket rack in px.
pub(super) const BLD_TUR_RACK_LEN: f64 = 13.0;
/// Width of the rocket rack in px.
pub(super) const BLD_TUR_RACK_WID: f64 = 13.0;
/// Height of the rack rear edge above the dome roof in px.
pub(super) const BLD_TUR_RACK_BACK_H: f64 = 2.5;
/// Height of the rack front edge above the dome roof in px (tilted up).
pub(super) const BLD_TUR_RACK_FRONT_H: f64 = 8.5;
/// Shift of the rocket tubes towards the aim direction in px.
pub(super) const BLD_TUR_TUBE_SHIFT: f64 = 3.0;
/// Length of one rocket tube in px.
pub(super) const BLD_TUR_TUBE_LEN: f64 = 9.0;
/// Cross-section of one rocket tube in px.
pub(super) const BLD_TUR_TUBE_WID: f64 = 2.8;
/// Distance of each tube from the aim axis in px.
pub(super) const BLD_TUR_TUBE_OFF: f64 = 3.2;
/// Height of the lower tube row above the dome roof in px.
pub(super) const BLD_TUR_TUBE_RISE: f64 = 3.2;
/// Extra height of the upper tube row in px.
pub(super) const BLD_TUR_TUBE_STACK: f64 = 2.8;
/// Height of the turret antenna above the dome roof in px.
pub(super) const BLD_TUR_ANTENNA_H: f64 = 6.0;

// Healing tower (rules.md section 11).
/// Radius of the tower shaft at its base in px. Slim on purpose: the mast has
/// to read as a tower, not as another squat emplacement.
pub(super) const BLD_HEAL_SHAFT_R: f64 = 6.0;
/// Radius of the tower shaft at its top in px.
pub(super) const BLD_HEAL_SHAFT_TOP_R: f64 = 5.0;
/// Height of the shaft in px.
pub(super) const BLD_HEAL_SHAFT_H: f64 = 14.0;
/// Radius of the light-green ring around the shaft in px.
pub(super) const BLD_HEAL_BAND_R: f64 = 6.8;
/// Height of the ring around the shaft in px.
pub(super) const BLD_HEAL_BAND_H: f64 = 1.6;
/// Height of the ring above the slab in px.
pub(super) const BLD_HEAL_BAND_LIFT: f64 = 4.5;
/// Radius of the crown at its base in px (only a little wider than the
/// shaft, so the silhouette stays a mast with a head).
pub(super) const BLD_HEAL_CROWN_R: f64 = 8.0;
/// Radius of the crown at its top in px.
pub(super) const BLD_HEAL_CROWN_TOP_R: f64 = 7.0;
/// Height of the crown in px.
pub(super) const BLD_HEAL_CROWN_H: f64 = 3.0;
/// Radius of the roof dome in px.
pub(super) const BLD_HEAL_DOME_R: f64 = 7.0;
/// Roof radius of the dome in px.
pub(super) const BLD_HEAL_DOME_TOP_R: f64 = 2.6;
/// Height of the dome in px.
pub(super) const BLD_HEAL_DOME_H: f64 = 2.6;
/// Facets of the tower cylinders.
pub(super) const BLD_HEAL_SEGMENTS: usize = 8;
/// Offset of each corner rib from the tower centre in px.
pub(super) const BLD_HEAL_RIB_OFF: f64 = 5.6;

/// Aim direction of a turret gun as a unit `(dx, dy)` vector in world space.
///
/// Points at the last target the simulation shot at (rules.md section 10).
/// A turret that never fired keeps the historical fallback of aiming east,
/// so a level always renders the same way.
pub(super) fn turret_aim(b: &crate::entities::Building, cx: f64, cy: f64) -> (f64, f64) {
    match b.last_target_pos {
        Some((tx, ty)) => {
            let d = dist2((tx, ty), (cx, cy)).sqrt().max(1e-6);
            ((tx - cx) / d, (ty - cy) / d)
        }
        None => (1.0, 0.0),
    }
}

/// Tank base: an assembly hall with a sloped entry ramp, a dark gate, two
/// roof vents and a panel seam (rules.md section 3: the plant producing
/// tanks). The hall is the tallest of the four bases, so the tank base stays
/// apart from the low hovercraft dock and the flat helicopter pad.
fn push_base_tank(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    cx: f64,
    cy: f64,
    slab: f64,
    color: [u8; 3],
) {
    let dark = constants::shade(color, 0.7);
    let darker = constants::shade(color, 0.5);
    // Entry ramp and gate sit on the +y wall: the isometric camera looks from
    // +x/+y, so only those two walls (and the roof) are actually visible.
    let front = cy + BLD_TANK_HALL_Y / 2.0;
    // Entry ramp: a sloped plate rising from the field to the hall floor, so
    // a produced tank has somewhere to roll out of.
    push_oriented_slope(
        mesh,
        cx,
        front + BLD_TANK_RAMP_LEN / 2.0,
        slab,
        BLD_TANK_RAMP_LEN,
        BLD_TANK_RAMP_WID,
        BLD_TANK_RAMP_H,
        0.4,
        0.0,
        1.0,
        darker,
    );
    push_box(
        mesh,
        cx,
        cy,
        slab,
        BLD_TANK_HALL_X,
        BLD_TANK_HALL_Y,
        BLD_TANK_HALL_H,
        color,
    );
    // Dark gate on the +x wall: the ramp at the front would hide it on the
    // +y wall, and a thin plate proud of the wall reads as an opening
    // instead of as a painted rectangle.
    push_box(
        mesh,
        cx + BLD_TANK_HALL_X / 2.0 + BLD_TANK_GATE_OUT / 2.0,
        cy,
        slab,
        BLD_TANK_GATE_OUT,
        BLD_TANK_GATE_W,
        BLD_TANK_GATE_H,
        constants::shade(color, 0.25),
    );
    let roof = slab + BLD_TANK_HALL_H;
    push_box(
        mesh,
        cx,
        cy,
        roof,
        BLD_TANK_HALL_X + 2.0,
        BLD_TANK_HALL_Y + 2.0,
        BLD_ROOF_H,
        dark,
    );
    let deck = roof + BLD_ROOF_H;
    for side in [-1.0, 1.0] {
        push_cylinder(
            mesh,
            cx + side * BLD_TANK_VENT_OFF,
            cy + 3.0,
            deck,
            BLD_TANK_VENT_R,
            BLD_TANK_VENT_TOP_R,
            BLD_TANK_VENT_H,
            6,
            darker,
        );
    }
    // Panel seam across the rear half of the roof, a thin stroke like the
    // tank's tread marks.
    let seam = constants::shade(color, 0.35);
    push_beam(
        lines,
        cx - 9.0,
        cy - 6.0,
        deck + 0.2,
        cx + 9.0,
        cy - 6.0,
        deck + 0.2,
        seam,
    );
}

/// Buffer base: the tank-base hall carrying two light-green tanks and the
/// healing cross on a raised plinth (rules.md sections 3 and 5.4; the base
/// speaks the colour of the healing vehicle it produces).
fn push_base_buffer(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    cx: f64,
    cy: f64,
    slab: f64,
    color: [u8; 3],
) {
    let dark = constants::shade(color, 0.7);
    let darker = constants::shade(color, 0.5);
    push_box(
        mesh,
        cx,
        cy,
        slab,
        BLD_BUF_HALL_X,
        BLD_BUF_HALL_Y,
        BLD_BUF_HALL_H,
        color,
    );
    let roof = slab + BLD_BUF_HALL_H;
    push_box(
        mesh,
        cx,
        cy,
        roof,
        BLD_BUF_HALL_X + 2.0,
        BLD_BUF_HALL_Y + 2.0,
        BLD_ROOF_H,
        dark,
    );
    let deck = roof + BLD_ROOF_H;
    for side in [-1.0, 1.0] {
        push_cylinder(
            mesh,
            cx + side * BLD_BUF_TANK_OFF,
            cy - 2.0,
            deck,
            BLD_BUF_TANK_R,
            BLD_BUF_TANK_R,
            BLD_BUF_TANK_H,
            8,
            BLD_MED_COLOR,
        );
    }
    // Raised plinth with the cross at the front of the roof.
    push_box(
        mesh,
        cx,
        cy + 5.5,
        deck,
        BLD_BUF_PLINTH,
        BLD_BUF_PLINTH,
        BLD_BUF_PLINTH_H,
        darker,
    );
    push_cross(
        lines,
        cx,
        cy + 5.5,
        deck + BLD_BUF_PLINTH_H + 0.4,
        BLD_CROSS_COLOR,
    );
    // Green stripe along the visible (+y) wall, in the same healing colour.
    push_beam(
        lines,
        cx - 7.0,
        cy + BLD_BUF_HALL_Y / 2.0 + 0.2,
        slab + 4.5,
        cx + 7.0,
        cy + BLD_BUF_HALL_Y / 2.0 + 0.2,
        slab + 4.5,
        BLD_MED_COLOR,
    );
}

/// Helicopter base: a round pad painted with a white H inside a thin ring,
/// with a control shack and its mast at the pad edge (rules.md section 3).
/// The landing mark makes the kind unmistakable even at the smallest zoom.
fn push_base_helicopter(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    cx: f64,
    cy: f64,
    slab: f64,
    color: [u8; 3],
) {
    let dark = constants::shade(color, 0.7);
    push_cylinder(
        mesh,
        cx,
        cy,
        slab,
        BLD_PAD_R,
        BLD_PAD_R,
        BLD_PAD_H,
        BLD_PAD_SEGMENTS,
        constants::shade(color, 0.85),
    );
    let deck = slab + BLD_PAD_H;
    // Painted markings: a ring plus the H, thin strokes floating just above
    // the pad (a coplanar line loses the depth race against its own disc).
    push_ring(
        lines,
        cx,
        cy,
        deck + 0.2,
        BLD_PAD_RING_R,
        BLD_PAD_RING_SEGMENTS,
        BLD_MARK_COLOR,
        130,
    );
    let mark = deck + 0.3;
    for side in [-1.0, 1.0] {
        push_beam(
            lines,
            cx + side * BLD_MARK_HW,
            cy - BLD_MARK_HH,
            mark,
            cx + side * BLD_MARK_HW,
            cy + BLD_MARK_HH,
            mark,
            BLD_MARK_COLOR,
        );
    }
    push_beam(
        lines,
        cx - BLD_MARK_HW,
        cy,
        mark,
        cx + BLD_MARK_HW,
        cy,
        mark,
        BLD_MARK_COLOR,
    );
    // Control shack with a mast at the pad edge (inside the foundation slab,
    // on the visible +x side of the pad).
    let shack = (cx + BLD_SHACK_X, cy + BLD_SHACK_Y);
    push_box(
        mesh,
        shack.0,
        shack.1,
        slab,
        BLD_SHACK_SIDE,
        BLD_SHACK_SIDE,
        BLD_SHACK_H,
        dark,
    );
    push_beam(
        lines,
        shack.0,
        shack.1,
        slab + BLD_SHACK_H,
        shack.0,
        shack.1,
        slab + BLD_SHACK_H + BLD_MAST_H,
        BLD_MARK_COLOR,
    );
    push_box(
        mesh,
        shack.0,
        shack.1,
        slab + BLD_SHACK_H + BLD_MAST_H,
        2.0,
        2.0,
        2.0,
        BLD_MARK_COLOR,
    );
}

/// Hovercraft base: a low, wide dock with two guide rails, a sloped bow ramp
/// and four blower housings (rules.md section 3). Flat and wide instead of
/// tall, so it never reads like a tank plant.
fn push_base_hovercraft(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    cx: f64,
    cy: f64,
    slab: f64,
    color: [u8; 3],
) {
    let dark = constants::shade(color, 0.7);
    let darker = constants::shade(color, 0.5);
    push_box(
        mesh, cx, cy, slab, BLD_DOCK_X, BLD_DOCK_Y, BLD_DOCK_H, color,
    );
    let deck = slab + BLD_DOCK_H;
    // Sloped bow on the front (+y) side: the slipway a hovercraft leaves on.
    push_oriented_slope(
        mesh,
        cx,
        cy + BLD_DOCK_Y / 2.0 + BLD_DOCK_BOW_LEN / 2.0,
        slab,
        BLD_DOCK_BOW_LEN,
        BLD_DOCK_X,
        0.4,
        BLD_DOCK_H,
        0.0,
        -1.0,
        dark,
    );
    // Two guide rails along the dock; the channel between them is where a
    // hovercraft is serviced.
    for side in [-1.0, 1.0] {
        push_oriented_box(
            mesh,
            cx + side * BLD_DOCK_RAIL_OFF,
            cy,
            deck,
            BLD_DOCK_X - BLD_DOCK_RAIL_SHORT,
            BLD_DOCK_RAIL_WID,
            BLD_DOCK_RAIL_H,
            0.0,
            1.0,
            darker,
        );
    }
    // Blower housings in the corners: the dock feeds hovercraft skirts.
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            push_cylinder(
                mesh,
                cx + sx * BLD_DOCK_FAN_OFF,
                cy + sy * BLD_DOCK_FAN_OFF,
                deck,
                BLD_DOCK_FAN_R,
                BLD_DOCK_FAN_TOP_R,
                BLD_DOCK_FAN_H,
                BLD_DOCK_FAN_SEGMENTS,
                dark,
            );
        }
    }
    // Centre line of the slipway, a thin marking like the landing pad ring.
    push_beam(
        lines,
        cx,
        cy - BLD_DOCK_Y / 2.0 + 2.0,
        deck + 0.2,
        cx,
        cy + BLD_DOCK_Y / 2.0 - 2.0,
        deck + 0.2,
        BLD_MARK_COLOR,
    );
}

/// Turret: a stepped emplacement (parapet, turret body, armoured dome) whose
/// weapon turns towards the last target the gun fired at (rules.md section
/// 10). The three kinds differ exactly where it matters: one long barrel, two
/// short barrels with an ammo drum, or a tilted rack of four rocket tubes.
fn push_turret(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    b: &crate::entities::Building,
    cx: f64,
    cy: f64,
    slab: f64,
    color: [u8; 3],
) {
    use crate::constants::TurretKind;
    let dark = constants::shade(color, 0.7);
    let darker = constants::shade(color, 0.5);
    push_cylinder(
        mesh,
        cx,
        cy,
        slab,
        BLD_TUR_PARA_R,
        BLD_TUR_PARA_TOP_R,
        BLD_TUR_PARA_H,
        BLD_TUR_SEGMENTS,
        darker,
    );
    let barbette = slab + BLD_TUR_PARA_H;
    push_cylinder(
        mesh,
        cx,
        cy,
        barbette,
        BLD_TUR_BODY_R,
        BLD_TUR_BODY_TOP_R,
        BLD_TUR_BODY_H,
        BLD_TUR_SEGMENTS,
        dark,
    );
    let body = barbette + BLD_TUR_BODY_H;
    push_cylinder(
        mesh,
        cx,
        cy,
        body,
        BLD_TUR_DOME_R,
        BLD_TUR_DOME_TOP_R,
        BLD_TUR_DOME_H,
        BLD_TUR_SEGMENTS,
        color,
    );
    let roof = body + BLD_TUR_DOME_H;
    let (ax, ay) = turret_aim(b, cx, cy);
    let (px, py) = (-ay, ax);
    // Sloped mantlet (a gun shield) on the turret front, turning with the
    // weapon so the battery never looks symmetrical.
    push_oriented_slope(
        mesh,
        cx + ax * BLD_TUR_MANTLET_SHIFT,
        cy + ay * BLD_TUR_MANTLET_SHIFT,
        body,
        BLD_TUR_MANTLET_LEN,
        BLD_TUR_MANTLET_WID,
        1.2,
        BLD_TUR_DOME_H + 0.8,
        ax,
        ay,
        dark,
    );
    // Whip antenna on the rear dome roof, a stroke like the tank's antenna.
    let antenna = (cx - ax * 4.5 + px * 2.5, cy - ay * 4.5 + py * 2.5);
    push_beam(
        lines,
        antenna.0,
        antenna.1,
        roof,
        antenna.0,
        antenna.1,
        roof + BLD_TUR_ANTENNA_H,
        darker,
    );
    let barrel = roof + BLD_TUR_BARREL_LIFT;
    match crate::entities::turret_kind_of(b.kind).unwrap_or(TurretKind::Normal) {
        TurretKind::Normal => {
            // One long barrel ending in a wider muzzle brake.
            let mid = BLD_TUR_BARREL_GAP + BLD_TUR_BARREL_LEN / 2.0;
            push_oriented_box(
                mesh,
                cx + ax * mid,
                cy + ay * mid,
                barrel,
                BLD_TUR_BARREL_LEN,
                BLD_TUR_BARREL_WID,
                BLD_TUR_BARREL_WID,
                ax,
                ay,
                dark,
            );
            let muzzle = BLD_TUR_BARREL_GAP + BLD_TUR_BARREL_LEN - BLD_TUR_MUZZLE_LEN / 2.0;
            push_oriented_box(
                mesh,
                cx + ax * muzzle,
                cy + ay * muzzle,
                barrel - 0.3,
                BLD_TUR_MUZZLE_LEN,
                BLD_TUR_MUZZLE_WID,
                BLD_TUR_MUZZLE_H,
                ax,
                ay,
                darker,
            );
        }
        TurretKind::Rapid => {
            // Twin short barrels side by side, plus an ammo drum on the rear.
            let mid = BLD_TUR_BARREL_GAP + BLD_TUR_TWIN_LEN / 2.0;
            let reach = BLD_TUR_BARREL_GAP + BLD_TUR_TWIN_LEN;
            for side in [-1.0, 1.0] {
                let (ox, oy) = (px * side * BLD_TUR_TWIN_OFF, py * side * BLD_TUR_TWIN_OFF);
                push_oriented_box(
                    mesh,
                    cx + ax * mid + ox,
                    cy + ay * mid + oy,
                    barrel,
                    BLD_TUR_TWIN_LEN,
                    BLD_TUR_TWIN_WID,
                    BLD_TUR_TWIN_WID,
                    ax,
                    ay,
                    dark,
                );
                push_oriented_box(
                    mesh,
                    cx + ax * (reach - 1.0) + ox,
                    cy + ay * (reach - 1.0) + oy,
                    barrel - 0.2,
                    2.0,
                    BLD_TUR_TWIN_WID + 0.8,
                    BLD_TUR_TWIN_WID + 0.8,
                    ax,
                    ay,
                    darker,
                );
            }
            push_cylinder(
                mesh,
                cx - ax * BLD_TUR_DRUM_SHIFT,
                cy - ay * BLD_TUR_DRUM_SHIFT,
                roof - 0.5,
                BLD_TUR_DRUM_R,
                BLD_TUR_DRUM_R,
                BLD_TUR_DRUM_H,
                BLD_TUR_DRUM_SEGMENTS,
                darker,
            );
        }
        TurretKind::Rocket => {
            // Tilted rack carrying four tubes with dark muzzle rings: a rocket
            // battery can never be mistaken for a gun barrel.
            push_oriented_slope(
                mesh,
                cx + ax * BLD_TUR_RACK_SHIFT,
                cy + ay * BLD_TUR_RACK_SHIFT,
                roof,
                BLD_TUR_RACK_LEN,
                BLD_TUR_RACK_WID,
                BLD_TUR_RACK_BACK_H,
                BLD_TUR_RACK_FRONT_H,
                ax,
                ay,
                dark,
            );
            let mid = BLD_TUR_TUBE_SHIFT + BLD_TUR_TUBE_LEN / 2.0;
            let front = BLD_TUR_TUBE_SHIFT + BLD_TUR_TUBE_LEN;
            for side in [-1.0, 1.0] {
                for row in 0..2 {
                    let (ox, oy) = (px * side * BLD_TUR_TUBE_OFF, py * side * BLD_TUR_TUBE_OFF);
                    let z = roof + BLD_TUR_TUBE_RISE + row as f64 * BLD_TUR_TUBE_STACK;
                    push_oriented_box(
                        mesh,
                        cx + ax * mid + ox,
                        cy + ay * mid + oy,
                        z,
                        BLD_TUR_TUBE_LEN,
                        BLD_TUR_TUBE_WID,
                        BLD_TUR_TUBE_WID,
                        ax,
                        ay,
                        dark,
                    );
                    let w = BLD_TUR_TUBE_WID * 0.7;
                    push_oriented_box(
                        mesh,
                        cx + ax * front + ox,
                        cy + ay * front + oy,
                        z + 0.5,
                        1.4,
                        w,
                        w,
                        ax,
                        ay,
                        constants::shade(color, 0.2),
                    );
                }
            }
        }
    }
}

/// Healing tower: a slim shaft with a light-green ring, a wider crown, a
/// domed roof carrying the bright cross and four corner ribs (rules.md
/// section 11). The tall mast shape keeps it apart from the squat turret
/// emplacements, which also stand on a round base.
fn push_heal_tower(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    cx: f64,
    cy: f64,
    slab: f64,
    color: [u8; 3],
) {
    let dark = constants::shade(color, 0.7);
    let rib = constants::shade(color, 0.4);
    push_cylinder(
        mesh,
        cx,
        cy,
        slab,
        BLD_HEAL_SHAFT_R,
        BLD_HEAL_SHAFT_TOP_R,
        BLD_HEAL_SHAFT_H,
        BLD_HEAL_SEGMENTS,
        dark,
    );
    push_cylinder(
        mesh,
        cx,
        cy,
        slab + BLD_HEAL_BAND_LIFT,
        BLD_HEAL_BAND_R,
        BLD_HEAL_BAND_R,
        BLD_HEAL_BAND_H,
        BLD_HEAL_SEGMENTS,
        BLD_MED_COLOR,
    );
    let crown = slab + BLD_HEAL_SHAFT_H;
    push_cylinder(
        mesh,
        cx,
        cy,
        crown,
        BLD_HEAL_CROWN_R,
        BLD_HEAL_CROWN_TOP_R,
        BLD_HEAL_CROWN_H,
        BLD_HEAL_SEGMENTS,
        color,
    );
    let dome = crown + BLD_HEAL_CROWN_H;
    push_cylinder(
        mesh,
        cx,
        cy,
        dome,
        BLD_HEAL_DOME_R,
        BLD_HEAL_DOME_TOP_R,
        BLD_HEAL_DOME_H,
        BLD_HEAL_SEGMENTS,
        constants::shade(color, 0.85),
    );
    push_cross(lines, cx, cy, dome + BLD_HEAL_DOME_H + 0.4, BLD_CROSS_COLOR);
    // Four corner ribs along the shaft: thin strokes give the mast a profile.
    for (sx, sy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        let (rx, ry) = (cx + sx * BLD_HEAL_RIB_OFF, cy + sy * BLD_HEAL_RIB_OFF);
        push_beam(lines, rx, ry, slab, rx, ry, crown, rib);
    }
}
