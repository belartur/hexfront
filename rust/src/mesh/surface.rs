//! Walkable elevations: which surface a vehicle stands on, and how a route
//! crosses a bridge (rules.md section 8).
//!
//! A ground vehicle drives on the terrain, on a ramp at the lower of the two
//! heights it joins, or on a bridge deck; the drawing code asks this module
//! instead of deciding for itself, so the game, the routes and the shadows all
//! agree on where a vehicle is.

use super::terrain::bridge_deck_z;
use super::{ramp_waypoint_z, tile_top_z};
use crate::board::{Board, Crossing};
use crate::constants;
use crate::game::Game;
use crate::hexgrid::Tile;

/// Ground elevation under a vehicle, ignoring bridges.
///
/// A ramp tilts from the height of one joined neighbour to the height of the
/// other (rules.md section 7), so a vehicle standing on it interpolates its
/// elevation along the ramp axis instead of sitting at the lower end. Only
/// points past either edge clamp to the nearer end height.
pub fn vehicle_ground_z(game: &Game, x: f64, y: f64) -> f64 {
    if let Some(t) = game.board.world_to_tile(x, y) {
        if let Some(z) = game.board.ramp_height_at(t, x, y) {
            return z;
        }
        return game.board.height(t) as f64 * constants::ELEVATION_PX;
    }
    0.0
}

/// Tile a vehicle comes from: the last visited waypoint, or the source
/// field stored on the vehicle right after departure (`None` when the
/// vehicle has no route at all).
pub(super) fn route_prev(v: &crate::entities::Vehicle) -> Option<Tile> {
    if v.route_index > 0 && v.route_index <= v.route.len() {
        Some(v.route[v.route_index - 1])
    } else {
        v.src_tile
    }
}

/// Deck elevation of the bridge flying over `tile`, if any.
pub fn deck_z_of(board: &Board, tile: Option<Tile>) -> Option<f64> {
    let tile = tile?;
    board
        .tiles
        .get(&tile)
        .and_then(|t| t.bridge)
        .map(|i| bridge_deck_z(&board.bridges[i]))
}

/// Crossing mode after one more hop of a route.
///
/// A route never mixes the two ways across a bridge (rules.md section 8), so
/// this replays the very rule the simulation drives by and the renderer ends
/// up on the deck exactly when the vehicle is. A hop that starts without a
/// known field, or that no vehicle of `kind` could make, counts as ground.
fn next_crossing(
    board: &Board,
    kind: constants::VehicleKind,
    from: Option<Tile>,
    to: Tile,
    mode: Crossing,
) -> Crossing {
    from.and_then(|u| board.step(u, to, kind, mode))
        .unwrap_or(Crossing::Ground)
}

/// Crossing mode a vehicle stands in on the field it came from: the mode
/// reached after every hop of its route it has already finished.
///
/// This is the mode the rest of the route has to continue in, and it is
/// [`Crossing::Deck`] in the middle of a bridge, where the next hop runs on
/// the deck (rules.md section 8).
pub fn route_prev_crossing(game: &Game, v: &crate::entities::Vehicle) -> Crossing {
    let board = &game.board;
    let done = v.route_index.min(v.route.len());
    let mut cur = v.src_tile;
    let mut mode = Crossing::Ground;
    for t in v.route[..done].iter() {
        mode = next_crossing(board, v.kind, cur, *t, mode);
        cur = Some(*t);
    }
    mode
}

/// Crossing mode at every field of a route, starting from `src`.
///
/// Index 0 is the mode at `src`, index `i + 1` the mode reached after the hop
/// to `route[i]`. `src_mode` is the mode the vehicle is in on `src`: a route
/// a vehicle is setting off along starts on the ground ([`Crossing::Ground`],
/// no building stands on a bridge fragment), while the rest of a route
/// already under way continues in the mode the vehicle really reached
/// ([`route_prev_crossing`]). Starting from `Ground` in the middle of a
/// bridge would drop every following waypoint to the terrain below the deck,
/// because a deck is only ever stepped on from one of its two land ends.
pub fn route_crossings(
    board: &Board,
    kind: constants::VehicleKind,
    src: Option<Tile>,
    src_mode: Crossing,
    route: &[Tile],
) -> Vec<Crossing> {
    let mut modes = Vec::with_capacity(route.len() + 1);
    modes.push(src_mode);
    let mut cur = src;
    let mut mode = src_mode;
    for t in route {
        mode = next_crossing(board, kind, cur, *t, mode);
        modes.push(mode);
        cur = Some(*t);
    }
    modes
}

/// Crossing mode a vehicle drives in at its current position.
///
/// The vehicle is between the field it came from and the waypoint it drives
/// towards, and that hop already decided which of the two ways across a
/// bridge it takes, so the unfinished hop counts too. Once the route is
/// finished the mode of the last driven hop is kept, which the deck lookup
/// then ignores on the field where the vehicle came off the bridge.
pub fn vehicle_crossing(game: &Game, v: &crate::entities::Vehicle) -> Crossing {
    let done = v.route_index.min(v.route.len());
    let mode = route_prev_crossing(game, v);
    if done < v.route.len() {
        return next_crossing(&game.board, v.kind, route_prev(v), v.route[done], mode);
    }
    mode
}

/// Walkable-surface elevation under a ground vehicle, bridge deck included.
///
/// A vehicle travelling along a bridge stands on the deck (rules.md section
/// 8), so it is drawn over the water and over anything crossing underneath;
/// a vehicle crossing under a bridge stays on the terrain and is hidden by
/// the deck.
pub fn vehicle_surface_z(game: &Game, v: &crate::entities::Vehicle) -> f64 {
    let tile = game.board.world_to_tile(v.x, v.y);
    if vehicle_crossing(game, v) == Crossing::Deck
        && let Some(z) = deck_z_of(&game.board, tile)
    {
        return z;
    }
    vehicle_ground_z(game, v.x, v.y)
}

/// Elevation of the route waypoint `tile`, bridge deck included.
///
/// `mode` is the crossing mode the vehicle reaches `tile` in, as computed
/// by [`route_crossings`]: on a deck the waypoint rides it, a vehicle
/// crossing *under* a bridge keeps the terrain elevation. A waypoint on a
/// ramp sits mid-slope (see [`Board::ramp_center_z`]).
pub fn waypoint_z(board: &Board, tile: Tile, mode: Crossing) -> f64 {
    if mode == Crossing::Deck
        && let Some(z) = deck_z_of(board, Some(tile))
    {
        return z;
    }
    if let Some(z) = ramp_waypoint_z(board, tile) {
        return z;
    }
    tile_top_z(board, tile)
}

/// Points a drawn route runs through on field `tile`, in world space.
///
/// An ordinary field is a single point in its centre. A ramp (rules.md
/// section 7) is three: the edge the vehicle comes in over, the middle of the
/// slope and the edge it leaves over. Straight centre-to-centre waypoints would
/// halve the slope instead -- the vehicle drives onto the ramp across its full
/// width, from the height of the joined field on one edge to the height on the
/// other, while a centre point is only halfway up. Splitting on both edges
/// makes the drawn line climb exactly as steeply as the drawn slope.
///
/// `from` is the field the route enters `tile` from, which decides which edge
/// comes first; the exit edge is the opposite one. `None` (a route with no
/// known origin) falls back to the a/b order of the ramp.
///
/// A field crossed on a bridge deck is flat, so it stays a single point.
pub fn waypoint_points(
    board: &Board,
    tile: Tile,
    mode: Crossing,
    from: Option<Tile>,
) -> Vec<(f64, f64, f64)> {
    let (cx, cy) = board.center_world(tile);
    let middle = waypoint_z(board, tile, mode);
    if mode == Crossing::Deck {
        return vec![(cx, cy, middle)];
    }
    let Some((a, b)) = board.ramps.get(&tile).copied() else {
        return vec![(cx, cy, middle)];
    };
    let Some((pa, pb)) = board.ramp_edges(tile) else {
        return vec![(cx, cy, middle)];
    };
    let z_of = |n: Tile| board.height(n) as f64 * constants::ELEVATION_PX;
    // Entering over `b` means the route runs b -> a, so the edges swap.
    let (enter, leave) = if from == Some(b) {
        ((pb.0, pb.1, z_of(b)), (pa.0, pa.1, z_of(a)))
    } else {
        ((pa.0, pa.1, z_of(a)), (pb.0, pb.1, z_of(b)))
    };
    vec![enter, (cx, cy, middle), leave]
}
