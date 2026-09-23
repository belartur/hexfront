//! The game board: tiles, obstacles, ramps, bridges and path-finding.
//!
//! Ground-movement rules implemented in [`Board::passable`] follow rules.md
//! sections 4, 5, 7 (ramps) and 8 (bridges).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::constants::{self, VehicleKind};
use crate::hexgrid::{self, Tile};

/// A static obstacle standing on a tile (rules.md sections 1, 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Obstacle {
    /// Obstacle kind string: wall, mine, mine_water, trap_fire, trap_ice.
    pub kind: ObstacleKind,
    /// Only walls have hit points.
    pub hp: i32,
}

/// Static obstacle kinds (rules.md section 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObstacleKind {
    /// 20 hp, blocks ground vehicles until destroyed.
    Wall,
    /// 25 damage once, then removed (land).
    Mine,
    /// Mine on water.
    MineWater,
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
    /// String name of the kind (matches the Python implementation).
    pub fn kind_str(&self) -> &'static str {
        match self.kind {
            ObstacleKind::Wall => "wall",
            ObstacleKind::Mine => "mine",
            ObstacleKind::MineWater => "mine_water",
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
            let dist = ((wx - cx).powi(2) + (wy - cy).powi(2)).sqrt();
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
    /// True when a vehicle of `kind` may drive directly u -> v.
    pub fn passable(&self, u: Tile, v: Tile, kind: VehicleKind) -> bool {
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
        // Bridge decks (sec. 8): travelling along the bridge ignores
        // terrain heights; crossing under follows the normal rules.
        let br = tu.bridge.or(tv.bridge);
        if let Some(bi) = br
            && let Some(br) = self.bridges.get(bi)
            && br.connects(u, v)
        {
            return true;
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
    pub fn find_path(&self, src: Tile, dst: Tile, kind: VehicleKind) -> Option<Vec<Tile>> {
        if src == dst || !self.contains(src) || !self.contains(dst) {
            return None;
        }
        let mut prev: HashMap<Tile, Option<Tile>> = HashMap::new();
        prev.insert(src, None);
        let mut queue: VecDeque<Tile> = VecDeque::from([src]);
        while let Some(cur) = queue.pop_front() {
            for n in self.neighbors(cur) {
                if prev.contains_key(&n) || !self.passable(cur, n, kind) {
                    continue;
                }
                prev.insert(n, Some(cur));
                if n == dst {
                    let mut path = vec![n];
                    while let Some(Some(p)) = prev.get(path.last().copied().as_ref().unwrap_or(&n))
                    {
                        path.push(*p);
                        if *p == src {
                            break;
                        }
                    }
                    path.reverse();
                    // Drop the source tile.
                    if !path.is_empty() && path[0] == src {
                        path.remove(0);
                    }
                    return Some(path);
                }
                queue.push_back(n);
            }
        }
        None
    }
    #[allow(dead_code)]
    /// Set of tiles reachable from `src` by vehicle `kind`.
    pub fn reachable(&self, src: Tile, kind: VehicleKind) -> HashSet<Tile> {
        let mut seen: HashSet<Tile> = HashSet::from([src]);
        let mut queue: VecDeque<Tile> = VecDeque::from([src]);
        while let Some(cur) = queue.pop_front() {
            for n in self.neighbors(cur) {
                if !seen.contains(&n) && self.passable(cur, n, kind) {
                    seen.insert(n);
                    queue.push_back(n);
                }
            }
        }
        seen
    }
    /// World-space length of a tile path (for travel-time estimates).
    /// When `start` is given, the hop from `start` to `path[0]` is included.
    pub fn path_world_length(&self, path: &[Tile], start: Option<Tile>) -> f64 {
        let mut total = 0.0;
        let mut prev = start.map(|s| self.center_world(s));
        for t in path.iter() {
            let pos = self.center_world(*t);
            if let Some(p) = prev {
                total += ((pos.0 - p.0).powi(2) + (pos.1 - p.1).powi(2)).sqrt();
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
