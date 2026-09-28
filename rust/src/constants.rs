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
/// Colour of the translucent shadow decal (specification.md).
pub const SHADOW_COLOR: [u8; 3] = [0, 0, 0];
/// Alpha of the translucent shadow decal, 70/255 (specification.md).
pub const SHADOW_ALPHA: u8 = 70;
/// Lift of the helicopter shadow silhouette above the receiving surface in
/// px (rendering only; rules.md has no shadows). Just high enough that the
/// decal never loses the depth race against the terrain (no flicker), low
/// enough that it still reads as lying on the ground.
pub const SHADOW_LIFT: f64 = 0.25 * UNIT_J_TO_PX;
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

// ---------------------------------------------------------------------------
// Explosion effects (rendering only)
//
// rules.md has no explosions at all: section 9 only says a vehicle stops
// existing once its units drop to zero. Every number below is therefore a pure
// presentation value in px or seconds and must never influence the simulation.
// ---------------------------------------------------------------------------

/// Upper bound of particles alive at the same time (rendering budget only).
pub const EXPLOSION_MAX_PARTICLES: usize = 512;
/// Fraction of a blob's half-size filled with the opaque core colour; the
/// remaining rim fades out, which turns the flat quad into a soft round puff
/// (see [`crate::mesh::push_fx_blob`]).
pub const FX_CORE_FILL: f64 = 0.45;
/// Segments of one flat ground shock-wave ring (rendering only).
pub const FX_RING_SEGMENTS: usize = 20;
/// Fraction of the wave's current radius that stays transparent, so the ring
/// reads as a moving band instead of a filled disc.
pub const EXPLOSION_RING_INNER_FRAC: f64 = 0.55;
/// Shortest lifetime a particle may get, in s (a jittered emitter must never
/// produce a zero-length particle that would never be drawn).
pub const FX_MIN_LIFETIME: f64 = 0.02;
/// Initial vertical speed given to every particle of a burst in px/s, so the
/// explosion opens upwards instead of spreading flat.
pub const FX_BURST_UP: f64 = 14.0;
/// Jitter of that initial vertical speed in px/s.
pub const FX_BURST_UP_RANDOM: f64 = 26.0;
/// Maximum spin of a shard in rad/s.
pub const FX_SHARD_SPIN: f64 = 9.0;
/// Default seed of the explosion particle stream, used before a level is
/// loaded and mixed into the level seed by `Fx::reseed()` (rendering only).
pub const FX_DEFAULT_SEED: u64 = 0x5EED_CAFE_BABE_1234;
/// Start radius of the ground shock wave in px.
pub const EXPLOSION_RING_START_R: f64 = 6.0;
/// End radius of the ground shock wave in px.
pub const EXPLOSION_RING_END_R: f64 = 44.0;
/// Lifetime of the ground shock wave in s.
pub const EXPLOSION_RING_LIFETIME: f64 = 0.45;
/// Peak alpha of the ground shock wave (0..1).
pub const EXPLOSION_RING_ALPHA: f64 = 0.55;
/// Lift of the ground shock wave above the receiving surface in px, so the
/// ring does not fight the terrain it runs over (same reason as
/// [`SHADOW_LIFT`]).
pub const EXPLOSION_RING_LIFT: f64 = 0.3 * UNIT_J_TO_PX;
/// Half-size of the one-frame white flash of a new explosion in px.
pub const EXPLOSION_FLASH_SIZE: f64 = 26.0;
/// Lifetime of the white flash in s.
pub const EXPLOSION_FLASH_LIFETIME: f64 = 0.16;
/// Peak alpha of the white flash (0..1).
pub const EXPLOSION_FLASH_ALPHA: f64 = 0.9;
/// Colour of the white flash: hot core at the first frame of the blast.
pub const EXPLOSION_FLASH_COLOR: [u8; 3] = [255, 246, 214];
/// Number of fireball blobs of one explosion.
pub const EXPLOSION_FIREBALL_COUNT: usize = 9;
/// Start size of one fireball blob in px.
pub const EXPLOSION_FIREBALL_SIZE: f64 = 13.0;
/// Size jitter of the fireball blobs (fraction of the start size).
pub const EXPLOSION_FIREBALL_SIZE_RANDOM: f64 = 0.45;
/// Growth of the fireball size in px/s (the blast swells while fading out).
pub const EXPLOSION_FIREBALL_GROWTH: f64 = 16.0;
/// Lifetime of one fireball blob in s.
pub const EXPLOSION_FIREBALL_LIFETIME: f64 = 0.45;
/// Lifetime jitter of the fireball blobs (fraction of the lifetime).
pub const EXPLOSION_FIREBALL_LIFETIME_RANDOM: f64 = 0.4;
/// Vertical speed of the fireball blobs in px/s (the flame front rises).
pub const EXPLOSION_FIREBALL_RISE: f64 = 26.0;
/// Horizontal drift speed of the fireball blobs in px/s.
pub const EXPLOSION_FIREBALL_DRIFT: f64 = 30.0;
/// Fireball colour at birth: yellow-hot.
pub const EXPLOSION_FIRE_COLOR_START: [u8; 3] = [255, 232, 150];
/// Fireball colour in mid-life: orange.
pub const EXPLOSION_FIRE_COLOR_MID: [u8; 3] = [255, 138, 46];
/// Fireball colour at death: dark red.
pub const EXPLOSION_FIRE_COLOR_END: [u8; 3] = [122, 30, 18];
/// Peak alpha of one fireball blob (0..1).
pub const EXPLOSION_FIRE_ALPHA: f64 = 0.95;
/// Number of smoke puffs of one explosion.
pub const EXPLOSION_SMOKE_COUNT: usize = 10;
/// Start size of one smoke puff in px.
pub const EXPLOSION_SMOKE_SIZE: f64 = 11.0;
/// Size jitter of the smoke puffs (fraction of the start size).
pub const EXPLOSION_SMOKE_SIZE_RANDOM: f64 = 0.4;
/// Growth of the smoke size in px/s (smoke keeps spreading as it dies).
pub const EXPLOSION_SMOKE_GROWTH: f64 = 15.0;
/// Lifetime of one smoke puff in s.
pub const EXPLOSION_SMOKE_LIFETIME: f64 = 1.5;
/// Lifetime jitter of the smoke puffs (fraction of the lifetime).
pub const EXPLOSION_SMOKE_LIFETIME_RANDOM: f64 = 0.35;
/// Rise speed of the smoke in px/s (hot air lifts it).
pub const EXPLOSION_SMOKE_RISE: f64 = 30.0;
/// Horizontal drift speed of the smoke in px/s.
pub const EXPLOSION_SMOKE_DRIFT: f64 = 16.0;
/// Smoke colour at birth: light grey, still tinted by the blast.
pub const EXPLOSION_SMOKE_COLOR_START: [u8; 3] = [156, 150, 144];
/// Smoke colour in mid-life: darker grey.
pub const EXPLOSION_SMOKE_COLOR_MID: [u8; 3] = [104, 100, 98];
/// Smoke colour at death: almost black, the puff has thinned out.
pub const EXPLOSION_SMOKE_COLOR_END: [u8; 3] = [48, 46, 46];
/// Peak alpha of one smoke puff (0..1); smoke is thinner than fire.
pub const EXPLOSION_SMOKE_ALPHA: f64 = 0.6;
/// Number of sparks of one explosion.
pub const EXPLOSION_SPARK_COUNT: usize = 16;
/// Start size of one spark in px.
pub const EXPLOSION_SPARK_SIZE: f64 = 2.6;
/// Size jitter of the sparks (fraction of the start size).
pub const EXPLOSION_SPARK_SIZE_RANDOM: f64 = 0.5;
/// Lifetime of one spark in s.
pub const EXPLOSION_SPARK_LIFETIME: f64 = 0.55;
/// Lifetime jitter of the sparks (fraction of the lifetime).
pub const EXPLOSION_SPARK_LIFETIME_RANDOM: f64 = 0.5;
/// Speed of a spark leaving the wreck in px/s.
pub const EXPLOSION_SPARK_SPEED: f64 = 150.0;
/// Speed jitter of the sparks (fraction of the speed).
pub const EXPLOSION_SPARK_SPEED_RANDOM: f64 = 0.7;
/// Downward acceleration of a spark in px/s^2 (they arc down like debris).
pub const EXPLOSION_SPARK_GRAVITY: f64 = 190.0;
/// Spark colour at birth: white-hot.
pub const EXPLOSION_SPARK_COLOR_START: [u8; 3] = [255, 244, 206];
/// Spark colour in mid-life: ember orange.
pub const EXPLOSION_SPARK_COLOR_MID: [u8; 3] = [255, 152, 58];
/// Spark colour at death: dark ember.
pub const EXPLOSION_SPARK_COLOR_END: [u8; 3] = [150, 52, 20];
/// Peak alpha of one spark (0..1).
pub const EXPLOSION_SPARK_ALPHA: f64 = 1.0;
/// Number of dark wreck fragments thrown out by one explosion.
pub const EXPLOSION_DEBRIS_COUNT: usize = 7;
/// Size of one wreck fragment in px.
pub const EXPLOSION_DEBRIS_SIZE: f64 = 3.0;
/// Size jitter of the fragments (fraction of the size).
pub const EXPLOSION_DEBRIS_SIZE_RANDOM: f64 = 0.5;
/// Lifetime of one wreck fragment in s.
pub const EXPLOSION_DEBRIS_LIFETIME: f64 = 0.8;
/// Lifetime jitter of the fragments (fraction of the lifetime).
pub const EXPLOSION_DEBRIS_LIFETIME_RANDOM: f64 = 0.4;
/// Speed of a fragment leaving the wreck in px/s.
pub const EXPLOSION_DEBRIS_SPEED: f64 = 95.0;
/// Speed jitter of the fragments (fraction of the speed).
pub const EXPLOSION_DEBRIS_SPEED_RANDOM: f64 = 0.6;
/// Downward acceleration of a fragment in px/s^2.
pub const EXPLOSION_DEBRIS_GRAVITY: f64 = 260.0;
/// Colour of a thrown wreck fragment: the burnt hull of the vehicle.
pub const EXPLOSION_DEBRIS_COLOR: [u8; 3] = [58, 56, 58];
/// Peak alpha of one wreck fragment (0..1).
pub const EXPLOSION_DEBRIS_ALPHA: f64 = 0.9;
/// Fraction of the player colour mixed into the fire, so the blast still
/// reads as belonging to the destroyed vehicle's owner.
pub const EXPLOSION_TINT_MIX: f64 = 0.35;
/// Multiplier of the whole explosion size for a helicopter (rules.md section
/// 5.2): a bigger airframe burns bigger than a tank.
pub const EXPLOSION_HELICOPTER_SCALE: f64 = 1.25;
/// Vertical offset of the blast above the wreck position in px, so the fire
/// starts in the middle of the vehicle body instead of under its tracks.
pub const EXPLOSION_CENTER_LIFT: f64 = 6.0;

// ---------------------------------------------------------------------------
// Audio (presentation only)
//
// rules.md says nothing about sound: no rule mentions hearing an explosion
// or a shot, and none of the values below may ever influence the simulation.
// They live here next to the explosion particle constants for the same reason
// -- they tune presentation, not gameplay. Every sound is synthesised at
// runtime by `sound.rs` (there are no audio files in the repository), so these
// constants describe waveforms, not samples.
// ---------------------------------------------------------------------------

/// Sample rate of every synthesised sound in Hz. macroquad's audio backend
/// mixes at 44100 Hz, so generating at that rate avoids a resampling pass.
pub const AUDIO_SAMPLE_RATE: u32 = 44_100;
/// Peak level of one synthesised sound, before the distance attenuation and
/// the master volume are applied. Kept below full scale so that several
/// sounds overlapping in the mixer do not clip hard.
pub const AUDIO_PEAK: f64 = 0.85;
/// Master volume of all game sound (0..1).
pub const AUDIO_MASTER_VOLUME: f32 = 0.7;
/// Distance in j at which a sound is already fully inaudible. Beyond this
/// range nothing is played at all, so a big battle does not spend its voice
/// budget on fights happening off screen.
pub const AUDIO_MAX_DISTANCE_J: f64 = 900.0;
/// Distance in j within which a sound keeps its full loudness. Past it the
/// volume falls off linearly down to silence at `AUDIO_MAX_DISTANCE_J`.
pub const AUDIO_FULL_DISTANCE_J: f64 = 260.0;
/// Volume below which a sound is dropped instead of played. Very quiet events
/// are inaudible anyway and each one still costs a mixer voice.
pub const AUDIO_MIN_VOLUME: f32 = 0.01;
/// Maximum number of sounds started from one simulation step. A big firefight
/// can report dozens of events per tick; playing all of them turns into noise,
/// and the closest ones are the ones the player can actually see.
pub const AUDIO_MAX_VOICES_PER_STEP: usize = 6;
/// Shortest gap in seconds between two starts of the *same* sound. Repeated
/// shots of one rapid turret would otherwise stack into a single loud buzz.
pub const AUDIO_RETRIGGER_INTERVAL: f64 = 0.045;
/// Length of the fade-out at the end of every sound, in s. The decay
/// envelopes below still leave a few percent of the amplitude on the last
/// sample, and a waveform that stops at 4% of full scale ends on an audible
/// click; fading the tail to silence removes it.
pub const AUDIO_FADE_OUT_SECONDS: f64 = 0.02;

// -- explosion (a destroyed vehicle, `Wreck` in `entities.rs`) ----------------
/// Duration of a ground explosion in s.
pub const AUDIO_EXPLOSION_GROUND_SECONDS: f64 = 1.1;
/// Duration of an air explosion in s (a helicopter dies over the board, with
/// no low ground thump).
pub const AUDIO_EXPLOSION_AIR_SECONDS: f64 = 0.7;
/// Start frequency of the noise burst of an explosion in Hz; the sweep falls
/// to `AUDIO_EXPLOSION_NOISE_END_HZ` over the burst, which is what turns a
/// hiss into a "whoomph".
pub const AUDIO_EXPLOSION_NOISE_START_HZ: f64 = 2_600.0;
/// End frequency of the noise sweep of an explosion in Hz.
pub const AUDIO_EXPLOSION_NOISE_END_HZ: f64 = 180.0;
/// Level of the noise layer of an explosion, relative to the tone layer.
pub const AUDIO_EXPLOSION_NOISE_LEVEL: f64 = 0.9;
/// Start frequency of the low tone of a ground explosion in Hz.
pub const AUDIO_EXPLOSION_TONE_START_HZ: f64 = 130.0;
/// End frequency of the low tone of a ground explosion in Hz.
pub const AUDIO_EXPLOSION_TONE_END_HZ: f64 = 34.0;
/// Decay constant of the noise layer of an explosion (1/s).
pub const AUDIO_EXPLOSION_NOISE_DECAY: f64 = 5.0;
/// Decay constant of the low tone of a ground explosion (1/s).
pub const AUDIO_EXPLOSION_TONE_DECAY: f64 = 2.6;
/// Level of the low tone of an explosion, relative to the noise layer.
pub const AUDIO_EXPLOSION_TONE_LEVEL: f64 = 1.0;

// -- shots ------------------------------------------------------------------
/// Duration of a shot of a vehicle in combat (rules.md section 9) in s.
pub const AUDIO_VEHICLE_FIRE_SECONDS: f64 = 0.26;
/// Start frequency of the tone of a vehicle shot in Hz.
pub const AUDIO_VEHICLE_FIRE_TONE_START_HZ: f64 = 220.0;
/// End frequency of the tone of a vehicle shot in Hz.
pub const AUDIO_VEHICLE_FIRE_TONE_END_HZ: f64 = 70.0;
/// Decay constant of a vehicle shot (1/s).
pub const AUDIO_VEHICLE_FIRE_DECAY: f64 = 16.0;
/// Level of the noise layer of a vehicle shot, relative to the tone layer.
pub const AUDIO_VEHICLE_FIRE_NOISE_LEVEL: f64 = 0.75;
/// Start frequency of the noise layer of a vehicle shot in Hz.
pub const AUDIO_VEHICLE_FIRE_NOISE_START_HZ: f64 = 3_200.0;
/// End frequency of the noise layer of a vehicle shot in Hz.
pub const AUDIO_VEHICLE_FIRE_NOISE_END_HZ: f64 = 400.0;
/// Duration of a shot of a `Normal` turret (rules.md section 10.1) in s.
pub const AUDIO_TURRET_NORMAL_SECONDS: f64 = 0.3;
/// Start frequency of a `Normal` turret shot in Hz (a heavy gun).
pub const AUDIO_TURRET_NORMAL_TONE_START_HZ: f64 = 260.0;
/// End frequency of a `Normal` turret shot in Hz.
pub const AUDIO_TURRET_NORMAL_TONE_END_HZ: f64 = 90.0;
/// Duration of a shot of a `Rapid` turret (rules.md section 10.3) in s; short,
/// because it fires every second.
pub const AUDIO_TURRET_RAPID_SECONDS: f64 = 0.13;
/// Start frequency of a `Rapid` turret shot in Hz (a light, snappy gun).
pub const AUDIO_TURRET_RAPID_TONE_START_HZ: f64 = 620.0;
/// End frequency of a `Rapid` turret shot in Hz.
pub const AUDIO_TURRET_RAPID_TONE_END_HZ: f64 = 240.0;
/// Duration of a shot of a `Rocket` turret (rules.md section 10.2) in s; long
/// and hissy, it is a projectile leaving a launcher.
pub const AUDIO_TURRET_ROCKET_SECONDS: f64 = 0.55;
/// Start frequency of the noise layer of a rocket shot in Hz.
pub const AUDIO_TURRET_ROCKET_NOISE_START_HZ: f64 = 900.0;
/// End frequency of the noise layer of a rocket shot in Hz; the rising sweep
/// is the classic "whoosh" of a rocket motor.
pub const AUDIO_TURRET_ROCKET_NOISE_END_HZ: f64 = 3_400.0;
/// Decay constant of a rocket shot (1/s).
pub const AUDIO_TURRET_ROCKET_DECAY: f64 = 4.5;
/// Level of the noise layer of a rocket shot, relative to the tone layer.
pub const AUDIO_TURRET_ROCKET_NOISE_LEVEL: f64 = 1.0;
/// Duration of the sound of a projectile hitting its target in s.
pub const AUDIO_IMPACT_SECONDS: f64 = 0.18;
/// Start frequency of the tone of an impact in Hz.
pub const AUDIO_IMPACT_TONE_START_HZ: f64 = 900.0;
/// End frequency of the tone of an impact in Hz.
pub const AUDIO_IMPACT_TONE_END_HZ: f64 = 300.0;
/// Decay constant of an impact (1/s).
pub const AUDIO_IMPACT_DECAY: f64 = 26.0;
/// Level of the noise layer of an impact, relative to the tone layer.
pub const AUDIO_IMPACT_NOISE_LEVEL: f64 = 0.5;
/// Duration of the sound of a vehicle shooting at a wall (rules.md section 4)
/// in s; duller and shorter than a shot at a vehicle.
pub const AUDIO_WALL_HIT_SECONDS: f64 = 0.16;
/// Start frequency of the tone of a wall hit in Hz.
pub const AUDIO_WALL_HIT_TONE_START_HZ: f64 = 150.0;
/// End frequency of the tone of a wall hit in Hz.
pub const AUDIO_WALL_HIT_TONE_END_HZ: f64 = 60.0;
/// Decay constant of a wall hit (1/s).
pub const AUDIO_WALL_HIT_DECAY: f64 = 24.0;
/// Level of the noise layer of a wall hit, relative to the tone layer.
pub const AUDIO_WALL_HIT_NOISE_LEVEL: f64 = 0.35;
/// Level of an impact of a normal or rapid turret shot, relative to the sound
/// of a full turret shot. A light tick under its own shot.
pub const AUDIO_IMPACT_TURRET_LEVEL: f32 = 0.5;
/// Level of the impact of a rocket, relative to the explosion of a vehicle: a
/// rocket that hits kills on its own, so it reads as a small blast.
pub const AUDIO_IMPACT_ROCKET_LEVEL: f32 = 0.75;
/// Level of a vehicle shot compared to a turret shot; the player mostly hears
/// their own turrets, so vehicles are a little quieter.
pub const AUDIO_VEHICLE_FIRE_LEVEL: f32 = 0.7;

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
/// Top face of a bridge deck (rendering only; rules.md section 8 fixes the
/// deck geometry and elevation, never its colour).
pub const BRIDGE_DECK_COLOR: [u8; 3] = [150, 120, 90];
/// Edge band of the deck slab, below [`BRIDGE_DECK_COLOR`].
pub const BRIDGE_DECK_SIDE_COLOR: [u8; 3] = [104, 84, 63];
/// Thickness of the deck slab in px (rendering only). The deck is a plate on
/// pillars, never a solid block: rules.md section 8 lets vehicles pass under a
/// bridge, so the space below it must stay open.
pub const BRIDGE_DECK_THICKNESS: f64 = 2.0 * UNIT_J_TO_PX;
/// Footprint of one support pillar in px (rendering only).
pub const BRIDGE_PILLAR_WID: f64 = 5.0 * UNIT_J_TO_PX;
/// Colour of the support pillars holding the deck over the field below.
pub const BRIDGE_PILLAR_COLOR: [u8; 3] = [84, 68, 52];
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
/// graphics: ranges are mostly transparent).
pub const RANGE_TURRET_FILL_ALPHA: u8 = 26;
/// Alpha of a light-green heal range fill.
pub const RANGE_HEAL_FILL_ALPHA: u8 = 30;
/// Alpha of range outlines (drawn in the owner colour, clearly less
/// transparent than the fill, so an outline stays readable over other fills).
pub const RANGE_OUTLINE_ALPHA: u8 = 130;
/// Alpha written into the offscreen range masks. Range fills are composited
/// from a mask, never blended on the scene itself, so the mask vertices are
/// fully opaque: a second overlapping mask overwrites the first one instead of
/// stacking its alpha, which keeps two overlapping ranges of one kind at the
/// coverage of a single range (specification.md, graphics: overlapping ranges
/// must not darken).
pub const RANGE_MASK_ALPHA: u8 = 255;
/// Outline colour of a range owned by no player (specification.md, graphics:
/// range outlines are drawn in the player colour, white when neutral).
pub const RANGE_OUTLINE_NEUTRAL: [u8; 3] = [255, 255, 255];
/// Fill colour of a turret range once its mask is composited onto the scene.
pub const RANGE_TURRET_FILL_COLOR: [u8; 3] = [255, 255, 255];
/// Fill colour of a heal (tower or buffer) range once composited.
pub const RANGE_HEAL_FILL_COLOR: [u8; 3] = [150, 245, 150];
/// Colour written into the offscreen range masks. Each mask is multiplied by
/// the presentation colour chosen at composition time
/// ([`RANGE_TURRET_FILL_COLOR`] for turrets, [`RANGE_HEAL_FILL_COLOR`] for
/// heals), so the mask itself carries no hue and stays white.
pub const RANGE_MASK_COLOR: [u8; 3] = [255, 255, 255];
/// Height of a range disc above the tile top in px (rendering only; the range
/// itself is defined by rules.md sections 10-11). Keeps the disc slightly off
/// the terrain plane so it never z-fights on flat ground.
pub const RANGE_FILL_LIFT: f64 = 0.5 * UNIT_J_TO_PX;
/// Height of a range outline above the tile top in px (rendering only), just
/// above [`RANGE_FILL_LIFT`] so the outline stays readable over the fill.
pub const RANGE_OUTLINE_LIFT: f64 = 0.6 * UNIT_J_TO_PX;
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
// Board editor (specification_rust.md, section "Edytor plansz"; rules.md
// section 1 for the 0..15 heights and player bases).
/// Seconds after which an unfinished 1- or 2-digit units entry commits.
pub const EDITOR_DIGIT_COMMIT_DELAY: f64 = 1.0;
/// Columns of a newly created editor board (mostly water).
pub const EDITOR_NEW_COLS: i32 = 256;
/// Rows of a newly created editor board.
pub const EDITOR_NEW_ROWS: i32 = 256;
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
