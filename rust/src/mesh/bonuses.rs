//! Bonus field markers and the drone model (rules.md section 13).
//!
//! The yellow pad of a drone bonus and the two little figures inside a
//! `+x` / `*x` field are drawn here; the yellow ring around the whole field and
//! the value text are 2D overlays drawn by [`crate::app`], because the mesh
//! layer has no font.
//!
//! A drone is drawn wherever it currently flies: resting on its own field
//! while it waits there, orbiting its anchor when it is tied to a vehicle or a
//! building, and on the line between anchor and target while it shoots. Where
//! it *flies* never changes what it can shoot -- the range is always measured
//! from the centre of the anchor.

use super::vehicles::vehicle_z;
use super::{
    AlphaVertex, TriangleSoup, alpha_vert, push_box, push_cylinder, push_disc, push_ring,
    tile_top_z,
};
use crate::constants;
use crate::entities::{Drone, DroneAnchor};
use crate::game::Game;

// --- Rendering-only sizes in px (rules.md section 13 fixes the rules, not the
// drawing; like the BLD_* / TANK_* blocks these live next to the model) ------

/// Radius of the drone hull in px; a drone is far smaller than any vehicle.
pub const DRON_R: f64 = 5.0;
/// Height of the drone hull in px.
pub const DRON_H: f64 = 4.0;
/// Radius of the drone rotor disc in px.
pub const DRON_ROTOR_R: f64 = 8.0;
/// Radius of the orbit an anchored drone circles its anchor at, in px.
pub const DRON_ORBIT_R: f64 = 30.0;
/// Angular speed of that orbit in rad/s.
pub const DRON_ORBIT_W: f64 = 1.6;
/// Height of the orbit above the anchor surface in px.
pub const DRON_ORBIT_Z: f64 = 26.0;
/// Fraction of the anchor-to-target line where a shooting drone sits.
pub const DRON_SHOT_POS: f64 = 0.4;
/// Height of a shooting drone above that line in px.
pub const DRON_SHOT_Z: f64 = 14.0;
/// Radius of the yellow pad drawn on a field holding a drone bonus, in px.
pub const BONUS_DRONE_MARK_R: f64 = 12.0;
/// Half distance between the two figures of a unit bonus, in px.
pub const BONUS_FIG_DX: f64 = 6.0;
/// Half height of one figure of a unit bonus, in px.
pub const BONUS_FIG_H: f64 = 3.0;
/// Width of one figure of a unit bonus, in px.
pub const BONUS_FIG_W: f64 = 4.0;

/// Where the anchor of `drone` is drawn: world `(x, y)` and its elevation.
///
/// A drone tied to a destroyed vehicle has no anchor any more; it is waiting
/// on its own field until it is picked up again.
pub(super) fn anchor_point(game: &Game, drone: &Drone) -> (f64, f64, f64) {
    let home = |game: &Game| {
        let (x, y) = game.board.center_world(drone.home);
        (x, y, tile_top_z(&game.board, drone.home))
    };
    match drone.anchor {
        DroneAnchor::Bonus(tile) => {
            let (x, y) = game.board.center_world(tile);
            (x, y, tile_top_z(&game.board, tile))
        }
        DroneAnchor::Vehicle(id) => match game.vehicles.iter().find(|v| v.id == id && !v.dead) {
            Some(v) => (v.x, v.y, vehicle_z(game, v)),
            None => home(game),
        },
        DroneAnchor::Building(tile) => match game.building_at_tile(tile) {
            Some(b) => {
                let (x, y) = b.pos(game.board.side);
                (x, y, tile_top_z(&game.board, tile))
            }
            None => home(game),
        },
    }
}

/// Draw every drone of `game`: hull, rotor ring and the beam back to its
/// anchor. `phase` advances the orbit (see [`DRON_ORBIT_W`]).
pub(super) fn push_drones(
    game: &Game,
    phase: f64,
    mesh: &mut TriangleSoup,
    lines: &mut Vec<(AlphaVertex, AlphaVertex)>,
) {
    for (i, drone) in game.drones.iter().enumerate() {
        // A per-drone offset keeps a whole squadron from moving as one blob.
        let angle = (phase + i as f64 * 0.7) * DRON_ORBIT_W;
        let (ax, ay, az) = anchor_point(game, drone);
        let target = drone
            .target
            .and_then(|id| game.vehicles.iter().find(|v| v.id == id && !v.dead))
            .map(|v| (v.x, v.y, vehicle_z(game, v)));
        let (x, y, z) = match target {
            Some((tx, ty, tz)) => (
                ax + (tx - ax) * DRON_SHOT_POS,
                ay + (ty - ay) * DRON_SHOT_POS,
                az + (tz - az) * DRON_SHOT_POS + DRON_SHOT_Z,
            ),
            None if matches!(drone.anchor, DroneAnchor::Bonus(_)) => {
                (ax, ay, az + DRON_ORBIT_Z * 0.4)
            }
            None => (
                ax + DRON_ORBIT_R * angle.cos(),
                ay + DRON_ORBIT_R * angle.sin(),
                az + DRON_ORBIT_Z,
            ),
        };
        let color = match game.drone_owner(drone) {
            Some(owner) => constants::player_color(owner),
            None => constants::NEUTRAL_COLOR,
        };
        // The beam back to the anchor reads as a tether and shows what the
        // drone is actually guarding.
        lines.push((
            alpha_vert(
                ax,
                ay,
                az + DRON_SHOT_Z * 0.2,
                constants::shade(color, 1.0),
                140,
            ),
            alpha_vert(x, y, z, constants::shade(color, 1.0), 140),
        ));
        push_cylinder(mesh, x, y, z, DRON_R, DRON_R * 0.7, DRON_H, 8, color);
        push_ring(lines, x, y, z + DRON_H, DRON_ROTOR_R, 12, color, 190);
    }
}

/// Draw the in-field markers of every bonus still standing: the yellow pad of
/// a drone bonus and the pair of figures of a `+x` / `*x` bonus.
pub(super) fn push_bonus_markers(game: &Game, mesh: &mut TriangleSoup) {
    use crate::entities::BonusKind;
    for bonus in game.bonuses.iter().flatten() {
        let (x, y) = game.board.center_world(bonus.tile);
        let z = tile_top_z(&game.board, bonus.tile);
        match bonus.kind {
            BonusKind::Drone => push_disc(
                mesh,
                x,
                y,
                z + constants::OBSTACLE_LIFT,
                BONUS_DRONE_MARK_R,
                12,
                constants::shade(constants::BONUS_MARK_COLOR, 0.75),
            ),
            BonusKind::Add(_) | BonusKind::Mul(_) => {
                for dx in [-BONUS_FIG_DX, BONUS_FIG_DX] {
                    push_box(
                        mesh,
                        x + dx,
                        y,
                        z + constants::OBSTACLE_LIFT,
                        BONUS_FIG_W,
                        BONUS_FIG_W,
                        BONUS_FIG_H,
                        constants::BONUS_MARK_COLOR,
                    );
                }
            }
        }
    }
}
