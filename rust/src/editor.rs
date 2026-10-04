//! In-game board editor: headless editing model plus macroquad UI state.
//!
//! The editor is a state of the same application rather than a separate
//! program (see specification_rust.md, section "Edytor plansz"). It shares
//! the game's renderer, camera and tile picking with the game itself, so a
//! change to how a board looks or is picked applies to both at once.
//!
//! The pure editing operations live on [`EditorState`] and deliberately do
//! not depend on macroquad, so the unit tests below run headlessly. The
//! keyboard/mouse handling lives in [`crate::app`].
//!
//! # Keys
//!
//! Editing is key-driven: the mouse only picks a tile, a key acts on it. The
//! on-screen legend ([`LEGEND`]) is the authoritative list, and the code is
//! the source of truth for the cycling orders ([`BUILDING_ORDER`],
//! [`OWNER_ORDER`], [`OBSTACLE_ORDER`]). `p` starts a test run of the level
//! being edited; `r` is therefore free for ramps.
//!
//! # Validation
//!
//! [`EditorState::validate`] returns the rule violations of rules.md as human
//! readable strings: a building on water, a bridge over too high land or
//! joining two different heights, a ramp joining tiles of the same height or
//! sitting at the wrong height, a missing player base, a missing opponent
//! base and a bonus no vehicle can reach. The editor lists them on screen but
//! never blocks a save, so a map can be stored and fixed later.
//!
//! # Board size
//!
//! A new board starts at [`EDITOR_NEW_COLS`] x
//! [`EDITOR_NEW_ROWS`], mostly water with a small land rectangle in
//! the middle. [`trim_map`] drops empty borders when saving and [`pad_map`]
//! grows a smaller map back up when loading, so a stored level keeps only the
//! area it really uses while the editor still works on a full-size board. Both
//! operations are documented where they are implemented; the odd-q grid forces
//! the column shift of a trim/pad to be even.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::board::{Board, Obstacle, ObstacleKind};
use crate::constants;
use crate::entities::{Bonus, BonusKind, Building, BuildingKind, Player, is_base};
use crate::game::Game;
use crate::hexgrid::{self, Tile};
use crate::mapfile;

/// Board editor tunables (specification_rust.md, section "Edytor plansz";
/// rules.md section 1 for the 0..15 heights and player bases).
///
/// These live here, next to the only code that uses them, instead of the
/// shared [`crate::constants`] module: they tune the editing tool, not the
/// game. Gameplay and presentation values shared by several modules stay in
/// `constants.rs`; a value used by a single module lives in that module
/// (same precedent as the `BLD_*`/`OBS_*`/`TANK_*` blocks in `mesh/`).
///
// --- Editor tunables -------------------------------------------------------
/// Seconds after which an unfinished 1- or 2-digit units entry commits.
///
/// A third digit commits immediately (the value cannot grow any further), so
/// this delay only applies to a short entry the editor is waiting on. It has
/// to be long enough to type the next digit and short enough not to surprise
/// the user with a value they were still editing.
pub const EDITOR_DIGIT_COMMIT_DELAY: f64 = 1.0;
/// Columns of a newly created editor board (mostly water).
pub const EDITOR_NEW_COLS: i32 = 256;
/// Rows of a newly created editor board.
pub const EDITOR_NEW_ROWS: i32 = 256;
/// Value a freshly placed `+x` bonus gets (rules.md section 13).
pub const EDITOR_DEFAULT_BONUS_ADD: u32 = 10;
/// Value a freshly placed `*x` bonus gets (rules.md section 13).
pub const EDITOR_DEFAULT_BONUS_MUL: u32 = 2;
/// Columns of the central land rectangle on a new board.
pub const EDITOR_LAND_COLS: i32 = 20;
/// Rows of the central land rectangle on a new board.
pub const EDITOR_LAND_ROWS: i32 = 13;
/// Terrain height of the central land rectangle of a new board.
pub const EDITOR_LAND_HEIGHT: i32 = 1;
/// Highest unit count typed in the editor (the map format stores 0-999).
pub const EDITOR_MAX_UNITS: u32 = 999;
/// Colour of the rule-violation lines on the editor screen.
pub const EDITOR_ERROR_COLOR: [u8; 3] = [255, 80, 80];

/// Building kinds behind the `b` key, in cycling order.
pub const BUILDING_ORDER: [BuildingKind; 8] = [
    BuildingKind::BaseTank,
    BuildingKind::BaseHelicopter,
    BuildingKind::BaseHovercraft,
    BuildingKind::BaseBuffer,
    BuildingKind::TurretNormal,
    BuildingKind::TurretRapid,
    BuildingKind::TurretRocket,
    BuildingKind::HealTower,
];

/// Building owners behind the `o` key: neutral first, then player ids.
pub const OWNER_ORDER: [Option<usize>; 5] = [None, Some(0), Some(1), Some(2), Some(3)];

/// Obstacle kinds behind the `t` key, in cycling order.
pub const OBSTACLE_ORDER: [ObstacleKind; 4] = [
    ObstacleKind::Wall,
    ObstacleKind::Mine,
    ObstacleKind::TrapFire,
    ObstacleKind::TrapIce,
];

/// Legend lines shown on the editor screen.
pub const LEGEND: [&str; 7] = [
    "b: building (again: cycle kind)    i: bonus (again: cycle kind)",
    "o: cycle owner                     t: obstacle (again: cycle kind)",
    "digits: units 0-999, bonus 1-999 (+x) / 2-99 (*x)",
    "m: bridge (again: rotate)          r: ramp (again: rotate)",
    "[/]: lower/raise terrain           Del/RMB: delete object",
    "l: load   s: save   ctrl+s: quick save   ctrl+n: new map",
    "p: play the level (Esc there: back to editing)   Esc: menu",
];

/// Which overlay (if any) the editor UI currently shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EditorOverlay {
    /// Normal tile editing.
    #[default]
    None,
    /// `l`: pick a map from the maps directory to load.
    Load,
    /// `s`: type a name or pick an existing map to save under.
    Save,
    /// Leaving with unsaved changes: save / discard / keep editing.
    Exit,
}

/// True when the editor UI reads typed characters (only the Save overlay's
/// name field does).
///
/// macroquad queues one char per pressed key and never drops it on its own,
/// so every other state must drain that queue — otherwise keys pressed while
/// playing or editing pile up and spill into the map name when Save opens
/// (in particular the `s` that opens the save overlay is swallowed too).
pub fn consumes_text(overlay: EditorOverlay) -> bool {
    overlay == EditorOverlay::Save
}

/// Map name to save under: the text typed in the Save overlay (when it is
/// open and not empty), else the current map name, else no name at all.
pub fn resolve_save_name(
    overlay: EditorOverlay,
    input_text: &str,
    map_name: Option<&str>,
) -> Result<String, String> {
    if overlay == EditorOverlay::Save && !input_text.trim().is_empty() {
        Ok(input_text.trim().to_string())
    } else if let Some(name) = map_name {
        Ok(name.to_string())
    } else {
        Err("no map name".to_string())
    }
}

/// Rewrite path separators in a map name so a save can never leave `maps/`.
pub fn sanitize_map_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();
    safe.trim().to_string()
}

/// Stateful map editor: a board plus its buildings plus remembered defaults.
///
/// Placing an object overwrites the object previously on the tile. The first
/// placed building is a neutral tank base with zero units; changing building
/// properties (kind, owner, unit counts) is remembered and reused for newly
/// placed buildings, and the last used obstacle kind is reused the same way.
#[derive(Clone, Debug)]
pub struct EditorState {
    /// Board under edit.
    pub board: Board,
    /// Buildings standing on the board.
    pub buildings: Vec<Building>,
    /// Bonuses standing on the board (rules.md section 13).
    pub bonuses: Vec<Bonus>,
    /// File name (without extension) the map was loaded from or saved under.
    pub map_name: Option<String>,
    /// True once the board differs from the last loaded/saved state.
    pub dirty: bool,
    /// Defaults reused for newly placed buildings.
    pub last_kind: BuildingKind,
    /// Defaults reused for newly placed buildings.
    pub last_owner: Option<usize>,
    /// Defaults reused for newly placed buildings.
    pub last_units: u32,
    /// Obstacle kind reused for newly placed obstacles.
    pub last_obstacle: ObstacleKind,
    /// Bonus kind reused for newly placed bonuses.
    pub last_bonus: BonusKind,
    /// Bonus value reused for newly placed bonuses and for the next step of
    /// the `+x` → `*x` → drone cycle, clamped to the range of the kind.
    pub last_bonus_value: u32,
    /// Pending digit entry for the unit count: tile + typed digits.
    pub digit_tile: Option<Tile>,
    /// Pending digit entry, committed on the third digit.
    pub digit_buf: String,
    /// Seconds since the last typed digit (commits after a delay).
    pub digit_age: f64,
    /// Overlay currently shown by the UI (macroquad input in `app.rs`).
    pub overlay: EditorOverlay,
    /// Map list shown by the load/save overlays.
    pub overlay_items: Vec<PathBuf>,
    /// Cursor position inside the overlay list.
    pub overlay_cursor: usize,
    /// Text typed into the save-name field.
    pub input_text: String,
}

impl EditorState {
    /// Create a fresh board: mostly water with a land rectangle of height
    /// [`EDITOR_LAND_HEIGHT`] in the middle.
    pub fn new_board() -> Self {
        let cols = EDITOR_NEW_COLS;
        let rows = EDITOR_NEW_ROWS;
        let mut board = Board::new(cols, rows);
        for t in board.tiles.values_mut() {
            t.height = 0;
        }
        let lw = EDITOR_LAND_COLS;
        let lh = EDITOR_LAND_ROWS;
        let q0 = (cols - lw) / 2;
        let r0 = (rows - lh) / 2;
        for q in q0..q0 + lw {
            for r in r0..r0 + lh {
                if let Some(t) = board.tiles.get_mut(&(q, r)) {
                    t.height = EDITOR_LAND_HEIGHT;
                }
            }
        }
        Self {
            board,
            buildings: Vec::new(),
            bonuses: Vec::new(),
            map_name: None,
            dirty: false,
            last_kind: BuildingKind::BaseTank,
            last_owner: None,
            last_units: 0,
            last_obstacle: ObstacleKind::Wall,
            last_bonus: BonusKind::Add(EDITOR_DEFAULT_BONUS_ADD),
            last_bonus_value: EDITOR_DEFAULT_BONUS_ADD,
            digit_tile: None,
            digit_buf: String::new(),
            digit_age: 0.0,
            overlay: EditorOverlay::None,
            overlay_items: Vec::new(),
            overlay_cursor: 0,
            input_text: String::new(),
        }
    }

    /// Fingerprint of everything the static terrain mesh is built from:
    /// tile heights, ramps and bridge fragments. Building-only edits (units,
    /// owner, kind) do not change it, so the UI can skip the expensive
    /// `build_terrain` call for them (see `sync_editor_game` in `app.rs`).
    pub fn terrain_fingerprint(&self) -> u64 {
        // FNV-1a over the deterministic tile order (BTreeMap-backed
        // iteration in Board::tiles is not guaranteed, so sort the keys).
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;
        let mut h = FNV_OFFSET;
        let mut mix = |b: u64| {
            h ^= b;
            h = h.wrapping_mul(FNV_PRIME);
        };
        mix(self.board.cols as u64);
        mix(self.board.rows as u64);
        let mut keys: Vec<Tile> = self.board.tiles.keys().copied().collect();
        keys.sort();
        for k in keys {
            let t = &self.board.tiles[&k];
            mix(k.0 as u64);
            mix(k.1 as u64);
            mix(t.height as u64);
            mix(u64::from(t.obstacle.is_some()));
            mix(u64::from(t.ramp.is_some()));
            mix(u64::from(t.bridge.is_some()));
        }
        mix(self.board.bridges.len() as u64);
        for b in self.board.bridges.iter() {
            mix(b.a.0 as u64);
            mix(b.a.1 as u64);
            mix(b.b.0 as u64);
            mix(b.b.1 as u64);
            mix(b.w as u64);
            mix(b.direction as u64);
            mix(b.fragments.len() as u64);
        }
        // Ramp rotation keeps the tile height but changes which neighbours
        // are joined, so the (a, b) ends must be hashed too — otherwise the
        // UI would skip the mesh rebuild and the rotated ramp would only
        // appear after an unrelated terrain edit.
        let mut ramp_keys: Vec<Tile> = self.board.ramps.keys().copied().collect();
        ramp_keys.sort();
        mix(ramp_keys.len() as u64);
        for k in ramp_keys {
            let (a, b) = self.board.ramps[&k];
            mix(k.0 as u64);
            mix(k.1 as u64);
            mix(a.0 as u64);
            mix(a.1 as u64);
            mix(b.0 as u64);
            mix(b.1 as u64);
        }
        h
    }

    /// Accept pending digit entry when the cursor leaves its tile.
    fn accept_digits(&mut self, tile: Option<Tile>) {
        if self.digit_tile.is_some() && self.digit_tile != tile {
            self.commit_digits();
        }
    }

    /// Commit the pending digit entry to its building or bonus, if still valid.
    pub fn commit_digits(&mut self) {
        if self.digit_buf.is_empty() {
            self.digit_tile = None;
            return;
        }
        if let Some(tile) = self.digit_tile
            && let Ok(units) = self.digit_buf.parse::<u32>()
        {
            if let Some(b) = self.building_at_mut(tile) {
                let units = units.min(EDITOR_MAX_UNITS);
                b.units = units as f64;
                self.last_units = units;
                self.dirty = true;
            } else if let Some(kind) = self.bonus_kind_at(tile) {
                // Bonus values share the digit entry with buildings
                // (rules.md section 13): +x takes 1-999, *x 2-99.
                let value = match kind {
                    BonusKind::Add(_) => {
                        units.clamp(constants::BONUS_ADD_MIN, constants::BONUS_ADD_MAX)
                    }
                    BonusKind::Mul(_) => {
                        units.clamp(constants::BONUS_MUL_MIN, constants::BONUS_MUL_MAX)
                    }
                    BonusKind::Drone => return,
                };
                self.set_bonus_value(tile, value);
            }
        }
        self.digit_tile = None;
        self.digit_buf.clear();
        self.digit_age = 0.0;
    }
    /// Kind of the bonus on `tile`, if any.
    fn bonus_kind_at(&self, tile: Tile) -> Option<BonusKind> {
        self.bonuses.iter().find(|b| b.tile == tile).map(|b| b.kind)
    }
    /// Write `value` into the bonus on `tile` (a drone has no value, so the
    /// call is ignored) and remember it for the next bonus.
    fn set_bonus_value(&mut self, tile: Tile, value: u32) {
        if let Some(bonus) = self.bonuses.iter_mut().find(|b| b.tile == tile) {
            match bonus.kind {
                BonusKind::Add(_) => bonus.kind = BonusKind::Add(value),
                BonusKind::Mul(_) => bonus.kind = BonusKind::Mul(value),
                BonusKind::Drone => return,
            }
            self.last_bonus_value = value;
            self.dirty = true;
        }
    }

    /// Advance the digit-entry commit timer (call every frame).
    ///
    /// A pending entry is committed once the user stops typing for
    /// [`EDITOR_DIGIT_COMMIT_DELAY`]; a full three-digit entry
    /// commits on the third key instead, so a completed value is never delayed
    /// (see [`EditorState::type_digit`]).
    pub fn tick(&mut self, dt: f64) {
        if !self.digit_buf.is_empty() {
            self.digit_age += dt;
            if self.digit_age >= EDITOR_DIGIT_COMMIT_DELAY {
                self.commit_digits();
            }
        }
    }

    /// Mutable building standing on `tile`, if any.
    fn building_at_mut(&mut self, tile: Tile) -> Option<&mut Building> {
        self.buildings.iter_mut().find(|b| b.tile == tile)
    }

    /// Index of the building standing on `tile`, if any.
    fn building_index(&self, tile: Tile) -> Option<usize> {
        self.buildings.iter().position(|b| b.tile == tile)
    }

    /// Remove every object (building, bonus, obstacle, ramp, bridge fragment)
    /// from `tile`, returning true when anything was removed.
    fn clear_tile(&mut self, tile: Tile) -> bool {
        let mut removed = false;
        if let Some(i) = self.building_index(tile) {
            self.buildings.remove(i);
            removed = true;
        }
        if let Some(i) = self.bonuses.iter().position(|b| b.tile == tile) {
            self.bonuses.remove(i);
            removed = true;
        }
        if self.board.tiles.get(&tile).and_then(|t| t.ramp).is_some() {
            if let Some(t) = self.board.tiles.get_mut(&tile) {
                t.ramp = None;
            }
            self.board.ramps.remove(&tile);
            removed = true;
        }
        if self.clear_bridge_fragment(tile) {
            removed = true;
        }
        if let Some(t) = self.board.tiles.get_mut(&tile)
            && t.obstacle.is_some()
        {
            t.obstacle = None;
            removed = true;
        }
        removed
    }

    /// Drop only the bridge fragment on `tile`, re-deriving every bridge from
    /// the remaining fragment marks.
    ///
    /// Editing concerns a single field, so the other fragments of a
    /// multi-field bridge keep their axis marks and the run is rebuilt around
    /// the hole. Returns true when a fragment was actually there.
    fn clear_bridge_fragment(&mut self, tile: Tile) -> bool {
        if self.board.tiles.get(&tile).and_then(|t| t.bridge).is_none() {
            return false;
        }
        let mut marks = self.frag_marks();
        marks.remove(&tile);
        crate::mapfile::rebuild_bridges(&mut self.board, &marks, false);
        true
    }

    /// Fragment axis marks kept across edits (deck tile -> axis 0-2).
    fn frag_marks(&self) -> std::collections::HashMap<Tile, usize> {
        let mut marks = std::collections::HashMap::new();
        for (tile, t) in self.board.tiles.iter() {
            if let Some(i) = t.bridge
                && let Some(br) = self.board.bridges.get(i)
            {
                marks.insert(*tile, br.direction % 3);
            }
        }
        marks
    }

    /// Place a bridge fragment of `axis` on `tile`.
    ///
    /// Only this one field is edited: the axis marks of the other fragments
    /// are kept, so a multi-field bridge survives an edit of a single field
    /// (rotating one fragment used to swallow the rest of its run). The
    /// terrain height is never touched -- the editor reports a fragment that
    /// sits too high instead of flooding it.
    fn put_bridge_fragment(&mut self, tile: Tile, axis: usize) -> bool {
        if !self.board.contains(tile) {
            return false;
        }
        // Snapshot every mark *before* clearing, so the fragments of this and
        // of every other bridge are rebuilt from the same set.
        let mut marks = self.frag_marks();
        self.board.bridges.clear();
        for t in self.board.tiles.values_mut() {
            t.bridge = None;
        }
        marks.insert(tile, axis % 3);
        crate::mapfile::rebuild_bridges(&mut self.board, &marks, false);
        true
    }

    /// `b`: place a building or cycle the kind of the existing one.
    pub fn press_b(&mut self, tile: Option<Tile>) -> bool {
        let Some(tile) = tile else { return false };
        if !self.board.contains(tile) {
            return false;
        }
        self.accept_digits(Some(tile));
        if self.building_index(tile).is_none() {
            self.clear_tile(tile);
            let b = Building::new(
                self.last_kind,
                self.last_owner,
                tile.0,
                tile.1,
                self.last_units as f64,
            );
            self.buildings.push(b);
            self.dirty = true;
            true
        } else {
            let i = self.building_index(tile).unwrap();
            let pos = BUILDING_ORDER
                .iter()
                .position(|k| *k == self.buildings[i].kind)
                .unwrap_or(0);
            let kind = BUILDING_ORDER[(pos + 1) % BUILDING_ORDER.len()];
            self.buildings[i].kind = kind;
            self.buildings[i].capacity = crate::entities::capacity_of(kind);
            self.last_kind = kind;
            self.dirty = true;
            true
        }
    }

    /// Type one digit of the unit count (0-999) of the building on `tile`,
    /// or of the bonus value (+x 1-999, *x 2-99) on `tile`.
    pub fn type_digit(&mut self, tile: Option<Tile>, digit: char) -> bool {
        let Some(tile) = tile else { return false };
        if !digit.is_ascii_digit() {
            return false;
        }
        let is_bonus = self.bonuses.iter().any(|b| b.tile == tile);
        if self.building_index(tile).is_none() && !is_bonus {
            return false;
        }
        if self.digit_tile != Some(tile) {
            self.commit_digits();
            self.digit_tile = Some(tile);
            self.digit_buf.clear();
        }
        self.digit_buf.push(digit);
        self.digit_age = 0.0;
        if self.digit_buf.len() >= 3 {
            self.commit_digits();
        } else if let Ok(units) = self.digit_buf.parse::<u32>() {
            if let Some(b) = self.building_at_mut(tile) {
                let units = units.min(EDITOR_MAX_UNITS);
                b.units = units as f64;
            } else if let Some(kind) = self.bonus_kind_at(tile) {
                let value = match kind {
                    BonusKind::Add(_) => {
                        units.clamp(constants::BONUS_ADD_MIN, constants::BONUS_ADD_MAX)
                    }
                    BonusKind::Mul(_) => {
                        units.clamp(constants::BONUS_MUL_MIN, constants::BONUS_MUL_MAX)
                    }
                    BonusKind::Drone => 0,
                };
                if value > 0 {
                    self.set_bonus_value(tile, value);
                }
            }
        }
        self.dirty = true;
        true
    }

    /// `o`: cycle the owner of the building on `tile`.
    pub fn press_o(&mut self, tile: Option<Tile>) -> bool {
        let Some(tile) = tile else { return false };
        self.accept_digits(Some(tile));
        let Some(i) = self.building_index(tile) else {
            return false;
        };
        let pos = OWNER_ORDER
            .iter()
            .position(|o| *o == self.buildings[i].owner)
            .unwrap_or(0);
        let owner = OWNER_ORDER[(pos + 1) % OWNER_ORDER.len()];
        self.buildings[i].owner = owner;
        self.last_owner = owner;
        self.dirty = true;
        true
    }

    /// `t`: place an obstacle or cycle the kind of the existing one.
    pub fn press_t(&mut self, tile: Option<Tile>) -> bool {
        let Some(tile) = tile else { return false };
        if !self.board.contains(tile) {
            return false;
        }
        self.accept_digits(Some(tile));
        let cur = self
            .board
            .tiles
            .get(&tile)
            .and_then(|t| t.obstacle.as_ref().map(|o| o.kind));
        if cur.is_none() {
            self.clear_tile(tile);
            if let Some(t) = self.board.tiles.get_mut(&tile) {
                t.obstacle = Some(Obstacle::new(self.last_obstacle));
            }
            self.dirty = true;
            true
        } else {
            let pos = OBSTACLE_ORDER
                .iter()
                .position(|k| *k == cur.unwrap())
                .unwrap_or(0);
            let kind = OBSTACLE_ORDER[(pos + 1) % OBSTACLE_ORDER.len()];
            if let Some(t) = self.board.tiles.get_mut(&tile) {
                t.obstacle = Some(Obstacle::new(kind));
            }
            self.last_obstacle = kind;
            self.dirty = true;
            true
        }
    }

    /// Best bridge axis for `tile`: a neighbouring fragment pointing at this
    /// tile wins, else the axis joining the highest pair of opposite
    /// neighbours at the same height.
    fn bridge_axis(tile: Tile, board: &Board) -> usize {
        let (q, r) = tile;
        for d in 0..6 {
            let n = hexgrid::neighbor(q, r, d);
            if let Some(t) = board.tiles.get(&n)
                && let Some(i) = t.bridge
                && let Some(br) = board.bridges.get(i)
            {
                let axis = br.direction % 3;
                let back = (d + 3) % 6;
                if back % 3 == axis {
                    return axis;
                }
            }
        }
        let mut best: Option<(usize, i32)> = None;
        for a in 0..3 {
            let na = hexgrid::neighbor(q, r, a);
            let nb = hexgrid::neighbor(q, r, a + 3);
            let (ha, hb) = (board.height(na), board.height(nb));
            if board.contains(na)
                && board.contains(nb)
                && ha == hb
                && best.map(|(_, bh)| ha > bh).unwrap_or(true)
            {
                best = Some((a, ha));
            }
        }
        best.map(|(a, _)| a).unwrap_or(0)
    }

    /// `m`: place a bridge fragment or rotate it.
    ///
    /// An edit always concerns exactly one field: the object on `tile` is
    /// cleared (the spec says placing overwrites whatever was there) and a
    /// single fragment mark of the chosen axis is written back, then every
    /// bridge is re-derived from the marks. The other fragments of a
    /// multi-field bridge keep their marks, so rotating one field no longer
    /// deletes the rest of the run.
    pub fn press_m(&mut self, tile: Option<Tile>) -> bool {
        let Some(tile) = tile else { return false };
        if !self.board.contains(tile) {
            return false;
        }
        self.accept_digits(Some(tile));
        let existing = self.board.tiles.get(&tile).and_then(|t| t.bridge);
        let axis = if let Some(i) = existing {
            self.board
                .bridges
                .get(i)
                .map(|b| b.direction % 3)
                .map(|a| (a + 1) % 3)
                .unwrap_or_else(|| Self::bridge_axis(tile, &self.board))
        } else {
            Self::bridge_axis(tile, &self.board)
        };
        self.clear_tile(tile);
        let ok = self.put_bridge_fragment(tile, axis);
        self.dirty = self.dirty || ok;
        ok
    }

    /// Preferred ramp axis for `tile`: the axis whose opposite neighbours
    /// differ in height, when one exists.
    fn ramp_axis(tile: Tile, board: &Board) -> usize {
        let (q, r) = tile;
        for a in 0..3 {
            let na = hexgrid::neighbor(q, r, a);
            let nb = hexgrid::neighbor(q, r, a + 3);
            if board.contains(na) && board.contains(nb) && board.height(na) != board.height(nb) {
                return a;
            }
        }
        0
    }

    /// Axis of an existing ramp on `tile` (from its `a` end).
    fn existing_ramp_axis(&self, tile: Tile) -> usize {
        if let Some((a, _)) = self.board.tiles.get(&tile).and_then(|t| t.ramp) {
            for d in 0..6 {
                if hexgrid::neighbor(tile.0, tile.1, d) == a {
                    return d % 3;
                }
            }
        }
        0
    }

    /// Place a ramp of `axis` on `tile`.
    fn put_ramp(&mut self, tile: Tile, axis: usize) {
        let a = hexgrid::neighbor(tile.0, tile.1, axis);
        let b = hexgrid::neighbor(tile.0, tile.1, axis + 3);
        let h = self.board.height(a).min(self.board.height(b));
        if let Some(t) = self.board.tiles.get_mut(&tile) {
            t.height = h;
        }
        self.board.set_ramp(tile, a, b);
    }

    /// `r`: place a ramp or rotate it.
    pub fn press_r(&mut self, tile: Option<Tile>) -> bool {
        let Some(tile) = tile else { return false };
        if !self.board.contains(tile) {
            return false;
        }
        self.accept_digits(Some(tile));
        if self.board.tiles.get(&tile).and_then(|t| t.ramp).is_some() {
            let axis = (self.existing_ramp_axis(tile) + 1) % 3;
            self.put_ramp(tile, axis);
        } else {
            self.clear_tile(tile);
            let axis = Self::ramp_axis(tile, &self.board);
            self.put_ramp(tile, axis);
        }
        self.dirty = true;
        true
    }

    /// `[` / `]`: lower / raise the terrain by 1 (no-op at 0 / 15).
    pub fn change_height(&mut self, tile: Option<Tile>, delta: i32) -> bool {
        let Some(tile) = tile else { return false };
        if !self.board.contains(tile) {
            return false;
        }
        self.accept_digits(Some(tile));
        let h = self.board.height(tile);
        let nh = (h + delta).clamp(0, 15);
        if nh == h {
            return false;
        }
        if let Some(t) = self.board.tiles.get_mut(&tile) {
            t.height = nh;
        }
        self.board.refresh_ramps_around(tile);
        self.board.refresh_bridges_around(tile);
        self.dirty = true;
        true
    }

    /// `i`: place a bonus or cycle the kind of the existing one
    /// (rules.md section 13: +x, *x, drone).
    ///
    /// A bonus fills the whole field, so placing one removes whatever stood on
    /// the tile (see [`EditorState::clear_tile`]). Pressing `i` again on the
    /// same field walks the kinds in the cycle and back to the first one; the
    /// value of `+x` and `*x` follows the last one used, like the remembered
    /// properties of a building.
    pub fn press_i(&mut self, tile: Option<Tile>) -> bool {
        let Some(tile) = tile else { return false };
        if !self.board.contains(tile) {
            return false;
        }
        self.accept_digits(Some(tile));
        // Bonuses stand on land only (rules.md section 13).
        if self.board.height(tile) == 0 {
            return false;
        }
        if let Some(i) = self.bonuses.iter().position(|b| b.tile == tile) {
            let kind = next_bonus_kind(self.bonuses[i].kind);
            self.bonuses[i].kind = kind;
            self.last_bonus = kind;
            if let BonusKind::Add(x) | BonusKind::Mul(x) = kind {
                self.last_bonus_value = x;
            }
            self.dirty = true;
            return true;
        }
        self.clear_tile(tile);
        let kind = match self.last_bonus {
            BonusKind::Add(_) => BonusKind::Add(
                self.last_bonus_value
                    .clamp(constants::BONUS_ADD_MIN, constants::BONUS_ADD_MAX),
            ),
            BonusKind::Mul(_) => BonusKind::Mul(
                self.last_bonus_value
                    .clamp(constants::BONUS_MUL_MIN, constants::BONUS_MUL_MAX),
            ),
            BonusKind::Drone => BonusKind::Drone,
        };
        self.bonuses.push(Bonus::new(tile, kind));
        self.last_bonus = kind;
        self.dirty = true;
        true
    }

    /// `Del` / RMB: delete the object on `tile`.
    pub fn delete_at(&mut self, tile: Option<Tile>) -> bool {
        let Some(tile) = tile else { return false };
        if !self.board.contains(tile) {
            return false;
        }
        self.accept_digits(Some(tile));
        let removed = self.clear_tile(tile);
        self.dirty = self.dirty || removed;
        removed
    }

    /// Open the load overlay (`l`): list maps, current name first.
    pub fn open_load(&mut self) {
        self.commit_digits();
        let mut items = mapfile::list_maps(None);
        self.move_current_first(&mut items);
        self.overlay_items = items;
        self.overlay_cursor = 0;
        self.overlay = EditorOverlay::Load;
    }

    /// Open the save overlay (`s`): type a name or pick an existing one.
    pub fn open_save(&mut self) {
        self.commit_digits();
        let mut items = mapfile::list_maps(None);
        self.move_current_first(&mut items);
        self.overlay_items = items;
        self.overlay_cursor = 0;
        self.input_text.clear();
        self.overlay = EditorOverlay::Save;
    }

    fn move_current_first(&self, items: &mut Vec<PathBuf>) {
        if let Some(name) = &self.map_name
            && let Some(i) = items.iter().position(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s == name)
                    .unwrap_or(false)
            })
        {
            let cur = items.remove(i);
            items.insert(0, cur);
        }
    }

    /// Quick save (`ctrl+s`): save under the current name, or open the save
    /// overlay when no name was chosen yet.
    pub fn quick_save(&mut self) -> Result<bool, String> {
        if self.map_name.is_some() {
            self.save()?;
            Ok(true)
        } else {
            self.open_save();
            Ok(false)
        }
    }

    /// Start a new map (`ctrl+n`), without asking for confirmation.
    pub fn new_map(&mut self) {
        *self = Self::new_board();
    }

    /// Build a game from the edited board for the in-editor playtest (`p`).
    ///
    /// Players are derived from the placed buildings exactly like
    /// [`crate::mapfile::load_game`] does for a saved map: ids `0..=highest
    /// owner`, player 0 human, so the AI drives every other owner. The editor
    /// keeps its own board — the returned game works on a copy, so a playtest
    /// never modifies the map being edited.
    pub fn playtest_game(&self) -> Game {
        let mut owners: Vec<usize> = self.buildings.iter().filter_map(|b| b.owner).collect();
        owners.sort_unstable();
        owners.dedup();
        let top = owners.last().copied().unwrap_or(0);
        let players: Vec<Player> = (0..=top).map(|i| Player::new(i, i == 0)).collect();
        let mut game = Game::new(
            self.board.clone(),
            players,
            self.buildings.clone(),
            self.bonuses.clone(),
            self.playtest_seed(),
        );
        // A test run is a sandbox: the match never ends, so an unfinished map
        // (no enemy base yet) can still be played and the AI keeps acting.
        game.sandbox = true;
        game
    }

    /// Deterministic AI seed of a playtest run: the map file name (like a
    /// level seed), or 0 for a map that was never saved under a name.
    pub fn playtest_seed(&self) -> u64 {
        match &self.map_name {
            Some(name) => crate::mapfile::level_seed(&crate::mapfile::save_path(name)),
            None => 0,
        }
    }

    /// Load the map stored at `path`, padding small boards up to the standard
    /// new-map size.
    pub fn load_path(&mut self, path: &Path) -> Result<(), String> {
        let (board, buildings, bonuses) = mapfile::load_board(path)?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("map")
            .to_string();
        let (board, buildings, bonuses) = pad_map(board, buildings, bonuses);
        self.board = board;
        self.buildings = buildings;
        self.bonuses = bonuses;
        // Remember the loaded bonus kind and value as defaults for new bonuses.
        if let Some(last) = self.bonuses.last() {
            self.last_bonus = last.kind;
            if let BonusKind::Add(x) | BonusKind::Mul(x) = last.kind {
                self.last_bonus_value = x;
            }
        }
        self.map_name = Some(name);
        self.dirty = false;
        self.digit_tile = None;
        self.digit_buf.clear();
        self.overlay = EditorOverlay::None;
        Ok(())
    }

    /// Save the map into the maps directory, trimming empty border rows and
    /// columns first. The name is the typed one (Save overlay) or the current
    /// map name; path separators are rewritten so a save stays in `maps/`.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        let name = resolve_save_name(self.overlay, &self.input_text, self.map_name.as_deref())?;
        let name = sanitize_map_name(&name);
        self.save_into(&crate::constants::maps_dir(), &name)
    }

    /// Save the map into `dir` under the file name `name` (without the
    /// extension), trimming empty border rows and columns first.
    ///
    /// Split from [`EditorState::save`] so tests can round-trip through a
    /// temporary directory: the menu and the simulation tests read the real
    /// `maps/` directory, and writing it from a test would race them.
    pub fn save_into(&mut self, dir: &Path, name: &str) -> Result<PathBuf, String> {
        if name.is_empty() {
            return Err("no map name".to_string());
        }
        self.commit_digits();
        let (board, buildings, bonuses) = trim_map(&self.board, &self.buildings, &self.bonuses);
        let path = dir.join(format!("{}{}", name, constants::MAP_EXTENSION));
        mapfile::save_map(&path, &board, &buildings, &bonuses).map_err(|e| e.to_string())?;
        self.map_name = Some(name.to_string());
        self.dirty = false;
        self.overlay = EditorOverlay::None;
        Ok(path)
    }

    /// Rule violations shown in red on the editor screen: building on water,
    /// bridge over too high land, bridge joining different heights, ramp on
    /// the wrong height, ramp joining equal heights, missing player base,
    /// missing enemy base, unreachable bonus. A map with errors can still be
    /// saved and loaded.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for b in self.buildings.iter() {
            if self.board.height(b.tile) == 0 {
                errors.push(format!("building on water at {},{}", b.tile.0, b.tile.1));
            }
        }
        for b in self.board.bridges.iter() {
            if self.board.height(b.a) != self.board.height(b.b) {
                errors.push(format!(
                    "bridge joins heights {} and {} at {},{} - {},{}",
                    self.board.height(b.a),
                    self.board.height(b.b),
                    b.a.0,
                    b.a.1,
                    b.b.0,
                    b.b.1
                ));
            }
            for f in b.fragments.iter() {
                let h = self.board.height(*f);
                if h > b.w.saturating_sub(2) {
                    errors.push(format!("bridge over too high land at {},{}", f.0, f.1));
                    break;
                }
            }
        }
        for (tile, (a, b)) in self.board.ramps.iter() {
            let (ha, hb) = (self.board.height(*a), self.board.height(*b));
            if ha == hb {
                errors.push(format!("ramp joins equal heights at {},{}", tile.0, tile.1));
            }
            if self.board.height(*tile) != ha.min(hb) {
                errors.push(format!("ramp on wrong height at {},{}", tile.0, tile.1));
            }
        }
        let mut player_base = false;
        let mut enemy_base = false;
        for b in self.buildings.iter() {
            if is_base(b.kind) {
                if b.owner == Some(0) {
                    player_base = true;
                } else {
                    enemy_base = true;
                }
            }
        }
        if !player_base {
            errors.push("missing player base".to_string());
        }
        if !enemy_base {
            errors.push("missing enemy base".to_string());
        }
        for bonus in self.bonuses.iter() {
            // A bonus no vehicle can reach is useless (rules.md section 13).
            let mut reachable = false;
            for b in self.buildings.iter() {
                let kind = crate::entities::vehicle_kind_of(b.kind);
                if self.board.find_path(b.tile, bonus.tile, kind).is_some() {
                    reachable = true;
                    break;
                }
            }
            if !reachable {
                errors.push(format!(
                    "unreachable bonus at {},{}",
                    bonus.tile.0, bonus.tile.1
                ));
            }
        }
        errors
    }
}

/// True when `tile` holds anything a trimmed map must keep: non-water, an
/// object, or a tile kept so no ramp/bridge endpoint falls off the board.
fn tile_occupied(board: &Board, tile: Tile, buildings: &[Building], bonuses: &[Bonus]) -> bool {
    let Some(t) = board.tiles.get(&tile) else {
        return false;
    };
    if t.height != 0 || t.obstacle.is_some() {
        return true;
    }
    if t.ramp.is_some() || t.bridge.is_some() {
        return true;
    }
    if buildings.iter().any(|b| b.tile == tile) {
        return true;
    }
    bonuses.iter().any(|b| b.tile == tile)
}

/// The kind that follows `current` in the editor cycle `+x` → `*x` → drone
/// → `+x`, with the starting value of the kind (rules.md section 13).
///
/// Cycling does not carry a typed number over: `*x` starts at 2 and `+x` at
/// 10, the same values a freshly placed bonus gets. A typed number is
/// remembered per kind in [`EditorState::last_bonus`] /
/// [`EditorState::last_bonus_value`] and applies to the next bonus of that
/// kind the map author places.
fn next_bonus_kind(current: BonusKind) -> BonusKind {
    match current {
        BonusKind::Add(_) => BonusKind::Mul(EDITOR_DEFAULT_BONUS_MUL),
        BonusKind::Mul(_) => BonusKind::Drone,
        BonusKind::Drone => BonusKind::Add(EDITOR_DEFAULT_BONUS_ADD),
    }
}

/// Remove empty border rows and columns from the board.
///
/// Empty means pure water (height 0) without any object. Every kept ramp
/// endpoint and bridge end is kept on the board as well, so no object loses
/// its neighbours. Returns a new `(board, buildings, bonuses)` triple with
/// shifted coordinates; the smallest possible result is a 1 x 1 board. The
/// column shift is always *even* (the odd-q grid is translation-invariant
/// only then), so one extra water column may be kept on the left when the
/// first occupied column is odd.
pub fn trim_map(
    board: &Board,
    buildings: &[Building],
    bonuses: &[Bonus],
) -> (Board, Vec<Building>, Vec<Bonus>) {
    use std::collections::HashSet;
    let mut occupied_cols: HashSet<i32> = HashSet::new();
    let mut occupied_rows: HashSet<i32> = HashSet::new();
    for tile in board.tiles.keys() {
        if tile_occupied(board, *tile, buildings, bonuses) {
            occupied_cols.insert(tile.0);
            occupied_rows.insert(tile.1);
        }
    }
    for (a, b) in board.ramps.values() {
        for end in [*a, *b] {
            if board.contains(end) {
                occupied_cols.insert(end.0);
                occupied_rows.insert(end.1);
            }
        }
    }
    for br in board.bridges.iter() {
        for end in [br.a, br.b] {
            if board.contains(end) {
                occupied_cols.insert(end.0);
                occupied_rows.insert(end.1);
            }
        }
    }
    if occupied_cols.is_empty() {
        occupied_cols.insert(0);
        occupied_rows.insert(0);
    }
    let (mut q0, r0) = (
        *occupied_cols.iter().min().unwrap(),
        *occupied_rows.iter().min().unwrap(),
    );
    let (q1, r1) = (
        *occupied_cols.iter().max().unwrap(),
        *occupied_rows.iter().max().unwrap(),
    );
    if q0 % 2 != 0 {
        q0 -= 1;
    }
    let (cols, rows) = (q1 - q0 + 1, r1 - r0 + 1);
    let mut out = Board::new(cols, rows);
    for ((q, r), t) in board.tiles.iter() {
        let (nq, nr) = (q - q0, r - r0);
        if nq < 0 || nr < 0 || nq >= cols || nr >= rows {
            continue;
        }
        if let Some(nt) = out.tiles.get_mut(&(nq, nr)) {
            nt.height = t.height;
            nt.obstacle = t.obstacle.clone();
        }
    }
    let shift = |(q, r): Tile| (q - q0, r - r0);
    let mut frag_marks: HashMap<Tile, usize> = HashMap::new();
    for (tile, t) in board.tiles.iter() {
        if t.bridge.is_some() {
            let nt = shift(*tile);
            if nt.0 >= 0 && nt.1 >= 0 && nt.0 < cols && nt.1 < rows {
                if let Some(nt_ref) = out.tiles.get_mut(&nt) {
                    nt_ref.height = t.height;
                }
                let axis = t
                    .bridge
                    .and_then(|i| board.bridges.get(i).map(|b| b.direction % 3))
                    .unwrap_or(0);
                frag_marks.insert(nt, axis);
            }
        }
    }
    for (tile, (a, b)) in board.ramps.iter() {
        let (nt, na, nb) = (shift(*tile), shift(*a), shift(*b));
        if out.contains(nt) && out.contains(na) && out.contains(nb) {
            out.set_ramp(nt, na, nb);
        }
    }
    mapfile::rebuild_bridges(&mut out, &frag_marks, false);
    let mut out_buildings = Vec::with_capacity(buildings.len());
    for b in buildings.iter() {
        let nt = shift(b.tile);
        if out.contains(nt) {
            let mut nb = b.clone();
            nb.tile = nt;
            out_buildings.push(nb);
        }
    }
    let mut out_bonuses = Vec::with_capacity(bonuses.len());
    for b in bonuses.iter() {
        let nt = shift(b.tile);
        if out.contains(nt) {
            let mut nb = *b;
            nb.tile = nt;
            out_bonuses.push(nb);
        }
    }
    (out, out_buildings, out_bonuses)
}

/// Pad boards smaller than the standard new-map size back up to it,
/// spreading the extra rows/columns evenly at the start/end (the surplus
/// column goes at the end); the view centres on the result.
pub fn pad_map(
    board: Board,
    buildings: Vec<Building>,
    bonuses: Vec<Bonus>,
) -> (Board, Vec<Building>, Vec<Bonus>) {
    let (cols, rows) = (board.cols, board.rows);
    let (tc, tr) = (EDITOR_NEW_COLS, EDITOR_NEW_ROWS);
    if cols >= tc && rows >= tr {
        return (board, buildings, bonuses);
    }
    let mut q0 = ((tc - cols) / 2).max(0);
    q0 -= q0 % 2;
    if q0 + cols > tc {
        q0 -= 2;
    }
    let r0 = ((tr - rows) / 2).max(0);
    let mut out = Board::new(tc.max(cols), tr.max(rows));
    // Board::new() fills height 1, but the padding around a loaded level must
    // be water (height 0) — otherwise the padded area counts as land, trim
    // keeps it, and the map overflows the format limits on save.
    for t in out.tiles.values_mut() {
        t.height = 0;
    }
    for ((q, r), t) in board.tiles.iter() {
        if let Some(nt) = out.tiles.get_mut(&(q + q0, r + r0)) {
            nt.height = t.height;
            nt.obstacle = t.obstacle.clone();
        }
    }
    let mut frag_marks: HashMap<Tile, usize> = HashMap::new();
    for (tile, t) in board.tiles.iter() {
        if t.bridge.is_some() {
            let axis = t
                .bridge
                .and_then(|i| board.bridges.get(i).map(|b| b.direction % 3))
                .unwrap_or(0);
            frag_marks.insert((tile.0 + q0, tile.1 + r0), axis);
        }
    }
    for (tile, (a, b)) in board.ramps.iter() {
        out.set_ramp(
            (tile.0 + q0, tile.1 + r0),
            (a.0 + q0, a.1 + r0),
            (b.0 + q0, b.1 + r0),
        );
    }
    mapfile::rebuild_bridges(&mut out, &frag_marks, false);
    let out_buildings = buildings
        .into_iter()
        .map(|mut b| {
            b.tile = (b.tile.0 + q0, b.tile.1 + r0);
            b
        })
        .collect();
    let out_bonuses = bonuses
        .into_iter()
        .map(|mut b| {
            b.tile = (b.tile.0 + q0, b.tile.1 + r0);
            b
        })
        .collect();
    (out, out_buildings, out_bonuses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::ObstacleKind;

    fn test_state(cols: i32, rows: i32) -> EditorState {
        let mut board = Board::new(cols, rows);
        for t in board.tiles.values_mut() {
            t.height = 1;
        }
        EditorState {
            board,
            buildings: Vec::new(),
            bonuses: Vec::new(),
            map_name: None,
            dirty: false,
            last_kind: BuildingKind::BaseTank,
            last_owner: None,
            last_units: 0,
            last_obstacle: ObstacleKind::Wall,
            last_bonus: BonusKind::Add(EDITOR_DEFAULT_BONUS_ADD),
            last_bonus_value: EDITOR_DEFAULT_BONUS_ADD,
            digit_tile: None,
            digit_buf: String::new(),
            digit_age: 0.0,
            overlay: EditorOverlay::None,
            overlay_items: Vec::new(),
            overlay_cursor: 0,
            input_text: String::new(),
        }
    }

    /// The building standing on `tile`, read back through the editor's own
    /// index rather than through a test-only lookup.
    fn building_at(ed: &EditorState, tile: Tile) -> &Building {
        &ed.buildings[ed.building_index(tile).expect("a building on the tile")]
    }

    /// The legend documents the play/ramp keys: `r` places a ramp and `p`
    /// starts the playtest.
    #[test]
    fn legend_documents_the_ramp_and_play_keys() {
        assert!(
            LEGEND.iter().any(|l| l.contains("r: ramp")),
            "legend must advertise `r` for the ramp"
        );
        assert!(
            LEGEND.iter().any(|l| l.contains("p: play")),
            "legend must advertise `p` for the playtest"
        );
    }

    #[test]
    fn place_and_cycle_building() {
        let mut ed = test_state(8, 8);
        assert!(ed.press_b(Some((2, 2))));
        assert_eq!(ed.buildings.len(), 1);
        assert_eq!(ed.buildings[0].kind, BuildingKind::BaseTank);
        assert!(ed.dirty);
        assert!(ed.press_b(Some((2, 2))));
        assert_eq!(ed.buildings[0].kind, BuildingKind::BaseHelicopter);
        assert_eq!(ed.last_kind, BuildingKind::BaseHelicopter);
        assert!(ed.press_b(Some((3, 3))));
        assert_eq!(building_at(&ed, (3, 3)).kind, BuildingKind::BaseHelicopter);
    }

    #[test]
    fn digits_commit_on_third_or_delay() {
        let mut ed = test_state(8, 8);
        ed.buildings
            .push(Building::new(BuildingKind::BaseTank, Some(0), 2, 2, 0.0));
        assert!(ed.type_digit(Some((2, 2)), '1'));
        assert!(ed.type_digit(Some((2, 2)), '2'));
        assert_eq!(building_at(&ed, (2, 2)).units as i64, 12);
        ed.tick(EDITOR_DIGIT_COMMIT_DELAY + 0.1);
        assert_eq!(building_at(&ed, (2, 2)).units as i64, 12);
        assert_eq!(ed.last_units, 12);
        assert!(ed.type_digit(Some((2, 2)), '5'));
        assert!(ed.press_b(Some((3, 3))));
        assert_eq!(building_at(&ed, (2, 2)).units as i64, 5);
        ed.buildings
            .push(Building::new(BuildingKind::BaseTank, Some(0), 4, 4, 0.0));
        assert!(ed.type_digit(Some((4, 4)), '1'));
        assert!(ed.type_digit(Some((4, 4)), '2'));
        assert!(ed.type_digit(Some((4, 4)), '3'));
        assert_eq!(building_at(&ed, (4, 4)).units as i64, 123);
        assert!(ed.digit_tile.is_none());
    }

    #[test]
    fn cycle_owner_and_obstacle() {
        let mut ed = test_state(8, 8);
        ed.buildings
            .push(Building::new(BuildingKind::BaseTank, None, 2, 2, 0.0));
        assert!(ed.press_o(Some((2, 2))));
        assert_eq!(building_at(&ed, (2, 2)).owner, Some(0));
        assert!(!ed.press_o(Some((7, 7))));
        assert!(ed.press_t(Some((3, 3))));
        assert_eq!(
            ed.board.tiles[&(3, 3)].obstacle.as_ref().unwrap().kind,
            ObstacleKind::Wall
        );
        assert!(ed.press_t(Some((3, 3))));
        assert_eq!(
            ed.board.tiles[&(3, 3)].obstacle.as_ref().unwrap().kind,
            ObstacleKind::Mine
        );
        assert_eq!(ed.last_obstacle, ObstacleKind::Mine);
        assert!(ed.delete_at(Some((3, 3))));
        assert!(ed.board.tiles[&(3, 3)].obstacle.is_none());
    }

    #[test]
    fn bridge_and_ramp_keys() {
        let mut ed = test_state(8, 8);
        assert!(ed.press_m(Some((3, 3))));
        assert!(ed.board.tiles[&(3, 3)].bridge.is_some());
        assert!(ed.press_r(Some((4, 4))));
        assert!(ed.board.tiles[&(4, 4)].ramp.is_some());
        assert!(ed.change_height(Some((5, 5)), 1));
        assert_eq!(ed.board.height((5, 5)), 2);
        assert!(ed.delete_at(Some((4, 4))));
        assert!(ed.board.tiles[&(4, 4)].ramp.is_none());
    }

    #[test]
    fn placing_a_bridge_fragment_never_floods_the_field() {
        // The editor only marks the field; it does not sink it to make the
        // geometry valid -- an over-high fragment is reported instead
        // (placing a fragment never leaves the height alone).
        let mut ed = test_state(10, 10);
        let before = ed.board.height((5, 5));
        assert!(ed.press_m(Some((5, 5))));
        assert_eq!(
            ed.board.height((5, 5)),
            before,
            "placing a fragment changed the terrain height"
        );
        // Same for a field that is water: it stays water, it is not raised.
        let mut ed = test_state(10, 10);
        ed.board.tiles.get_mut(&(6, 6)).unwrap().height = 0;
        assert!(ed.press_m(Some((6, 6))));
        assert_eq!(ed.board.height((6, 6)), 0);
    }

    #[test]
    fn rotating_a_fragment_keeps_the_rest_of_the_bridge() {
        // A bridge spanning three deck fields, seeded as one whole object: the
        // editor edit concerns exactly one field, so rotating the middle
        // fragment must not delete the other two (they used to disappear with
        // the old "remove the whole bridge first" path).
        let mut ed = test_state(12, 12);
        for t in ed.board.tiles.values_mut() {
            t.height = 3;
        }
        for t in [(5, 6), (5, 7)] {
            ed.board.tiles.get_mut(&t).unwrap().height = 0;
        }
        assert!(ed.board.add_bridge((5, 5), (5, 8), 1).is_some());
        let fragments = |ed: &EditorState| {
            let mut v: Vec<Tile> = ed
                .board
                .tiles
                .iter()
                .filter(|(_, t)| t.bridge.is_some())
                .map(|(k, _)| *k)
                .collect();
            v.sort();
            v
        };
        assert_eq!(fragments(&ed), vec![(5, 6), (5, 7)]);
        // Rotating the middle fragment: only this field changes.
        assert!(ed.press_m(Some((5, 7))));
        assert_eq!(
            fragments(&ed),
            vec![(5, 6), (5, 7)],
            "rotating one field deleted the rest of the bridge"
        );
        // Any further rotation keeps the run: an edit is always one field.
        for _ in 0..3 {
            let before = fragments(&ed);
            assert!(ed.press_m(Some((5, 7))));
            assert_eq!(fragments(&ed), before, "the bridge shrank on rotation");
        }
        // Placing a fragment on a second, unrelated bridge keeps both.
        for t in [(2, 2), (2, 3)] {
            assert!(ed.press_m(Some(t)));
        }
        assert_eq!(
            fragments(&ed).len(),
            4,
            "an edit on another field lost a fragment"
        );
        // Deleting a fragment of a run leaves the rest of it in place.
        assert!(ed.delete_at(Some((5, 6))));
        assert!(ed.board.tiles[&(5, 7)].bridge.is_some());
    }

    #[test]
    fn changing_a_field_height_relevels_the_bridges_touching_it() {
        // A bridge from (5,5) to (5,8) at height 3; raising its end must move
        // the deck with it, otherwise the deck floats above the land it is
        // built from.
        let mut ed = test_state(12, 12);
        for t in ed.board.tiles.values_mut() {
            t.height = 3;
        }
        for t in [(5, 6), (5, 7)] {
            ed.board.tiles.get_mut(&t).unwrap().height = 0;
        }
        assert!(ed.board.add_bridge((5, 5), (5, 8), 1).is_some());
        assert_eq!(ed.board.bridges.len(), 1);
        assert_eq!(ed.board.bridges[0].w, 3);
        // Raise the land end the bridge is built on.
        for _ in 0..2 {
            assert!(ed.change_height(Some((5, 5)), 1));
        }
        assert_eq!(ed.board.bridges[0].w, 5, "the deck did not follow the land");
        // Lowering it again lowers the deck.
        assert!(ed.change_height(Some((5, 5)), -1));
        assert_eq!(ed.board.bridges[0].w, 4);
        // A field far away leaves the bridge alone.
        assert!(ed.change_height(Some((1, 1)), 1));
        assert_eq!(ed.board.bridges[0].w, 4);
    }

    #[test]
    fn validation_reports_bases() {
        let mut ed = EditorState::new_board();
        let errors = ed.validate();
        assert!(errors.iter().any(|e| e.contains("player base")));
        assert!(errors.iter().any(|e| e.contains("enemy base")));
        ed.buildings.push(Building::new(
            BuildingKind::BaseTank,
            Some(0),
            128,
            128,
            10.0,
        ));
        ed.buildings.push(Building::new(
            BuildingKind::BaseTank,
            Some(1),
            129,
            128,
            10.0,
        ));
        let errors = ed.validate();
        assert!(!errors.iter().any(|e| e.contains("base")));
        ed.buildings
            .push(Building::new(BuildingKind::BaseTank, Some(0), 0, 0, 1.0));
        let errors = ed.validate();
        assert!(errors.iter().any(|e| e.contains("water")));
    }

    /// A bonus fills its whole field, is placed with `i` and cycles its kind
    /// on every further press; digits set the value of the `+x` / `*x` kinds.
    #[test]
    fn bonus_placement_cycles_and_takes_a_value() {
        let mut ed = test_state(6, 3);
        let land = (2, 1);
        // Land only: water takes no bonus (rules.md section 13).
        ed.board.tiles.get_mut(&(2, 0)).unwrap().height = 0;
        assert!(!ed.press_i(Some((2, 0))));
        assert!(ed.press_i(Some(land)));
        assert_eq!(ed.bonuses[0].tile, land);
        assert_eq!(
            ed.bonuses[0].kind,
            BonusKind::Add(10),
            "the default +x value"
        );
        // A bonus fills the field, so placing a building there drops the
        // bonus, and pressing `i` again drops the building (rules.md 13).
        assert!(ed.press_b(Some(land)));
        assert!(ed.bonuses.is_empty());
        assert!(ed.press_i(Some(land)));
        assert!(ed.buildings.is_empty());
        // Further presses walk +x -> *x -> drone -> +x again.
        assert!(ed.press_i(Some(land)));
        assert_eq!(ed.bonuses[0].kind, BonusKind::Mul(2));
        assert!(ed.press_i(Some(land)));
        assert_eq!(ed.bonuses[0].kind, BonusKind::Drone);
        assert!(ed.press_i(Some(land)));
        assert_eq!(ed.bonuses[0].kind, BonusKind::Add(10));
        // Digits set the value of the current kind, clamped to its range.
        assert!(ed.press_i(Some(land))); // *x with its default value
        assert_eq!(ed.bonuses[0].kind, BonusKind::Mul(2));
        for c in ['4', '2'] {
            assert!(ed.type_digit(Some(land), c));
        }
        assert_eq!(ed.bonuses[0].kind, BonusKind::Mul(42));
        // A third digit commits the entry at once, clamped to the range.
        assert!(ed.type_digit(Some(land), '9'));
        assert_eq!(ed.bonuses[0].kind, BonusKind::Mul(99), "429 clamps to 99");
        // A drone has no value, so digits change nothing.
        assert!(ed.press_i(Some(land)));
        assert_eq!(ed.bonuses[0].kind, BonusKind::Drone);
        assert!(ed.type_digit(Some(land), '7'));
        assert_eq!(ed.bonuses[0].kind, BonusKind::Drone);
        // A newly placed bonus repeats the kind and value last used.
        assert!(ed.press_i(Some((3, 1))));
        assert_eq!(ed.bonuses[1].kind, BonusKind::Drone);
        assert!(ed.press_i(Some((3, 1))));
        assert_eq!(ed.bonuses[1].kind, BonusKind::Add(10));
        for c in ['5', '0'] {
            assert!(ed.type_digit(Some((3, 1)), c));
        }
        ed.commit_digits();
        assert_eq!(ed.bonuses[1].kind, BonusKind::Add(50));
        // Typing a value also switches the remembered kind: the next bonus
        // starts as `+x` with the number just typed.
        assert!(ed.press_i(Some((4, 1))));
        assert_eq!(ed.bonuses[2].kind, BonusKind::Add(50));
        // Deleting the tile takes the bonus with it.
        assert!(ed.delete_at(Some(land)));
        assert!(ed.bonuses.iter().all(|b| b.tile != land));
        for tile in [(3, 1), (4, 1)] {
            assert!(ed.delete_at(Some(tile)));
            assert!(ed.bonuses.iter().all(|b| b.tile != tile));
        }
        assert!(ed.bonuses.is_empty());
    }

    #[test]
    fn validate_reports_a_bonus_no_building_can_reach() {
        // A bonus off every road is dead content, so the editor says so --
        // as a warning that never blocks the save (rules.md section 13).
        let mut ed = test_state(6, 3);
        // A water column cuts the tank base off from the field behind it.
        for r in 0..3 {
            ed.board.tiles.get_mut(&(3, r)).unwrap().height = 0;
        }
        ed.press_b(Some((2, 1)));
        assert!(ed.press_i(Some((5, 1))));
        let errors = ed.validate();
        assert!(
            errors.iter().any(|e| e.contains("unreachable bonus")),
            "{errors:?}"
        );
    }

    #[test]
    fn terrain_fingerprint_ignores_building_only_edits() {
        // Building-only edits (units/owner/kind) keep the fingerprint, so the
        // UI can skip the expensive static mesh rebuild for them.
        let mut ed = EditorState::new_board();
        let base = ed.terrain_fingerprint();
        ed.buildings.push(Building::new(
            BuildingKind::BaseTank,
            Some(0),
            128,
            128,
            10.0,
        ));
        assert_eq!(ed.terrain_fingerprint(), base);
        assert!(ed.press_o(Some((128, 128))));
        assert_eq!(ed.terrain_fingerprint(), base);
        assert!(ed.press_b(Some((128, 128))));
        assert_eq!(ed.terrain_fingerprint(), base);
        // Terrain-affecting edits change it.
        assert!(ed.change_height(Some((128, 128)), 1));
        assert_ne!(ed.terrain_fingerprint(), base);
        let after_height = ed.terrain_fingerprint();
        assert!(ed.press_r(Some((129, 128))));
        assert_ne!(ed.terrain_fingerprint(), after_height);
        // Rotating an existing ramp keeps the height but changes the (a, b)
        // ends, which must also change the fingerprint (regression test: the
        // rotated ramp used to appear only after an unrelated terrain edit).
        let after_place = ed.terrain_fingerprint();
        assert!(ed.press_r(Some((129, 128))));
        assert_ne!(ed.terrain_fingerprint(), after_place);
        let after_ramp = ed.terrain_fingerprint();
        assert!(ed.press_m(Some((130, 128))));
        assert_ne!(ed.terrain_fingerprint(), after_ramp);
    }

    #[test]
    fn pad_map_fills_water_around_loaded_level() {
        // Regression test: Board::new() fills height 1, but the area padded
        // around a loaded level must be water — otherwise trim keeps it and
        // the map overflows the format limits on save.
        let mut board = Board::new(10, 8);
        for t in board.tiles.values_mut() {
            t.height = 0;
        }
        board.tiles.get_mut(&(3, 2)).unwrap().height = 1;
        let buildings = vec![Building::new(BuildingKind::BaseTank, Some(0), 5, 4, 7.0)];
        let (trimmed, tb, _) = trim_map(&board, &buildings, &[]);
        let (padded, pb, _) = pad_map(trimmed, tb, Vec::new());
        assert_eq!(
            (padded.cols, padded.rows),
            (EDITOR_NEW_COLS, EDITOR_NEW_ROWS)
        );
        assert_eq!(pb.len(), 1);
        // Only the trimmed content is land; everything padded is water.
        // (The building stands at height 0 — trim keeps its tile as occupied,
        // but the height stays 0, so only one tile is land.)
        let land = padded.tiles.values().filter(|t| t.height != 0).count();
        assert_eq!(land, 1);
        // ...so saving the padded board trims back down to the level size.
        let (retrimmed, rb, _) = trim_map(&padded, &pb, &[]);
        assert_eq!((retrimmed.cols, retrimmed.rows), (4, 3));
        assert_eq!(rb.len(), 1);
    }

    #[test]
    fn playtest_is_a_sandbox_that_never_touches_the_edited_map() {
        // `p` starts a test run on a copy of the edited board: player 0 is
        // human, other players come from the placed owners, and the match
        // never ends, so an unfinished map (only a player base here) stays
        // playable — without the sandbox flag check_elimination would end it
        // on the first step.
        let mut ed = EditorState::new_board();
        ed.buildings.push(Building::new(
            BuildingKind::BaseTank,
            Some(0),
            128,
            128,
            10.0,
        ));
        ed.map_name = Some("playtest_seed_src".to_string());
        let mut game = ed.playtest_game();
        assert!(game.sandbox);
        assert_eq!(game.human_id, 0);
        assert_eq!(game.players.len(), 1);
        // The AI seed follows the map file name, like a level's does.
        assert_eq!(
            ed.playtest_seed(),
            crate::mapfile::level_seed(&crate::mapfile::save_path("playtest_seed_src"))
        );
        let fp_before = ed.terrain_fingerprint();
        let buildings_before = ed.buildings.len();
        // Long enough for a base to produce its first units (three spawn
        // intervals, rules.md section 3).
        let steps = (3.0 * constants::BASE_SPAWN_INTERVAL / constants::SIM_DT) as usize;
        for _ in 0..steps {
            game.update(constants::SIM_DT);
        }
        assert!(!game.over, "a sandbox playtest must not end the match");
        // The simulation ran on its own copy...
        assert!(game.buildings.iter().any(|b| b.units > 10.0));
        // ...while the edited map stays exactly as it was.
        assert_eq!(ed.terrain_fingerprint(), fp_before);
        assert_eq!(ed.buildings.len(), buildings_before);
        assert!(!ed.playtest_game().over);
    }

    #[test]
    fn save_name_resolves_and_stays_in_the_maps_directory() {
        // Typed overlay text wins, else the current name, else no name;
        // path separators can never escape the maps directory.
        assert_eq!(
            resolve_save_name(EditorOverlay::Save, " typed ", Some("old")).unwrap(),
            "typed"
        );
        assert_eq!(
            resolve_save_name(EditorOverlay::None, "typed", Some("old")).unwrap(),
            "old"
        );
        assert_eq!(
            resolve_save_name(EditorOverlay::Save, "  ", None),
            Err("no map name".to_string())
        );
        assert_eq!(
            resolve_save_name(EditorOverlay::Exit, "", None),
            Err("no map name".to_string())
        );
        assert_eq!(sanitize_map_name("../evil"), ".._evil");
        assert_eq!(sanitize_map_name("  plain "), "plain");
    }

    #[test]
    fn only_save_overlay_consumes_typed_text() {
        // The UI drains macroquad's char queue in every state except the
        // Save overlay, so keys pressed while editing never leak into the
        // file-name field (the `s` that opens Save included).
        assert!(!consumes_text(EditorOverlay::None));
        assert!(!consumes_text(EditorOverlay::Load));
        assert!(!consumes_text(EditorOverlay::Exit));
        assert!(consumes_text(EditorOverlay::Save));
    }

    #[test]
    fn trim_and_pad_roundtrip() {
        let mut board = Board::new(10, 8);
        for t in board.tiles.values_mut() {
            t.height = 0;
        }
        board.tiles.get_mut(&(3, 2)).unwrap().height = 1;
        let buildings = vec![Building::new(BuildingKind::BaseTank, Some(0), 5, 4, 7.0)];
        let (trimmed, tb, _) = trim_map(&board, &buildings, &[]);
        assert_eq!((trimmed.cols, trimmed.rows), (4, 3));
        assert_eq!(tb.len(), 1);
        let (padded, pb, _) = pad_map(trimmed, tb, Vec::new());
        assert_eq!(
            (padded.cols, padded.rows),
            (EDITOR_NEW_COLS, EDITOR_NEW_ROWS)
        );
        assert_eq!(pb.len(), 1);
        let mut empty = Board::new(4, 4);
        for t in empty.tiles.values_mut() {
            t.height = 0;
        }
        let (t, _, _) = trim_map(&empty, &[], &[]);
        assert_eq!((t.cols, t.rows), (1, 1));
    }

    #[test]
    fn save_trims_and_load_pads() {
        let mut ed = EditorState::new_board();
        ed.buildings.push(Building::new(
            BuildingKind::BaseTank,
            Some(0),
            128,
            128,
            10.0,
        ));
        ed.buildings.push(Building::new(
            BuildingKind::BaseTank,
            Some(1),
            129,
            129,
            5.0,
        ));
        ed.map_name = Some("hexfront_editor_test_tmp".to_string());
        // Save into a temporary directory: the real maps/ folder is read by
        // other tests (menu list, full-sim) while the suite runs.
        let tmp = std::env::temp_dir();
        let path = ed
            .save_into(&tmp, "hexfront_editor_test_tmp")
            .expect("save");
        let data = std::fs::read(&path).unwrap();
        assert!(data.len() < 40000, "len={}", data.len());
        let mut ed2 = EditorState::new_board();
        ed2.load_path(&path).expect("load");
        assert_eq!(
            (ed2.board.cols, ed2.board.rows),
            (EDITOR_NEW_COLS, EDITOR_NEW_ROWS)
        );
        assert_eq!(ed2.buildings.len(), 2);
        assert!(!ed2.dirty);
        let _ = std::fs::remove_file(&path);
    }
}
