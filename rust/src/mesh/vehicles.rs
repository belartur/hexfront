//! Vehicle models: tank, helicopter and hovercraft (rules.md section 5).
//!
//! Each is oriented along [`vehicle_heading`] so a moving vehicle faces the way
//! it drives, and the tank gun along [`tank_aim`] so it faces whatever it is
//! shooting. The helicopter shadow is built here too, because every part of it
//! reuses the px constants of the airframe and so has to live next to them.

use super::surface::{deck_z_of, vehicle_ground_z, vehicle_surface_z};
use super::terrain::{DeckQuad, bridge_deck_quad, bridge_deck_z};
use super::{
    AlphaVertex, RangeSoup, TriangleSoup, max_height, push_beam, push_box, push_cross,
    push_cylinder, push_disc, push_oriented_box, push_oriented_slope, push_range_rect,
    push_range_rect_on_deck,
};
use crate::constants;
use crate::game::Game;
use crate::math::dist2;

/// Flight heading of a vehicle as a unit `(fx, fy)` vector in world space.
///
/// Points at the next route waypoint; when the vehicle has nowhere to go
/// (empty route, arrived, ad-hoc test vehicle) it falls back to the leg it
/// came from, and finally to east (+x), so parked helicopters still face a
/// deterministic direction instead of snapping arbitrarily.
pub(super) fn vehicle_heading(game: &Game, v: &crate::entities::Vehicle) -> (f64, f64) {
    if v.route_index < v.route.len() {
        let (wx, wy) = game.board.center_world(v.route[v.route_index]);
        if let Some(h) = unit_dir(v, wx, wy) {
            return h;
        }
    }
    if v.route_index > 0 && v.route_index <= v.route.len() {
        let prev = v.route[v.route_index - 1];
        let (wx, wy) = game.board.center_world(prev);
        // Heading is where we came *from* reversed: from the previous
        // waypoint towards the current position.
        if let Some(h) = unit_dir(v, wx, wy) {
            return h;
        }
    }
    if let Some(src) = v.src_tile {
        let (wx, wy) = game.board.center_world(src);
        if let Some(h) = unit_dir(v, wx, wy) {
            return h;
        }
    }
    (1.0, 0.0)
}

/// Unit vector from `v` towards the world point `(tx, ty)`.
///
/// `None` when the target sits on top of the vehicle, where the direction is
/// undefined; the caller then falls back to the next candidate instead of
/// drawing a hull or a gun barrel at a random angle.
fn unit_dir(v: &crate::entities::Vehicle, tx: f64, ty: f64) -> Option<(f64, f64)> {
    let (dx, dy) = (tx - v.x, ty - v.y);
    let len = dist2((v.x, v.y), (tx, ty)).sqrt();
    (len > 1e-6).then(|| (dx / len, dy / len))
}

/// Aim direction of a tank turret as a unit `(ax, ay)` vector in world space.
///
/// The gun points at whatever the tank is currently shooting: the enemy
/// vehicle of its duel (rules.md section 9) or, when no duel is running, the
/// wall it shells on its way (section 4). With no target at all the turret
/// stays aligned with `heading` -- the chassis direction from
/// [`vehicle_heading`] -- so a marching column keeps its barrels forward.
/// `heading` is passed in because the caller needs it for the chassis too.
pub(super) fn tank_aim(
    game: &Game,
    v: &crate::entities::Vehicle,
    heading: (f64, f64),
) -> (f64, f64) {
    if let Some(tid) = v.combat_target
        && let Some(enemy) = game.vehicles.iter().find(|x| x.id == tid && !x.dead)
        && let Some(dir) = unit_dir(v, enemy.x, enemy.y)
    {
        return dir;
    }
    // A tank shelling from beyond detection range has no duel to aim at, but
    // its gun still points at the enemy it is shooting (rules.md section 5.1).
    if let Some(tid) = v.gun_target
        && let Some(enemy) = game.vehicles.iter().find(|x| x.id == tid && !x.dead)
        && let Some(dir) = unit_dir(v, enemy.x, enemy.y)
    {
        return dir;
    }
    if let Some(tile) = v.wall_target {
        let (wx, wy) = game.board.center_world(tile);
        if let Some(dir) = unit_dir(v, wx, wy) {
            return dir;
        }
    }
    heading
}

/// Rendered elevation of a vehicle in px.
///
/// A ground vehicle stands on the walkable surface below it — the bridge deck
/// when it drives along one, otherwise the terrain (ramps included; see
/// [`vehicle_surface_z`]). A helicopter ignores the terrain (rules.md section
/// 5.2), so it flies at a fixed altitude above the *highest* tile of the board
/// ([`helicopter_altitude`]): the altitude is constant for the whole level
/// instead of following every bump, and the shadow silhouette built by
/// [`push_helicopter_shadow`] still tells which tile the helicopter is over.
pub fn vehicle_z(game: &Game, v: &crate::entities::Vehicle) -> f64 {
    if v.kind == constants::VehicleKind::Helicopter {
        helicopter_altitude(game)
    } else {
        vehicle_surface_z(game, v)
    }
}

/// [`vehicle_z`] with the board height handed in (see [`helicopter_altitude`]):
/// the per-frame mesh builder caches it once per level instead of rescanning
/// the board for every helicopter.
pub fn vehicle_z_with_height(game: &Game, v: &crate::entities::Vehicle, max_height_px: f64) -> f64 {
    if v.kind == constants::VehicleKind::Helicopter {
        helicopter_altitude_px(max_height_px)
    } else {
        vehicle_surface_z(game, v)
    }
}

/// Rendered elevation of an explosion of the destroyed vehicle `w` in px.
///
/// A ground vehicle is drawn on the surface it stood on, so its blast starts
/// there -- a bridge deck included, the wreck may have been driving along one.
/// A helicopter explodes at its flight altitude, because rules.md section 5.2
/// makes it ignore terrain heights: the same [`helicopter_altitude`] the hull
/// was drawn at. The wreck is only a position now (the vehicle is already
/// gone), so the mode it was crossing in cannot be replayed; the deck is
/// therefore used whenever the wreck sits on one.
pub fn wreck_z(game: &Game, w: &crate::entities::Wreck) -> f64 {
    if w.kind == constants::VehicleKind::Helicopter {
        return helicopter_altitude_px(max_height(&game.board));
    }
    let tile = game.board.world_to_tile(w.x, w.y);
    match deck_z_of(&game.board, tile) {
        Some(z) if z > vehicle_ground_z(game, w.x, w.y) => z,
        _ => vehicle_ground_z(game, w.x, w.y),
    }
}

/// Fixed flight altitude of the helicopters of `game` in px.
///
/// Measured above the highest terrain of the board, so a helicopter never
/// hides behind a peak no matter where it crosses the map.
pub(super) fn helicopter_altitude(game: &Game) -> f64 {
    helicopter_altitude_px(max_height(&game.board))
}

/// Flight altitude over an already known board height: the same arithmetic as
/// [`helicopter_altitude`], callable without rescanning the board when the
/// caller caches [`max_height`] once per level.
pub(super) fn helicopter_altitude_px(max_height_px: f64) -> f64 {
    max_height_px + constants::HELICOPTER_ALTITUDE_PX
}

// ---------------------------------------------------------------------------
// Hovercraft (rules.md section 5.3: crosses water and land alike) and the
// buffer badge. Every size below is a rendering-only value in px, like the
// tank and helicopter constants; the hull takes the owner colour, the skirt
// and the fan duct are its dark shades, and only the cockpit glass has a fixed
// tint. The craft is deliberately built along the travel heading, because its
// boat-like bow, its stern fan and its rudder all have a front and a back.
// ---------------------------------------------------------------------------

/// Bottom radius of the inflated skirt in px: the widest part of the craft
/// and what makes it read as a hovering, low platform instead of a hull.
pub(super) const HOVER_SKIRT_R: f64 = 15.0;
/// Top radius of the skirt in px. Smaller than the bottom one, so the cylinder
/// leans inwards and the skirt reads as an inflated cushion rather than a
/// second deck stacked on the first.
const HOVER_SKIRT_TOP_R: f64 = 12.5;
/// Height of the skirt in px (the cushion the hull floats on).
const HOVER_SKIRT_H: f64 = 3.0;
/// Facets of the skirt: it is the widest ring of the craft, so it gets the
/// most facets of the three cylinders.
const HOVER_SKIRT_SEGMENTS: usize = 16;
/// Shade factor of the skirt: dark rubber, not another player-coloured deck.
const HOVER_SKIRT_SHADE: f64 = 0.45;
/// Length of the hull along the travel heading in px. Clearly longer than it
/// is wide, so the craft reads as a boat whose bow points somewhere.
pub(super) const HOVER_HULL_LEN: f64 = 24.0;
/// Width of the hull across the heading in px.
const HOVER_HULL_WID: f64 = 12.0;
/// Height of the hull box in px (it sits on the skirt).
const HOVER_HULL_H: f64 = 4.0;
/// Length of the raked bow wedge in px.
const HOVER_BOW_LEN: f64 = 6.0;
/// Width of the bow wedge in px.
const HOVER_BOW_WID: f64 = 9.0;
/// Height of the bow wedge at its rear edge in px. Its front edge stays flush
/// with the deck, so the wedge *is* the foredeck sloping down to the bow
/// instead of a block glued onto the hull front.
const HOVER_BOW_H: f64 = 2.5;
/// Length of the glazed cockpit box in px.
const HOVER_CANOPY_LEN: f64 = 7.0;
/// Width of the cockpit box in px.
const HOVER_CANOPY_WID: f64 = 8.0;
/// Height of the cockpit box in px.
const HOVER_CANOPY_H: f64 = 3.0;
/// Forward shift of the cockpit centre from the hull centre in px: it sits on
/// the foredeck, so the windscreen faces the direction of travel.
const HOVER_CANOPY_SHIFT: f64 = 4.0;
/// Elevation of the cockpit base relative to the hull top in px (slightly
/// sunk into it, so no gap opens at any heading).
const HOVER_CANOPY_SINK: f64 = -0.4;
/// Fixed tint of the cockpit glass: the same dark canopy tint the helicopter
/// uses, so both windscreens read as glass instead of a glaring white box.
pub(super) const HOVER_CANOPY_COLOR: [u8; 3] = [72, 106, 126];
/// Outer radius of the stern fan duct in px. Smaller than half the hull width,
/// so the duct sits on the deck instead of overhanging it.
const HOVER_DUCT_R: f64 = 5.8;
/// Roof radius of the fan duct in px (barely tapered, so the housing is a drum
/// rather than a cone).
const HOVER_DUCT_TOP_R: f64 = 5.2;
/// Height of the duct housing wall in px.
const HOVER_DUCT_H: f64 = 2.5;
/// Facets of the fan duct.
const HOVER_DUCT_SEGMENTS: usize = 12;
/// Rearward shift of the fan duct from the hull centre in px: over the stern.
const HOVER_DUCT_SHIFT: f64 = -7.0;
/// Radius of the fan plate recessed in the duct in px.
pub(super) const HOVER_FAN_R: f64 = 4.6;
/// Lift of the fan plate above the duct top in px: enough that the recessed
/// plate never z-fights the drum top it lies in.
const HOVER_FAN_PLATE_LIFT: f64 = 0.3;
/// Lift of the spinning fan blades above the duct top in px.
const HOVER_FAN_BLADE_LIFT: f64 = 0.6;
/// Shade factor of the fan plate: darker than the duct wall, so the recess
/// reads as a shadowed opening.
const HOVER_FAN_PLATE_SHADE: f64 = 0.35;
/// Number of lift-fan blades.
pub(super) const HOVER_FAN_BLADES: usize = 3;
/// Phase multiplier of the lift fan. It shares the rotor phase with the
/// helicopter rotor, and spins a little faster, so a hovercraft idling next
/// to a helicopter does not look like two copies of one animation.
const HOVER_FAN_SPIN_RATIO: f64 = 1.4;
/// Length of the stern rudder fin along the heading in px.
const HOVER_FIN_LEN: f64 = 3.6;
/// Thickness of the rudder fin across the heading in px.
const HOVER_FIN_WID: f64 = 1.6;
/// Height of the rudder fin in px.
const HOVER_FIN_H: f64 = 5.0;
/// Base of the rudder fin relative to the hull top in px (sunk into it, so the
/// fin emerges from the duct and the deck instead of floating behind them).
const HOVER_FIN_SINK: f64 = -0.2;
/// Rearward shift of the rudder fin from the hull centre in px. It is derived
/// so the fin's front edge stays tucked into the rear of the duct (derived:
/// duct shift - duct radius + a little overlap): the silhouette then ends in
/// a tail instead of a flat stern cut.
const HOVER_FIN_SHIFT: f64 = HOVER_DUCT_SHIFT - HOVER_DUCT_R + HOVER_FIN_LEN / 2.0 - 0.8;
/// Colour of the healing cross on a buffer hull. It repeats the medical green
/// of the buffer base roof, which is what identifies the support role.
const BUFFER_CROSS_COLOR: [u8; 3] = [130, 235, 140];

pub(super) fn push_vehicle(
    game: &Game,
    v: &crate::entities::Vehicle,
    rotor_phase: f64,
    max_height_px: f64,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
) {
    let color = constants::player_color(v.owner);
    let z = vehicle_z_with_height(game, v, max_height_px);
    let (x, y) = (v.x, v.y);
    match v.kind {
        constants::VehicleKind::Tank => push_tank(game, v, mesh, lines, x, y, z, color),
        constants::VehicleKind::Helicopter => {
            push_helicopter(game, v, mesh, lines, x, y, z, rotor_phase, color);
        }
        constants::VehicleKind::Hovercraft => {
            push_hovercraft(game, v, mesh, lines, x, y, z, rotor_phase, color);
        }
        constants::VehicleKind::Buffer => {
            // Same chassis as a tank (rules.md section 5.4: a buffer drives
            // exactly like one) without a gun: the green healing cross the
            // base uses marks it as the support vehicle instead.
            let (fx, fy) = vehicle_heading(game, v);
            // `deck_top` is already absolute (it includes the vehicle's `z`).
            let deck_top = push_tank_chassis(mesh, lines, x, y, z, color, fx, fy);
            push_cross(lines, x, y, deck_top + 4.0, BUFFER_CROSS_COLOR);
        }
    }
}

/// Detailed hovercraft (rules.md section 5.3): an inflated dark skirt carrying
/// a boat-like hull with a raked bow, a glazed cockpit, a pair of deck rails,
/// a stern fan duct whose blades spin with `rotor_phase` and a rudder fin.
///
/// Unlike the tank and the helicopter, the model is built along the travel
/// heading (see [`vehicle_heading`]): the bow points at the next waypoint and
/// the fan duct with the rudder trail behind it, so the craft visibly drives
/// forwards. Being the only amphibious vehicle, it also has to stay *low and
/// wide*: the skirt is the widest part of the craft and the whole model is
/// flatter than the tank, so a hovercraft is never mistaken for a ground
/// vehicle at a glance -- it is the silhouette, not the colour, that carries
/// that.
#[allow(clippy::too_many_arguments)]
fn push_hovercraft(
    game: &Game,
    v: &crate::entities::Vehicle,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
    x: f64,
    y: f64,
    z: f64,
    rotor_phase: f64,
    color: [u8; 3],
) {
    let (fx, fy) = vehicle_heading(game, v);
    push_hovercraft_oriented(mesh, lines, x, y, z, rotor_phase, color, fx, fy);
}

/// Hovercraft parts in an explicit heading frame (unit tests drive this).
#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
fn push_hovercraft_oriented(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
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
    let darker = constants::shade(color, 0.5);
    // Skirt: one flared dark cylinder, lifted like the flat obstacle markers
    // so it never z-fights the tile top. Its bottom rim is the widest ring of
    // the craft, which is what makes the whole model read as low and wide.
    let skirt_base = z + constants::OBSTACLE_LIFT;
    push_cylinder(
        mesh,
        x,
        y,
        skirt_base,
        HOVER_SKIRT_R,
        HOVER_SKIRT_TOP_R,
        HOVER_SKIRT_H,
        HOVER_SKIRT_SEGMENTS,
        constants::shade(color, HOVER_SKIRT_SHADE),
    );
    // Hull: a boat, not a box -- clearly longer along the heading than across
    // it, so its bow can point at the waypoint.
    let hull_base = skirt_base + HOVER_SKIRT_H;
    push_oriented_box(
        mesh,
        x,
        y,
        hull_base,
        HOVER_HULL_LEN,
        HOVER_HULL_WID,
        HOVER_HULL_H,
        fx,
        fy,
        color,
    );
    let hull_top = hull_base + HOVER_HULL_H;
    // Foredeck: a wedge whose front edge stays flush with the deck, so the bow
    // slopes down to the skirt instead of ending in a square corner.
    let bow_mid = HOVER_HULL_LEN / 2.0 - HOVER_BOW_LEN / 2.0;
    push_oriented_slope(
        mesh,
        x + fx * bow_mid,
        y + fy * bow_mid,
        hull_top,
        HOVER_BOW_LEN,
        HOVER_BOW_WID,
        HOVER_BOW_H,
        0.2,
        fx,
        fy,
        dark,
    );
    // Glazed cockpit on the foredeck, behind the bow, so the windscreen faces
    // the direction of travel.
    push_oriented_box(
        mesh,
        x + fx * HOVER_CANOPY_SHIFT,
        y + fy * HOVER_CANOPY_SHIFT,
        hull_top + HOVER_CANOPY_SINK,
        HOVER_CANOPY_LEN,
        HOVER_CANOPY_WID,
        HOVER_CANOPY_H,
        fx,
        fy,
        HOVER_CANOPY_COLOR,
    );
    // Rails along both deck edges: two thin strokes keep the wide hull from
    // reading as one flat plate. They run from the cockpit back to the duct.
    let rail_from = HOVER_CANOPY_SHIFT - HOVER_CANOPY_LEN / 2.0 - 1.0;
    let rail_to = HOVER_DUCT_SHIFT + 1.0;
    for side in [-1.0, 1.0] {
        push_beam(
            lines,
            x + fx * rail_from + px * side * HOVER_HULL_WID / 2.0,
            y + fy * rail_from + py * side * HOVER_HULL_WID / 2.0,
            hull_top + 0.15,
            x + fx * rail_to + px * side * HOVER_HULL_WID / 2.0,
            y + fy * rail_to + py * side * HOVER_HULL_WID / 2.0,
            hull_top + 0.15,
            darker,
        );
    }
    // Stern fan duct: a dark housing drum on the deck whose recessed plate
    // carries the lift fan. The blades spin with `rotor_phase` (a little
    // faster than the helicopter rotor), so the craft is visibly hovering
    // even while it stands still.
    let (dx, dy) = (x + fx * HOVER_DUCT_SHIFT, y + fy * HOVER_DUCT_SHIFT);
    push_cylinder(
        mesh,
        dx,
        dy,
        hull_top,
        HOVER_DUCT_R,
        HOVER_DUCT_TOP_R,
        HOVER_DUCT_H,
        HOVER_DUCT_SEGMENTS,
        darker,
    );
    let duct_top = hull_top + HOVER_DUCT_H;
    push_disc(
        mesh,
        dx,
        dy,
        duct_top + HOVER_FAN_PLATE_LIFT,
        HOVER_FAN_R,
        HOVER_DUCT_SEGMENTS,
        constants::shade(color, HOVER_FAN_PLATE_SHADE),
    );
    let fan_z = duct_top + HOVER_FAN_BLADE_LIFT;
    let phase = rotor_phase * HOVER_FAN_SPIN_RATIO;
    let blade = [200, 200, 200];
    for k in 0..HOVER_FAN_BLADES {
        let a = phase + std::f64::consts::TAU * k as f64 / HOVER_FAN_BLADES as f64;
        push_beam(
            lines,
            dx,
            dy,
            fan_z,
            dx + HOVER_FAN_R * a.cos(),
            dy + HOVER_FAN_R * a.sin(),
            fan_z,
            blade,
        );
    }
    // Rudder fin trailing at the stern: thin across the heading, tall. It is
    // sunken into the hull top, so it emerges from the duct instead of
    // floating behind it, and its rear edge gives the silhouette a tail.
    push_oriented_box(
        mesh,
        x + fx * HOVER_FIN_SHIFT,
        y + fy * HOVER_FIN_SHIFT,
        hull_top + HOVER_FIN_SINK,
        HOVER_FIN_LEN,
        HOVER_FIN_WID,
        HOVER_FIN_H,
        fx,
        fy,
        darker,
    );
}

/// Detailed helicopter: slender pod hull with a glazed cockpit, tail boom
/// with a fin and a spinning two-blade main rotor plus a tail rotor.
///
/// The airframe is oriented along the flight heading (see
/// [`vehicle_heading`]): the cockpit faces the next waypoint and the tail
/// boom trails behind it, so the tail always stays at the back of the
/// flight direction. The body stays an opaque box stack (depth-tested like
/// every other vehicle), while the thin rotor blades and skid struts are 3D
/// line strokes: they need no depth fighting on the GPU path, and drawing
/// the rotor as lines above the body keeps it readable in every frame.
/// `rotor_phase` rotates the main blades around the mast, so consecutive
/// frames built with an advancing phase show the spin.
#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
fn push_helicopter(
    game: &Game,
    v: &crate::entities::Vehicle,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
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
pub(super) const HELI_HULL_H: f64 = 6.0;
/// Clearance of the hull belly above the skid base in px.
const HELI_HULL_LIFT: f64 = 3.0;
/// Length of the glazed cockpit on the forward hull top in px.
const HELI_CANOPY_LEN: f64 = 8.0;
/// Width of the cockpit canopy in px.
const HELI_CANOPY_WID: f64 = 8.0;
/// Height of the cockpit canopy in px.
pub(super) const HELI_CANOPY_H: f64 = 3.5;
/// Forward shift of the canopy centre from the hull centre in px.
const HELI_CANOPY_SHIFT: f64 = 6.5;
/// Elevation of the canopy base above the skid base in px: it sits on the
/// forward hull top, so the glass reads as a windscreen (not a floating box).
const HELI_CANOPY_BASE: f64 = HELI_HULL_LIFT + HELI_HULL_H - 1.5;
/// Fixed tint of the cockpit glass (a dark canopy, never a white box).
pub(super) const HELI_CANOPY_COLOR: [u8; 3] = [72, 106, 126];
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
pub(super) const HELI_MAST_BASE: f64 = HELI_HULL_LIFT + HELI_HULL_H;
/// Height of the rotor mast above the hull top in px.
const HELI_MAST_H: f64 = 5.0;
/// Thickness of the rotor mast in px.
const HELI_MAST_THICK: f64 = 2.5;
/// Radius of the main rotor in px.
pub(super) const HELI_ROTOR_R: f64 = 22.0;
/// Radius of the tail rotor in px.
const HELI_TAIL_ROTOR_R: f64 = 5.0;
/// Width of one rotor blade in the helicopter shadow in px (wider than the
/// air stroke, so the spinning blades stay readable on the ground).
pub(super) const HELI_SHADOW_BLADE_WID: f64 = 2.5;
/// Phase multiplier of the tail rotor: it spins faster than the main rotor.
const HELI_TAIL_ROTOR_RATIO: f64 = 2.0;
/// Elevation of the tail rotor axis above the skid base in px: just above
/// the tip of the vertical fin.
const HELI_TAIL_ROTOR_Z: f64 = HELI_FIN_BASE + HELI_FIN_H + 1.0;

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
fn push_helicopter_oriented(
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
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
    // bright strokes; the disc sits one px above the mast top.
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
    // runs faster than the main rotor.
    let t_phase = rotor_phase * HELI_TAIL_ROTOR_RATIO;
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

/// Surface that receives the helicopter shadow at a world point, with the
/// deck rectangle when the point lies on a bridge.
///
/// A deck fragment shields the water below it, so a helicopter crossing a
/// bridge drops its shadow on the deck; everywhere else the shadow follows
/// the terrain (ramps included, see [`vehicle_ground_z`]). The rectangle is
/// returned alongside because the deck is narrow: the silhouette is wider
/// than one deck fragment, so it has to be clipped to the deck instead of
/// floating over the water around it.
pub(super) fn helicopter_shadow_surface(game: &Game, x: f64, y: f64) -> (f64, Option<DeckQuad>) {
    if let Some(tile) = game.board.world_to_tile(x, y)
        && let Some(br) = game
            .board
            .tiles
            .get(&tile)
            .and_then(|t| t.bridge)
            .map(|i| &game.board.bridges[i])
    {
        return (
            bridge_deck_z(br),
            Some(bridge_deck_quad(&game.board, br, tile)),
        );
    }
    (vehicle_ground_z(game, x, y), None)
}

/// Elevation in px of the surface that receives a helicopter shadow.
///
/// A deck fragment of a bridge shields the water below it, so a helicopter
/// crossing a bridge drops its shadow on the deck; everywhere else the
/// shadow follows the terrain (ramps included, see [`vehicle_ground_z`]).
#[cfg(test)]
pub(super) fn helicopter_shadow_z(game: &Game, x: f64, y: f64) -> f64 {
    helicopter_shadow_surface(game, x, y).0
}

/// Detailed shadow of one helicopter, projected straight down.
///
/// The flight altitude does not follow the terrain (rules.md section 5.2),
/// so the isometric view alone cannot tell which tile a helicopter is over;
/// the dark silhouette marks it (the shared contract in specification.md).
/// Instead of one plain disc it traces the parts of
/// the airframe, all at the same elevation so the light reads as coming from
/// straight above: the two main rotor blades at their current `rotor_phase` —
/// the very phase the airframe used this frame, so a helicopter and its
/// shadow spin in lockstep — both skid rails, the tail boom with its fin
/// and the hull. Every part reuses the px constants of the 3D
/// parts, so the shadow follows a redesign of the airframe for free.
///
/// All pieces share one flat elevation, so no part of the silhouette fights
/// another in the depth buffer; they sit
/// [`crate::constants::SHADOW_LIFT`] above the receiving surface (a bridge
/// deck when the helicopter crosses one), high enough to
/// win the depth race against the terrain (no flicker) yet low enough to
/// read as lying on the ground. They go into the translucent pass: the GPU
/// depth test keeps them from darkening the hull, other vehicles or nearer
/// cliffs. Over a bridge the parts are additionally clipped to the deck
/// rectangle, so a shadow crossing the water never hangs beside the bridge.
pub(super) fn push_helicopter_shadow(
    game: &Game,
    v: &crate::entities::Vehicle,
    rotor_phase: f64,
    shadow: &mut RangeSoup,
) {
    let (surface, deck) = helicopter_shadow_surface(game, v.x, v.y);
    let z = surface + constants::SHADOW_LIFT;
    let (fx, fy) = vehicle_heading(game, v);
    let (px, py) = (-fy, fx);
    let black = constants::SHADOW_COLOR;
    let solid = constants::SHADOW_ALPHA;
    // One silhouette part: on a bridge deck it is clipped to the deck strip,
    // on open terrain it is a plain rectangle.
    let part =
        |shadow: &mut RangeSoup, cx: f64, cy: f64, len: f64, wid: f64, fx: f64, fy: f64| match deck
        {
            Some(d) => {
                push_range_rect_on_deck(shadow, cx, cy, z, len, wid, fx, fy, black, solid, &d)
            }
            None => push_range_rect(shadow, cx, cy, z, len, wid, fx, fy, black, solid),
        };
    // The two main rotor blades, at the phase the airframe shows this frame.
    let (c, s) = (rotor_phase.cos(), rotor_phase.sin());
    for (bx, by) in [(c, s), (s, -c)] {
        part(
            shadow,
            v.x,
            v.y,
            2.0 * HELI_ROTOR_R,
            HELI_SHADOW_BLADE_WID,
            bx,
            by,
        );
    }
    // Skid rails.
    for side in [-1.0, 1.0] {
        part(
            shadow,
            v.x + px * side * HELI_SKID_OFFSET,
            v.y + py * side * HELI_SKID_OFFSET,
            HELI_SKID_LEN,
            HELI_SKID_THICK,
            fx,
            fy,
        );
    }
    // Tail boom with its end fin.
    part(
        shadow,
        v.x + fx * HELI_TAIL_SHIFT,
        v.y + fy * HELI_TAIL_SHIFT,
        HELI_TAIL_LEN,
        HELI_TAIL_WID,
        fx,
        fy,
    );
    part(
        shadow,
        v.x + fx * HELI_FIN_SHIFT,
        v.y + fy * HELI_FIN_SHIFT,
        HELI_FIN_LEN,
        HELI_FIN_WID,
        fx,
        fy,
    );
    // Hull (the widest part of the silhouette).
    part(shadow, v.x, v.y, HELI_HULL_LEN, HELI_HULL_WID, fx, fy);
}

// ---------------------------------------------------------------------------
// Tank rendering (rules.md section 5.1; every value is a rendering-only size
// in px, exactly like the other mesh dimensions -- colours come from the
// owning player, not from here).
// ---------------------------------------------------------------------------

/// Length of the tank hull along its heading in px.
pub(super) const TANK_HULL_LEN: f64 = 22.0;
/// Width of the tank hull across its heading in px.
const TANK_HULL_WID: f64 = 13.0;
/// Height of the lower hull box in px.
pub(super) const TANK_HULL_H: f64 = 5.0;
/// Ground clearance of the hull bottom in px (the tracks touch the ground).
pub(super) const TANK_HULL_LIFT: f64 = 1.0;
/// Length of the upper deck box in px.
const TANK_DECK_LEN: f64 = 15.0;
/// Width of the upper deck box in px.
const TANK_DECK_WID: f64 = 10.0;
/// Height of the upper deck box in px (it sits on the lower hull).
pub(super) const TANK_DECK_H: f64 = 3.0;
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
pub(super) const TANK_TREAD_MARKS: usize = 4;
/// Base radius of the rotating turret body in px.
const TANK_TURRET_R: f64 = 7.5;
/// Roof radius of the turret body in px (slightly tapered).
const TANK_TURRET_TOP_R: f64 = 6.5;
/// Height of the turret body in px.
pub(super) const TANK_TURRET_H: f64 = 4.5;
/// Facets of the turret cylinder (low-poly, flat-shaded look).
const TANK_TURRET_SEGMENTS: usize = 8;
/// Radius of the commander cupola in px.
const TANK_CUPOLA_R: f64 = 2.6;
/// Roof radius of the commander cupola in px.
const TANK_CUPOLA_TOP_R: f64 = 2.2;
/// Height of the commander cupola in px.
pub(super) const TANK_CUPOLA_H: f64 = 2.0;
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
pub(super) const TANK_ANTENNA_H: f64 = 8.0;
/// Reach of the barrel tip from the vehicle centre in px (derived: barrel
/// gap + barrel length + muzzle length).
pub(super) const TANK_BARREL_REACH: f64 = TANK_BARREL_GAP + TANK_BARREL_LEN + TANK_MUZZLE_LEN;

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
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
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
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
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
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
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
