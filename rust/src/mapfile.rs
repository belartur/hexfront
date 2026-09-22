//! Binary map file format (specification_of_map_format.md).
//!
//! Layout of a `.map` file, all multi-byte integers little-endian:
//!
//! * 2 header bytes: number of columns `k` (1 B) and rows `w` (1 B);
//! * `k * w` 4-bit heights, two tiles per byte in little-endian nibble order;
//! * objects, one record each: 1 B column + 1 B row + 1 B type, followed by
//!   2 extra bytes for buildings: a little-endian 16-bit word holding the
//!   owner code on the high 6 bits and the starting unit count on the low
//!   10 bits.
//!
//! Bridge fragments are reassembled into whole [`crate::board::Bridge`]
//! objects by [`rebuild_bridges`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::board::{Board, Bridge, Obstacle, ObstacleKind};
use crate::constants;
use crate::entities::{Building, BuildingKind, Player};
use crate::game::Game;
use crate::hexgrid::{self, Tile};

/// Building kind behind each building type code 0-19 (rest unused).
pub fn building_kind_of(code: u8) -> Option<BuildingKind> {
    match code {
        0 => Some(BuildingKind::BaseTank),
        1 => Some(BuildingKind::BaseHelicopter),
        2 => Some(BuildingKind::BaseHovercraft),
        3 => Some(BuildingKind::BaseBuffer),
        4 => Some(BuildingKind::TurretNormal),
        5 => Some(BuildingKind::TurretRapid),
        6 => Some(BuildingKind::TurretRocket),
        7 => Some(BuildingKind::HealTower),
        _ => None,
    }
}

#[allow(dead_code)]
/// Inverse of [`building_kind_of`].
pub fn building_code_of(kind: BuildingKind) -> u8 {
    match kind {
        BuildingKind::BaseTank => 0,
        BuildingKind::BaseHelicopter => 1,
        BuildingKind::BaseHovercraft => 2,
        BuildingKind::BaseBuffer => 3,
        BuildingKind::TurretNormal => 4,
        BuildingKind::TurretRapid => 5,
        BuildingKind::TurretRocket => 6,
        BuildingKind::HealTower => 7,
    }
}

/// First type code of bridges (code - BASE = axis 0-2).
pub const BRIDGE_CODE_BASE: u8 = 20;
/// First type code of ramps (code - BASE = axis 0-2).
pub const RAMP_CODE_BASE: u8 = 23;

/// Obstacle kind behind each obstacle type code 26 and up.
pub fn obstacle_kind_of(code: u8) -> Option<ObstacleKind> {
    match code {
        26 => Some(ObstacleKind::Wall),
        27 => Some(ObstacleKind::Mine),
        28 => Some(ObstacleKind::MineWater),
        29 => Some(ObstacleKind::TrapFire),
        30 => Some(ObstacleKind::TrapIce),
        _ => None,
    }
}

#[allow(dead_code)]
/// Inverse of [`obstacle_kind_of`].
pub fn obstacle_code_of(kind: ObstacleKind) -> u8 {
    match kind {
        ObstacleKind::Wall => 26,
        ObstacleKind::Mine => 27,
        ObstacleKind::MineWater => 28,
        ObstacleKind::TrapFire => 29,
        ObstacleKind::TrapIce => 30,
    }
}

/// Highest starting unit count storable for a building (10 bits).
pub const MAX_SAVED_UNITS: u32 = 999;
#[allow(dead_code)]
/// Building owner code: neutral.
pub const OWNER_CODE_NEUTRAL: u32 = 0;
#[allow(dead_code)]
/// Owner code uses 6 bits.
pub const OWNER_CODE_BITS: u32 = 6;

/// Non-fatal map-loading warning.
fn warn(path: &Path, message: &str) {
    eprintln!("warning: {}: {}", path.display(), message);
}
#[allow(dead_code)]
/// Write `board` and its `buildings` to the binary file `path`.
pub fn save_map(path: &Path, board: &Board, buildings: &[Building]) -> std::io::Result<()> {
    if board.cols > 255 || board.rows > 255 || board.cols < 1 || board.rows < 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{}: {}x{} board does not fit the map format",
                path.display(),
                board.cols,
                board.rows
            ),
        ));
    }
    let mut out: Vec<u8> = Vec::new();
    out.push(board.cols as u8);
    out.push(board.rows as u8);
    // Heights: two 4-bit nibbles per byte, low nibble first.
    let total = (board.cols * board.rows) as usize;
    let mut nibbles: Vec<u8> = Vec::with_capacity(total);
    for r in 0..board.rows {
        for q in 0..board.cols {
            nibbles.push((board.height((q, r)) & 0xF) as u8);
        }
    }
    for i in (0..total).step_by(2) {
        let lo = nibbles[i];
        let hi = if i + 1 < total { nibbles[i + 1] } else { 0 };
        out.push((hi << 4) | lo);
    }
    // Buildings.
    for b in buildings.iter() {
        let code = building_code_of(b.kind);
        let owner_code: u32 = match b.owner {
            None => OWNER_CODE_NEUTRAL,
            Some(id) => (id as u32) + 1,
        };
        let units = (b.units.round() as i64).clamp(0, MAX_SAVED_UNITS as i64) as u32;
        let word: u16 = ((owner_code << 10) | units) as u16;
        out.push(b.tile.0 as u8);
        out.push(b.tile.1 as u8);
        out.push(code);
        out.push((word & 0xFF) as u8);
        out.push((word >> 8) as u8);
    }
    // Ramps: axis = direction mod 3 of (a -> tile).
    for (tile, (a, _b)) in board.ramps.iter() {
        // Find axis: direction from a to tile mod 3.
        let mut axis = 0u8;
        for d in 0..6 {
            if hexgrid::neighbor(a.0, a.1, d) == *tile {
                axis = (d % 3) as u8;
                break;
            }
        }
        out.push(tile.0 as u8);
        out.push(tile.1 as u8);
        out.push(RAMP_CODE_BASE + axis);
    }
    // Bridges: one record per deck fragment.
    for br in board.bridges.iter() {
        let axis = (br.direction % 3) as u8;
        for f in br.fragments.iter() {
            out.push(f.0 as u8);
            out.push(f.1 as u8);
            out.push(BRIDGE_CODE_BASE + axis);
        }
    }
    // Obstacles.
    for (tile, t) in board.tiles.iter() {
        if let Some(o) = t.obstacle.as_ref() {
            out.push(tile.0 as u8);
            out.push(tile.1 as u8);
            out.push(obstacle_code_of(o.kind));
        }
    }
    std::fs::write(path, out)
}

/// Load a board and its buildings from `path`.
/// Returns an error string on hard errors; soft issues only warn.
pub fn load_board(path: &Path) -> Result<(Board, Vec<Building>), String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    if data.len() < 2 {
        return Err(format!("{}: file shorter than 2 bytes", path.display()));
    }
    let (cols, rows) = (data[0] as i32, data[1] as i32);
    if cols == 0 || rows == 0 {
        return Err(format!("{}: empty board", path.display()));
    }
    let total = (cols * rows) as usize;
    let need = total.div_ceil(2);
    if data.len() < 2 + need {
        return Err(format!("{}: missing height data", path.display()));
    }
    let mut board = Board::new(cols, rows);
    for i in 0..total {
        let byte = data[2 + i / 2];
        let h = if i % 2 == 0 {
            byte & 0xF
        } else {
            (byte >> 4) & 0xF
        };
        let q = (i as i32) % cols;
        let r = (i as i32) / cols;
        if let Some(t) = board.tiles.get_mut(&(q, r)) {
            t.height = h as i32;
        }
    }
    let mut buildings: Vec<Building> = Vec::new();
    let mut frag_marks: HashMap<Tile, usize> = HashMap::new();
    let mut ramp_marks: Vec<(Tile, usize)> = Vec::new();
    let mut used: HashMap<Tile, String> = HashMap::new();
    let mut pos = 2 + need;
    while pos < data.len() {
        if pos + 3 > data.len() {
            // Trailing garbage byte(s): warn and stop (matches Python reader
            // which would fail unpack; be lenient: report error).
            return Err(format!("{}: truncated object record", path.display()));
        }
        let (q, r, typ) = (data[pos] as i32, data[pos + 1] as i32, data[pos + 2]);
        pos += 3;
        let tile = (q, r);
        if !board.contains(tile) {
            return Err(format!(
                "{}: object outside the board ({}, {})",
                path.display(),
                q,
                r
            ));
        }
        if typ <= 19 {
            if pos + 2 > data.len() {
                return Err(format!("{}: truncated building record", path.display()));
            }
            let word = data[pos] as u16 | ((data[pos + 1] as u16) << 8);
            pos += 2;
            let kind = match building_kind_of(typ) {
                Some(k) => k,
                None => {
                    warn(path, &format!("reserved building type {}", typ));
                    continue;
                }
            };
            let owner_code = (word >> 10) as u32;
            let mut units = (word & 0x3FF) as u32;
            if units > MAX_SAVED_UNITS {
                warn(path, &format!("unit count {} clamped to 999", units));
                units = MAX_SAVED_UNITS;
            }
            let t = board.tiles.get(&tile).unwrap();
            if t.height == 0 {
                warn(path, "building on water ignored");
                continue;
            }
            if used.contains_key(&tile) {
                warn(path, "two objects on one tile ignored");
                continue;
            }
            let owner = match owner_code {
                0 => None,
                1..=4 => Some((owner_code - 1) as usize),
                _ => {
                    warn(path, &format!("reserved owner {}", owner_code));
                    continue;
                }
            };
            used.insert(tile, "building".to_string());
            buildings.push(Building::new(kind, owner, q, r, units as f64));
        } else if (BRIDGE_CODE_BASE..BRIDGE_CODE_BASE + 3).contains(&typ) {
            if used.contains_key(&tile) {
                warn(path, "two objects on one tile ignored");
                continue;
            }
            used.insert(tile, "bridge".to_string());
            frag_marks.insert(tile, (typ - BRIDGE_CODE_BASE) as usize);
        } else if (RAMP_CODE_BASE..RAMP_CODE_BASE + 3).contains(&typ) {
            if used.contains_key(&tile) {
                warn(path, "two objects on one tile ignored");
                continue;
            }
            used.insert(tile, "ramp".to_string());
            ramp_marks.push((tile, (typ - RAMP_CODE_BASE) as usize));
        } else if typ >= 26 {
            let kind = match obstacle_kind_of(typ) {
                Some(k) => k,
                None => {
                    warn(path, &format!("unknown obstacle type {}", typ));
                    continue;
                }
            };
            if used.contains_key(&tile) {
                warn(path, "two objects on one tile ignored");
                continue;
            }
            let t = board.tiles.get(&tile).unwrap();
            let ok = match kind {
                ObstacleKind::MineWater => t.height == 0,
                _ => t.height > 0,
            };
            if !ok {
                warn(path, "obstacle on wrong terrain ignored");
                continue;
            }
            used.insert(tile, "obstacle".to_string());
            if let Some(tile_ref) = board.tiles.get_mut(&tile) {
                tile_ref.obstacle = Some(Obstacle::new(kind));
            }
        } else {
            warn(path, &format!("unknown type {}", typ));
        }
    }
    // Ramps: axis -> opposite neighbours.
    for (tile, axis) in ramp_marks {
        let a = hexgrid::neighbor(tile.0, tile.1, axis);
        let b = hexgrid::neighbor(tile.0, tile.1, axis + 3);
        if !board.contains(a) || !board.contains(b) {
            warn(path, "ramp with ends outside the board ignored");
            continue;
        }
        board.set_ramp(tile, a, b);
    }
    rebuild_bridges(&mut board, &frag_marks, true);
    Ok((board, buildings))
}
/// Load a full game (board + players + buildings) from `path`.
pub fn load_game(path: &Path) -> Result<Game, String> {
    let (board, buildings) = load_board(path)?;
    let mut owners: Vec<usize> = buildings.iter().filter_map(|b| b.owner).collect();
    owners.sort();
    owners.dedup();
    let top = owners.last().copied().unwrap_or(0);
    let players: Vec<Player> = (0..=top).map(|i| Player::new(i, i == 0)).collect();
    Ok(Game::new(board, players, buildings, level_seed(path)))
}

/// Deterministic AI seed of a level, derived from its file name.
/// Uses CRC-32 (IEEE) like Python `zlib.crc32` of the base file name.
pub fn level_seed(path: &Path) -> u64 {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    crc32(name.as_bytes()) as u64
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for b in data.iter() {
        crc ^= *b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Sorted paths of all map files in `directory` (default [`crate::constants::maps_dir`]).
pub fn list_maps(directory: Option<&Path>) -> Vec<PathBuf> {
    let dir: PathBuf = match directory {
        Some(d) => d.to_path_buf(),
        None => constants::maps_dir(),
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("map") {
            out.push(p);
        }
    }
    out.sort();
    out
}

/// (Re)build whole bridges from per-tile fragment marks.
///
/// `frag_marks` maps a deck tile to the geometric axis 0-2 of its bridge.
/// Every maximal straight run of same-axis fragments becomes one
/// [`Bridge`] via [`Board::add_bridge`], which validates the geometry of
/// rules.md section 8; invalid runs are silently dropped. With
/// `validate = false` geometry-invalid runs still get a bridge object.
pub fn rebuild_bridges(board: &mut Board, frag_marks: &HashMap<Tile, usize>, validate: bool) {
    board.bridges.clear();
    for t in board.tiles.values_mut() {
        t.bridge = None;
    }
    let mut remaining: HashMap<Tile, usize> = frag_marks.clone();
    while !remaining.is_empty() {
        let (tile, axis) = remaining.iter().next().map(|(k, v)| (*k, *v)).unwrap();
        let back = (axis + 3) % 6;
        let mut start = tile;
        let mut prev = hexgrid::neighbor(start.0, start.1, back);
        while remaining.get(&prev) == Some(&axis) {
            start = prev;
            prev = hexgrid::neighbor(start.0, start.1, back);
        }
        let mut run = vec![start];
        let mut cur = hexgrid::neighbor(start.0, start.1, axis);
        while remaining.get(&cur) == Some(&axis) {
            run.push(cur);
            cur = hexgrid::neighbor(cur.0, cur.1, axis);
        }
        for f in run.iter() {
            remaining.remove(f);
        }
        let a = hexgrid::neighbor(start.0, start.1, back);
        if board.contains(a) && board.contains(cur) {
            let bridge = board.add_bridge(a, cur, axis);
            if bridge.is_none() && !validate {
                let mut w = board.height(a).max(board.height(cur));
                for f in run.iter() {
                    w = w.max(board.height(*f));
                }
                let idx = board.bridges.len();
                let br = Bridge::new(a, cur, w, axis, run.clone());
                for f in run.iter() {
                    if let Some(t) = board.tiles.get_mut(f) {
                        t.bridge = Some(idx);
                    }
                }
                board.bridges.push(br);
            }
        } else if !validate {
            let mut w = 0;
            for f in run.iter() {
                w = w.max(board.height(*f));
            }
            if board.contains(a) {
                w = w.max(board.height(a));
            }
            if board.contains(cur) {
                w = w.max(board.height(cur));
            }
            let idx = board.bridges.len();
            let br = Bridge::new(a, cur, w, axis, run.clone());
            for f in run.iter() {
                if let Some(t) = board.tiles.get_mut(f) {
                    t.bridge = Some(idx);
                }
            }
            board.bridges.push(br);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::VehicleKind;
    #[test]
    fn all_repo_maps_load() {
        let dir = constants::maps_dir();
        let maps = list_maps(Some(&dir));
        assert!(!maps.is_empty(), "no maps in {}", dir.display());
        for m in maps {
            let (board, buildings) =
                load_board(&m).unwrap_or_else(|e| panic!("{}: {}", m.display(), e));
            assert!(board.cols >= 1 && board.rows >= 1);
            assert!(!buildings.is_empty());
        }
    }
    #[test]
    fn save_load_roundtrip() {
        // A board exercising every feature of the map format (mirrors the
        // Python `sample_board`/`sample_buildings` in test_mapfile.py).
        let mut board = Board::new(8, 6);
        for t in board.tiles.clone().keys().copied().collect::<Vec<_>>() {
            board.tiles.get_mut(&t).unwrap().height = 2;
        }
        board.tiles.get_mut(&(0, 0)).unwrap().height = 0;
        board.tiles.get_mut(&(7, 5)).unwrap().height = 7;
        // Ramp along axis 0: (2, 2) and (4, 3) are the opposite neighbours
        // of (3, 2) (odd column: dir 3 and dir 0).
        board.set_ramp((3, 2), (4, 3), (2, 2));
        // Bridge over water straight down column 5 between height-3 ends.
        board.tiles.get_mut(&(5, 2)).unwrap().height = 3;
        board.tiles.get_mut(&(5, 5)).unwrap().height = 3;
        board.tiles.get_mut(&(5, 3)).unwrap().height = 0;
        board.tiles.get_mut(&(5, 4)).unwrap().height = 0;
        assert!(board.add_bridge((5, 2), (5, 5), 1).is_some());
        board.tiles.get_mut(&(5, 0)).unwrap().obstacle = Some(Obstacle::new(ObstacleKind::Wall));
        board.tiles.get_mut(&(6, 0)).unwrap().obstacle = Some(Obstacle::new(ObstacleKind::TrapIce));
        board.tiles.get_mut(&(0, 0)).unwrap().obstacle =
            Some(Obstacle::new(ObstacleKind::MineWater));
        board.tiles.get_mut(&(6, 5)).unwrap().obstacle =
            Some(Obstacle::new(ObstacleKind::TrapFire));
        let buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(0), 1, 1, 20.0),
            Building::new(BuildingKind::TurretRocket, Some(1), 4, 1, 15.0),
            Building::new(BuildingKind::HealTower, None, 6, 1, 10.0),
            Building::new(BuildingKind::BaseHelicopter, Some(3), 7, 0, 5.0),
        ];
        let dir = std::env::temp_dir();
        let path = dir.join("hexfront_roundtrip_test.map");
        save_map(&path, &board, &buildings).unwrap();
        // 2 header bytes + ceil(48/2) height bytes + 4 building records
        // (2+1+2 B each) + 1 ramp + 2 bridge fragments + 4 obstacles
        // (2+1 B each) — one obstacle slot is taken by the bridge end.
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            (2 + 24 + 4 * 5 + 7 * 3) as u64
        );
        let (loaded_board, loaded_buildings) = load_board(&path).unwrap();
        assert_eq!((loaded_board.cols, loaded_board.rows), (8, 6));
        for (tile, t) in board.tiles.iter() {
            let lt = &loaded_board.tiles[tile];
            assert_eq!(lt.height, t.height, "height at {tile:?}");
            assert_eq!(
                lt.obstacle.as_ref().map(|o| o.kind),
                t.obstacle.as_ref().map(|o| o.kind),
                "obstacle at {tile:?}"
            );
        }
        // Ramp survives with its ends and the enforced min height.
        let lr = loaded_board.tiles[&(3, 2)].ramp.unwrap();
        assert!((lr.0 == (4, 3) && lr.1 == (2, 2)) || (lr.0 == (2, 2) && lr.1 == (4, 3)));
        assert_eq!(loaded_board.tiles[&(3, 2)].height, 2);
        // Bridge survives as a whole object with the same passable pairs.
        let lb = loaded_board.tiles[&(5, 3)]
            .bridge
            .map(|i| &loaded_board.bridges[i]);
        assert!(lb.is_some() && lb.unwrap().w == 3 && lb.unwrap().direction == 1);
        assert!(loaded_board.passable((5, 2), (5, 3), VehicleKind::Tank));
        assert!(loaded_board.passable((5, 4), (5, 5), VehicleKind::Tank));
        assert!(!loaded_board.passable((4, 3), (5, 3), VehicleKind::Tank));
        // Buildings survive with kind, owner and units.
        assert_eq!(loaded_buildings.len(), 4);
        let by_tile: HashMap<Tile, &Building> =
            loaded_buildings.iter().map(|b| (b.tile, b)).collect();
        assert_eq!(by_tile[&(1, 1)].kind, BuildingKind::BaseTank);
        assert_eq!(by_tile[&(1, 1)].owner, Some(0));
        assert_eq!(by_tile[&(1, 1)].units as i64, 20);
        assert_eq!(by_tile[&(4, 1)].owner, Some(1));
        assert_eq!(by_tile[&(6, 1)].owner, None);
        assert_eq!(by_tile[&(7, 0)].owner, Some(3));
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn unit_count_and_owner_bits() {
        // Units 0-999 ride the 10 low bits, the owner the 6 high ones.
        let mut board = Board::new(2, 1);
        for t in board.tiles.clone().keys().copied().collect::<Vec<_>>() {
            board.tiles.get_mut(&t).unwrap().height = 1;
        }
        let buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(3), 0, 0, 999.0),
            Building::new(BuildingKind::BaseTank, None, 1, 0, 1500.0),
        ];
        let dir = std::env::temp_dir();
        let path = dir.join("hexfront_bits_test.map");
        save_map(&path, &board, &buildings).unwrap();
        // Building record at offset 3 (2 header bytes + 1 height byte):
        // 2 coord bytes + 1 type + 2 property bytes. Owner 3 -> code 4 =
        // 0b000100, units 999 = 0b1111100111, so the 16-bit word is
        // 0b000100_1111100111 = 0x13E7, stored little-endian as E7 13.
        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[6..8], b"\xe7\x13");
        let (_, loaded) = load_board(&path).unwrap();
        let by_tile: HashMap<Tile, &Building> = loaded.iter().map(|b| (b.tile, b)).collect();
        assert_eq!(by_tile[&(0, 0)].units as i64, 999);
        assert_eq!(by_tile[&(0, 0)].owner, Some(3));
        // 1500 does not fit the format: clamped to 999 on save.
        assert_eq!(by_tile[&(1, 0)].units as i64, 999);
        assert_eq!(by_tile[&(1, 0)].owner, None);
        let _ = std::fs::remove_file(&path);
    }
}
