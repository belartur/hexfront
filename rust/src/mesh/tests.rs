//! Tests of the mesh builders.
//!
//! They live in one module rather than next to each model because most of them
//! build a whole frame with [`super::build_dynamic`] and then measure what came
//! out -- a test about a turret spans the turret, the foundations and the ring
//! its outline shares with the range masks.

use super::buildings::*;
use super::obstacles::*;
use super::overlays::route_start_z;
use super::surface::*;
use super::terrain::{
    CHUNK_VERTICES, bridge_deck_quad, bridge_deck_z, build_terrain, tile_top_z,
    visible_world_bounds,
};
use super::vehicles::*;
#[cfg(test)]
use super::*;
use super::{
    AlphaVertex, DynamicMesh, GpuVertex, RangeSoup, billboard_axes, build_dynamic, push_fx_blob,
};
use crate::board::Board;
use crate::board::Crossing;
use crate::constants;
use crate::hexgrid::{self, Tile};
use crate::math::dist2;

#[test]
fn billboard_quads_are_flat_in_the_depth_buffer() {
    // A particle quad has to be perfectly flat in D, otherwise its two
    // halves land on different depths and the translucent blend of one
    // puff over another flickers while the camera pans.
    let camera = crate::camera::Camera::new((1180.0, 720.0));
    let iso = crate::iso::IsoCamera::from_camera(&camera, 0.0, 40000.0);
    let d = |x: f64, y: f64, z: f64| {
        let (_, _, depth) = iso.project_point(&camera, x, y, z);
        depth
    };
    for (x, y, z, radius) in [
        (100.0, 200.0, 36.0, 12.0),
        (-500.0, 900.0, 0.0, 4.0),
        (2500.0, 1700.0, 81.0, 30.0),
    ] {
        let mut soup = RangeSoup::default();
        push_fx_blob(
            &mut soup,
            x,
            y,
            z,
            radius,
            [255, 200, 100, 255],
            [255, 200, 100, 40],
        );
        let center = d(x, y, z);
        for v in soup.vertices.iter() {
            let vert_d = d(f64::from(v.x), f64::from(v.y), f64::from(v.z));
            assert!(
                (vert_d - center).abs() < 1e-4,
                "blob vertex off the particle depth: {vert_d} vs {center}"
            );
        }
    }
}

#[test]
fn billboard_axes_move_exactly_one_pixel_on_screen() {
    // The axes are unit *screen* steps, which is why the particle sizes in
    // `fx` can be written in plain px.
    let camera = crate::camera::Camera::new((1180.0, 720.0));
    let (right, up) = billboard_axes();
    let (ox, oy) = camera.world_to_screen(500.0, 500.0, 45.0);
    let (rx, ry) = camera.world_to_screen(500.0 + right.0, 500.0 + right.1, 45.0 + right.2);
    let (ux, uy) = camera.world_to_screen(500.0 + up.0, 500.0 + up.1, 45.0 + up.2);
    assert!((rx - ox - 1.0).abs() < 1e-3, "right axis: {}", rx - ox);
    assert!(
        (ry - oy).abs() < 1e-3,
        "right axis must not move vertically: {}",
        ry - oy
    );
    assert!(
        (ux - ox).abs() < 1e-3,
        "up axis must not move sideways: {}",
        ux - ox
    );
    assert!((uy - oy + 1.0).abs() < 1e-3, "up axis: {}", uy - oy);
}

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
    let blades = |mesh: &DynamicMesh| -> Vec<(AlphaVertex, AlphaVertex)> {
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
    let (fx, fy) = vehicle_heading(&game, &game.vehicles[0]);
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
    let along = |p: &(AlphaVertex, AlphaVertex)| {
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
    // tile far below the hull, on the hill closer to it, lifted
    // SHADOW_LIFT above the receiving surface.
    let low_shadow = helicopter_shadow_z(&game, lx, ly) + constants::SHADOW_LIFT;
    let hill_shadow = helicopter_shadow_z(&game, hx, hy) + constants::SHADOW_LIFT;
    assert!((low_shadow - (constants::ELEVATION_PX + constants::SHADOW_LIFT)).abs() < 1e-9);
    assert!((hill_shadow - (hill_top + constants::SHADOW_LIFT)).abs() < 1e-9);
    assert!(
        low_shadow < alt && hill_shadow < alt,
        "shadow {low_shadow}/{hill_shadow} not below the hull {alt}"
    );
    let mut dynamic = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut dynamic);
    // The shadow is a flat silhouette (blades + skids + tail boom + fin
    // + hull), not one plain disc, and every piece is black, 70/255.
    let per_heli = 7 * 6; // seven oriented rectangles, two triangles each
    assert_eq!(
        dynamic.shadow.vertices.len(),
        2 * per_heli,
        "helicopter shadows: {}",
        dynamic.shadow.vertices.len()
    );
    // One flat elevation, one black, one alpha: the silhouette can neither
    // fight itself in the depth buffer nor accumulate darker overlaps.
    for vert in dynamic.shadow.vertices.iter() {
        assert_eq!(&vert.color[..3], &constants::SHADOW_COLOR[..]);
        assert_eq!(vert.color[3], constants::SHADOW_ALPHA);
    }
    // Every part lies in the receiving plane, on the surface right below
    // the helicopter it belongs to.
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
    // The blade part of the silhouette follows the rotor phase.
    let mut spun = DynamicMesh::default();
    build_dynamic(&game, 0.9, &mut spun);
    assert_eq!(spun.shadow.vertices.len(), dynamic.shadow.vertices.len());
    let coords = |mesh: &DynamicMesh| -> Vec<(f32, f32)> {
        mesh.shadow
            .vertices
            .iter()
            .map(|vert| (vert.x, vert.y))
            .collect()
    };
    assert_ne!(
        coords(&dynamic),
        coords(&spun),
        "the rotor shadow does not spin with the phase"
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
fn helicopter_shadow_blades_follow_the_rotor_phase() {
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
    // Both the airframe blades and the shadow blades read one shared
    // phase, so every blade tip must have a shadow right below it.
    for phase in [0.0, 1.1, 3.7] {
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, phase, &mut dynamic);
        let top = dynamic
            .lines
            .iter()
            .map(|(a, _)| f64::from(a.z))
            .fold(f64::NEG_INFINITY, f64::max);
        let blades: Vec<_> = dynamic
            .lines
            .iter()
            .filter(|(a, b)| {
                (f64::from(a.z) - top).abs() < 1e-6 && (f64::from(b.z) - top).abs() < 1e-6
            })
            .collect();
        assert_eq!(blades.len(), 2, "phase {phase}");
        for (a, b) in blades {
            for tip in [a, b] {
                let covered = dynamic.shadow.vertices.iter().any(|s| {
                    (f64::from(s.x) - f64::from(tip.x)).hypot(f64::from(s.y) - f64::from(tip.y))
                        < HELI_SHADOW_BLADE_WID
                });
                assert!(
                    covered,
                    "no shadow blade under the tip {},{} at phase {phase}",
                    tip.x, tip.y
                );
            }
        }
    }
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
    let (fx, fy) = vehicle_heading(&game, &game.vehicles[0]);
    assert!(fx.abs() < 1e-9 && fy < -0.999, "chassis heading {fx},{fy}");
    let (ax, ay) = tank_aim(&game, &game.vehicles[0], (fx, fy));
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
        (along_gun - TANK_BARREL_REACH).abs() < 0.05,
        "barrel reach {along_gun} vs {}",
        TANK_BARREL_REACH
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
    let (ax, ay) = tank_aim(&game, &game.vehicles[0], heading);
    assert_eq!((ax, ay), heading);
    // Shelling a wall (rules.md section 4): the gun turns onto it.
    game.vehicles[0].wall_target = Some(wall_tile);
    let (wx, wy) = game.board.center_world(wall_tile);
    let (vx, vy) = (game.vehicles[0].x, game.vehicles[0].y);
    let (dx, dy) = (wx - vx, wy - vy);
    let len = (dx * dx + dy * dy).sqrt();
    let (ax, ay) = tank_aim(&game, &game.vehicles[0], heading);
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
        2 * TANK_TREAD_MARKS + 1,
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
    let roof = ground + TANK_HULL_LIFT + TANK_HULL_H + TANK_DECK_H + TANK_TURRET_H + TANK_CUPOLA_H;
    assert!((top - roof).abs() < 0.05, "roof {top} vs {roof}");
    // The tracks stick out past the hull, so the chassis is wider than
    // the deck and the tank does not read as one flat block.
    let mut wide = 0.0f64;
    for vtx in dynamic.opaque.vertices.iter() {
        wide = wide.max((f64::from(vtx.x) - sx).abs());
    }
    assert!(
        wide > TANK_HULL_LEN / 2.0,
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
    let expected = ground + TANK_HULL_LIFT + TANK_HULL_H + TANK_DECK_H + 4.0;
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
        reach < TANK_BARREL_REACH - 5.0,
        "buffer grew a barrel: {reach}"
    );
}

#[test]
fn hovercraft_floats_on_a_skirt_with_a_spinning_fan() {
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
        VehicleKind::Hovercraft,
        0,
        30.0,
        Vec::new(),
        (sx, sy),
        None,
    ));
    let mut first = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut first);
    // Skirt + hull + bow + cockpit + fan duct + fan plate + rudder: far more
    // than the two plain discs the craft used to be (24 triangles = 72
    // vertices).
    assert!(
        first.opaque.vertices.len() > 300,
        "hovercraft lost its details: {} vertices",
        first.opaque.vertices.len()
    );
    // Two deck rails + three fan blades.
    assert_eq!(
        first.lines.len(),
        2 + HOVER_FAN_BLADES,
        "rail/fan strokes: {}",
        first.lines.len()
    );
    // Low and wide: the flared skirt makes the craft clearly wider across than
    // it is tall, which is what separates it from the boxy tank at a glance.
    let (mut lo, mut hi) = (
        [f64::INFINITY, f64::INFINITY, f64::INFINITY],
        [f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
    );
    for vert in first.opaque.vertices.iter() {
        let p = [f64::from(vert.x), f64::from(vert.y), f64::from(vert.z)];
        for (k, v) in p.iter().enumerate() {
            lo[k] = lo[k].min(*v);
            hi[k] = hi[k].max(*v);
        }
    }
    let (wide, tall) = ((hi[0] - lo[0]).max(hi[1] - lo[1]), hi[2] - lo[2]);
    assert!(
        wide > 2.0 * tall,
        "hovercraft not low and wide: {wide} across by {tall} tall"
    );
    // The skirt is the widest ring of all, so the craft spans its full
    // diameter and it overhangs the hull.
    assert!(
        (wide - 2.0 * HOVER_SKIRT_R).abs() < 1e-3,
        "the skirt does not set the width: {wide} vs {}",
        2.0 * HOVER_SKIRT_R
    );
    assert!(
        wide > HOVER_HULL_LEN,
        "the skirt does not overhang the hull: {wide}"
    );
    // The fan blades are the light strokes; advancing the rotor phase turns
    // them, so the craft visibly hovers while it stands still.
    let blades = |mesh: &DynamicMesh| -> Vec<(f32, f32, f32, f32)> {
        mesh.lines
            .iter()
            .filter(|(a, _)| a.color[..3] == [200, 200, 200])
            .map(|(a, b)| (a.x, a.y, b.x, b.y))
            .collect()
    };
    let first_blades = blades(&first);
    assert_eq!(
        first_blades.len(),
        HOVER_FAN_BLADES,
        "expected a {} blade lift fan",
        HOVER_FAN_BLADES
    );
    for (ax, ay, bx, by) in first_blades.iter().copied() {
        let len = (f64::from(ax) - f64::from(bx)).hypot(f64::from(ay) - f64::from(by));
        assert!((len - HOVER_FAN_R).abs() < 1e-3, "fan blade length {len}");
    }
    let mut second = DynamicMesh::default();
    build_dynamic(&game, 0.5, &mut second);
    assert_ne!(
        first_blades,
        blades(&second),
        "the lift fan does not spin with the phase"
    );
}

#[test]
fn hovercraft_bow_and_fan_follow_the_travel_heading() {
    use crate::constants::VehicleKind;
    use crate::entities::{Player, Vehicle};
    use crate::game::Game;
    let mut board = Board::new(10, 10);
    for t in board.tiles.clone().keys() {
        board.tiles.get_mut(t).unwrap().height = 1;
    }
    let start = (4, 4);
    let dest = (8, 4);
    let (sx, sy) = hexgrid::hex_to_world(start.0, start.1, board.side);
    let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
    game.vehicles.push(Vehicle::new(
        VehicleKind::Hovercraft,
        0,
        30.0,
        vec![dest],
        (sx, sy),
        Some(start),
    ));
    let (vx, vy) = (game.vehicles[0].x, game.vehicles[0].y);
    let (fx, fy) = vehicle_heading(&game, &game.vehicles[0]);
    let mut dynamic = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut dynamic);
    // Distance of a world point along the travel heading.
    let along = |x: f64, y: f64| (x - vx) * fx + (y - vy) * fy;
    // The glazed cockpit (its own fixed tint) is on the foredeck, so it sits
    // ahead of the hull centre: a hovercraft drives bow first.
    let canopy: Vec<&GpuVertex> = dynamic
        .opaque
        .vertices
        .iter()
        .filter(|p| p.color == HOVER_CANOPY_COLOR)
        .collect();
    assert!(!canopy.is_empty(), "no glazed cockpit found");
    let n = canopy.len() as f64;
    let cockpit = canopy
        .iter()
        .map(|p| along(f64::from(p.x), f64::from(p.y)))
        .sum::<f64>()
        / n;
    assert!(
        cockpit > 1.0,
        "cockpit {cockpit} does not face the heading {fx},{fy}"
    );
    // The lift fan is the rearmost stroke: its duct trails the bow.
    let fan = dynamic
        .lines
        .iter()
        .filter(|(a, _)| a.color[..3] == [200, 200, 200])
        .map(|(a, _)| along(f64::from(a.x), f64::from(a.y)))
        .fold(f64::INFINITY, f64::min);
    assert!(
        fan < cockpit - 8.0,
        "fan at {fan} does not trail the cockpit at {cockpit}"
    );
}

#[test]
fn range_fills_are_opaque_masks_and_outlines_use_owner_colour() {
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
        .push(Building::new(BuildingKind::TurretNormal, None, 9, 9, 10.0));
    game.buildings
        .push(Building::new(BuildingKind::HealTower, Some(0), 8, 8, 10.0));
    let mut dynamic = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut dynamic);
    assert_eq!(dynamic.range_turret.vertices.len(), 3 * 40 * 3);
    assert!(!dynamic.range_heal.vertices.is_empty());
    // Fills are binary offscreen masks: fully opaque and colourless, so the
    // composition pass alone decides tint and transparency. An overlapping
    // mask overwrites the previous one instead of stacking alpha, which
    // keeps two overlapping ranges at the coverage of a single range.
    for v in dynamic.range_turret.vertices.iter() {
        assert_eq!(v.color[3], constants::RANGE_MASK_ALPHA);
        assert_eq!(&v.color[..3], &constants::RANGE_MASK_COLOR);
    }
    for v in dynamic.range_heal.vertices.iter() {
        assert_eq!(v.color[3], constants::RANGE_MASK_ALPHA);
        assert_eq!(&v.color[..3], &constants::RANGE_MASK_COLOR);
    }
    // Every disc sits just above the tile top with a constant lift: the
    // masks carry no depth, so no per-disc lift is needed any more.
    let z0 = dynamic.range_turret.vertices[0].z;
    let z1 = dynamic.range_turret.vertices[40 * 3].z;
    assert!((z1 - z0).abs() < 1e-6, "{z0} vs {z1}");
    // Outlines are separate strokes drawn after the fills, in the owner
    // colour and white for a neutral turret.
    let owners = [
        constants::player_color(0),
        constants::player_color(1),
        constants::RANGE_OUTLINE_NEUTRAL,
    ];
    let mut seen = [false; 3];
    for (a, b) in dynamic.range_lines.iter() {
        assert_eq!(a.color[3], constants::RANGE_OUTLINE_ALPHA);
        assert_eq!(a.color, b.color);
        for (slot, color) in seen.iter_mut().zip(owners.iter()) {
            if a.color[..3] == *color {
                *slot = true;
            }
        }
    }
    assert!(
        seen.iter().all(|hit| *hit),
        "missing owner colour: {seen:?}"
    );
    // Outlines never end up in the regular 3D stroke buffer, which is
    // drawn before the range fills.
    assert!(
        dynamic
            .lines
            .iter()
            .all(|(a, _)| a.color[3] != constants::RANGE_OUTLINE_ALPHA),
        "range outline leaked into the main line pass"
    );
}

#[test]
fn obstacles_float_above_terrain_with_details() {
    use crate::board::Obstacle;
    use crate::board::ObstacleKind;
    use crate::entities::Player;
    use crate::game::Game;
    // Every modelled obstacle stands on the terrain it is legal on: on
    // land, and -- for the mine, which rules.md section 1 allows on water
    // too -- floating on the water surface. The ice trap is land-only, so its
    // water shape would be dead code and is not drawn at all. (The fire trap
    // is unmodelled on purpose: it is a cluster of particle fires, so it
    // contributes no mesh here at all -- see `fire_trap_has_no_static_model`.)
    let cases: [(ObstacleKind, i32); 3] = [
        (ObstacleKind::Mine, 1),
        (ObstacleKind::Mine, 0),
        (ObstacleKind::TrapIce, 1),
    ];
    for (i, (kind, height)) in cases.iter().enumerate() {
        let mut board = Board::new(8, 8);
        let tile = (2 + i as i32, 2);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        board.tiles.get_mut(&tile).unwrap().height = *height;
        board.tiles.get_mut(&tile).unwrap().obstacle = Some(Obstacle::new(*kind));
        let game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        let top = tile_top_z(&game.board, tile);
        assert!(
            !dynamic.opaque.vertices.is_empty(),
            "{kind:?} on height {height} has no opaque body"
        );
        // Every part of the model floats above the tile top: coplanar
        // faces would lose the depth race against the terrain.
        for v in dynamic.opaque.vertices.iter() {
            assert!(
                (v.z as f64) >= top + constants::OBSTACLE_LIFT - 1e-6,
                "{kind:?} on height {height} not lifted: {} vs {top}",
                v.z
            );
        }
        // Every obstacle carries detail lines (the red mine cross or belt, the
        // ice slashes), so the kind reads even at small zoom.
        assert!(
            !dynamic.lines.is_empty(),
            "{kind:?} on height {height} has no detail lines"
        );
        // The model stays inside its field and below the height cap.
        let (cx, cy) = game.board.center_world(tile);
        let (mut reach, mut high) = (0.0_f64, 0.0_f64);
        for v in dynamic.opaque.vertices.iter() {
            let (dx, dy) = (f64::from(v.x) - cx, f64::from(v.y) - cy);
            reach = reach.max((dx * dx + dy * dy).sqrt());
            high = high.max(f64::from(v.z) - top);
        }
        assert!(
            reach <= OBS_MAX_REACH + 1e-6,
            "{kind:?} reaches {reach} px past the field centre"
        );
        assert!(
            high <= OBS_MAX_H + 1e-6,
            "{kind:?} is {high} px tall, above the {OBS_MAX_H} px cap"
        );
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
fn fire_trap_has_no_static_model() {
    use crate::board::Obstacle;
    use crate::board::ObstacleKind;
    use crate::entities::Player;
    use crate::game::Game;
    // The fire trap is a cluster of living campfires (`crate::fx`) and nothing
    // else: its field must stay empty in the mesh passes, so the flames never
    // fight a base mesh for the same pixels.
    let mut board = Board::new(8, 8);
    for t in board.tiles.clone().keys() {
        board.tiles.get_mut(t).unwrap().height = 1;
    }
    let tile = (2, 2);
    board.tiles.get_mut(&tile).unwrap().obstacle = Some(Obstacle::new(ObstacleKind::TrapFire));
    let game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
    let mut dynamic = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut dynamic);
    assert!(
        dynamic.opaque.vertices.is_empty(),
        "a fire trap must not leave an opaque base behind"
    );
    assert!(
        dynamic.lines.is_empty(),
        "a fire trap must not leave detail strokes behind"
    );
}

#[test]
fn mine_changes_shape_with_the_terrain() {
    // rules.md section 1 lets a mine wait on land and on water alike, so
    // the renderer gives it one shape per terrain (a pressure plate ashore,
    // a taller moored body afloat). Both are built from the same field and
    // the same red marker, but they must not be the same model: otherwise
    // a floating mine would read as a ground mine standing on the water.
    use crate::board::Obstacle;
    use crate::board::ObstacleKind;
    use crate::entities::Player;
    use crate::game::Game;
    let mut shapes = Vec::new();
    for height in [1, 0] {
        let mut board = Board::new(8, 8);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let tile = (3, 3);
        board.tiles.get_mut(&tile).unwrap().height = height;
        board.tiles.get_mut(&tile).unwrap().obstacle = Some(Obstacle::new(ObstacleKind::Mine));
        let game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        let top = tile_top_z(&game.board, tile);
        let tall = dynamic
            .opaque
            .vertices
            .iter()
            .map(|v| f64::from(v.z) - top)
            .fold(f64::NEG_INFINITY, f64::max);
        shapes.push((tall, dynamic.opaque.vertices.len(), dynamic.lines.len()));
    }
    let (land_tall, land_body, land_lines) = shapes[0];
    let (water_tall, water_body, water_lines) = shapes[1];
    assert!(
        water_tall > land_tall + 1.0,
        "the floating mine ({water_tall} px) is not taller than the ground mine ({land_tall} px)"
    );
    assert_ne!(
        (land_body, land_lines),
        (water_body, water_lines),
        "both terrains render the same mine mesh"
    );
}

/// Every building kind in a fixed order (rules.md section 3).
fn all_building_kinds() -> [crate::entities::BuildingKind; 8] {
    use crate::entities::BuildingKind;
    [
        BuildingKind::BaseTank,
        BuildingKind::BaseHelicopter,
        BuildingKind::BaseHovercraft,
        BuildingKind::BaseBuffer,
        BuildingKind::TurretNormal,
        BuildingKind::TurretRapid,
        BuildingKind::TurretRocket,
        BuildingKind::HealTower,
    ]
}

#[test]
fn flat_building_parts_float_above_terrain() {
    use crate::entities::{Building, Player};
    use crate::game::Game;
    let mut board = Board::new(12, 12);
    for t in board.tiles.clone().keys() {
        board.tiles.get_mut(t).unwrap().height = 1;
    }
    let buildings: Vec<Building> = all_building_kinds()
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            Building::new(
                *kind,
                Some(0),
                1 + (i as i32 % 6) * 2,
                1 + (i as i32 / 6) * 2,
                10.0,
            )
        })
        .collect();
    let game = Game::new(board, vec![Player::new(0, true)], buildings, 1);
    let mut dynamic = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut dynamic);
    assert!(!dynamic.opaque.vertices.is_empty());
    // Every foundation slab floats like the flat obstacle markers: the
    // lowest opaque vertex of the whole scene sits at the shared lift
    // above the (identical) tile top, so no building part is coplanar
    // with the terrain anywhere.
    let top = tile_top_z(&game.board, (1, 1));
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
fn every_building_kind_fits_its_field() {
    use crate::entities::{Building, Player};
    use crate::game::Game;
    for kind in all_building_kinds() {
        let mut board = Board::new(8, 8);
        for t in board.tiles.clone().keys() {
            board.tiles.get_mut(t).unwrap().height = 1;
        }
        let tile = (3, 3);
        let building = Building::new(kind, Some(1), tile.0, tile.1, 10.0);
        let game = Game::new(
            board,
            vec![Player::new(0, true), Player::new(1, false)],
            vec![building],
            1,
        );
        let mut dynamic = DynamicMesh::default();
        build_dynamic(&game, 0.0, &mut dynamic);
        assert!(
            !dynamic.opaque.vertices.is_empty(),
            "{kind:?} has no geometry"
        );
        // Detail strokes: gates, crosses, landing marks, antennae, ribs.
        assert!(!dynamic.lines.is_empty(), "{kind:?} has no detail strokes");
        let (cx, cy) = game.board.center_world(tile);
        let top = tile_top_z(&game.board, tile);
        let (mut reach, mut high) = (0.0_f64, 0.0_f64);
        let mut lowest = f64::INFINITY;
        for v in dynamic.opaque.vertices.iter() {
            let (dx, dy) = (f64::from(v.x) - cx, f64::from(v.y) - cy);
            reach = reach.max((dx * dx + dy * dy).sqrt());
            high = high.max(f64::from(v.z) - top);
            lowest = lowest.min(f64::from(v.z));
        }
        assert!(
            reach <= BLD_MAX_REACH + 1e-6,
            "{kind:?} reaches {reach} px past the field centre"
        );
        assert!(
            high <= BLD_MAX_H + 1e-6,
            "{kind:?} is {high} px tall, above the {BLD_MAX_H} px cap"
        );
        // The foundation slab is the lowest part and floats above terrain.
        assert!(
            lowest >= top + constants::OBSTACLE_LIFT - 1e-6,
            "{kind:?} is not lifted: {lowest} vs {top}"
        );
    }
}

#[test]
fn building_mesh_is_deterministic() {
    use crate::entities::{Building, Player};
    use crate::game::Game;
    let mut board = Board::new(12, 12);
    for t in board.tiles.clone().keys() {
        board.tiles.get_mut(t).unwrap().height = 1;
    }
    let buildings: Vec<Building> = all_building_kinds()
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            Building::new(
                *kind,
                Some(0),
                1 + (i as i32 % 6) * 2,
                1 + (i as i32 / 6) * 2,
                10.0,
            )
        })
        .collect();
    let game = Game::new(board, vec![Player::new(0, true)], buildings, 1);
    let mut first = DynamicMesh::default();
    let mut second = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut first);
    build_dynamic(&game, 0.0, &mut second);
    assert_eq!(first.opaque.vertices.len(), second.opaque.vertices.len());
    assert_eq!(first.lines.len(), second.lines.len());
    for (a, b) in first
        .opaque
        .vertices
        .iter()
        .zip(second.opaque.vertices.iter())
    {
        assert_eq!((a.x, a.y, a.z, a.color), (b.x, b.y, b.z, b.color));
    }
}

/// Furthest turret vertex from its field centre, ignoring the symmetric
/// parts of the emplacement (foundation, parapet, turret body): the
/// remaining tip is the weapon, so tests can read which way it points.
/// `aim` records a shot towards `centre + aim`; `None` keeps the east
/// fallback of a turret that never fired.
fn turret_weapon_tip(kind: crate::entities::BuildingKind, aim: Option<(f64, f64)>) -> (f64, f64) {
    use crate::entities::{Building, Player};
    use crate::game::Game;
    let mut board = Board::new(8, 8);
    for t in board.tiles.clone().keys() {
        board.tiles.get_mut(t).unwrap().height = 1;
    }
    let tile = (3, 3);
    let (cx, cy) = board.center_world(tile);
    let mut b = Building::new(kind, Some(0), tile.0, tile.1, 10.0);
    if let Some((ox, oy)) = aim {
        b.last_target_pos = Some((cx + ox, cy + oy));
    }
    let game = Game::new(board, vec![Player::new(0, true)], vec![b], 1);
    let mut dynamic = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut dynamic);
    let weapon_base = tile_top_z(&game.board, tile)
        + constants::OBSTACLE_LIFT
        + BLD_FOUND_H
        + BLD_TUR_PARA_H
        + BLD_TUR_BODY_H;
    let (mut best, mut best_d) = ((0.0_f64, 0.0_f64), 0.0_f64);
    for v in dynamic.opaque.vertices.iter() {
        if f64::from(v.z) < weapon_base - 1e-6 {
            continue;
        }
        let (dx, dy) = (f64::from(v.x) - cx, f64::from(v.y) - cy);
        let d = (dx * dx + dy * dy).sqrt();
        if d > best_d {
            best_d = d;
            best = (dx, dy);
        }
    }
    best
}

#[test]
fn turret_weapon_turns_with_its_last_target() {
    use crate::entities::BuildingKind;
    // The same gun, recorded as firing first east and then north: the tip
    // of the weapon must lie in the direction of that shot (rules.md
    // section 10).
    let (ex, ey) = turret_weapon_tip(BuildingKind::TurretNormal, Some((100.0, 0.0)));
    assert!(
        ex > 15.0 && ey.abs() < 5.0,
        "barrel did not turn east: ({ex}, {ey})"
    );
    let (nx, ny) = turret_weapon_tip(BuildingKind::TurretNormal, Some((0.0, 100.0)));
    assert!(
        ny > 15.0 && nx.abs() < 5.0,
        "barrel did not turn north: ({nx}, {ny})"
    );
}

#[test]
fn the_three_turrets_carry_different_weapons() {
    use crate::entities::BuildingKind;
    // The ordinary gun reaches further than the twin barrels of the rapid
    // mount, and the rocket rack is the shortest of the three, so the
    // kinds stay apart without reading the ammo drum (rules.md section 10).
    let length = |kind| {
        let (dx, dy) = turret_weapon_tip(kind, None);
        (dx * dx + dy * dy).sqrt()
    };
    let normal = length(BuildingKind::TurretNormal);
    let rapid = length(BuildingKind::TurretRapid);
    let rocket = length(BuildingKind::TurretRocket);
    assert!(
        normal > rapid + 2.0,
        "ordinary gun is not the longest: {normal} vs {rapid}"
    );
    assert!(
        rapid > rocket,
        "the rocket rack reaches past the twin guns: {rapid} vs {rocket}"
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
            let d = dist2(*m, (tx, ty));
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
            let d = dist2((f64::from(v.x), f64::from(v.y)), edge).sqrt();
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
    assert_eq!(count(&a), count(&b));
    // One spatial chunk; 12 tiles at height 1 surrounded by water:
    // 12 tops (4 tris each) plus outer skirts (3 edges x 2 tris each).
    assert_eq!(a.chunks.len(), 1);
    assert_eq!(count(&a), 300);
    let grid: usize = a.chunks.iter().map(|c| c.grid_lines.len()).sum();
    assert_eq!(grid, 12 * 6);
    for chunk in a.chunks.iter() {
        assert!(chunk.soup.vertices.len() <= CHUNK_VERTICES);
        // Vertices come in whole triangles, which is what lets the
        // renderer index them as 0..len without a stored index buffer.
        assert_eq!(chunk.soup.vertices.len() % 3, 0);
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

/// Board with one bridge along column 5: land of height 3 with two water
/// fields in between, so the deck flies over water (rules.md section 8).
fn bridge_board() -> (Board, Tile, Tile, Vec<Tile>) {
    let mut board = Board::new(14, 14);
    for t in board.tiles.clone().keys() {
        board.tiles.get_mut(t).unwrap().height = 3;
    }
    for t in [(5, 6), (5, 7)] {
        board.tiles.get_mut(&t).unwrap().height = 0;
    }
    let a = (5, 5);
    let b = (5, 8);
    let idx = board.add_bridge(a, b, 1).expect("bridge over water");
    let frags = board.bridges[idx].fragments.clone();
    (board, a, b, frags)
}

#[test]
fn bridge_shadows_lie_on_the_field_below_the_deck() {
    let (board, _a, _b, frags) = bridge_board();
    let br = &board.bridges[0];
    let mesh = build_terrain(&board);
    // One rectangle per fragment: two triangles, six vertices, all on the
    // water surface lifted by SHADOW_LIFT.
    let shadow_vertices: usize = mesh.chunks.iter().map(|c| c.shadows.vertices.len()).sum();
    assert_eq!(shadow_vertices, frags.len() * 2 * 3);
    let water = 0.0; // the fragment fields of the test board are water
    let expected_z = water + constants::SHADOW_LIFT;
    for v in mesh.chunks.iter().flat_map(|c| c.shadows.vertices.iter()) {
        assert_eq!(&v.color[..3], &constants::SHADOW_COLOR[..]);
        assert_eq!(v.color[3], constants::SHADOW_ALPHA);
        assert!(
            (f64::from(v.z) - expected_z).abs() < 1e-6,
            "shadow at z={} instead of {expected_z}",
            f64::from(v.z)
        );
    }
    // The shadow covers exactly the deck footprint, dropped straight down.
    for frag in frags.iter().copied() {
        let deck = bridge_deck_quad(&board, br, frag);
        let near = mesh
            .chunks
            .iter()
            .flat_map(|c| c.shadows.vertices.iter())
            .any(|v| {
                let (dx, dy) = (
                    f64::from(v.x) - deck.center.0,
                    f64::from(v.y) - deck.center.1,
                );
                let along = dx * deck.axis.0 + dy * deck.axis.1;
                let across = dx * -deck.axis.1 + dy * deck.axis.0;
                (along - deck.half_len).abs() < 1e-3 && (across - deck.half_wid).abs() < 1e-3
            });
        assert!(near, "no shadow corner at the deck corner of {frag:?}");
    }
    // The shadow never lands on the deck itself, so the deck stays lit.
    assert!(
        (expected_z - bridge_deck_z(br)).abs() > constants::SHADOW_LIFT,
        "the shadow was drawn on the deck"
    );
}

#[test]
fn a_board_without_bridges_has_no_shadows() {
    let board = Board::new(8, 8);
    let mesh = build_terrain(&board);
    assert!(
        mesh.chunks.iter().all(|c| c.shadows.vertices.is_empty()),
        "a board without bridges still produced shadows"
    );
}

#[test]
fn bridge_shadow_follows_the_field_below_even_over_land() {
    // The same bridge, but its fragments span low land instead of water:
    // the shadow must sit on that land, not hover at the water level.
    let mut board = Board::new(14, 14);
    for t in board.tiles.clone().keys() {
        board.tiles.get_mut(t).unwrap().height = 5;
    }
    for t in [(5, 6), (5, 7)] {
        board.tiles.get_mut(&t).unwrap().height = 1;
    }
    assert!(board.add_bridge((5, 5), (5, 8), 1).is_some());
    let mesh = build_terrain(&board);
    let land = 1.0 * constants::ELEVATION_PX;
    let expected = land + constants::SHADOW_LIFT;
    for v in mesh.chunks.iter().flat_map(|c| c.shadows.vertices.iter()) {
        assert!(
            (f64::from(v.z) - expected).abs() < 1e-6,
            "shadow at z={} instead of {expected}",
            f64::from(v.z)
        );
    }
    assert!(
        mesh.chunks.iter().any(|c| !c.shadows.vertices.is_empty()),
        "a bridge over land cast no shadow"
    );
}

#[test]
fn bridge_deck_is_a_narrow_strip_oriented_along_the_bridge() {
    let (board, _a, _b, frags) = bridge_board();
    let br = &board.bridges[0];
    for frag in frags.iter().copied() {
        let deck = bridge_deck_quad(&board, br, frag);
        let (cx, cy) = board.center_world(frag);
        assert!((deck.center.0 - cx).abs() < 1e-9 && (deck.center.1 - cy).abs() < 1e-9);
        // The axis points at the next field along the bridge...
        let nxt = hexgrid::neighbor(frag.0, frag.1, br.direction);
        let (nx, ny) = board.center_world(nxt);
        let len = dist2((nx, ny), (cx, cy)).sqrt();
        assert!(
            (deck.axis.0 * (nx - cx) + deck.axis.1 * (ny - cy) - len).abs() < 1e-6,
            "axis does not follow the bridge direction"
        );
        // ...the strip spans the gap between the two field centres...
        assert!(deck.half_len > len / 2.0, "deck is shorter than one step");
        // ...but it is far narrower than a field, so it reads as a bridge
        // and its direction is visible from above.
        assert!(
            deck.half_wid * 2.0 < board.side,
            "deck {} px wide is not narrower than a field",
            deck.half_wid * 2.0
        );
        for (x, y) in deck.corners.iter() {
            let (dx, dy) = (x - cx, y - cy);
            let along = dx * deck.axis.0 + dy * deck.axis.1;
            let across = dx * -deck.axis.1 + dy * deck.axis.0;
            assert!((along.abs() - deck.half_len).abs() < 1e-6);
            assert!((across.abs() - deck.half_wid).abs() < 1e-6);
        }
        assert_eq!(
            deck.z,
            br.w as f64 * constants::ELEVATION_PX + constants::BRIDGE_DECK_LIFT
        );
    }
    // The terrain mesh carries the deck strip, not a hexagon per fragment.
    // Top face and the four-sided slab band sit at deck height; the space
    // below the deck is carried by pillars only, because rules.md sec. 8
    // lets vehicles pass under a bridge.
    let mesh = build_terrain(&board);
    let count_color = |color: [u8; 3]| {
        mesh.chunks
            .iter()
            .flat_map(|c| c.soup.vertices.iter())
            .filter(|v| v.color == color)
            .count()
    };
    // Top face: one quad. Slab band: four quads.
    assert_eq!(
        count_color(constants::BRIDGE_DECK_COLOR),
        frags.len() * 2 * 3
    );
    assert_eq!(
        count_color(constants::BRIDGE_DECK_SIDE_COLOR),
        frags.len() * 4 * 2 * 3
    );
    for frag in frags.iter().copied() {
        let deck = bridge_deck_quad(&board, &board.bridges[0], frag);
        // The middle of the span is empty: a solid block down to the field
        // would put vertices there, while a deck on pillars leaves the
        // passage under the bridge open (rules.md sec. 8). Only the pillar
        // feet touch the field, and they sit at the deck corners.
        let inside = |v: &GpuVertex, share: f64| {
            let (dx, dy) = (
                f64::from(v.x) - deck.center.0,
                f64::from(v.y) - deck.center.1,
            );
            (dx * deck.axis.0 + dy * deck.axis.1).abs() < deck.half_len * share
                && (dx * -deck.axis.1 + dy * deck.axis.0).abs() < deck.half_wid * share
        };
        let low = |share: f64| {
            mesh.chunks
                .iter()
                .flat_map(|c| c.soup.vertices.iter())
                .filter(|v| inside(v, share) && f64::from(v.z) < constants::BRIDGE_DECK_THICKNESS)
                .count()
        };
        assert_eq!(
            low(0.5),
            0,
            "the deck is filled down to the field, blocking the passage below"
        );
        // The pillars do reach the field, at the four deck corners.
        for (cx, cy) in deck.corners.iter() {
            let standing = mesh
                .chunks
                .iter()
                .flat_map(|c| c.soup.vertices.iter())
                .any(|v| {
                    (f64::from(v.x) - cx).abs() <= constants::BRIDGE_PILLAR_WID
                        && (f64::from(v.y) - cy).abs() <= constants::BRIDGE_PILLAR_WID
                        && f64::from(v.z) < deck.z - constants::BRIDGE_DECK_THICKNESS
                });
            assert!(standing, "no pillar carries the deck corner {cx},{cy}");
        }
    }
}

#[test]
fn vehicle_on_the_deck_stands_on_it_and_under_the_bridge_stays_on_the_water() {
    use crate::constants::VehicleKind;
    use crate::entities::{Player, Vehicle};
    use crate::game::Game;
    let (board, a, b, frags) = bridge_board();
    let mid = frags[0];
    let (mx, my) = board.center_world(mid);
    let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
    // Driving along the deck: the vehicle rides the bridge, high above the
    // water it crosses (rules.md section 8).
    game.vehicles.push(Vehicle::new(
        VehicleKind::Tank,
        0,
        30.0,
        vec![mid, frags[1], b],
        (mx, my),
        Some(a),
    ));
    let deck_z = vehicle_surface_z(&game, &game.vehicles[0]);
    assert_eq!(deck_z, bridge_deck_z(&game.board.bridges[0]));
    assert!(
        deck_z > constants::ELEVATION_PX + 1.0,
        "deck {deck_z} is not above the water"
    );
    // Crossing under the same bridge: the same field, but a route that
    // walks across it instead of along it, so the vehicle stays on the
    // water and the deck hides it. It entered the fragment from the
    // field beside the bridge, so rules.md section 8 keeps it under the
    // deck even though the next hop runs along the bridge.
    let side = hexgrid::neighbor(mid.0, mid.1, 3);
    game.vehicles.push(Vehicle::new(
        VehicleKind::Hovercraft,
        0,
        30.0,
        vec![frags[1]],
        (mx, my),
        Some(side),
    ));
    let under_z = vehicle_surface_z(&game, &game.vehicles[1]);
    assert_eq!(under_z, 0.0, "crossing under the bridge used the deck");
    assert!(
        under_z < deck_z,
        "under {under_z} is not below the deck {deck_z}"
    );
    // A helicopter above the bridge keeps its fixed altitude and only its
    // shadow drops onto the deck.
    game.vehicles.push(Vehicle::new(
        VehicleKind::Helicopter,
        0,
        10.0,
        Vec::new(),
        (mx, my),
        None,
    ));
    assert_eq!(
        vehicle_z(&game, &game.vehicles[2]),
        helicopter_altitude(&game)
    );
    assert_eq!(
        helicopter_shadow_z(&game, mx, my),
        bridge_deck_z(&game.board.bridges[0])
    );
}

#[test]
fn route_waypoints_ride_the_deck_only_while_crossing_along_the_bridge() {
    use crate::constants::VehicleKind;
    let (board, a, b, frags) = bridge_board();
    let route = vec![frags[0], frags[1], b];
    let modes = route_crossings(&board, VehicleKind::Tank, Some(a), &route);
    for i in 0..route.len() - 1 {
        assert_eq!(
            waypoint_z(&board, &route, i, modes[i + 1]),
            bridge_deck_z(&board.bridges[0]),
            "waypoint {i} left the deck"
        );
    }
    // The far end is land again, so the vehicle has already left the deck.
    assert_eq!(modes[route.len()], Crossing::Deck);
    assert_eq!(
        waypoint_z(&board, &route, route.len() - 1, modes[route.len()]),
        tile_top_z(&board, b)
    );
    // A route crossing under the bridge enters its fragment from the
    // field beside it, so every waypoint keeps the water elevation and
    // the deck hides whatever drives there -- even where the route runs
    // along the bridge (rules.md section 8).
    let (mut board, _a, _b, frags) = bridge_board();
    let west = hexgrid::neighbor(frags[0].0, frags[0].1, 3);
    let east = hexgrid::neighbor(frags[1].0, frags[1].1, 5);
    for t in [west, east] {
        board.tiles.get_mut(&t).unwrap().height = 0;
    }
    let under = vec![frags[0], frags[1], east];
    let modes = route_crossings(&board, VehicleKind::Hovercraft, Some(west), &under);
    assert_eq!(
        modes[1],
        Crossing::Ground,
        "the route went under the bridge"
    );
    for i in 0..under.len() {
        assert_eq!(
            waypoint_z(&board, &under, i, modes[i + 1]),
            0.0,
            "waypoint {i} climbed"
        );
    }
}

#[test]
fn helicopter_shadow_over_a_bridge_is_clipped_to_the_deck() {
    use crate::constants::VehicleKind;
    use crate::entities::{Player, Vehicle};
    use crate::game::Game;
    let (board, _a, _b, frags) = bridge_board();
    let mid = frags[0];
    let (mx, my) = board.center_world(mid);
    let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
    game.vehicles.push(Vehicle::new(
        VehicleKind::Helicopter,
        0,
        10.0,
        Vec::new(),
        (mx, my),
        None,
    ));
    let mut dynamic = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut dynamic);
    let deck = bridge_deck_quad(&game.board, &game.board.bridges[0], mid);
    assert!(
        !dynamic.shadow.vertices.is_empty(),
        "a helicopter over a bridge still casts a shadow"
    );
    // Every shadow vertex lies on the deck plane, inside the deck strip:
    // the silhouette is much wider than the deck, so without a clip part
    // of it would hang in mid-air over the water.
    let z = deck.z + constants::SHADOW_LIFT;
    for v in dynamic.shadow.vertices.iter() {
        assert!(
            (f64::from(v.z) - z).abs() < 1e-6,
            "shadow left the deck plane"
        );
        let (dx, dy) = (
            f64::from(v.x) - deck.center.0,
            f64::from(v.y) - deck.center.1,
        );
        let along = dx * deck.axis.0 + dy * deck.axis.1;
        let across = dx * -deck.axis.1 + dy * deck.axis.0;
        assert!(
            along.abs() <= deck.half_len + 1e-6,
            "shadow {along} past the deck"
        );
        assert!(
            across.abs() <= deck.half_wid + 1e-6,
            "shadow {across} hangs beside the deck"
        );
    }
    // Away from the bridge the silhouette is unclipped and much wider, so
    // the clip is local to the deck it falls on.
    let (gx, gy) = game.board.center_world((1, 1));
    game.vehicles[0].x = gx;
    game.vehicles[0].y = gy;
    let mut open = DynamicMesh::default();
    build_dynamic(&game, 0.0, &mut open);
    let span = |mesh: &DynamicMesh, (ox, oy): (f64, f64)| {
        let mut widest: f64 = 0.0;
        for v in mesh.shadow.vertices.iter() {
            let d = (f64::from(v.x) - ox).hypot(f64::from(v.y) - oy);
            widest = widest.max(d);
        }
        widest
    };
    assert!(
        span(&open, (gx, gy)) > span(&dynamic, (mx, my)),
        "the shadow was clipped off the bridge too"
    );
}
#[test]
fn helicopter_route_leaves_from_its_shadow_not_from_the_hull() {
    use crate::constants::VehicleKind;
    use crate::entities::{Player, Vehicle};
    use crate::game::Game;
    let (board, a, b, frags) = bridge_board();
    let (mx, my) = board.center_world(frags[0]);
    let mut game = Game::new(board, vec![Player::new(0, true)], Vec::new(), 1);
    game.vehicles.push(Vehicle::new(
        VehicleKind::Helicopter,
        0,
        10.0,
        vec![frags[1], b],
        (mx, my),
        Some(a),
    ));
    let v = &game.vehicles[0];
    // The hull is at the fixed flight altitude, the route starts where the
    // shadow falls: on the deck while the helicopter crosses the bridge.
    assert_eq!(vehicle_z(&game, v), helicopter_altitude(&game));
    let deck = bridge_deck_z(&game.board.bridges[0]);
    assert_eq!(route_start_z(&game, v), deck);
    // The first drawn leg leaves from that surface, not from the hull.
    let mut lines = Vec::new();
    push_paths(&game, &mut lines);
    assert_eq!(lines.len(), 2, "one line per remaining leg");
    assert!((f64::from(lines[0].0.z) - deck).abs() < 1e-6);
    assert!(
        (f64::from(lines[0].0.z) - vehicle_z(&game, v)).abs() > 1.0,
        "the route still starts at the flying hull"
    );
    // Away from the bridge the shadow falls on the terrain, and so does
    // the route.
    let (lx, ly) = game.board.center_world((1, 1));
    game.vehicles[0].x = lx;
    game.vehicles[0].y = ly;
    let ground = vehicle_ground_z(&game, lx, ly);
    assert_eq!(route_start_z(&game, &game.vehicles[0]), ground);
    // A ground vehicle is drawn on the surface it drives on, so its route
    // still leaves the hull.
    game.vehicles.push(Vehicle::new(
        VehicleKind::Tank,
        0,
        10.0,
        vec![(2, 2)],
        (lx, ly),
        Some((1, 1)),
    ));
    let tank = &game.vehicles[1];
    assert_eq!(route_start_z(&game, tank), vehicle_z(&game, tank));
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
