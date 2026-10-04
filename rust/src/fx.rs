//! Explosion effects: the particle system of destroyed vehicles.
//!
//! rules.md has no explosions at all -- section 9 only says that a vehicle
//! stops existing once its units drop to zero. This module is therefore pure
//! presentation: the simulation reports *where* a vehicle died
//! ([`crate::entities::Wreck`], drained from [`crate::game::Game::take_wrecks`])
//! and this module decides how that spot looks for the next second or so.
//!
//! The design follows the `macroquad-particles` recipe (one `EmitterConfig`
//! per burst, a `ColorCurve` sampled over the particle lifetime, particles
//! kept in a plain `Vec` and advanced with the frame delta), but everything is
//! built here instead of taken from that crate, because this game draws the
//! whole scene itself: particles are ordinary mesh geometry ([`crate::mesh`])
//! pushed into the same GPU pipeline as the terrain, so they get the hardware
//! depth test and a wreck exploding behind a hill is properly hidden by it.
//! It also means no raster assets, no extra dependency, and headless unit
//! tests -- the module deliberately does not depend on macroquad at all.
//!
//! Randomness comes from the stateful [`crate::rng::Rng`] stream kept in
//! [`Fx`], which the application seeds with the level seed: every explosion
//! looks different from the last, yet a given level always plays its blasts
//! the same way.

use crate::constants::{self, VehicleKind};
use crate::entities::Wreck;
use crate::mesh::DynamicMesh;
use crate::rng::Rng;

/// Colour ramp of one particle, sampled over its lifetime: `start` at birth,
/// `mid` halfway through, `end` at death. Interpolating the three stops gives
/// every particle a life-like colour trail (white-hot ember -> orange -> dark)
/// without any texture lookup.
#[derive(Clone, Copy, Debug)]
pub struct ColorCurve {
    /// Colour at the moment of the burst.
    pub start: [u8; 3],
    /// Colour halfway through the lifetime.
    pub mid: [u8; 3],
    /// Colour in the last frame before the particle dies.
    pub end: [u8; 3],
}

impl ColorCurve {
    /// Sample the ramp at `t` in 0..1 (clamped).
    pub fn sample(&self, t: f64) -> [u8; 3] {
        let t = t.clamp(0.0, 1.0);
        let (from, to, k) = if t < 0.5 {
            (self.start, self.mid, t * 2.0)
        } else {
            (self.mid, self.end, t * 2.0 - 1.0)
        };
        [
            lerp_channel(from[0], to[0], k),
            lerp_channel(from[1], to[1], k),
            lerp_channel(from[2], to[2], k),
        ]
    }
}

/// Linear interpolation of one 0..255 colour channel.
fn lerp_channel(a: u8, b: u8, t: f64) -> u8 {
    (a as f64 + (b as f64 - a as f64) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Mix `base` with the player colour `tint` (a fraction of
/// [`constants::EXPLOSION_TINT_MIX`]), so the blast of a red vehicle still
/// reads as red without losing the fire colours.
fn tint(base: [u8; 3], tint: [u8; 3]) -> [u8; 3] {
    let k = constants::EXPLOSION_TINT_MIX;
    [
        lerp_channel(base[0], tint[0], k),
        lerp_channel(base[1], tint[1], k),
        lerp_channel(base[2], tint[2], k),
    ]
}

/// Geometry a particle is drawn as. The three shapes cover the whole blast:
/// soft puffs, hard sparks/shards and a flat wave running over the ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxShape {
    /// Soft round puff (flash, fire, smoke).
    Blob,
    /// Hard-edged square shard (sparks, thrown hull fragments).
    Shard,
    /// Flat ring lying on the ground (blast shock wave).
    Ring,
}

/// One burst of particles, i.e. the "emitter" of the effect.
///
/// Every field is a rendering value in px or seconds; none of them comes from
/// rules.md, because the game has no explosions. One explosion is a handful of
/// emitters with different shapes, so the whole blast stays declarative.
#[derive(Clone, Debug)]
pub struct EmitterConfig {
    /// Number of particles to spawn.
    pub amount: usize,
    /// Lifetime of one particle in s.
    pub lifetime: f64,
    /// Lifetime jitter as a fraction of `lifetime` (0 = all equal).
    pub lifetime_randomness: f64,
    /// Share of the burst that starts immediately: 1 = every particle at once,
    /// 0 = the particles are spread evenly over `lifetime`. The first frame of
    /// a blast should read as one flash, so the flash, the fire and the sparks
    /// use a high value and the smoke a low one.
    pub explosiveness: f64,
    /// Initial horizontal speed in px/s (0 = the particles only rise).
    pub initial_velocity: f64,
    /// Speed jitter as a fraction of `initial_velocity`.
    pub initial_velocity_randomness: f64,
    /// Initial size in px (half-width of a quad, start radius of a ring).
    pub size: f64,
    /// Size jitter as a fraction of `size`.
    pub size_randomness: f64,
    /// Size change over the whole lifetime in px (negative shrinks the
    /// particle, positive makes it spread).
    pub size_growth: f64,
    /// Upward acceleration in px/s^2 (flame front, rising smoke).
    pub rise: f64,
    /// Downward acceleration in px/s^2 (sparks and debris arc down).
    pub gravity: f64,
    /// Peak opacity in 0..1.
    pub alpha: f64,
    /// Exponent of the fade-out curve: 1 = linear, higher = the particle
    /// disappears earlier and leaves a longer, dimmer tail.
    pub fade_power: f64,
    /// Colour ramp of the particles.
    pub colors: ColorCurve,
    /// Geometry the particles are drawn as.
    pub shape: FxShape,
}

/// One spot of the burning field of a fire trap: a flame of `scale` times the
/// size (and the emission rate) of the reference fire
/// (`CAMPFIRE_SCALE_CENTER`).
///
/// A burning field carries a whole [`campfire_cluster`] of these: a few
/// roughly even fires, the closest to the centre burning the biggest. They are
/// plain world positions, already lifted to the tile top, so [`Fx`] never
/// touches the board.
#[derive(Clone, Copy, Debug)]
pub struct Campfire {
    /// World x in j.
    pub x: f64,
    /// World y in j.
    pub y: f64,
    /// Rendered elevation in px.
    pub z: f64,
    /// Fire size relative to the reference fire (1.0 at the field centre).
    pub scale: f64,
}

/// Size scale of a campfire spot `radius` px from its field centre: a linear
/// falloff from [`constants::CAMPFIRE_SCALE_CENTER`] to
/// [`constants::CAMPFIRE_SCALE_EDGE`], clamped at both ends, so the fire is
/// biggest in the middle and fades out towards the edge.
fn campfire_scale(radius: f64) -> f64 {
    let t = (radius / constants::CAMPFIRE_SCALE_REF_R).clamp(0.0, 1.0);
    constants::CAMPFIRE_SCALE_CENTER
        + (constants::CAMPFIRE_SCALE_EDGE - constants::CAMPFIRE_SCALE_CENTER) * t
}

/// The campfire cluster of one burning field: [`constants::CAMPFIRE_SPOTS`]
/// laid out around the field centre `(cx, cy)` at elevation `z`, rotated by a
/// multiple of 60 deg derived from the tile `(q, r)` so neighbouring fields do
/// not burn in lockstep. The layout is a pure function of the tile: it is
/// identical every frame, so the fires never jump around.
pub fn campfire_cluster(cx: f64, cy: f64, z: f64, q: i32, r: i32) -> Vec<Campfire> {
    let step = ((q * 7 + r * 13).rem_euclid(6)) as f64;
    let ang = step * std::f64::consts::FRAC_PI_3;
    let (s, c) = (ang.sin(), ang.cos());
    constants::CAMPFIRE_SPOTS
        .iter()
        .map(|(dx, dy)| {
            let radius = (dx * dx + dy * dy).sqrt();
            Campfire {
                x: cx + dx * c - dy * s,
                y: cy + dx * s + dy * c,
                z,
                scale: campfire_scale(radius),
            }
        })
        .collect()
}

/// One live particle of the effect.
#[derive(Clone, Debug)]
pub struct Particle {
    /// World x in j.
    pub x: f64,
    /// World y in j.
    pub y: f64,
    /// Rendered elevation in px.
    pub z: f64,
    /// Horizontal speed in px/s.
    pub vx: f64,
    /// Horizontal speed in px/s.
    pub vy: f64,
    /// Vertical speed in px/s.
    pub vz: f64,
    /// Upward acceleration in px/s^2.
    pub rise: f64,
    /// Downward acceleration in px/s^2.
    pub gravity: f64,
    /// Seconds since the explosion was requested (the particle may still be
    /// waiting for its spawn delay, see `delay`).
    pub age: f64,
    /// Seconds the particle waits at the origin before it starts moving.
    pub delay: f64,
    /// Total lifetime in s, counted from the moment the particle was
    /// requested: a particle of a delayed burst is therefore visible for
    /// `life - delay` seconds.
    pub life: f64,
    /// Current size in px.
    pub size: f64,
    /// Size change in px per second.
    pub growth: f64,
    /// Peak opacity in 0..1.
    pub alpha: f64,
    /// Exponent of the fade-out curve.
    pub fade_power: f64,
    /// Colour ramp sampled over the lifetime.
    pub colors: ColorCurve,
    /// Geometry this particle is drawn as.
    pub shape: FxShape,
    /// Screen-space rotation of a [`FxShape::Shard`] in rad.
    pub angle: f64,
    /// Spin of a [`FxShape::Shard`] in rad/s.
    pub spin: f64,
    /// True for the continuous flame of a fire trap
    /// ([`Fx::maintain_campfires`]), false for the one-shot explosion of a
    /// destroyed vehicle ([`Fx::explode`]). Only used to keep the two budgets
    /// apart in [`Fx::trim`].
    pub campfire: bool,
}

impl Particle {
    /// Progress through the *visible* lifetime in 0..1; a particle that has not
    /// been born yet sits at 0.
    pub fn age_ratio(&self) -> f64 {
        if self.life <= self.delay {
            1.0
        } else {
            ((self.age - self.delay) / (self.life - self.delay)).clamp(0.0, 1.0)
        }
    }

    /// True while the particle still waits for its spawn delay.
    pub fn is_delayed(&self) -> bool {
        self.age < self.delay
    }

    /// Opacity of the particle right now: full at birth, then falling off with
    /// `fade_power`, so the tail of a spark dies much faster than its core.
    pub fn opacity(&self) -> f64 {
        (self.alpha * (1.0 - self.age_ratio()).powf(self.fade_power)).clamp(0.0, 1.0)
    }

    /// Colour of the particle right now.
    pub fn color(&self) -> [u8; 3] {
        self.colors.sample(self.age_ratio())
    }
}

/// All live effect particles of one match (presentation state only).
///
/// Carries both the one-shot explosions of destroyed vehicles and the
/// continuous flames of the fire traps ([`Fx::maintain_campfires`]). Kept out
/// of [`crate::game::Game`] on purpose: the simulation must stay free of
/// rendering state, and the effects must never influence gameplay or the
/// determinism of the simulation.
#[derive(Clone, Debug)]
pub struct Fx {
    /// All live particles, in spawn order.
    pub particles: Vec<Particle>,
    /// Random stream every burst draws from.
    ///
    /// It is a *stateful* stream, not a fresh generator per explosion: two
    /// wrecks of the same kind, and the same wreck blown up twice, look
    /// different each time. The stream is seeded per level
    /// ([`Fx::reseed`]) so a level always plays its explosions the same way --
    /// random enough to stay lively, reproducible enough to be testable and to
    /// replay a match identically.
    rng: Rng,
    /// Random stream the campfires draw from.
    ///
    /// Kept apart from `rng` on purpose: the flame is fed every frame, so
    /// sharing one stream would make the explosion sequence depend on the frame
    /// rate. The flame is instead its own, deliberately non-reproducible
    /// presentation detail (like the rotor phase), and the explosions stay
    /// replayable.
    fire_rng: Rng,
    /// Fractional spawn accumulators of the three campfire emitters, in
    /// particles: `[flame, spark, smoke]`. The fractional part carries over to
    /// the next frame, so the emission rate does not depend on the frame rate.
    fire_acc: [f64; 3],
}

impl Default for Fx {
    fn default() -> Self {
        Self::new()
    }
}

impl Fx {
    /// Create an empty effect system with the default seed
    /// ([`constants::FX_DEFAULT_SEED`]).
    pub fn new() -> Self {
        Self {
            particles: Vec::new(),
            rng: Rng::new(constants::FX_DEFAULT_SEED),
            fire_rng: Rng::new(constants::CAMPFIRE_SEED),
            fire_acc: [0.0; 3],
        }
    }

    /// Drop every particle and restart the random streams from `seed`.
    ///
    /// Called with the level seed whenever a match or an editor playtest
    /// starts: the explosions of one level are then reproducible, while every
    /// explosion inside it still differs from the previous one. The campfire
    /// stream is seeded from the same level seed too, but is never meant to be
    /// replayed bit for bit (the flame is fed every frame, so its randomness
    /// depends on the frame rate).
    pub fn reseed(&mut self, seed: u64) {
        self.particles.clear();
        self.rng = Rng::new(seed ^ constants::FX_DEFAULT_SEED);
        self.fire_rng = Rng::new(seed ^ constants::CAMPFIRE_SEED);
        self.fire_acc = [0.0; 3];
    }

    /// Drop every particle (new match, new level, leaving a playtest).
    pub fn clear(&mut self) {
        self.particles.clear();
        self.fire_acc = [0.0; 3];
    }

    /// True while no particle is alive.
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// Number of live particles.
    pub fn len(&self) -> usize {
        self.particles.len()
    }

    /// Play the explosion of the destroyed vehicle `w` at rendered height `z`.
    ///
    /// `z` is the elevation the wreck occupied -- [`crate::mesh`] computes it
    /// (the walkable surface for a ground vehicle, the fixed flight altitude
    /// for a helicopter). The burst draws from the shared random stream of
    /// [`Fx`], so no two explosions look alike even for the same vehicle; the
    /// sequence stays reproducible within one level, because that stream is
    /// seeded with the level seed.
    pub fn explode(&mut self, w: &Wreck, z: f64) {
        let owner = constants::player_color(w.owner);
        // Bigger airframes burn bigger (rules.md section 5.2).
        let scale = if w.kind == VehicleKind::Helicopter {
            constants::EXPLOSION_HELICOPTER_SCALE
        } else {
            1.0
        };
        let cy = z + constants::EXPLOSION_CENTER_LIFT;

        // White flash: one big, very short puff at the wreck. It sells the
        // moment of the blast before the fire has even spread.
        self.emit(
            &EmitterConfig {
                amount: 1,
                lifetime: constants::EXPLOSION_FLASH_LIFETIME,
                lifetime_randomness: 0.0,
                explosiveness: 1.0,
                initial_velocity: 0.0,
                initial_velocity_randomness: 0.0,
                size: constants::EXPLOSION_FLASH_SIZE * scale,
                size_randomness: 0.0,
                size_growth: constants::EXPLOSION_FLASH_SIZE * scale,
                rise: 0.0,
                gravity: 0.0,
                alpha: constants::EXPLOSION_FLASH_ALPHA,
                fade_power: 1.4,
                colors: ColorCurve {
                    start: constants::EXPLOSION_FLASH_COLOR,
                    mid: constants::EXPLOSION_FLASH_COLOR,
                    end: constants::EXPLOSION_FIRE_COLOR_END,
                },
                shape: FxShape::Blob,
            },
            (w.x, w.y, cy),
        );
        // Fireball: the body of the blast, swelling and drifting upwards.
        self.emit(
            &EmitterConfig {
                amount: constants::EXPLOSION_FIREBALL_COUNT,
                lifetime: constants::EXPLOSION_FIREBALL_LIFETIME,
                lifetime_randomness: constants::EXPLOSION_FIREBALL_LIFETIME_RANDOM,
                explosiveness: 0.8,
                initial_velocity: constants::EXPLOSION_FIREBALL_DRIFT,
                initial_velocity_randomness: 1.0,
                size: constants::EXPLOSION_FIREBALL_SIZE * scale,
                size_randomness: constants::EXPLOSION_FIREBALL_SIZE_RANDOM,
                size_growth: constants::EXPLOSION_FIREBALL_GROWTH,
                rise: constants::EXPLOSION_FIREBALL_RISE,
                gravity: 0.0,
                alpha: constants::EXPLOSION_FIRE_ALPHA,
                fade_power: 1.2,
                colors: ColorCurve {
                    start: tint(constants::EXPLOSION_FIRE_COLOR_START, owner),
                    mid: tint(constants::EXPLOSION_FIRE_COLOR_MID, owner),
                    end: constants::EXPLOSION_FIRE_COLOR_END,
                },
                shape: FxShape::Blob,
            },
            (w.x, w.y, cy),
        );
        // Ground shock wave: a flat ring running over the surface the wreck
        // stood on. Ground vehicles only -- a helicopter explodes in the air,
        // where a wave on the ground would point at the wrong spot.
        if w.kind != VehicleKind::Helicopter {
            self.emit(
                &EmitterConfig {
                    amount: 1,
                    lifetime: constants::EXPLOSION_RING_LIFETIME,
                    lifetime_randomness: 0.0,
                    explosiveness: 1.0,
                    initial_velocity: 0.0,
                    initial_velocity_randomness: 0.0,
                    size: constants::EXPLOSION_RING_START_R,
                    size_randomness: 0.0,
                    size_growth: constants::EXPLOSION_RING_END_R,
                    rise: 0.0,
                    gravity: 0.0,
                    alpha: constants::EXPLOSION_RING_ALPHA,
                    fade_power: 1.0,
                    colors: ColorCurve {
                        start: constants::EXPLOSION_SMOKE_COLOR_START,
                        mid: constants::EXPLOSION_FIRE_COLOR_MID,
                        end: constants::EXPLOSION_FIRE_COLOR_END,
                    },
                    shape: FxShape::Ring,
                },
                (w.x, w.y, z + constants::EXPLOSION_RING_LIFT),
            );
        }
        // Sparks: hard embers thrown out of the hull in every direction,
        // arcing down under gravity.
        self.emit(
            &EmitterConfig {
                amount: constants::EXPLOSION_SPARK_COUNT,
                lifetime: constants::EXPLOSION_SPARK_LIFETIME,
                lifetime_randomness: constants::EXPLOSION_SPARK_LIFETIME_RANDOM,
                explosiveness: 1.0,
                initial_velocity: constants::EXPLOSION_SPARK_SPEED,
                initial_velocity_randomness: constants::EXPLOSION_SPARK_SPEED_RANDOM,
                size: constants::EXPLOSION_SPARK_SIZE * scale,
                size_randomness: constants::EXPLOSION_SPARK_SIZE_RANDOM,
                size_growth: -constants::EXPLOSION_SPARK_SIZE * scale,
                rise: 0.0,
                gravity: constants::EXPLOSION_SPARK_GRAVITY,
                alpha: constants::EXPLOSION_SPARK_ALPHA,
                fade_power: 1.6,
                colors: ColorCurve {
                    start: tint(constants::EXPLOSION_SPARK_COLOR_START, owner),
                    mid: tint(constants::EXPLOSION_SPARK_COLOR_MID, owner),
                    end: constants::EXPLOSION_SPARK_COLOR_END,
                },
                shape: FxShape::Shard,
            },
            (w.x, w.y, cy),
        );
        // Smoke: the longest-lived part of the blast, spreading and rising
        // slowly long after the fire is gone.
        self.emit(
            &EmitterConfig {
                amount: constants::EXPLOSION_SMOKE_COUNT,
                lifetime: constants::EXPLOSION_SMOKE_LIFETIME,
                lifetime_randomness: constants::EXPLOSION_SMOKE_LIFETIME_RANDOM,
                explosiveness: 0.25,
                initial_velocity: constants::EXPLOSION_SMOKE_DRIFT,
                initial_velocity_randomness: 1.0,
                size: constants::EXPLOSION_SMOKE_SIZE * scale,
                size_randomness: constants::EXPLOSION_SMOKE_SIZE_RANDOM,
                size_growth: constants::EXPLOSION_SMOKE_GROWTH,
                rise: constants::EXPLOSION_SMOKE_RISE,
                gravity: 0.0,
                alpha: constants::EXPLOSION_SMOKE_ALPHA,
                fade_power: 1.0,
                colors: ColorCurve {
                    start: constants::EXPLOSION_SMOKE_COLOR_START,
                    mid: constants::EXPLOSION_SMOKE_COLOR_MID,
                    end: constants::EXPLOSION_SMOKE_COLOR_END,
                },
                shape: FxShape::Blob,
            },
            (w.x, w.y, cy),
        );
        // Wreck fragments: dark hull pieces flung out of the wreck, the only
        // particles that keep the memory of a vehicle shape in the blast.
        self.emit(
            &EmitterConfig {
                amount: constants::EXPLOSION_DEBRIS_COUNT,
                lifetime: constants::EXPLOSION_DEBRIS_LIFETIME,
                lifetime_randomness: constants::EXPLOSION_DEBRIS_LIFETIME_RANDOM,
                explosiveness: 1.0,
                initial_velocity: constants::EXPLOSION_DEBRIS_SPEED,
                initial_velocity_randomness: constants::EXPLOSION_DEBRIS_SPEED_RANDOM,
                size: constants::EXPLOSION_DEBRIS_SIZE * scale,
                size_randomness: constants::EXPLOSION_DEBRIS_SIZE_RANDOM,
                size_growth: 0.0,
                rise: 0.0,
                gravity: constants::EXPLOSION_DEBRIS_GRAVITY,
                alpha: constants::EXPLOSION_DEBRIS_ALPHA,
                fade_power: 2.0,
                colors: ColorCurve {
                    start: constants::EXPLOSION_DEBRIS_COLOR,
                    mid: constants::EXPLOSION_DEBRIS_COLOR,
                    end: constants::EXPLOSION_DEBRIS_COLOR,
                },
                shape: FxShape::Shard,
            },
            (w.x, w.y, cy),
        );
        self.trim();
    }
    /// Keep a living flame burning on every fire trap; called once per frame.
    ///
    /// `fires` holds the campfire spots of every burning field: the
    /// [`campfire_cluster`] of each trap, already lifted to its tile top
    /// ([`crate::app`] collects them from the board). rules.md section 4 makes
    /// a fire trap permanent and land only and says nothing about how it looks;
    /// drawing it as a static model made it read as a plastic prop, so the
    /// trap -- really a cluster of small campfires -- is a continuous emitter
    /// here: flames, a few sparks and a wisp of smoke. There is no base mesh at
    /// all; the fire *is* the trap.
    ///
    /// Emission is rate-based: every spot emits
    /// `CAMPFIRE_*_RATE * scale` particles per second, with a fractional
    /// accumulator per kind, so the standing flame looks the same at any frame
    /// rate. The flame draws from the campfire stream, which keeps the
    /// explosion stream reproducible. A field that disappears (the editor
    /// removed the trap) simply stops being fed and its particles burn out on
    /// their own.
    pub fn maintain_campfires(&mut self, fires: &[Campfire], dt: f64) {
        if fires.is_empty() || dt <= 0.0 {
            return;
        }
        let weight: f64 = fires.iter().map(|f| f.scale).sum();
        self.fire_acc[0] += constants::CAMPFIRE_FLAME_RATE * weight * dt;
        self.fire_acc[1] += constants::CAMPFIRE_SPARK_RATE * weight * dt;
        self.fire_acc[2] += constants::CAMPFIRE_SMOKE_RATE * weight * dt;
        let flame = self.fire_acc[0].floor();
        self.fire_acc[0] -= flame;
        let spark = self.fire_acc[1].floor();
        self.fire_acc[1] -= spark;
        let smoke = self.fire_acc[2].floor();
        self.fire_acc[2] -= smoke;
        for _ in 0..flame as usize {
            let f = self.pick_campfire(fires);
            let cfg = Self::campfire_flame(f.scale);
            self.emit_tagged(&cfg, (f.x, f.y, f.z), true);
        }
        for _ in 0..spark as usize {
            let f = self.pick_campfire(fires);
            let cfg = Self::campfire_spark(f.scale);
            self.emit_tagged(&cfg, (f.x, f.y, f.z), true);
        }
        for _ in 0..smoke as usize {
            let f = self.pick_campfire(fires);
            let cfg = Self::campfire_smoke(f.scale);
            let lifted = (f.x, f.y, f.z + constants::CAMPFIRE_SMOKE_LIFT);
            self.emit_tagged(&cfg, lifted, true);
        }
        self.trim();
    }

    /// Pick one burning spot, with a probability proportional to its `scale`
    /// (the campfire stream): a bigger fire gets more particles *and* bigger
    /// puffs, so it reads as the main fire of the cluster.
    fn pick_campfire(&mut self, fires: &[Campfire]) -> Campfire {
        let total: f64 = fires.iter().map(|f| f.scale).sum();
        let mut lot = self.fire_rng.next_f64() * total.max(f64::MIN_POSITIVE);
        for f in fires {
            lot -= f.scale;
            if lot <= 0.0 {
                return *f;
            }
        }
        fires[fires.len() - 1]
    }

    /// Emitter config of one campfire flame puff of a `scale`-sized fire.
    fn campfire_flame(scale: f64) -> EmitterConfig {
        EmitterConfig {
            amount: 1,
            lifetime: constants::CAMPFIRE_FLAME_LIFETIME,
            lifetime_randomness: constants::CAMPFIRE_FLAME_LIFETIME_RANDOM,
            explosiveness: 1.0,
            initial_velocity: constants::CAMPFIRE_FLAME_DRIFT,
            initial_velocity_randomness: 1.0,
            size: constants::CAMPFIRE_FLAME_SIZE * scale,
            size_randomness: constants::CAMPFIRE_FLAME_SIZE_RANDOM,
            size_growth: constants::CAMPFIRE_FLAME_GROWTH * scale,
            rise: constants::CAMPFIRE_FLAME_RISE,
            gravity: 0.0,
            alpha: constants::CAMPFIRE_FLAME_ALPHA,
            fade_power: 1.1,
            colors: ColorCurve {
                start: constants::CAMPFIRE_FLAME_COLOR_START,
                mid: constants::CAMPFIRE_FLAME_COLOR_MID,
                end: constants::CAMPFIRE_FLAME_COLOR_END,
            },
            shape: FxShape::Blob,
        }
    }

    /// Emitter config of one campfire spark of a `scale`-sized fire.
    fn campfire_spark(scale: f64) -> EmitterConfig {
        EmitterConfig {
            amount: 1,
            lifetime: constants::CAMPFIRE_SPARK_LIFETIME,
            lifetime_randomness: constants::CAMPFIRE_SPARK_LIFETIME_RANDOM,
            explosiveness: 1.0,
            initial_velocity: constants::CAMPFIRE_SPARK_SPEED,
            initial_velocity_randomness: constants::CAMPFIRE_SPARK_SPEED_RANDOM,
            size: constants::CAMPFIRE_SPARK_SIZE * scale,
            size_randomness: constants::CAMPFIRE_SPARK_SIZE_RANDOM,
            size_growth: -constants::CAMPFIRE_SPARK_SIZE * scale,
            rise: 0.0,
            gravity: constants::CAMPFIRE_SPARK_GRAVITY,
            alpha: constants::CAMPFIRE_SPARK_ALPHA,
            fade_power: 1.6,
            colors: ColorCurve {
                start: constants::CAMPFIRE_SPARK_COLOR_START,
                mid: constants::CAMPFIRE_SPARK_COLOR_MID,
                end: constants::CAMPFIRE_SPARK_COLOR_END,
            },
            shape: FxShape::Shard,
        }
    }

    /// Emitter config of one campfire smoke puff of a `scale`-sized fire.
    fn campfire_smoke(scale: f64) -> EmitterConfig {
        EmitterConfig {
            amount: 1,
            lifetime: constants::CAMPFIRE_SMOKE_LIFETIME,
            lifetime_randomness: constants::CAMPFIRE_SMOKE_LIFETIME_RANDOM,
            explosiveness: 1.0,
            initial_velocity: constants::CAMPFIRE_SMOKE_DRIFT,
            initial_velocity_randomness: 1.0,
            size: constants::CAMPFIRE_SMOKE_SIZE * scale,
            size_randomness: constants::CAMPFIRE_SMOKE_SIZE_RANDOM,
            size_growth: constants::CAMPFIRE_SMOKE_GROWTH * scale,
            rise: constants::CAMPFIRE_SMOKE_RISE,
            gravity: 0.0,
            alpha: constants::CAMPFIRE_SMOKE_ALPHA,
            fade_power: 1.3,
            colors: ColorCurve {
                start: constants::CAMPFIRE_SMOKE_COLOR_START,
                mid: constants::CAMPFIRE_SMOKE_COLOR_MID,
                end: constants::CAMPFIRE_SMOKE_COLOR_END,
            },
            shape: FxShape::Blob,
        }
    }

    /// Spawn one burst of `cfg` at `(x, y, z)` from the explosion stream.
    fn emit(&mut self, cfg: &EmitterConfig, at: (f64, f64, f64)) {
        self.emit_tagged(cfg, at, false);
    }

    /// Spawn one burst of `cfg` at `(x, y, z)`, drawing every random value from
    /// the explosion stream (or, for a campfire, from the campfire stream) and
    /// tagging the particles so [`Fx::trim`] keeps the two budgets apart.
    ///
    /// Directions are uniform over the full circle (an isotropic burst, as in
    /// the `initial_direction_spread: 2 * PI` of `macroquad-particles`), and
    /// `explosiveness` decides how much of the burst is delayed: the share
    /// `1 - explosiveness` of a particle's lifetime becomes its spawn delay, so
    /// a high value fires everything at once and a low value trickles it out.
    fn emit_tagged(&mut self, cfg: &EmitterConfig, at: (f64, f64, f64), campfire: bool) {
        let rng = if campfire {
            &mut self.fire_rng
        } else {
            &mut self.rng
        };
        for _ in 0..cfg.amount {
            let life = (cfg.lifetime * (1.0 + rng.next_f64() * cfg.lifetime_randomness))
                .max(constants::FX_MIN_LIFETIME);
            let speed = cfg.initial_velocity
                * (1.0 + (rng.next_f64() * 2.0 - 1.0) * cfg.initial_velocity_randomness);
            let angle = rng.next_f64() * std::f64::consts::TAU;
            let size =
                (cfg.size * (1.0 + (rng.next_f64() * 2.0 - 1.0) * cfg.size_randomness)).max(0.0);
            // Shards spin, so sparks and fragments never read as a grid.
            let spin = if cfg.shape == FxShape::Shard {
                (rng.next_f64() * 2.0 - 1.0) * constants::FX_SHARD_SPIN
            } else {
                0.0
            };
            // `explosiveness` decides how much of the burst is delayed: the
            // share `1 - explosiveness` of a particle's lifetime becomes its
            // spawn delay, so a high value fires everything at once and a low
            // value trickles it out. The delay is part of `life` (the semantics
            // of `lifetime` in `macroquad-particles`), so a delayed particle
            // is visible for `life - delay` seconds.
            let delay = ((1.0 - cfg.explosiveness) * life * rng.next_f64()).max(0.0);
            self.particles.push(Particle {
                x: at.0,
                y: at.1,
                z: at.2,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                vz: constants::FX_BURST_UP + rng.next_f64() * constants::FX_BURST_UP_RANDOM,
                rise: cfg.rise,
                gravity: cfg.gravity,
                age: 0.0,
                delay,
                life,
                size,
                // The size change plays out over the *visible* part of the
                // lifetime, so a late particle still ends at the target size.
                growth: cfg.size_growth / (life - delay).max(constants::FX_MIN_LIFETIME),
                alpha: cfg.alpha,
                fade_power: cfg.fade_power,
                colors: cfg.colors,
                shape: cfg.shape,
                angle: rng.next_f64() * std::f64::consts::TAU,
                spin,
                campfire,
            });
        }
    }

    /// Keep each particle budget within its bound.
    ///
    /// The explosions and the campfires are capped separately
    /// ([`constants::EXPLOSION_MAX_PARTICLES`] and
    /// [`constants::CAMPFIRE_MAX_PARTICLES`]) so that a map full of burning
    /// fields can never starve the explosions, and a huge battle can never
    /// snuff out the fires. The oldest particles of a kind go first: dropping
    /// the tail of a purely decorative effect costs nothing.
    fn trim(&mut self) {
        let campfire = self.particles.iter().filter(|p| p.campfire).count();
        let explosion = self.particles.len() - campfire;
        let drop_campfire = campfire.saturating_sub(constants::CAMPFIRE_MAX_PARTICLES);
        let drop_explosion = explosion.saturating_sub(constants::EXPLOSION_MAX_PARTICLES);
        if drop_campfire == 0 && drop_explosion == 0 {
            return;
        }
        let (mut seen_campfire, mut seen_explosion) = (0usize, 0usize);
        self.particles.retain(|p| {
            if p.campfire {
                seen_campfire += 1;
                seen_campfire > drop_campfire
            } else {
                seen_explosion += 1;
                seen_explosion > drop_explosion
            }
        });
    }

    /// Advance every particle by `dt` seconds and drop the expired ones.
    ///
    /// `dt` is the frame delta, exactly like the rotor phase in
    /// [`crate::app`]: the effect is presentation, so it is not stepped in the
    /// fixed simulation tick and never touches the game state. Particles that
    /// are still waiting for their spawn delay stay at the origin.
    pub fn update(&mut self, dt: f64) {
        for p in self.particles.iter_mut() {
            p.age += dt;
            if p.is_delayed() {
                continue;
            }
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.z += p.vz * dt;
            p.vz += (p.rise - p.gravity) * dt;
            p.angle += p.spin * dt;
            p.size = (p.size + p.growth * dt).max(0.0);
        }
        self.particles.retain(|p| p.age < p.life);
    }

    /// Build the mesh geometry of every live particle into `out.fx`.
    ///
    /// The particles are submitted **far to near** (ascending depth `D`),
    /// since they are translucent and drawn without a depth write: back to
    /// front is the only order in which a cloud of puffs blends correctly.
    /// Depth testing still trims every puff against the opaque terrain, so a
    /// blast behind a hill is hidden by that hill.
    pub fn build(&self, out: &mut DynamicMesh) {
        if self.is_empty() {
            return;
        }
        let mut order: Vec<usize> = (0..self.len()).collect();
        // Ascending `D`: the smallest depth is the farthest from the camera.
        order.sort_by(|&a, &b| {
            depth(&self.particles[a])
                .partial_cmp(&depth(&self.particles[b]))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for i in order {
            let p = &self.particles[i];
            if p.is_delayed() {
                // Not born yet: nothing to draw.
                continue;
            }
            let color = p.color();
            let alpha = (p.opacity() * 255.0).round().clamp(0.0, 255.0) as u8;
            if alpha == 0 {
                continue;
            }
            match p.shape {
                FxShape::Blob => {
                    // The soft rim is drawn with a much lower alpha, which is
                    // what turns the flat quad into a round puff.
                    crate::mesh::push_fx_blob(
                        &mut out.fx,
                        p.x,
                        p.y,
                        p.z,
                        p.size,
                        [color[0], color[1], color[2], alpha],
                        [color[0], color[1], color[2], (alpha as f64 * 0.18) as u8],
                    );
                }
                FxShape::Shard => {
                    crate::mesh::push_fx_shard(
                        &mut out.fx,
                        p.x,
                        p.y,
                        p.z,
                        p.size,
                        [color[0], color[1], color[2], alpha],
                        p.angle,
                    );
                }
                FxShape::Ring => {
                    // The wave spreads from the start radius to the end one and
                    // fades out; the inner rim stays transparent so the ring
                    // does not fill in with a disc.
                    let t = p.age_ratio();
                    let outer = p.size;
                    let inner = p.size * constants::EXPLOSION_RING_INNER_FRAC;
                    let rim = (alpha as f64 * (0.35 + 0.65 * t)) as u8;
                    crate::mesh::push_fx_ring(
                        &mut out.fx,
                        p.x,
                        p.y,
                        p.z,
                        inner,
                        outer,
                        color,
                        0,
                        rim,
                    );
                }
            }
        }
    }
}

/// Depth coordinate `D` of a particle (larger = nearer the camera), the same
/// value the GPU camera of [`crate::iso`] sorts by.
fn depth(p: &Particle) -> f64 {
    (p.x + p.y) * constants::ISO_SIN + p.z
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::DynamicMesh;

    fn wreck(kind: VehicleKind) -> Wreck {
        Wreck {
            owner: 0,
            kind,
            x: 1234.0,
            y: -567.0,
        }
    }

    #[test]
    fn explode_spawns_every_part_of_the_blast() {
        let mut fx = Fx::new();
        assert!(fx.is_empty());
        fx.explode(&wreck(VehicleKind::Tank), 40.0);
        // Flash, fireball, ground wave, sparks, smoke and debris.
        let blobs = fx
            .particles
            .iter()
            .filter(|p| p.shape == FxShape::Blob)
            .count();
        let shards = fx
            .particles
            .iter()
            .filter(|p| p.shape == FxShape::Shard)
            .count();
        let rings = fx
            .particles
            .iter()
            .filter(|p| p.shape == FxShape::Ring)
            .count();
        assert!(blobs > 0, "no fire or smoke");
        assert!(shards > 0, "no sparks or debris");
        assert_eq!(rings, 1, "a ground vehicle explodes with one wave");
        assert_eq!(
            fx.len(),
            1 + constants::EXPLOSION_FIREBALL_COUNT
                + 1
                + constants::EXPLOSION_SPARK_COUNT
                + constants::EXPLOSION_SMOKE_COUNT
                + constants::EXPLOSION_DEBRIS_COUNT
        );
    }

    #[test]
    fn helicopter_explodes_in_the_air_without_a_ground_wave() {
        let mut fx = Fx::new();
        fx.explode(&wreck(VehicleKind::Helicopter), 200.0);
        assert!(
            !fx.particles.iter().any(|p| p.shape == FxShape::Ring),
            "a helicopter dies in the air, there is no wave to run over the ground"
        );
    }

    #[test]
    fn a_real_duel_produces_an_explosion_where_the_wreck_died() {
        // End-to-end check of the whole chain: the simulation reports the
        // wreck, the effect system turns it into particles, and those become
        // real geometry. Without this, a rename or a forgotten call in `app.rs`
        // would leave the feature silently dead.
        use crate::board::Board;
        use crate::entities::{Building, BuildingKind, Player};
        use crate::game::Game;

        let mut board = Board::new(20, 12);
        for tile in board.tiles.clone().keys().copied().collect::<Vec<_>>() {
            board.tiles.get_mut(&tile).unwrap().height = 1;
        }
        let buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(0), 1, 5, 10.0),
            Building::new(BuildingKind::BaseTank, Some(1), 12, 5, 10.0),
        ];
        let mut game = Game::new(
            board,
            vec![Player::new(0, true), Player::new(1, false)],
            buildings,
            Vec::new(),
            0,
        );
        game.building_at = [(game.buildings[0].tile, 0), (game.buildings[1].tile, 1)]
            .into_iter()
            .collect();
        assert!(game.try_send(0, (1, 5), (12, 5)));
        assert!(game.try_send(1, (12, 5), (1, 5)));
        for _ in 0..(30.0 / crate::constants::SIM_DT) as usize {
            game.update(crate::constants::SIM_DT);
        }
        let wrecks = game.take_wrecks();
        assert_eq!(wrecks.len(), 1, "the duel must end in one wreck");

        let mut fx = Fx::new();
        for w in wrecks.iter() {
            let z = crate::mesh::wreck_z(&game, w);
            fx.explode(w, z);
        }
        assert!(!fx.is_empty());
        let mut mesh = DynamicMesh::default();
        fx.build(&mut mesh);
        assert!(mesh.fx.vertices.len() > 100, "the blast has no geometry");

        // Every particle sits in the wreck's neighbourhood, and none of them
        // is buried under the field the wreck died on: the depth `D` the camera
        // sorts by stays at or above the depth of the surface right below it.
        // (Individual *vertices* of a puff legitimately dip below that -- a
        // billboard is a vertical quad in world space, so its lower edge
        // reaches down while moving forward, which is exactly how the depth
        // test is meant to work.)
        let w = wrecks[0];
        for p in fx.particles.iter() {
            assert!(
                (p.x - w.x).hypot(p.y - w.y) < 200.0,
                "particle far away from the wreck: {}, {}",
                p.x,
                p.y
            );
            let surface = crate::mesh::surface::vehicle_ground_z(&game, p.x, p.y);
            let d_particle = (p.x + p.y) * crate::constants::ISO_SIN + p.z;
            let d_surface = (p.x + p.y) * crate::constants::ISO_SIN + surface;
            assert!(
                d_particle >= d_surface,
                "particle buried under the field: {d_particle} < {d_surface}"
            );
        }
    }

    /// The full particle state of an explosion, compared field by field.
    fn signature(fx: &Fx) -> Vec<(f64, f64, f64, f64, f64, f64)> {
        fx.particles
            .iter()
            .map(|p| (p.x, p.y, p.z, p.vx, p.vy, p.vz))
            .collect()
    }

    #[test]
    fn every_explosion_looks_different() {
        // The whole point of the stateful stream: no two wrecks share a blast,
        // not even two explosions of the very same vehicle.
        let mut fx = Fx::new();
        fx.explode(&wreck(VehicleKind::Tank), 10.0);
        let first = signature(&fx);
        fx.particles.clear();
        fx.explode(&wreck(VehicleKind::Tank), 10.0);
        let second = signature(&fx);
        assert_eq!(first.len(), second.len(), "the burst size must be stable");
        assert_ne!(first, second, "the same wreck exploded identically twice");

        // A different vehicle of the same kind, exploded next in the same
        // system, differs as well -- the stream keeps moving.
        fx.particles.clear();
        fx.explode(&wreck(VehicleKind::Tank), 10.0);
        assert_ne!(
            second,
            signature(&fx),
            "two different wrecks exploded identically"
        );
    }

    #[test]
    fn a_level_seed_makes_the_whole_sequence_reproducible() {
        // Same seed, same order of explosions -> exactly the same effects, so
        // a level can still be replayed (and the effects unit-tested) even
        // though every single explosion is random.
        let play = |seed: u64| {
            let mut fx = Fx::new();
            fx.reseed(seed);
            for (i, w) in [
                wreck(VehicleKind::Tank),
                wreck(VehicleKind::Tank),
                wreck(VehicleKind::Helicopter),
            ]
            .iter()
            .enumerate()
            {
                fx.explode(w, 10.0 * (i as f64 + 1.0));
                fx.update(0.2);
            }
            signature(&fx)
        };
        assert_eq!(play(7), play(7), "the same seed must replay identically");
        assert_ne!(play(7), play(8), "another seed must give other effects");
    }

    #[test]
    fn reseed_clears_live_particles_and_restarts_the_stream() {
        let mut fx = Fx::new();
        fx.explode(&wreck(VehicleKind::Tank), 0.0);
        assert!(!fx.is_empty());
        fx.reseed(123);
        assert!(fx.is_empty(), "a new level must not inherit old particles");
        let mut fresh = Fx::new();
        fresh.reseed(123);
        fresh.explode(&wreck(VehicleKind::Tank), 0.0);
        fx.explode(&wreck(VehicleKind::Tank), 0.0);
        assert_eq!(signature(&fx), signature(&fresh));
    }

    #[test]
    fn particles_die_and_the_system_empties_itself() {
        let mut fx = Fx::new();
        fx.explode(&wreck(VehicleKind::Tank), 0.0);
        let longest = fx.particles.iter().map(|p| p.life).fold(0.0_f64, f64::max);
        // Far more than the smoke needs: nothing may survive.
        fx.update(longest + constants::SIM_DT);
        assert!(fx.is_empty(), "{} particles outlived the blast", fx.len());
        // `update` on an empty system stays a no-op.
        fx.update(1.0);
        assert!(fx.is_empty());
    }

    #[test]
    fn delayed_particles_appear_later_and_fade_afterwards() {
        let mut fx = Fx::new();
        // A single, fully delayed particle (`explosiveness: 0` spreads it over
        // its whole lifetime). The stream is seeded so that the delay is
        // comfortably long, which keeps the birth window easy to hit.
        fx.reseed(4);
        fx.emit(
            &EmitterConfig {
                amount: 1,
                lifetime: 1.0,
                lifetime_randomness: 0.0,
                explosiveness: 0.0,
                initial_velocity: 0.0,
                initial_velocity_randomness: 0.0,
                size: 5.0,
                size_randomness: 0.0,
                size_growth: 0.0,
                rise: 40.0,
                gravity: 0.0,
                alpha: 1.0,
                fade_power: 1.0,
                colors: ColorCurve {
                    start: [255, 255, 255],
                    mid: [255, 255, 255],
                    end: [255, 255, 255],
                },
                shape: FxShape::Blob,
            },
            (100.0, 200.0, 0.0),
        );
        let delay = fx.particles[0].delay;
        assert!(delay > 0.0, "the particle must start out delayed");
        // Inside the spawn delay it sits at the origin and is not drawn.
        fx.update(delay * 0.5);
        assert!(fx.particles[0].is_delayed(), "the particle is not born yet");
        assert_eq!(fx.particles[0].z, 0.0, "a delayed particle does not move");
        let mut mesh = DynamicMesh::default();
        fx.build(&mut mesh);
        assert!(
            mesh.fx.vertices.is_empty(),
            "a particle that has not been born is not drawn"
        );
        // Born now: it rises and its opacity starts to fall.
        fx.update(delay * 0.75);
        let p = &fx.particles[0];
        assert!(!p.is_delayed(), "the particle must be born by now");
        assert!(p.z > 0.0, "a born particle has left the origin");
        assert!(p.opacity() < 1.0, "opacity has to fade out");
    }

    #[test]
    fn smoke_rises_sparks_fall() {
        let mut fx = Fx::new();
        fx.explode(&wreck(VehicleKind::Tank), 0.0);
        // Highest vertical speed of the sparks / of the smoke right now. The
        // sparks are picked by their gravity: the flash and the debris are no
        // sparks even though they are shards, and the fire drifts upwards.
        let top_vz = |fx: &Fx, smoke: bool| {
            fx.particles
                .iter()
                .filter(|p| {
                    if smoke {
                        p.colors.start == constants::EXPLOSION_SMOKE_COLOR_START
                    } else {
                        p.gravity == constants::EXPLOSION_SPARK_GRAVITY
                    }
                })
                .map(|p| p.vz)
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let (spark_before, smoke_before) = (top_vz(&fx, false), top_vz(&fx, true));
        fx.update(0.5);
        // Gravity bends the sparks downwards, while the fire and the smoke are
        // pushed up by their `rise` -- the two must clearly diverge.
        assert!(top_vz(&fx, false) < spark_before, "sparks must fall");
        assert!(top_vz(&fx, true) > smoke_before, "smoke must keep rising");
        assert!(
            top_vz(&fx, true) > top_vz(&fx, false),
            "smoke is above sparks"
        );
        assert!(
            fx.particles
                .iter()
                .any(|p| p.colors.start == constants::EXPLOSION_SMOKE_COLOR_START && p.z > 0.0),
            "smoke must leave the ground"
        );
    }

    #[test]
    fn build_writes_particles_and_is_idempotent_between_frames() {
        let mut fx = Fx::new();
        fx.explode(&wreck(VehicleKind::Tank), 0.0);
        fx.update(0.05);
        let mut mesh = DynamicMesh::default();
        fx.build(&mut mesh);
        assert!(!mesh.fx.vertices.is_empty(), "no particle geometry built");
        assert!(mesh.fx.vertices.iter().any(|v| v.color[3] > 0));
        // Rebuilding the same frame must not accumulate geometry: `build`
        // appends to a buffer that `DynamicMesh::clear` emptied first.
        let count = mesh.fx.vertices.len();
        mesh.clear();
        fx.build(&mut mesh);
        assert_eq!(mesh.fx.vertices.len(), count);
        // A fresh system stays empty, so an idle frame costs nothing.
        let mut clean = DynamicMesh::default();
        Fx::new().build(&mut clean);
        assert!(clean.fx.vertices.is_empty());
    }

    #[test]
    fn particles_are_drawn_far_to_near() {
        // Translucent geometry without a depth write has to be submitted back
        // to front, otherwise overlapping puffs blend in the wrong order.
        let mut fx = Fx::new();
        let mut far = wreck(VehicleKind::Tank);
        far.x = 0.0;
        far.y = 0.0;
        let mut near = wreck(VehicleKind::Tank);
        near.x = 4000.0;
        near.y = 4000.0;
        fx.explode(&far, 0.0);
        fx.explode(&near, 0.0);
        let mut mesh = DynamicMesh::default();
        fx.build(&mut mesh);
        // The first vertex emitted must belong to the farther explosion, i.e.
        // the one with the smaller `D = (x + y) * ISO_SIN + z`.
        let first_d = f64::from(mesh.fx.vertices[0].x) + f64::from(mesh.fx.vertices[0].y);
        let last_d = f64::from(mesh.fx.vertices[mesh.fx.vertices.len() - 1].x)
            + f64::from(mesh.fx.vertices[mesh.fx.vertices.len() - 1].y);
        assert!(
            first_d < last_d,
            "particles must be submitted from far ({first_d}) to near ({last_d})"
        );
    }

    #[test]
    fn particles_never_exceed_the_budget() {
        let mut fx = Fx::new();
        for _ in 0..200 {
            fx.explode(&wreck(VehicleKind::Tank), 0.0);
        }
        assert!(
            fx.len() <= constants::EXPLOSION_MAX_PARTICLES,
            "{} particles above the budget of {}",
            fx.len(),
            constants::EXPLOSION_MAX_PARTICLES
        );
    }

    #[test]
    fn color_curve_hits_every_stop() {
        let c = ColorCurve {
            start: [0, 0, 0],
            mid: [100, 200, 50],
            end: [255, 255, 255],
        };
        assert_eq!(c.sample(0.0), [0, 0, 0]);
        assert_eq!(c.sample(0.5), [100, 200, 50]);
        assert_eq!(c.sample(1.0), [255, 255, 255]);
        // Out-of-range input is clamped, never wraps around.
        assert_eq!(c.sample(-5.0), [0, 0, 0]);
        assert_eq!(c.sample(5.0), [255, 255, 255]);
    }

    /// Campfire spots of one burning field for the tests: the
    /// [`campfire_cluster`] of a single tile, every spot at the same elevation.
    fn burning_field() -> Vec<Campfire> {
        campfire_cluster(500.0, -200.0, 12.0, 3, 4)
    }

    #[test]
    fn campfires_burn_into_a_steady_flame() {
        let mut fx = Fx::new();
        // Two seconds of frames at a typical rate: the flame has to reach a
        // standing size and then stop growing (the emitters replace what burns
        // out).
        let field = burning_field();
        for _ in 0..120 {
            fx.maintain_campfires(&field, 1.0 / 60.0);
            fx.update(1.0 / 60.0);
        }
        assert!(!fx.is_empty(), "a fire trap with no flame");
        assert!(
            fx.particles.iter().all(|p| p.campfire),
            "only campfire particles may come from maintain_campfires"
        );
        // Flame, sparks and smoke are all there.
        assert!(
            fx.particles
                .iter()
                .any(|p| p.colors.start == constants::CAMPFIRE_FLAME_COLOR_START)
        );
        assert!(fx.particles.iter().any(|p| p.shape == FxShape::Shard));
        assert!(
            fx.particles
                .iter()
                .any(|p| p.colors.start == constants::CAMPFIRE_SMOKE_COLOR_START)
        );
        // The standing flame stays in a sane band, not creeping up frame after
        // frame.
        let settled = fx.len();
        for _ in 0..120 {
            fx.maintain_campfires(&field, 1.0 / 60.0);
            fx.update(1.0 / 60.0);
        }
        assert!(
            (fx.len() as f64 - settled as f64).abs() < 20.0,
            "the flame is not steady: {settled} -> {}",
            fx.len()
        );
        assert!(fx.len() < constants::CAMPFIRE_MAX_PARTICLES);
    }

    #[test]
    fn campfire_burns_out_when_its_field_disappears() {
        let mut fx = Fx::new();
        let field = burning_field();
        for _ in 0..120 {
            fx.maintain_campfires(&field, 1.0 / 60.0);
            fx.update(1.0 / 60.0);
        }
        assert!(!fx.is_empty());
        // The trap is gone (the editor removed it): no more feeding, only decay.
        for _ in 0..300 {
            fx.maintain_campfires(&[], 1.0 / 60.0);
            fx.update(1.0 / 60.0);
        }
        assert!(fx.is_empty(), "{} particles outlived the fire", fx.len());
    }

    #[test]
    fn campfire_geometry_is_built() {
        let mut fx = Fx::new();
        let field = burning_field();
        for _ in 0..60 {
            fx.maintain_campfires(&field, 1.0 / 60.0);
            fx.update(1.0 / 60.0);
        }
        let mut mesh = DynamicMesh::default();
        fx.build(&mut mesh);
        assert!(!mesh.fx.vertices.is_empty(), "the fire has no geometry");
    }

    #[test]
    fn campfires_and_explosions_keep_separate_budgets() {
        let mut fx = Fx::new();
        // Flood the system with both effects at once.
        let field = burning_field();
        for _ in 0..200 {
            fx.explode(&wreck(VehicleKind::Tank), 0.0);
            fx.maintain_campfires(&field, 0.25);
        }
        let campfire = fx.particles.iter().filter(|p| p.campfire).count();
        let explosion = fx.len() - campfire;
        assert!(campfire <= constants::CAMPFIRE_MAX_PARTICLES);
        assert_eq!(
            explosion,
            constants::EXPLOSION_MAX_PARTICLES,
            "the fires starved the explosions"
        );
    }

    #[test]
    fn campfires_leave_the_explosion_stream_untouched() {
        // Campfires draw from their own stream, so a scene that has been
        // burning for a while still explodes exactly like a fresh one: the
        // replayability promised by `reseed` survives the fire.
        let play = |fires: bool| {
            let mut fx = Fx::new();
            fx.reseed(99);
            if fires {
                let field = burning_field();
                for _ in 0..60 {
                    fx.maintain_campfires(&field, 1.0 / 60.0);
                    fx.update(1.0 / 60.0);
                }
            }
            fx.explode(&wreck(VehicleKind::Tank), 10.0);
            fx.particles
                .iter()
                .filter(|p| !p.campfire)
                .map(|p| (p.x, p.y, p.z, p.vx, p.vy, p.vz))
                .collect::<Vec<_>>()
        };
        assert_eq!(play(true), play(false));
    }

    #[test]
    fn campfire_cluster_is_even_and_stays_inside_its_field() {
        let spots = campfire_cluster(0.0, 0.0, 0.0, 3, 4);
        assert_eq!(
            spots.len(),
            constants::CAMPFIRE_SPOTS.len(),
            "the cluster must hold one spot per template offset"
        );
        for s in spots.iter() {
            let radius = (s.x * s.x + s.y * s.y).sqrt();
            assert!(
                radius <= 20.0 + 1e-6,
                "spot {radius} px off centre spills out of the field"
            );
            // The size falls off with the distance from the centre, so the
            // middle of the field burns the biggest.
            let expected = constants::CAMPFIRE_SCALE_CENTER
                + (constants::CAMPFIRE_SCALE_EDGE - constants::CAMPFIRE_SCALE_CENTER)
                    * (radius / constants::CAMPFIRE_SCALE_REF_R).clamp(0.0, 1.0);
            assert!(
                (s.scale - expected).abs() < 1e-9,
                "spot at {radius} px scales {s_scale}, expected {expected}",
                s_scale = s.scale
            );
        }
        let centre = spots
            .iter()
            .min_by(|a, b| {
                (a.x * a.x + a.y * a.y)
                    .partial_cmp(&(b.x * b.x + b.y * b.y))
                    .unwrap()
            })
            .unwrap();
        assert!(
            spots.iter().all(|s| s.scale <= centre.scale + 1e-9),
            "a fire further from the centre must not burn bigger"
        );
    }

    #[test]
    fn campfire_clusters_are_stable_but_differ_between_tiles() {
        let again = campfire_cluster(0.0, 0.0, 0.0, 4, 4);
        let same = campfire_cluster(0.0, 0.0, 0.0, 4, 4);
        for (a, b) in again.iter().zip(same.iter()) {
            assert_eq!((a.x, a.y, a.scale), (b.x, b.y, b.scale));
        }
        let other = campfire_cluster(0.0, 0.0, 0.0, 5, 4);
        assert!(
            again
                .iter()
                .zip(other.iter())
                .any(|(a, b)| (a.x, a.y) != (b.x, b.y)),
            "neighbouring fields must not burn in lockstep"
        );
    }

    #[test]
    fn a_bigger_fire_burns_bigger_puffs() {
        // A fresh scale-1.0 spot, fed a full second without ageing a frame, so
        // the accumulated puffs are all at birth size (the base size, not yet
        // shrunk by overlap with older generations).
        let biggest = |scale: f64| {
            let mut fx = Fx::new();
            fx.maintain_campfires(
                &[Campfire {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    scale,
                }],
                1.0,
            );
            fx.particles
                .iter()
                .filter(|p| p.colors.start == constants::CAMPFIRE_FLAME_COLOR_START)
                .map(|p| p.size)
                .fold(0.0_f64, f64::max)
        };
        assert_eq!(biggest(1.0), biggest(1.0), "same scale burns the same");
        assert!(
            biggest(1.0) > biggest(constants::CAMPFIRE_SCALE_EDGE),
            "the centre of the field must burn bigger than its edge"
        );
    }
}
