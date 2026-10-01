//! The game board: tiles, obstacles, ramps, bridges and path-finding.
//!
//! Ground-movement rules implemented in [`Board::passable`] follow rules.md
//! sections 4, 5, 7 (ramps) and 8 (bridges).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::constants::{self, VehicleKind};
use crate::hexgrid::{self, Tile};
use crate::math::dist;

/// A static obstacle standing on a tile (rules.md sections 1, 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Obstacle {
    /// Obstacle kind string: wall, mine, trap_fire, trap_ice.
    pub kind: ObstacleKind,
    /// Only walls have hit points.
    pub hp: i32,
}

/// Static obstacle kinds (rules.md section 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObstacleKind {
    /// 20 hp, blocks ground vehicles until destroyed.
    Wall,
    /// 25 damage once, then removed; stands on land and on water alike.
    Mine,
    /// 1 dmg/s while on it, never removed.
    TrapFire,
    /// Halves speed while on it, never removed.
    TrapIce,
}

impl Obstacle {
    /// Create an obstacle of `kind`.
    pub fn new(kind: ObstacleKind) -> Self {
        let hp = if kind == ObstacleKind::Wall {
            constants::WALL_HP
        } else {
            0
        };
        Self { kind, hp }
    }
    #[allow(dead_code)]
    /// String name of the kind.
    pub fn kind_str(&self) -> &'static str {
        match self.kind {
            ObstacleKind::Wall => "wall",
            ObstacleKind::Mine => "mine",
            ObstacleKind::TrapFire => "trap_fire",
            ObstacleKind::TrapIce => "trap_ice",
        }
    }
}

/// One hexagonal field of the board.
#[derive(Clone, Debug)]
pub struct HexTile {
    /// Immutable in play; 0 = water, 1..15 = land (rules.md section 1).
    pub height: i32,
    /// Static obstacle, if any.
    pub obstacle: Option<Obstacle>,
    /// Ramp: pair (a, b) of *opposite* neighbour tiles it connects.
    pub ramp: Option<(Tile, Tile)>,
    /// Index into [`Board::bridges`] when a deck fragment flies over.
    pub bridge: Option<usize>,
}

impl HexTile {
    /// Create a tile of `height`.
    pub fn new(height: i32) -> Self {
        Self {
            height,
            obstacle: None,
            ramp: None,
            bridge: None,
        }
    }
}

/// A straight bridge connecting two non-adjacent equal-height tiles.
#[derive(Clone, Debug)]
pub struct Bridge {
    /// Land tile at one end (height `w`).
    pub a: Tile,
    /// Land tile at the other end (height `w`); unused by rendering but
    /// kept for format round-trips and gameplay queries.
    #[allow(dead_code)]
    pub b: Tile,
    /// Shared height of both ends.
    pub w: i32,
    /// Hex direction from `a` towards `b`.
    pub direction: usize,
    /// Deck tiles, ordered a -> b.
    pub fragments: Vec<Tile>,
    /// Adjacent tile pairs connected *along the deck*.
    pub pairs: HashSet<(Tile, Tile)>,
}

impl Bridge {
    /// Create a bridge; `pairs` connects consecutive tiles a..b.
    pub fn new(a: Tile, b: Tile, w: i32, direction: usize, fragments: Vec<Tile>) -> Self {
        let mut pairs = HashSet::new();
        let mut seq = Vec::with_capacity(fragments.len() + 2);
        seq.push(a);
        seq.extend(fragments.iter().copied());
        seq.push(b);
        for w2 in seq.windows(2) {
            let (u, v) = (w2[0], w2[1]);
            pairs.insert(if u <= v { (u, v) } else { (v, u) });
        }
        Self {
            a,
            b,
            w,
            direction,
            fragments,
            pairs,
        }
    }
    /// True when the deck directly connects `u` with `v`.
    pub fn connects(&self, u: Tile, v: Tile) -> bool {
        let key = if u <= v { (u, v) } else { (v, u) };
        self.pairs.contains(&key)
    }
    /// True when `t` is one of the two land ends of the bridge.
    ///
    /// The deck can only be stepped on from an end, i.e. by driving *along*
    /// the bridge (rules.md section 8); no other field touches the deck
    /// drivably.
    pub fn is_end(&self, t: Tile) -> bool {
        self.a == t || self.b == t
    }
}

/// How a ground vehicle crosses a field carrying a bridge fragment
/// (rules.md section 8).
///
/// A bridge is crossed either *on* its deck (along the bridge) or *under*
/// it (by the normal terrain rules) — never one way and then the other:
/// the mode a vehicle enters a bridge with is kept until it leaves the
/// bridge again, so a deck is never entered from the side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Crossing {
    /// On the terrain; a field carrying a fragment is crossed under the deck.
    Ground,
    /// On the deck of a bridge, travelling along it.
    Deck,
}

/// Rebuild the tile route of a breadth-first search from its parent links,
/// leaving out the source field.
fn walk_path(
    prev: &HashMap<(Tile, Crossing), (Tile, Crossing)>,
    start: (Tile, Crossing),
    last: (Tile, Crossing),
) -> Vec<Tile> {
    let mut path = vec![last.0];
    let mut cur = last;
    while cur != start {
        cur = prev[&cur];
        path.push(cur.0);
    }
    path.reverse();
    path.remove(0); // drop the source field
    path
}

/// Rectangular (odd-q) board of hexagonal tiles.
#[derive(Clone, Debug)]
pub struct Board {
    /// Number of columns.
    pub cols: i32,
    /// Number of rows.
    pub rows: i32,
    /// Hex side length in world units.
    pub side: f64,
    /// All tiles by coordinates.
    pub tiles: HashMap<Tile, HexTile>,
    /// All whole bridges.
    pub bridges: Vec<Bridge>,
    /// Tiles carrying a ramp: `{tile: (a, b)}`.
    pub ramps: HashMap<Tile, (Tile, Tile)>,
}

impl Board {
    /// Create an empty `cols` x `rows` board of height-1 tiles.
    pub fn new(cols: i32, rows: i32) -> Self {
        Self::with_side(cols, rows, constants::HEX_SIDE)
    }
    /// Create an empty board with an explicit hex side length.
    pub fn with_side(cols: i32, rows: i32, side: f64) -> Self {
        let mut tiles = HashMap::new();
        for q in 0..cols {
            for r in 0..rows {
                tiles.insert((q, r), HexTile::new(1));
            }
        }
        Self {
            cols,
            rows,
            side,
            tiles,
            bridges: Vec::new(),
            ramps: HashMap::new(),
        }
    }
    /// True when `tile` lies on the board.
    pub fn contains(&self, tile: Tile) -> bool {
        tile.0 >= 0 && tile.0 < self.cols && tile.1 >= 0 && tile.1 < self.rows
    }
    #[allow(dead_code)]
    /// Tile object or `None` when outside the board.
    pub fn tile(&self, tile: Tile) -> Option<&HexTile> {
        self.tiles.get(&tile)
    }
    #[allow(dead_code)]
    /// Mutable tile object or `None` when outside the board.
    pub fn tile_mut(&mut self, tile: Tile) -> Option<&mut HexTile> {
        self.tiles.get_mut(&tile)
    }
    /// Terrain height of `tile` (0 outside the board = open water).
    pub fn height(&self, tile: Tile) -> i32 {
        self.tiles.get(&tile).map(|t| t.height).unwrap_or(0)
    }
    /// All six neighbours of `tile` (on-board ones only).
    pub fn neighbors(&self, tile: Tile) -> Vec<Tile> {
        hexgrid::neighbors(tile.0, tile.1)
            .into_iter()
            .filter(|t| self.contains(*t))
            .collect()
    }
    /// World position of a tile centre.
    pub fn center_world(&self, tile: Tile) -> (f64, f64) {
        hexgrid::hex_to_world(tile.0, tile.1, self.side)
    }
    /// Tile containing a world point (clamped to the board or `None`).
    pub fn world_to_tile(&self, x: f64, y: f64) -> Option<Tile> {
        let t = hexgrid::world_to_hex(x, y, self.side);
        if self.contains(t) { Some(t) } else { None }
    }
    /// Tile under a screen position, refined against terrain elevation.
    ///
    /// Iteratively re-picks the tile assuming the elevation of the previous
    /// guess, so tall terrain is pointed at correctly (shared by the game
    /// input and the flat `Alt` mode of specification.md, section
    /// "Sterowanie"). With `flat` the tile is picked as if every field
    /// stood at height zero — no elevation refinement happens.
    #[allow(dead_code)]
    pub fn pick_tile(
        &self,
        camera: &crate::camera::Camera,
        sx: f32,
        sy: f32,
        flat: bool,
    ) -> Option<Tile> {
        let (mut wx, mut wy) = camera.screen_to_world(sx, sy, 0.0);
        let mut tile = self.world_to_tile(wx, wy);
        if flat {
            return tile;
        }
        for _ in 0..3 {
            let t = match tile {
                Some(t) => t,
                None => break,
            };
            let wz = self.height(t) as f64 * constants::ELEVATION_PX;
            (wx, wy) = camera.screen_to_world(sx, sy, wz);
            let refined = self.world_to_tile(wx, wy);
            if refined == tile {
                break;
            }
            tile = refined;
        }
        tile
    }
    /// Building tile nearest to the cursor within the snap radius.
    ///
    /// Measures, in world units (j), the distance between the cursor and
    /// each building tile centre (unprojecting the cursor at that tile's
    /// elevation, or at zero when `flat`), and returns the tile of the
    /// closest building within `HOVER_SNAP_RADIUS`, or `None` when every
    /// building is farther away. Exact-distance ties resolve
    /// deterministically by tile coordinates.
    pub fn snap_to_building(
        &self,
        camera: &crate::camera::Camera,
        sx: f32,
        sy: f32,
        buildings: &[crate::entities::Building],
        flat: bool,
    ) -> Option<Tile> {
        let mut best: Option<Tile> = None;
        let mut best_dist = f64::INFINITY;
        for b in buildings.iter() {
            let (cx, cy) = self.center_world(b.tile);
            let wz = if flat {
                0.0
            } else {
                self.height(b.tile) as f64 * constants::ELEVATION_PX
            };
            let (wx, wy) = camera.screen_to_world(sx, sy, wz);
            let dist = dist((wx, wy), (cx, cy));
            if dist > constants::HOVER_SNAP_RADIUS + 1e-9 {
                continue;
            }
            if best.is_none()
                || dist < best_dist - 1e-9
                || (dist - best_dist).abs() <= 1e-9 && b.tile < best.unwrap()
            {
                best = Some(b.tile);
                best_dist = dist;
            }
        }
        best
    }
    /// Turn tile `tile` into a ramp joining opposite neighbours `a`, `b`.
    /// The ramp tile's height becomes min(height(a), height(b)) (sec. 7).
    pub fn set_ramp(&mut self, tile: Tile, a: Tile, b: Tile) {
        let h = self.height(a).min(self.height(b));
        if let Some(t) = self.tiles.get_mut(&tile) {
            t.ramp = Some((a, b));
            t.height = h;
            t.obstacle = None;
            t.bridge = None;
        }
        self.ramps.insert(tile, (a, b));
    }
    #[allow(dead_code)]
    /// Remove the ramp standing on `tile`, if any.
    pub fn remove_ramp(&mut self, tile: Tile) {
        if let Some(t) = self.tiles.get_mut(&tile) {
            t.ramp = None;
        }
        self.ramps.remove(&tile);
    }
    /// Repair the bookkeeping after an in-place height edit of a ramp tile.
    ///
    /// [`Board::set_ramp`] normally sets the ramp height itself, but the map
    /// editor edits heights directly (ramps follow the lower end); this
    /// re-applies `min(height(a), height(b))` to every ramp
    /// touching `tile` without changing which tiles are joined.
    pub fn refresh_ramps_around(&mut self, tile: Tile) {
        use crate::hexgrid;
        let mut neighbours = hexgrid::neighbors(tile.0, tile.1).to_vec();
        neighbours.push(tile);
        for n in neighbours {
            if let Some((a, b)) = self.ramps.get(&n).copied() {
                let h = self.height(a).min(self.height(b));
                if let Some(t) = self.tiles.get_mut(&n) {
                    t.height = h;
                }
            }
        }
    }
    /// Repair the bookkeeping after an in-place height edit near a bridge.
    ///
    /// A bridge spans from `a` to `b` at the shared height `w` (rules.md
    /// section 8), so raising or lowering either of its land ends has to move
    /// the deck with it -- otherwise the deck keeps the old height and floats
    /// above (or sinks into) the terrain it is built from. Bridges whose
    /// fragments touch `tile` are re-levelled to the new end height; which
    /// tiles they join is left alone, and the editor still reports a run
    /// whose ends no longer match.
    pub fn refresh_bridges_around(&mut self, tile: Tile) {
        use crate::hexgrid;
        let mut neighbours = hexgrid::neighbors(tile.0, tile.1).to_vec();
        neighbours.push(tile);
        // End heights first, so the bridge list is not borrowed mutably while
        // the (immutable) tile lookup runs.
        let levels: Vec<i32> = self
            .bridges
            .iter()
            .map(|br| self.height(br.a).max(self.height(br.b)))
            .collect();
        for (br, w) in self.bridges.iter_mut().zip(levels) {
            let touches =
                br.a == tile || br.b == tile || br.fragments.iter().any(|f| neighbours.contains(f));
            if touches {
                br.w = w;
            }
        }
    }
    /// Try to build a bridge from `a` towards `b` in `direction`.
    /// Returns the new bridge index or `None` when the geometry does not
    /// permit one (sec. 8): the ends must share a height w >= 3, the
    /// straight corridor between them must stay on the board and every
    /// fragment tile must be lower than w - 2.
    pub fn add_bridge(&mut self, a: Tile, b: Tile, direction: usize) -> Option<usize> {
        let dir = direction % 6;
        if a == b || !self.contains(a) || !self.contains(b) {
            return None;
        }
        let mut fragments: Vec<Tile> = Vec::new();
        let mut cur = a;
        loop {
            cur = hexgrid::neighbor(cur.0, cur.1, dir);
            if cur == b {
                break;
            }
            if !self.contains(cur) {
                return None;
            }
            let t = self.tiles.get(&cur)?;
            if t.ramp.is_some() || t.bridge.is_some() || t.obstacle.is_some() {
                return None;
            }
            fragments.push(cur);
            if fragments.len() > (self.cols + self.rows) as usize {
                return None;
            }
        }
        let w = self.height(a);
        if w < 3 || w != self.height(b) {
            return None;
        }
        if fragments.iter().any(|t| self.height(*t) > w - 3) {
            return None;
        }
        let idx = self.bridges.len();
        let bridge = Bridge::new(a, b, w, dir, fragments.clone());
        for f in fragments.iter() {
            if let Some(t) = self.tiles.get_mut(f) {
                t.bridge = Some(idx);
            }
        }
        self.bridges.push(bridge);
        Some(idx)
    }
    /// The bridge whose deck connects `u` with `v`, if any.
    ///
    /// Only a step *along* the bridge counts, i.e. end-to-end hops of its
    /// straight run; a hop across the bridge from a field beside it is not
    /// a deck step (rules.md section 8).
    pub fn bridge_between(&self, u: Tile, v: Tile) -> Option<&Bridge> {
        let idx = self
            .tiles
            .get(&u)
            .and_then(|t| t.bridge)
            .or_else(|| self.tiles.get(&v).and_then(|t| t.bridge))?;
        self.bridges.get(idx).filter(|br| br.connects(u, v))
    }
    /// Terrain-only movement rules (rules.md sections 4, 5 and 7).
    ///
    /// Bridge decks are ignored, so a field carrying a fragment is driven
    /// on as the plain field below it — that is the *under the bridge*
    /// crossing of rules.md section 8.
    pub fn ground_step(&self, u: Tile, v: Tile, kind: VehicleKind) -> bool {
        if u == v {
            return false;
        }
        if kind == VehicleKind::Helicopter {
            return true; // sec. 5.2: flies everywhere
        }
        let tu = match self.tiles.get(&u) {
            Some(t) => t,
            None => return false,
        };
        let tv = match self.tiles.get(&v) {
            Some(t) => t,
            None => return false,
        };
        // Ramps (sec. 7): enterable only along the a/b axis; heights may
        // differ, but the non-ramp end must be drivable terrain.
        if tu.ramp.is_some() || tv.ramp.is_some() {
            if let Some((a, b)) = tu.ramp
                && (v == a || v == b)
            {
                return self.drivable(v, kind);
            }
            if let Some((a, b)) = tv.ramp
                && (u == a || u == b)
            {
                return self.drivable(u, kind);
            }
            return false; // off-axis move onto a ramp
        }
        let (hu, hv) = (tu.height, tv.height);
        if kind == VehicleKind::Hovercraft {
            if hu == hv {
                return true; // water-water / same land
            }
            let (lo, hi) = (hu.min(hv), hu.max(hv));
            return lo == 0 && hi == 1; // shore crossing only at h=1
        }
        // Tank / buffer (sec. 5.1, 5.4): equal-height land only.
        hu == hv && hu > 0
    }
    /// The step `u -> v` for a vehicle of `kind` crossing in `mode`, and
    /// the mode it continues in.
    ///
    /// Returns `None` when the step is not allowed. A deck is only stepped
    /// on from one of the two land ends, i.e. by driving *along* the
    /// bridge, and a vehicle that drove under a bridge stays under it: no
    /// step from the side ever puts anybody on a deck. Leaving the deck is
    /// the way down to the field below, which rules.md section 8 leaves
    /// open -- unlike a ramp, which may only be left at its ends (sec. 7).
    /// A helicopter flies over everything and keeps no crossing mode
    /// (sec. 5.2).
    pub fn step(&self, u: Tile, v: Tile, kind: VehicleKind, mode: Crossing) -> Option<Crossing> {
        if u == v {
            return None;
        }
        if kind == VehicleKind::Helicopter {
            return Some(Crossing::Ground);
        }
        let deck = self.bridge_between(u, v);
        if mode == Crossing::Deck {
            if deck.is_some() {
                return Some(Crossing::Deck);
            }
            if self.ground_step(u, v, kind) {
                return Some(Crossing::Ground);
            }
            return None;
        }
        if let Some(br) = deck
            && br.is_end(u)
        {
            return Some(Crossing::Deck);
        }
        if self.ground_step(u, v, kind) {
            return Some(Crossing::Ground);
        }
        None
    }
    #[allow(dead_code)]
    /// True when a vehicle of `kind` may drive directly u -> v.
    ///
    /// A single step of the movement graph, ignoring which way the vehicle
    /// entered the bridge: rules.md section 8 keeps that mode for the whole
    /// crossing, which is what [`Board::step`] and the route search do.
    pub fn passable(&self, u: Tile, v: Tile, kind: VehicleKind) -> bool {
        self.step(u, v, kind, Crossing::Ground).is_some()
            || self.step(u, v, kind, Crossing::Deck).is_some()
    }
    fn drivable(&self, tile: Tile, kind: VehicleKind) -> bool {
        let t = match self.tiles.get(&tile) {
            Some(t) => t,
            None => return false,
        };
        if t.ramp.is_some() {
            return true;
        }
        if kind == VehicleKind::Hovercraft {
            return true;
        }
        t.height > 0
    }
    /// Breadth-first shortest route src -> dst for vehicle `kind`.
    ///
    /// Returns the list of tiles *after* the source, including the
    /// destination, or `None` when no road exists. Mines, traps and walls
    /// are deliberately ignored (section 6); ramps and bridges are
    /// respected because they change the road graph itself.
    ///
    /// The search runs over `(field, crossing mode)` states, because
    /// rules.md section 8 keeps a vehicle either on a deck or under it for
    /// a whole crossing: a field beside a bridge is reachable both on the
    /// ground and on the deck, and only the route's own history decides
    /// which of the two is drivable.
    pub fn find_path(&self, src: Tile, dst: Tile, kind: VehicleKind) -> Option<Vec<Tile>> {
        if src == dst || !self.contains(src) || !self.contains(dst) {
            return None;
        }
        let start = (src, Crossing::Ground);
        let mut prev: HashMap<(Tile, Crossing), (Tile, Crossing)> = HashMap::new();
        let mut queue: VecDeque<(Tile, Crossing)> = VecDeque::from([start]);
        while let Some(cur) = queue.pop_front() {
            for n in self.neighbors(cur.0) {
                let Some(mode) = self.step(cur.0, n, kind, cur.1) else {
                    continue;
                };
                let next = (n, mode);
                if prev.contains_key(&next) {
                    continue;
                }
                prev.insert(next, cur);
                if n == dst {
                    return Some(walk_path(&prev, start, next));
                }
                queue.push_back(next);
            }
        }
        None
    }
    #[allow(dead_code)]
    /// Set of tiles reachable from `src` by vehicle `kind`.
    ///
    /// Like [`Board::find_path`], a bridge is crossed either on its deck or
    /// under it, never both; the returned set is the union of the fields
    /// reachable in either mode.
    pub fn reachable(&self, src: Tile, kind: VehicleKind) -> HashSet<Tile> {
        let start = (src, Crossing::Ground);
        let mut seen: HashSet<(Tile, Crossing)> = HashSet::from([start]);
        let mut queue: VecDeque<(Tile, Crossing)> = VecDeque::from([start]);
        let mut tiles: HashSet<Tile> = HashSet::from([src]);
        while let Some(cur) = queue.pop_front() {
            for n in self.neighbors(cur.0) {
                let Some(mode) = self.step(cur.0, n, kind, cur.1) else {
                    continue;
                };
                if seen.insert((n, mode)) {
                    tiles.insert(n);
                    queue.push_back((n, mode));
                }
            }
        }
        tiles
    }
    /// World-space length of a tile path (for travel-time estimates).
    /// When `start` is given, the hop from `start` to `path[0]` is included.
    pub fn path_world_length(&self, path: &[Tile], start: Option<Tile>) -> f64 {
        let mut total = 0.0;
        let mut prev = start.map(|s| self.center_world(s));
        for t in path.iter() {
            let pos = self.center_world(*t);
            if let Some(p) = prev {
                total += dist(pos, p);
            }
            prev = Some(pos);
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;
    use crate::constants::HOVER_SNAP_RADIUS;
    use crate::entities::{Building, BuildingKind};

    fn flat_board(cols: i32, rows: i32) -> Board {
        let mut board = Board::new(cols, rows);
        let tiles: Vec<Tile> = board.tiles.keys().copied().collect();
        for t in tiles {
            board.tiles.get_mut(&t).unwrap().height = 1;
        }
        board
    }

    /// Board with one bridge along column 5: the two land ends stand at
    /// height 4, the fields the deck flies over at `ground`.
    fn bridge_board(ground: i32) -> Board {
        let mut board = Board::new(14, 14);
        let tiles: Vec<Tile> = board.tiles.keys().copied().collect();
        for t in tiles {
            board.tiles.get_mut(&t).unwrap().height = ground;
        }
        for t in [(5, 5), (5, 8)] {
            board.tiles.get_mut(&t).unwrap().height = 4;
        }
        assert!(board.add_bridge((5, 5), (5, 8), 1).is_some());
        board
    }

    /// Crossing mode after every hop of `path` (index 0 = `src`).
    fn route_modes(board: &Board, kind: VehicleKind, src: Tile, path: &[Tile]) -> Vec<Crossing> {
        let mut modes = vec![Crossing::Ground];
        let mut cur = src;
        for t in path {
            let mode = board
                .step(cur, *t, kind, *modes.last().unwrap())
                .unwrap_or(Crossing::Ground);
            modes.push(mode);
            cur = *t;
        }
        modes
    }

    /// Every hop of the route is legal, and the deck is only ever entered
    /// from one of the two land ends of the bridge (rules.md section 8).
    fn assert_route_rules(board: &Board, kind: VehicleKind, src: Tile, path: &[Tile]) {
        let mut seq = vec![src];
        seq.extend_from_slice(path);
        let modes = route_modes(board, kind, src, path);
        for i in 1..seq.len() {
            let (from, to) = (seq[i - 1], seq[i]);
            assert!(
                board.step(from, to, kind, modes[i - 1]).is_some(),
                "{from:?} -> {to:?} is not a legal step of the route {seq:?}"
            );
            if modes[i] == Crossing::Deck && modes[i - 1] == Crossing::Ground {
                let br = board
                    .bridge_between(from, to)
                    .expect("a deck step runs along a bridge");
                assert!(
                    br.is_end(from),
                    "{from:?} -> {to:?} drives onto a deck from the side"
                );
            }
        }
    }

    #[test]
    fn a_deck_is_only_entered_from_the_ends_of_the_bridge() {
        let board = bridge_board(0);
        let (a, b) = ((5, 5), (5, 8));
        let (f1, f2) = ((5, 6), (5, 7));
        // Along the bridge: the ends step onto the deck and keep driving it.
        assert_eq!(
            board.step(a, f1, VehicleKind::Tank, Crossing::Ground),
            Some(Crossing::Deck)
        );
        assert_eq!(
            board.step(f1, f2, VehicleKind::Tank, Crossing::Deck),
            Some(Crossing::Deck)
        );
        assert_eq!(
            board.step(f2, b, VehicleKind::Tank, Crossing::Deck),
            Some(Crossing::Deck)
        );
        // The deck leads nowhere else: leaving it is the way down, and only
        // where the terrain below allows it (unlike a ramp, sec. 7).
        let side = hexgrid::neighbor(f1.0, f1.1, 3);
        assert_eq!(
            board.step(f1, side, VehicleKind::Tank, Crossing::Deck),
            None
        );
        let land = bridge_board(1);
        let side = hexgrid::neighbor(f1.0, f1.1, 3);
        assert_eq!(
            land.step(a, f1, VehicleKind::Tank, Crossing::Ground),
            Some(Crossing::Deck)
        );
        assert_eq!(
            land.step(f1, side, VehicleKind::Tank, Crossing::Deck),
            Some(Crossing::Ground)
        );
        // ...and a vehicle that came down stays under the deck from there,
        // even where the route runs along the bridge.
        assert_eq!(
            land.step(side, f1, VehicleKind::Tank, Crossing::Ground),
            Some(Crossing::Ground)
        );
        assert_eq!(
            land.step(f1, f2, VehicleKind::Tank, Crossing::Ground),
            Some(Crossing::Ground)
        );
        // A field beside a bridge is driven *under* the deck, and a vehicle
        // that went under stays there: a tank cannot use the water below.
        assert_eq!(
            board.step(side, f1, VehicleKind::Tank, Crossing::Ground),
            None
        );
        assert_eq!(
            board.step(f1, f2, VehicleKind::Tank, Crossing::Ground),
            None
        );
        // A hovercraft may cross under the bridge and keeps to the ground;
        // it also cannot climb out of the water under the deck onto the
        // height-4 end, so it never gets on a deck at all.
        assert_eq!(
            board.step(side, f1, VehicleKind::Hovercraft, Crossing::Ground),
            Some(Crossing::Ground)
        );
        assert_eq!(
            board.step(f1, f2, VehicleKind::Hovercraft, Crossing::Ground),
            Some(Crossing::Ground)
        );
        assert_eq!(
            board.step(f2, b, VehicleKind::Hovercraft, Crossing::Ground),
            None
        );
        // Helicopters fly over everything and keep no mode (sec. 5.2).
        assert_eq!(
            board.step(side, f1, VehicleKind::Helicopter, Crossing::Deck),
            Some(Crossing::Ground)
        );
    }

    #[test]
    fn a_route_between_the_ends_rides_the_whole_bridge() {
        let board = bridge_board(0);
        let (a, b) = ((5, 5), (5, 8));
        let path = board
            .find_path(a, b, VehicleKind::Tank)
            .expect("deck route");
        assert_eq!(path, vec![(5, 6), (5, 7), b]);
        assert_route_rules(&board, VehicleKind::Tank, a, &path);
        // The whole crossing is on the deck, entered at `a`.
        let modes = route_modes(&board, VehicleKind::Tank, a, &path);
        assert!(modes[1..].iter().all(|m| *m == Crossing::Deck));
    }

    #[test]
    fn a_route_across_a_bridge_stays_under_it() {
        let mut board = bridge_board(0);
        // The only shore reachable for a hovercraft stands on the far side
        // of the bridge, so its route runs across the fields below the deck.
        let shore = hexgrid::neighbor(5, 7, 5);
        board.tiles.get_mut(&shore).unwrap().height = 1;
        let side = hexgrid::neighbor(5, 6, 3);
        let path = board
            .find_path(side, shore, VehicleKind::Hovercraft)
            .expect("route under the bridge");
        assert_route_rules(&board, VehicleKind::Hovercraft, side, &path);
        let modes = route_modes(&board, VehicleKind::Hovercraft, side, &path);
        assert!(
            modes.iter().all(|m| *m == Crossing::Ground),
            "a hovercraft that entered from the side must not climb the deck: {path:?}"
        );
    }

    #[test]
    fn passable_reports_a_single_possible_step() {
        let board = bridge_board(0);
        // Both hops of the deck and the hop along it are single steps
        // someone can make; rules.md section 8 only constrains the sequence.
        assert!(board.passable((5, 5), (5, 6), VehicleKind::Tank));
        assert!(board.passable((5, 6), (5, 7), VehicleKind::Tank));
        assert!(board.passable((5, 7), (5, 8), VehicleKind::Tank));
        assert!(!board.passable((4, 6), (5, 6), VehicleKind::Tank));
    }

    #[test]
    fn pick_tile_hits_tall_terrain() {
        // Projecting a tile centre at its own elevation and picking it back
        // must round-trip (mirrors Board.pick_tile). A uniformly tall board
        // keeps every tile top on the same plane, so the iterative
        // refinement converges exactly.
        let mut board = flat_board(12, 12);
        let tiles: Vec<Tile> = board.tiles.keys().copied().collect();
        for t in tiles {
            board.tiles.get_mut(&t).unwrap().height = 4;
        }
        for tile in [(6, 6), (5, 5), (6, 5), (5, 6)] {
            let mut camera = Camera::new((800.0, 600.0));
            camera.limit_to_board(&board);
            let mid = board.center_world(tile);
            camera.center_on_world(mid.0, mid.1, 0.0);
            let (cx, cy) = board.center_world(tile);
            let z = board.height(tile) as f64 * constants::ELEVATION_PX;
            let (sx, sy) = camera.world_to_screen(cx, cy, z);
            assert_eq!(board.pick_tile(&camera, sx, sy, false), Some(tile));
        }
    }

    #[test]
    fn pick_tile_flat_ignores_elevation() {
        // Flat mode picks as if every field stood at height zero.
        let mut board = flat_board(10, 10);
        board.tiles.get_mut(&(4, 4)).unwrap().height = 10;
        let mut camera = Camera::new((800.0, 600.0));
        camera.limit_to_board(&board);
        let mid = board.center_world((4, 4));
        camera.center_on_world(mid.0, mid.1, 0.0);
        let (cx, cy) = board.center_world((4, 4));
        let (sx, sy) = camera.world_to_screen(cx, cy, 0.0);
        assert_eq!(
            board.pick_tile(&camera, sx, sy, true),
            board.world_to_tile(cx, cy)
        );
    }

    #[test]
    fn snap_to_building_prefers_closest() {
        let board = flat_board(20, 20);
        let buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(0), 2, 2, 10.0),
            Building::new(BuildingKind::BaseTank, Some(1), 8, 8, 10.0),
        ];
        let mut camera = Camera::new((800.0, 600.0));
        camera.limit_to_board(&board);
        let mid = board.center_world((2, 2));
        camera.center_on_world(mid.0, mid.1, 0.0);
        // Cursor exactly on the first building's tile centre.
        let (cx, cy) = board.center_world((2, 2));
        let z = board.height((2, 2)) as f64 * constants::ELEVATION_PX;
        let (sx, sy) = camera.world_to_screen(cx, cy, z);
        assert_eq!(
            board.snap_to_building(&camera, sx, sy, &buildings, false),
            Some((2, 2))
        );
        // Far away from every building: nothing is indicated.
        let (fx, fy) = camera.world_to_screen(1e6, 1e6, 0.0);
        assert_eq!(
            board.snap_to_building(&camera, fx, fy, &buildings, false),
            None
        );
        let _ = HOVER_SNAP_RADIUS;
    }
}
