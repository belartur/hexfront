//! The flat per-frame overlays: range fills and outlines, drawn vehicle routes
//! and projectiles.

use super::surface::{route_crossings, route_prev, waypoint_z};
use super::vehicles::{helicopter_shadow_surface, vehicle_z};
use super::{
    AlphaVertex, RangeSoup, TriangleSoup, alpha_vert, push_disc, push_range_disc, push_ring,
    tile_top_z,
};
use crate::constants;
use crate::game::Game;
use crate::math::sqr;

/// Outline colour of one range: the owner colour, white when the building
/// belongs to no player.
pub(super) fn range_outline_color(b: &crate::entities::Building) -> [u8; 3] {
    match b.owner {
        Some(id) => constants::player_color(id),
        None => constants::RANGE_OUTLINE_NEUTRAL,
    }
}

/// Facets of one range fill disc. High enough that the mask edge is smooth at
/// any zoom the game allows; it is a coverage mask, not a drawn circle.
const RANGE_DISC_SEGMENTS: usize = 40;
/// Facets of one range outline ring. Slightly more than the fill, so the
/// outline does not cut inside the disc it frames.
const RANGE_RING_SEGMENTS: usize = 48;

/// Build the range discs and outlines of every turret, heal tower and buffer.
///
/// Fills are **masks**, not the final look: they go into the offscreen buffer
/// of [`crate::render::Renderer`] with a fully opaque
/// [`constants::RANGE_MASK_ALPHA`], so a disc covering the same pixels as
/// another one simply overwrites it instead of stacking alpha. The renderer
/// then composites each mask once with the presentation colour and alpha, which
/// is what keeps two overlapping ranges at the coverage of a single range.
/// Outlines are 3D strokes collected in `outlines` and drawn after the
/// composited fills, in the owner colour, with the depth test switched off so
/// a range stays readable even behind a cliff, a bridge deck or a building.
pub(super) fn push_ranges(
    game: &Game,
    turret: &mut RangeSoup,
    heal: &mut RangeSoup,
    outlines: &mut Vec<(AlphaVertex, AlphaVertex)>,
) {
    use crate::entities::BuildingKind;
    for b in game.buildings.iter() {
        // A range is a disc plus its ring; the only thing that differs between
        // a turret and a heal tower is which mask it lands in and how far it
        // reaches, so both go through the same two calls.
        let is_turret = crate::entities::turret_kind_of(b.kind);
        let r = if let Some(tk) = is_turret {
            constants::turret_range(tk)
        } else if b.kind == BuildingKind::HealTower && b.owner.is_some() {
            // Neutral heal towers show no range (game rules and editor spec).
            b.units * constants::HEAL_TOWER_RANGE_PER_UNIT
        } else {
            continue;
        };
        if r <= 1.0 {
            continue;
        }
        let (cx, cy) = b.pos(game.board.side);
        let z = tile_top_z(&game.board, b.tile);
        let fill = if is_turret.is_some() {
            &mut *turret
        } else {
            &mut *heal
        };
        push_range_disc(
            fill,
            cx,
            cy,
            z + constants::RANGE_FILL_LIFT,
            r,
            RANGE_DISC_SEGMENTS,
            constants::RANGE_MASK_COLOR,
            constants::RANGE_MASK_ALPHA,
        );
        push_ring(
            outlines,
            cx,
            cy,
            z + constants::RANGE_OUTLINE_LIFT,
            r,
            RANGE_RING_SEGMENTS,
            range_outline_color(b),
            constants::RANGE_OUTLINE_ALPHA,
        );
    }
    for v in game.vehicles.iter() {
        if v.dead || v.kind != constants::VehicleKind::Buffer {
            continue;
        }
        let z = vehicle_z(game, v);
        push_range_disc(
            heal,
            v.x,
            v.y,
            z - constants::ELEVATION_PX + constants::RANGE_FILL_LIFT,
            constants::BUFFER_HEAL_RADIUS,
            40,
            constants::RANGE_MASK_COLOR,
            constants::RANGE_MASK_ALPHA,
        );
    }
}

/// Elevation the drawn route of `v` starts at.
///
/// A ground vehicle is drawn on the surface it drives on, so its route
/// leaves the hull. A helicopter flies at a fixed altitude above the whole
/// board (rules.md section 5.2), far from the ground, so a route starting at
/// the hull would hang in the air far from the route it describes. Its route
/// therefore starts at the shadow underneath it, on the surface the shadow
/// falls on: the bridge deck while it crosses one, the terrain otherwise (the
/// very surface used by [`push_helicopter_shadow`]). The rest of the route
/// already runs on the surfaces below the waypoints, so the whole line stays
/// on the ground.
pub(super) fn route_start_z(game: &Game, v: &crate::entities::Vehicle) -> f64 {
    if v.kind == constants::VehicleKind::Helicopter {
        helicopter_shadow_surface(game, v.x, v.y).0
    } else {
        vehicle_z(game, v)
    }
}

pub(super) fn push_paths(game: &Game, lines: &mut Vec<(AlphaVertex, AlphaVertex)>) {
    for v in game.vehicles.iter() {
        if v.dead || v.route.is_empty() {
            continue;
        }
        // The first leg starts at the vehicle: on the surface it stands on, or
        // -- for a helicopter -- on the ground under its shadow (see
        // `route_start_z`). The following waypoints sit on the surface below
        // them. A route crossing a bridge rides its deck, the fields of a
        // route crossing under one keep the terrain below (rules.md section 8).
        let seq = &v.route[v.route_index.min(v.route.len())..];
        let modes = route_crossings(&game.board, v.kind, route_prev(v), seq);
        let mut prev = (v.x, v.y, route_start_z(game, v));
        for i in 0..seq.len() {
            let (wx, wy) = game.board.center_world(seq[i]);
            let wz = waypoint_z(&game.board, seq, i, modes[i + 1]);
            lines.push((
                alpha_vert(prev.0, prev.1, prev.2, [255, 255, 255], 255),
                alpha_vert(wx, wy, wz, [255, 255, 255], 255),
            ));
            prev = (wx, wy, wz);
        }
    }
}

/// Flight height of a shot at its endpoints, in px. A shot leaves the muzzle
/// above the ground and comes down on the target, so both ends clear the
/// terrain and the arc is visible against it.
const PROJ_END_PX: f64 = 30.0;
/// Extra height at the middle of the flight, in px: a parabola from
/// [`PROJ_END_PX`] up to `2 * PROJ_END_PX`, which is what makes a long-range
/// shot read as a lob rather than a flat streak.
const PROJ_ARC_PX: f64 = PROJ_END_PX;
/// Facets of one projectile dot. Small on screen, so a coarse disc is enough.
const PROJ_SEGMENTS: usize = 10;
/// Colour of a rocket in flight; a normal shot is near-white.
const PROJ_ROCKET_COLOR: [u8; 3] = [255, 120, 60];
const PROJ_SHOT_COLOR: [u8; 3] = [250, 250, 250];

/// One disc per shot in flight, following the ballistic arc from muzzle to
/// target so a long-range shot is visibly lobbed over the terrain.
pub(super) fn push_projectiles(game: &Game, mesh: &mut TriangleSoup) {
    for p in game.projectiles.iter() {
        let t = (p.t / p.dur).clamp(0.0, 1.0);
        let x = p.from_pos.0 + (p.to.0 - p.from_pos.0) * t;
        let y = p.from_pos.1 + (p.to.1 - p.from_pos.1) * t;
        let arc = PROJ_ARC_PX * (1.0 - sqr(2.0 * t - 1.0));
        let z = PROJ_END_PX + arc;
        let (r, col) = if p.kind == constants::TurretKind::Rocket {
            (f64::from(constants::ROCKET_RADIUS), PROJ_ROCKET_COLOR)
        } else {
            (f64::from(constants::PROJECTILE_RADIUS), PROJ_SHOT_COLOR)
        };
        push_disc(mesh, x, y, z, r, PROJ_SEGMENTS, col);
    }
}
