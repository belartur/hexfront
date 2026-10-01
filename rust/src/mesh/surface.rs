//! Walkable elevations: which surface a vehicle stands on, and how a route
//! crosses a bridge (rules.md section 8).
//!
//! A ground vehicle drives on the terrain, on a ramp at the lower of the two
//! heights it joins, or on a bridge deck; the drawing code asks this module
//! instead of deciding for itself, so the game, the routes and the shadows all
//! agree on where a vehicle is.

use super::terrain::bridge_deck_z;
use super::tile_top_z;
use crate::board::{Board, Crossing};
use crate::constants;
use crate::game::Game;
use crate::hexgrid::Tile;

/// Ground elevation under a vehicle, ignoring bridges.
///
/// Ramps sit at the lower of the two heights they join. A vehicle driving
/// *along* a bridge stands on the deck instead — see
/// [`vehicle_surface_z`], which adds that case on top of this terrain lookup.
pub fn vehicle_ground_z(game: &Game, x: f64, y: f64) -> f64 {
    if let Some(t) = game.board.world_to_tile(x, y) {
        if let Some((a, b)) = game.board.ramps.get(&t) {
            return game.board.height(*a).min(game.board.height(*b)) as f64
                * constants::ELEVATION_PX;
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

/// Crossing mode at every field of a route, starting from `src`.
///
/// Index 0 is the mode at `src` (always [`Crossing::Ground`]: no building
/// stands on a bridge fragment, so a route never starts on a deck), index
/// `i + 1` the mode reached after the hop to `route[i]`.
pub fn route_crossings(
    board: &Board,
    kind: constants::VehicleKind,
    src: Option<Tile>,
    route: &[Tile],
) -> Vec<Crossing> {
    let mut modes = Vec::with_capacity(route.len() + 1);
    modes.push(Crossing::Ground);
    let mut cur = src;
    let mut mode = Crossing::Ground;
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
    let board = &game.board;
    let done = v.route_index.min(v.route.len());
    let mut cur = v.src_tile;
    let mut mode = Crossing::Ground;
    for t in v.route[..done].iter() {
        mode = next_crossing(board, v.kind, cur, *t, mode);
        cur = Some(*t);
    }
    if done < v.route.len() {
        mode = next_crossing(board, v.kind, cur, v.route[done], mode);
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

/// Elevation of the route waypoint `seq[i]`, bridge deck included.
///
/// `mode` is the crossing mode the vehicle reaches `seq[i]` in, as computed
/// by [`route_crossings`]: on a deck the waypoint rides it, a vehicle
/// crossing *under* a bridge keeps the terrain elevation.
pub fn waypoint_z(board: &Board, seq: &[Tile], i: usize, mode: Crossing) -> f64 {
    let tile = seq[i];
    if mode == Crossing::Deck
        && let Some(z) = deck_z_of(board, Some(tile))
    {
        return z;
    }
    tile_top_z(board, tile)
}
