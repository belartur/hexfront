//! Game constants of *Hexfront*.
//!
//! Every gameplay value (distances, ranges, speeds, radii, amounts) is taken
//! from `rules.md` and expressed in distance units **j**. The conversion
//! factor from *j* to pixels is defined exactly once, in [`UNIT_J_TO_PX`]:
//! zoom and window scaling affect *rendering only*, never the simulation.
//!
//! Each constant carries a documentation comment pointing at the rule it
//! implements, so values can be tweaked easily when experimenting.

use std::path::PathBuf;

/// Root of the repository: the rules (`rules.md`), the specifications and
/// the `maps` directory stay there, while this Rust implementation lives in
/// `rust/` (one directory per language implementation).
pub fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(parent) = manifest.parent() {
        let candidate = parent.to_path_buf();
        if candidate.join("maps").is_dir() || candidate.join("rules.md").is_file() {
            return candidate;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            if d.join("maps").is_dir() || d.join("rules.md").is_file() {
                return d;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    manifest.parent().unwrap_or(&manifest).to_path_buf()
}

/// Directory holding one binary map file per level (specification.md,
/// section "Plansze"). It lives in the repository root (see [`repo_root`]),
/// so the game and the tests find the maps no matter which working
/// directory they are started from.
pub fn maps_dir() -> PathBuf {
    repo_root().join("maps")
}

#[allow(dead_code)]
/// File name extension of the binary map files.
pub const MAP_EXTENSION: &str = ".map";

/// 1 j == 1 px at 1:1 view scale (specification.md, section "Parametry").
pub const UNIT_J_TO_PX: f64 = 1.0;
/// Side length of a flat-top hexagon in j (specification.md, "Parametry").
pub const HEX_SIDE: f64 = 36.0 * UNIT_J_TO_PX;
/// Pixels of screen elevation per one unit of tile height (rendering only;
/// heights themselves are integers 0..15 per rules.md section 1).
pub const ELEVATION_PX: f64 = 9.0 * UNIT_J_TO_PX;
/// cos(30 deg), horizontal factor of the isometric projection.
pub const ISO_COS: f64 = 0.8660254037844387;
/// Vertical squash factor of the isometric projection (2:1 isometric).
pub const ISO_SIN: f64 = 0.5;
/// Radius of an ordinary turret projectile in screen px.
pub const PROJECTILE_RADIUS: f32 = 3.0;
/// Radius of a rocket projectile in screen px.
pub const ROCKET_RADIUS: f32 = 5.0;
#[allow(dead_code)]
/// Outline width of projectiles in screen px.
pub const PROJECTILE_OUTLINE_WIDTH: f32 = 1.0;
/// Visual deck thickness; the bridge deck and shadows share one surface.
pub const BRIDGE_DECK_LIFT: f64 = 5.0 * UNIT_J_TO_PX;
/// Lift of flat ground markers (building bases, mine/trap discs) above the
/// tile top in px: exactly coplanar discs lose the depth race against the
/// terrain, so they look faint or vanish (rendering only; objects themselves
/// come from rules.md sections 1-2).
pub const OBSTACLE_LIFT: f64 = 0.5 * UNIT_J_TO_PX;
/// Fixed flight altitude of a helicopter above the *highest* terrain of the
/// board in px (rendering only; rules.md section 5.2 makes helicopters
/// ignore tile heights, so they do not bob up and down over hills). The
/// clearance is larger than the whole rotor stack (15 px in `mesh.rs`), so
/// even the blades stay above the tallest peak and a helicopter never
/// disappears behind a hill. Its shadow disc (see [`SHADOW_RADIUS`]) marks
/// the tile it flies over.
pub const HELICOPTER_ALTITUDE_PX: f64 = 2.0 * ELEVATION_PX;
/// Radius of a vehicle shadow decal in j (specification.md, section
/// "Grafika i interfejs użytkownika"). The Rust implementation draws the
/// helicopter silhouette instead of this disc (specification_rust.md,
/// "Renderowanie"), so this is kept only as the documented contract value.
#[allow(dead_code)]
pub const SHADOW_RADIUS: f64 = 14.0 * UNIT_J_TO_PX;
/// Outline segments of a shadow decal (specification.md). The Rust
/// helicopter silhouette needs no disc tessellation, so this is kept only
/// as the documented contract value (specification_rust.md, "Renderowanie").
#[allow(dead_code)]
pub const SHADOW_SEGMENTS: usize = 14;
/// Colour of the translucent shadow decal (specification.md).
pub const SHADOW_COLOR: [u8; 3] = [0, 0, 0];
/// Alpha of the translucent shadow decal, 70/255 (specification.md).
pub const SHADOW_ALPHA: u8 = 70;
/// Lift of the helicopter shadow silhouette above the receiving surface in
/// px (rendering only; rules.md has no shadows). Just high enough that the
/// decal never loses the depth race against the terrain (no flicker), low
/// enough that it still reads as lying on the ground.
pub const SHADOW_LIFT: f64 = 1.0 * UNIT_J_TO_PX;
/// Fixed simulation frame rate (specification_rust.md, "Determinism").
pub const FPS: f64 = 60.0;
/// Length of one simulation step in seconds.
pub const SIM_DT: f64 = 1.0 / FPS;
/// Angular speed of the helicopter rotor animation in rad/s (rendering
/// only; rules.md has no rotor state). The same phase drives the airframe
/// blades and their shadow, so both stay in lockstep, and the tail rotor
/// uses [`crate::mesh`]'s faster multiplier.
pub const ROTOR_SPIN_RAD_PER_S: f64 = 12.0;

// ---------------------------------------------------------------------------
// Vehicles (rules.md sections 4-5)
// ---------------------------------------------------------------------------

/// Kinds of vehicles that can travel over the map (rules.md section 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VehicleKind {
    /// Slow ground crawler (rules.md section 5.1).
    Tank,
    /// Fast flying scout (rules.md section 5.2).
    Helicopter,
    /// Amphibious slow vehicle (rules.md section 5.3).
    Hovercraft,
    /// Healing support vehicle (rules.md section 5.4).
    Buffer,
}

/// Cruise speed of each vehicle kind in j/s (rules.md section 5).
pub fn vehicle_speed(kind: VehicleKind) -> f64 {
    match kind {
        VehicleKind::Tank => 60.0,
        VehicleKind::Helicopter => 90.0,
        VehicleKind::Hovercraft => 48.0,
        VehicleKind::Buffer => 60.0,
    }
}

/// Radius of the healing aura of a buffer in j (rules.md section 5.4).
pub const BUFFER_HEAL_RADIUS: f64 = 160.0;
/// Units restored per second by a buffer (1 unit / 2 s, section 5.4).
pub const BUFFER_HEAL_RATE: f64 = 0.5;

// Obstacles (rules.md sections 1, 4)
/// Damage dealt by a mine that explodes under a vehicle (section 4).
pub const MINE_DAMAGE: f64 = 25.0;
/// Distance in j from a mined tile centre at which the mine explodes.
pub const MINE_TRIGGER_RADIUS: f64 = 8.0;
/// Damage per second while a vehicle sits on a fire trap (section 4).
pub const FIRE_TRAP_DPS: f64 = 1.0;
/// Speed multiplier while a ground vehicle is on an ice trap (section 4).
pub const ICE_TRAP_SLOWDOWN: f64 = 0.5;
/// Number of hits a wall can take before it collapses (section 4).
pub const WALL_HP: i32 = 20;
/// Interval between consecutive shots of a vehicle at a wall (section 4).
pub const WALL_ATTACK_INTERVAL: f64 = 1.0;
/// Damage of one shot at a wall, always exactly 1 (section 4).
pub const WALL_ATTACK_DAMAGE: i32 = 1;

// Combat (rules.md section 9)
/// Detection radius in j: vehicles stop and fight within this circle.
pub const DETECTION_RADIUS: f64 = 80.0;
/// Interval between consecutive shots in vehicle-vs-vehicle combat.
pub const FIRE_INTERVAL: f64 = 1.0;

// Turrets (rules.md section 10)
/// Kinds of gun emplacements (rules.md section 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TurretKind {
    /// Long-range, slow, full-damage gun (section 10.1).
    Normal,
    /// Short-range rapid gun, quarter damage (section 10.3).
    Rapid,
    /// Long-range rocket launcher with splash (section 10.2).
    Rocket,
}
/// Range of a turret in j (rules.md section 10).
pub fn turret_range(kind: TurretKind) -> f64 {
    match kind {
        TurretKind::Normal => 250.0,
        TurretKind::Rapid => 190.0,
        TurretKind::Rocket => 320.0,
    }
}
/// Cooldown between turret shots in seconds (rules.md section 10).
pub fn turret_cooldown(kind: TurretKind) -> f64 {
    match kind {
        TurretKind::Normal => 5.0,
        TurretKind::Rapid => 1.0,
        TurretKind::Rocket => 5.0,
    }
}
/// Damage divisor: damage is ceil(units / divisor) (rules.md section 10).
pub fn turret_damage_div(kind: TurretKind) -> f64 {
    match kind {
        TurretKind::Normal => 1.0,
        TurretKind::Rapid => 4.0,
        TurretKind::Rocket => 1.0,
    }
}
/// Splash radius of a rocket shot in j (rules.md section 10.2).
pub fn turret_splash(kind: TurretKind) -> f64 {
    match kind {
        TurretKind::Normal => 0.0,
        TurretKind::Rapid => 0.0,
        TurretKind::Rocket => 80.0,
    }
}
/// Flight time of one turret projectile in seconds (rules.md section 10).
pub fn turret_flight_time(kind: TurretKind) -> f64 {
    match kind {
        TurretKind::Normal => 0.4,
        TurretKind::Rapid => 0.2,
        TurretKind::Rocket => 0.8,
    }
}
// Healing tower (rules.md section 11)
/// Range of a healing tower equals this factor times its unit count.
pub const HEAL_TOWER_RANGE_PER_UNIT: f64 = 15.0;
/// Interval between healing pulses of a healing tower, in seconds.
pub const HEAL_TOWER_INTERVAL: f64 = 3.0;
/// Units granted per pulse to every friendly vehicle in range.
pub const HEAL_TOWER_AMOUNT: f64 = 2.0;
// Buildings (rules.md sections 2-3)
/// Capacity of every building except bases (section 3).
pub const BUILDING_CAPACITY: f64 = 50.0;
/// Capacity of bases (section 3).
pub const BASE_CAPACITY: f64 = 100.0;
/// Units dying per second in an overcrowded building (section 3).
pub const OVERCROWD_DEATH_RATE: f64 = 1.0;
/// Units arriving in a base at the end of each production cycle (sec. 3).
pub const BASE_SPAWN_AMOUNT: f64 = 5.0;
/// Length of one base production cycle in seconds (section 3).
pub const BASE_SPAWN_INTERVAL: f64 = 10.0;

// Camera / input (specification.md, section "Sterowanie")
/// Minimum zoom factor.
pub const ZOOM_MIN: f64 = 0.5;
/// Maximum zoom factor.
pub const ZOOM_MAX: f64 = 2.0;
/// Multiplicative factor per wheel notch.
pub const ZOOM_STEP: f64 = 1.1;
/// Screen px per second panned via keys/edges.
pub const PAN_SPEED: f64 = 700.0;
/// Px of screen edge that pans the view.
pub const EDGE_PAN_MARGIN: f32 = 12.0;
/// Px of movement before a mouse drag starts.
pub const DRAG_THRESHOLD: f32 = 5.0;
/// Seconds the loading screen lasts.
pub const LOADING_TIME: f64 = 1.0;
/// Cursor snap radius in j: hover and clicks snap to the nearest building
/// tile within this distance of its centre (UI choice, no rules.md section).
pub const HOVER_SNAP_RADIUS: f64 = 150.0;
// Floating combat text (specification.md, graphics)
/// Seconds a -x / +x number stays visible.
pub const FLOAT_TEXT_LIFETIME: f64 = 2.0;
/// Px/s of upward drift.
pub const FLOAT_TEXT_SPEED: f32 = 26.0;
// Colours (specification.md: land grey, water light blue; players differ)
/// Water fill colour.
pub const WATER_COLOR: [u8; 3] = [110, 170, 225];
/// Water edge colour.
pub const WATER_EDGE: [u8; 3] = [90, 150, 205];
/// Land fill colour.
pub const LAND_COLOR: [u8; 3] = [152, 152, 152];
/// Land fill variant (checkerboard).
pub const LAND_VARIANT: [u8; 3] = [140, 140, 140];
/// Land edge colour.
pub const LAND_EDGE: [u8; 3] = [110, 110, 110];
/// One distinct colour per player, indexed by player id (max 4 players).
pub const PLAYER_COLORS: [[u8; 3]; 4] = [
    [70, 135, 250],
    [230, 85, 70],
    [245, 195, 55],
    [115, 200, 95],
];
/// Colour of objects that belong to no player.
pub const NEUTRAL_COLOR: [u8; 3] = [165, 165, 165];
/// Alpha of a white turret range fill on the GPU path (specification.md,
/// graphics: ranges are mostly transparent; matches the Python fill).
pub const RANGE_TURRET_FILL_ALPHA: u8 = 42;
/// Alpha of a light-green heal range fill (matches the Python fill).
pub const RANGE_HEAL_FILL_ALPHA: u8 = 46;
/// Alpha of range outlines (same hue as the fill but clearly less
/// transparent, so an outline stays readable over other fills).
pub const RANGE_OUTLINE_ALPHA: u8 = 130;
#[allow(dead_code)]
/// Route line colour of moving vehicles.
pub const PATH_COLOR: [u8; 3] = [255, 255, 255];
/// Route preview colour.
#[allow(dead_code)]
pub const PATH_PREVIEW_COLOR: [u8; 3] = [255, 240, 120];
#[allow(dead_code)]
/// UI text colour.
pub const UI_TEXT_COLOR: [u8; 3] = [235, 235, 235];
#[allow(dead_code)]
/// UI background colour.
pub const UI_BACKGROUND: [u8; 3] = [24, 26, 34];
/// Level-menu grid layout (specification.md; UI only).
pub const MENU_COLUMNS: usize = 3;
/// Menu font size.
pub const MENU_FONT_SIZE: u16 = 26;
/// Menu cell horizontal padding.
pub const MENU_CELL_PAD_X: f32 = 18.0;
#[allow(dead_code)]
/// Menu cell vertical padding.
pub const MENU_CELL_PAD_Y: f32 = 8.0;
/// Menu row gap.
pub const MENU_ROW_GAP: f32 = 6.0;
/// Menu side margin.
pub const MENU_SIDE_MARGIN: f32 = 40.0;
/// Fraction of the screen height where the menu grid starts.
pub const MENU_GRID_TOP_FRACTION: f32 = 0.30;
/// Menu bottom margin.
pub const MENU_GRID_BOTTOM_MARGIN: f32 = 70.0;
/// Menu scroll step in px.
pub const MENU_SCROLL_STEP: f32 = 48.0;
// Map files (specification.md "Plansze")
/// AI difficulty used for maps loaded from files (rules.md section 13.8).
pub const MAP_DEFAULT_AI_DIFFICULTY: &str = "normal";
// AI difficulty (rules.md sections 13.5, 13.8)
/// Tunable parameters of one AI difficulty level (rules.md section 13.8).
#[derive(Clone, Copy, Debug)]
pub struct AiDifficulty {
    /// Preset name.
    pub name: &'static str,
    /// Seconds between decisions (section 13.2).
    pub interval: f64,
    /// Standard deviation of the score noise.
    pub noise: f64,
    /// Seconds before a fresh threat is reacted to.
    pub reaction_delay: f64,
    /// Minimum score to perform an action (section 13.5).
    pub threshold: f64,
    /// Weight: chance of capturing the target.
    pub w1: f64,
    /// Weight: value of the target building.
    pub w2: f64,
    /// Weight: defence need / evacuation.
    pub w3: f64,
    /// Weight: travel time.
    pub w4: f64,
    /// Weight: route danger.
    pub w5: f64,
    /// Weight: risk of losing the source.
    pub w6: f64,
}
/// Difficulty presets (rules.md section 13.8).
pub const AI_DIFFICULTIES: [AiDifficulty; 3] = [
    AiDifficulty {
        name: "easy",
        interval: 2.6,
        noise: 1.4,
        reaction_delay: 7.0,
        threshold: 1.2,
        w1: 6.0,
        w2: 3.0,
        w3: 4.0,
        w4: 1.2,
        w5: 2.0,
        w6: 2.0,
    },
    AiDifficulty {
        name: "normal",
        interval: 2.0,
        noise: 0.7,
        reaction_delay: 3.5,
        threshold: 0.9,
        w1: 8.0,
        w2: 4.0,
        w3: 5.0,
        w4: 1.8,
        w5: 3.0,
        w6: 3.0,
    },
    AiDifficulty {
        name: "hard",
        interval: 1.5,
        noise: 0.25,
        reaction_delay: 1.5,
        threshold: 0.6,
        w1: 10.0,
        w2: 5.0,
        w3: 6.0,
        w4: 2.2,
        w5: 4.0,
        w6: 4.0,
    },
];
/// Look up a difficulty preset by name.
pub fn ai_difficulty(name: &str) -> &'static AiDifficulty {
    for d in AI_DIFFICULTIES.iter() {
        if d.name == name {
            return d;
        }
    }
    &AI_DIFFICULTIES[1]
}
/// Shade an RGB colour by a multiplicative factor (clamped to 0..255).
pub fn shade(color: [u8; 3], factor: f64) -> [u8; 3] {
    let apply = |c: u8| ((c as f64 * factor).round() as i32).clamp(0, 255) as u8;
    [apply(color[0]), apply(color[1]), apply(color[2])]
}
/// Colour of a player id (wraps around [`PLAYER_COLORS`]).
pub fn player_color(id: usize) -> [u8; 3] {
    PLAYER_COLORS[id % PLAYER_COLORS.len()]
}
