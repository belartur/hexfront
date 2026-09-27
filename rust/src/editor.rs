//! In-game board editor: headless editing model plus macroquad UI state.
//!
//! The editor copies the behaviour of the Python editor described in
//! specification_of_map_editor.md (keys, validation, trim/pad), but it is a
//! state of the same application rather than a separate program (see
//! specification_rust.md, section "Edytor plansz").
//!
//! The pure editing operations live on [`EditorState`] and deliberately do
//! not depend on macroquad, so the unit tests below run headlessly. The
//! keyboard/mouse handling lives in [`crate::app`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::board::{Board, Obstacle, ObstacleKind};
use crate::constants;
use crate::entities::{Building, BuildingKind, Player, is_base};
use crate::game::Game;
use crate::hexgrid::{self, Tile};
use crate::mapfile;

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
pub const OBSTACLE_ORDER: [ObstacleKind; 5] = [
    ObstacleKind::Wall,
    ObstacleKind::Mine,
    ObstacleKind::MineWater,
    ObstacleKind::TrapFire,
    ObstacleKind::TrapIce,
];

/// Legend lines shown on the editor screen.
pub const LEGEND: [&str; 7] = [
    "b: building (again: cycle kind)    digits: units 0-999",
    "o: cycle owner                     t: obstacle (again: cycle kind)",
    "m: bridge (again: rotate)          r: ramp (again: rotate)",
    "[/]: lower/raise terrain           Del/RMB: delete object",
    "l: load   s: save   ctrl+s: quick save   ctrl+n: new map",
    "p: play the level (Esc there: back to editing)",
    "Esc: menu (asks to save if dirty)   view: drag/wheel/arrows",
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
/// (the Python editor solves the same problem by swallowing the `s`
/// TEXTINPUT that opens its save overlay).
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
    /// [`constants::EDITOR_LAND_HEIGHT`] in the middle.
    pub fn new_board() -> Self {
        let cols = constants::EDITOR_NEW_COLS;
        let rows = constants::EDITOR_NEW_ROWS;
        let mut board = Board::new(cols, rows);
        for t in board.tiles.values_mut() {
            t.height = 0;
        }
        let lw = constants::EDITOR_LAND_COLS;
        let lh = constants::EDITOR_LAND_ROWS;
        let q0 = (cols - lw) / 2;
        let r0 = (rows - lh) / 2;
        for q in q0..q0 + lw {
            for r in r0..r0 + lh {
                if let Some(t) = board.tiles.get_mut(&(q, r)) {
                    t.height = constants::EDITOR_LAND_HEIGHT;
                }
            }
        }
        Self {
            board,
            buildings: Vec::new(),
            map_name: None,
            dirty: false,
            last_kind: BuildingKind::BaseTank,
            last_owner: None,
            last_units: 0,
            last_obstacle: ObstacleKind::Wall,
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

    /// Commit the pending digit entry to its building, if still valid.
    pub fn commit_digits(&mut self) {
        if self.digit_buf.is_empty() {
            self.digit_tile = None;
            return;
        }
        if let Some(tile) = self.digit_tile
            && let Ok(units) = self.digit_buf.parse::<u32>()
        {
            let units = units.min(constants::EDITOR_MAX_UNITS);
            if let Some(b) = self.building_at_mut(tile) {
                b.units = units as f64;
                self.last_units = units;
                self.dirty = true;
            }
        }
        self.digit_tile = None;
        self.digit_buf.clear();
        self.digit_age = 0.0;
    }

    /// Advance the digit-entry commit timer (call every frame).
    pub fn tick(&mut self, dt: f64) {
        if !self.digit_buf.is_empty() {
            self.digit_age += dt;
            if self.digit_age >= constants::EDITOR_DIGIT_COMMIT_DELAY {
                self.commit_digits();
            }
        }
    }

    #[allow(dead_code)]
    /// Building standing on `tile`, if any.
    pub fn building_at(&self, tile: Tile) -> Option<&Building> {
        self.buildings.iter().find(|b| b.tile == tile)
    }

    /// Mutable building standing on `tile`, if any.
    fn building_at_mut(&mut self, tile: Tile) -> Option<&mut Building> {
        self.buildings.iter_mut().find(|b| b.tile == tile)
    }

    /// Index of the building standing on `tile`, if any.
    fn building_index(&self, tile: Tile) -> Option<usize> {
        self.buildings.iter().position(|b| b.tile == tile)
    }

    /// Remove every object (building, obstacle, ramp, bridge fragment) from
    /// `tile`, returning true when anything was removed.
    fn clear_tile(&mut self, tile: Tile) -> bool {
        let mut removed = false;
        if let Some(i) = self.building_index(tile) {
            self.buildings.remove(i);
            removed = true;
        }
        if self.board.tiles.get(&tile).and_then(|t| t.ramp).is_some() {
            if let Some(t) = self.board.tiles.get_mut(&tile) {
                t.ramp = None;
            }
            self.board.ramps.remove(&tile);
            removed = true;
        }
        if self.board.tiles.get(&tile).and_then(|t| t.bridge).is_some() {
            self.remove_bridge(tile);
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

    /// Remove the whole bridge a deck fragment on `tile` belongs to.
    fn remove_bridge(&mut self, tile: Tile) {
        let idx = match self.board.tiles.get(&tile).and_then(|t| t.bridge) {
            Some(i) => i,
            None => return,
        };
        let fragments: Vec<Tile> = self
            .board
            .bridges
            .get(idx)
            .map(|b| b.fragments.clone())
            .unwrap_or_default();
        for f in fragments {
            if let Some(t) = self.board.tiles.get_mut(&f) {
                t.bridge = None;
            }
        }
        self.board.bridges.remove(idx);
        for (i, b) in self.board.bridges.iter().enumerate() {
            for f in b.fragments.iter() {
                if let Some(t) = self.board.tiles.get_mut(f) {
                    t.bridge = Some(i);
                }
            }
        }
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

    /// Place a remembered bridge fragment when the axis is known.
    fn put_bridge_fragment(&mut self, tile: Tile, axis: usize) -> bool {
        if !self.board.contains(tile) {
            return false;
        }
        let mut marks = self.frag_marks();
        marks.insert(tile, axis % 3);
        // Drop the old whole-bridge objects; rebuild_single_bridge keeps the
        // neighbouring same-axis fragments (editor preview keeps invalid runs).
        self.board.bridges.clear();
        for t in self.board.tiles.values_mut() {
            t.bridge = None;
        }
        // Lower the new deck tile first so it stays below the ends.
        let mut w = 15;
        for d in 0..6 {
            let n = hexgrid::neighbor(tile.0, tile.1, d);
            if self.board.contains(n) {
                w = w.min(self.board.height(n));
            }
        }
        if let Some(t) = self.board.tiles.get_mut(&tile) {
            t.height = t.height.min(w.saturating_sub(3).max(0));
        }
        self.board.rebuild_single_bridge(tile, axis, &marks);
        // Restore the marks of the other surviving runs (rebuild_single_bridge
        // only rebuilds the run through `tile`).
        let mut rest = marks.clone();
        for f in self.board.bridges.iter().flat_map(|b| b.fragments.clone()) {
            rest.remove(&f);
        }
        if !rest.is_empty() {
            crate::mapfile::rebuild_bridges_keep(&mut self.board, &rest);
        }
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

    /// Type one digit of the unit count (0-999) of the building on `tile`.
    pub fn type_digit(&mut self, tile: Option<Tile>, digit: char) -> bool {
        let Some(tile) = tile else { return false };
        if !digit.is_ascii_digit() {
            return false;
        }
        if self.building_index(tile).is_none() {
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
            let units = units.min(constants::EDITOR_MAX_UNITS);
            if let Some(b) = self.building_at_mut(tile) {
                b.units = units as f64;
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
    pub fn press_m(&mut self, tile: Option<Tile>) -> bool {
        let Some(tile) = tile else { return false };
        if !self.board.contains(tile) {
            return false;
        }
        self.accept_digits(Some(tile));
        let existing = self.board.tiles.get(&tile).and_then(|t| t.bridge);
        if let Some(i) = existing {
            let axis = self
                .board
                .bridges
                .get(i)
                .map(|b| b.direction % 3)
                .unwrap_or(0);
            self.remove_bridge(tile);
            let ok = self.put_bridge_fragment(tile, (axis + 1) % 3);
            self.dirty = self.dirty || ok;
            ok
        } else {
            self.clear_tile(tile);
            let axis = Self::bridge_axis(tile, &self.board);
            let ok = self.put_bridge_fragment(tile, axis);
            self.dirty = self.dirty || ok;
            ok
        }
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

    /// Place a ramp of `axis` on `tile`, like the Python editor does.
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
        let (board, buildings) = mapfile::load_board(path)?;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("map")
            .to_string();
        let (board, buildings) = pad_map(board, buildings);
        self.board = board;
        self.buildings = buildings;
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
        let (board, buildings) = trim_map(&self.board, &self.buildings);
        let path = dir.join(format!("{}{}", name, constants::MAP_EXTENSION));
        mapfile::save_map(&path, &board, &buildings).map_err(|e| e.to_string())?;
        self.map_name = Some(name.to_string());
        self.dirty = false;
        self.overlay = EditorOverlay::None;
        Ok(path)
    }

    /// Rule violations shown in red on the editor screen: building on water,
    /// bridge over too high land, bridge joining different heights, ramp on
    /// the wrong height, ramp joining equal heights, missing player base,
    /// missing enemy base. A map with errors can still be saved and loaded.
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
        errors
    }
}

/// Axis 0-2 of a ramp whose neighbour `a` end is `ramp_a`.
#[allow(dead_code)]
pub fn ramp_axis(tile: Tile, ramp_a: Tile) -> usize {
    for d in 0..6 {
        if hexgrid::neighbor(tile.0, tile.1, d) == ramp_a {
            return d % 3;
        }
    }
    0 // unreachable for ramps
}

/// True when `tile` holds anything a trimmed map must keep: non-water, an
/// object, or a tile kept so no ramp/bridge endpoint falls off the board.
fn tile_occupied(board: &Board, tile: Tile, buildings: &[Building]) -> bool {
    let Some(t) = board.tiles.get(&tile) else {
        return false;
    };
    if t.height != 0 || t.obstacle.is_some() {
        return true;
    }
    if t.ramp.is_some() || t.bridge.is_some() {
        return true;
    }
    buildings.iter().any(|b| b.tile == tile)
}

/// Remove empty border rows and columns from the board.
///
/// Empty means pure water (height 0) without any object. Every kept ramp
/// endpoint and bridge end is kept on the board as well, so no object loses
/// its neighbours. Returns a new `(board, buildings)` pair with shifted
/// coordinates; the smallest possible result is a 1 x 1 board. The column
/// shift is always *even* (the odd-q grid is translation-invariant only
/// then), so one extra water column may be kept on the left when the first
/// occupied column is odd.
pub fn trim_map(board: &Board, buildings: &[Building]) -> (Board, Vec<Building>) {
    use std::collections::HashSet;
    let mut occupied_cols: HashSet<i32> = HashSet::new();
    let mut occupied_rows: HashSet<i32> = HashSet::new();
    for tile in board.tiles.keys() {
        if tile_occupied(board, *tile, buildings) {
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
    (out, out_buildings)
}

/// Pad boards smaller than the standard new-map size back up to it,
/// spreading the extra rows/columns evenly at the start/end (the surplus
/// column goes at the end); the view centres on the result.
pub fn pad_map(board: Board, buildings: Vec<Building>) -> (Board, Vec<Building>) {
    let (cols, rows) = (board.cols, board.rows);
    let (tc, tr) = (constants::EDITOR_NEW_COLS, constants::EDITOR_NEW_ROWS);
    if cols >= tc && rows >= tr {
        return (board, buildings);
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
    (out, out_buildings)
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
            map_name: None,
            dirty: false,
            last_kind: BuildingKind::BaseTank,
            last_owner: None,
            last_units: 0,
            last_obstacle: ObstacleKind::Wall,
            digit_tile: None,
            digit_buf: String::new(),
            digit_age: 0.0,
            overlay: EditorOverlay::None,
            overlay_items: Vec::new(),
            overlay_cursor: 0,
            input_text: String::new(),
        }
    }

    /// The legend documents the play/ramp keys: `r` places a ramp (the key
    /// the Python editor uses too) and `p` starts the playtest.
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
        assert_eq!(
            ed.building_at((3, 3)).unwrap().kind,
            BuildingKind::BaseHelicopter
        );
    }

    #[test]
    fn digits_commit_on_third_or_delay() {
        let mut ed = test_state(8, 8);
        ed.buildings
            .push(Building::new(BuildingKind::BaseTank, Some(0), 2, 2, 0.0));
        assert!(ed.type_digit(Some((2, 2)), '1'));
        assert!(ed.type_digit(Some((2, 2)), '2'));
        assert_eq!(ed.building_at((2, 2)).unwrap().units as i64, 12);
        ed.tick(constants::EDITOR_DIGIT_COMMIT_DELAY + 0.1);
        assert_eq!(ed.building_at((2, 2)).unwrap().units as i64, 12);
        assert_eq!(ed.last_units, 12);
        assert!(ed.type_digit(Some((2, 2)), '5'));
        assert!(ed.press_b(Some((3, 3))));
        assert_eq!(ed.building_at((2, 2)).unwrap().units as i64, 5);
        ed.buildings
            .push(Building::new(BuildingKind::BaseTank, Some(0), 4, 4, 0.0));
        assert!(ed.type_digit(Some((4, 4)), '1'));
        assert!(ed.type_digit(Some((4, 4)), '2'));
        assert!(ed.type_digit(Some((4, 4)), '3'));
        assert_eq!(ed.building_at((4, 4)).unwrap().units as i64, 123);
        assert!(ed.digit_tile.is_none());
    }

    #[test]
    fn cycle_owner_and_obstacle() {
        let mut ed = test_state(8, 8);
        ed.buildings
            .push(Building::new(BuildingKind::BaseTank, None, 2, 2, 0.0));
        assert!(ed.press_o(Some((2, 2))));
        assert_eq!(ed.building_at((2, 2)).unwrap().owner, Some(0));
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
        let (trimmed, tb) = trim_map(&board, &buildings);
        let (padded, pb) = pad_map(trimmed, tb);
        assert_eq!(
            (padded.cols, padded.rows),
            (constants::EDITOR_NEW_COLS, constants::EDITOR_NEW_ROWS)
        );
        assert_eq!(pb.len(), 1);
        // Only the trimmed content is land; everything padded is water.
        // (The building stands at height 0 — trim keeps its tile as occupied,
        // but the height stays 0, so only one tile is land.)
        let land = padded.tiles.values().filter(|t| t.height != 0).count();
        assert_eq!(land, 1);
        // ...so saving the padded board trims back down to the level size.
        let (retrimmed, rb) = trim_map(&padded, &pb);
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
        let (trimmed, tb) = trim_map(&board, &buildings);
        assert_eq!((trimmed.cols, trimmed.rows), (4, 3));
        assert_eq!(tb.len(), 1);
        let (padded, pb) = pad_map(trimmed, tb);
        assert_eq!(
            (padded.cols, padded.rows),
            (constants::EDITOR_NEW_COLS, constants::EDITOR_NEW_ROWS)
        );
        assert_eq!(pb.len(), 1);
        let mut empty = Board::new(4, 4);
        for t in empty.tiles.values_mut() {
            t.height = 0;
        }
        let (t, _) = trim_map(&empty, &[]);
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
            (constants::EDITOR_NEW_COLS, constants::EDITOR_NEW_ROWS)
        );
        assert_eq!(ed2.buildings.len(), 2);
        assert!(!ed2.dirty);
        let _ = std::fs::remove_file(&path);
    }
}
