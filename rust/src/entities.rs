//! Game entities: players, buildings and vehicles.
//!
//! Pure data plus small helpers; all rule logic lives in [`crate::game`].

use crate::constants::{self, VehicleKind};
use crate::hexgrid::{self, Tile};

/// All building kinds (rules.md section 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuildingKind {
    /// Tank-producing base.
    BaseTank,
    /// Helicopter-producing base.
    BaseHelicopter,
    /// Hovercraft-producing base.
    BaseHovercraft,
    /// Buffer-producing base.
    BaseBuffer,
    /// Ordinary turret building.
    TurretNormal,
    /// Rapid-fire turret building.
    TurretRapid,
    /// Rocket turret building.
    TurretRocket,
    /// Healing tower.
    HealTower,
}

/// True for the four base kinds.
pub fn is_base(kind: BuildingKind) -> bool {
    matches!(
        kind,
        BuildingKind::BaseTank
            | BuildingKind::BaseHelicopter
            | BuildingKind::BaseHovercraft
            | BuildingKind::BaseBuffer
    )
}

/// True for the three turret kinds.
pub fn is_turret(kind: BuildingKind) -> bool {
    matches!(
        kind,
        BuildingKind::TurretNormal | BuildingKind::TurretRapid | BuildingKind::TurretRocket
    )
}

/// Vehicle kind spawned by a building: base type or a tank (section 4).
pub fn vehicle_kind_of(kind: BuildingKind) -> VehicleKind {
    match kind {
        BuildingKind::BaseTank => VehicleKind::Tank,
        BuildingKind::BaseHelicopter => VehicleKind::Helicopter,
        BuildingKind::BaseHovercraft => VehicleKind::Hovercraft,
        BuildingKind::BaseBuffer => VehicleKind::Buffer,
        _ => VehicleKind::Tank,
    }
}

/// Turret kind of a turret building, else `None`.
pub fn turret_kind_of(kind: BuildingKind) -> Option<crate::constants::TurretKind> {
    match kind {
        BuildingKind::TurretNormal => Some(crate::constants::TurretKind::Normal),
        BuildingKind::TurretRapid => Some(crate::constants::TurretKind::Rapid),
        BuildingKind::TurretRocket => Some(crate::constants::TurretKind::Rocket),
        _ => None,
    }
}

/// Building capacity: bases 100, everything else 50 (section 3).
pub fn capacity_of(kind: BuildingKind) -> f64 {
    if is_base(kind) {
        constants::BASE_CAPACITY
    } else {
        constants::BUILDING_CAPACITY
    }
}

/// One of the 2-4 players (exactly one is human, rules.md section 1).
#[derive(Clone, Debug)]
pub struct Player {
    /// Player id (0 is the human when present).
    pub id: usize,
    /// True for the human-controlled player.
    pub is_human: bool,
    /// True once the player lost everything (rules.md section 2).
    pub eliminated: bool,
}

impl Player {
    /// Create a player with `id`; `is_human` marks the human player.
    pub fn new(id: usize, is_human: bool) -> Self {
        Self {
            id,
            is_human,
            eliminated: false,
        }
    }
    #[allow(dead_code)]
    /// Player colour used by the renderer.
    pub fn color(&self) -> [u8; 3] {
        constants::player_color(self.id)
    }
}

/// A floating combat number: `(amount, age)`.
#[derive(Clone, Copy, Debug)]
pub struct FloatText {
    /// Displayed amount (negative = loss, positive = gain).
    pub amount: f64,
    /// Seconds since the text appeared.
    pub age: f64,
}

/// A structure standing on one tile: base, turret or healing tower.
#[derive(Clone, Debug)]
pub struct Building {
    /// Building kind (rules.md section 3).
    pub kind: BuildingKind,
    /// Owning player id, or `None` when neutral.
    pub owner: Option<usize>,
    /// Tile the building stands on.
    pub tile: Tile,
    /// Units currently inside (may exceed capacity when overcrowded).
    pub units: f64,
    /// Maximum comfortable garrison (section 3).
    pub capacity: f64,
    /// Base spawn cycle progress.
    pub production_timer: f64,
    /// Turret cooldown progress.
    pub fire_timer: f64,
    /// Healing-tower pulse progress.
    pub heal_timer: f64,
    /// World position of the last turret target (for the barrel).
    pub last_target_pos: Option<(f64, f64)>,
    /// Accumulated fractional losses/gains (floating numbers).
    pub loss_acc: f64,
    /// Accumulated fractional gains (floating numbers).
    pub gain_acc: f64,
    /// Timer for committing fractional floating text.
    pub text_timer: f64,
    /// Visible floating texts.
    pub texts: Vec<FloatText>,
}

impl Building {
    /// Create a building of `kind` owned by `owner` on tile `(q, r)`.
    pub fn new(kind: BuildingKind, owner: Option<usize>, q: i32, r: i32, units: f64) -> Self {
        Self {
            kind,
            owner,
            tile: (q, r),
            units,
            capacity: capacity_of(kind),
            production_timer: 0.0,
            fire_timer: 0.0,
            heal_timer: 0.0,
            last_target_pos: None,
            loss_acc: 0.0,
            gain_acc: 0.0,
            text_timer: 0.0,
            texts: Vec::new(),
        }
    }
    /// World position of the tile centre.
    pub fn pos(&self, side: f64) -> (f64, f64) {
        hexgrid::hex_to_world(self.tile.0, self.tile.1, side)
    }
}

/// Next vehicle id (monotonic within one process).
static NEXT_VEHICLE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A moving group of units travelling along a fixed route (section 4).
#[derive(Clone, Debug)]
pub struct Vehicle {
    /// Unique id (debugging / projectile targeting).
    pub id: u64,
    /// Vehicle kind (rules.md section 4).
    pub kind: VehicleKind,
    /// Owning player id.
    pub owner: usize,
    /// Units carried.
    pub units: f64,
    /// Tiles after the source, including the target.
    pub route: Vec<Tile>,
    /// Index of the next waypoint into `route`.
    pub route_index: usize,
    /// Continuous world position (sprite centre).
    pub x: f64,
    /// Continuous world position (sprite centre).
    pub y: f64,
    /// Source tile the route starts from.
    pub src_tile: Option<Tile>,
    /// Combat opponent id (section 9).
    pub combat_target: Option<u64>,
    /// Id of the last vehicle shot at.
    pub last_opponent: Option<u64>,
    /// Combat shot cooldown progress.
    pub fire_timer: f64,
    /// Wall-attack shot cooldown progress.
    pub wall_timer: f64,
    /// Tile of the wall being shot.
    pub wall_target: Option<Tile>,
    /// Shots fired at that wall so far.
    pub wall_shots: u32,
    /// Accumulated fractional losses/gains.
    pub loss_acc: f64,
    /// Accumulated fractional gains.
    pub gain_acc: f64,
    /// Timer for committing fractional floating text.
    pub text_timer: f64,
    /// Visible floating texts.
    pub texts: Vec<FloatText>,
    /// Set when destroyed or arrived.
    pub dead: bool,
}

impl Vehicle {
    /// Create a vehicle starting at `start_pos` and following `route`.
    pub fn new(
        kind: VehicleKind,
        owner: usize,
        units: f64,
        route: Vec<Tile>,
        start_pos: (f64, f64),
        src_tile: Option<Tile>,
    ) -> Self {
        let id = NEXT_VEHICLE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            id,
            kind,
            owner,
            units,
            route,
            route_index: 0,
            x: start_pos.0,
            y: start_pos.1,
            src_tile,
            combat_target: None,
            last_opponent: None,
            fire_timer: 0.0,
            wall_timer: 0.0,
            wall_target: None,
            wall_shots: 0,
            loss_acc: 0.0,
            gain_acc: 0.0,
            text_timer: 0.0,
            texts: Vec::new(),
            dead: false,
        }
    }
    /// World position (centre of the sprite).
    pub fn pos(&self) -> (f64, f64) {
        (self.x, self.y)
    }
    #[allow(dead_code)]
    /// Final tile of the route (or `None`).
    pub fn dest_tile(&self) -> Option<Tile> {
        self.route.last().copied()
    }
}
