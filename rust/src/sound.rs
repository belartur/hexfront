//! Procedural sound synthesis (no macroquad, no audio files).
//!
//! The game ships no sample files: every sound is generated here as a
//! 16-bit PCM WAV byte stream, exactly like every mesh is built in code
//! (`mesh.rs`) and every explosion particle is computed (`fx.rs`). It keeps
//! the repository free of binary assets, keeps the sounds tunable through
//! constants in `constants.rs`, and -- because this module does not depend on
//! macroquad -- makes the whole synthesis testable headless, with no window
//! and no sound card.
//!
//! The simulation knows nothing about sound. It reports *what happened*
//! ([`crate::entities::SoundEvent`], drained from
//! [`crate::game::Game::take_sounds`]) and this module decides what that
//! sounds like, in the same spirit as `Wreck` -> [`crate::fx`] for the
//! explosion particles.
//!
//! Every sound is built from three primitives layered on top of each other:
//!
//! * a **tone**: a sine whose frequency slides from a start value to an end
//!   value over the length of the sound (a falling sweep is a shot, a rising
//!   one is a rocket motor),
//! * a **noise burst**: white noise put through a one-pole low-pass whose
//!   cutoff slides the same way, which is what gives an explosion its
//!   "whoomph" and a shot its "crack",
//! * an **exponential decay** envelope on both, so nothing clicks on or off.
//!
//! Noise comes from [`crate::rng::Rng`], seeded per sound kind, so the same
//! sound is byte-identical on every run -- no crackling from uninitialised
//! noise, and the unit tests can compare the exact output.
//!
//! The output format is WAV because that is what macroquad's audio backend
//! decodes ([`crate::audio`]); it is the only place that knows it.

use crate::constants;
use crate::rng::Rng;

/// Every distinct sound the game can make.
///
/// A sound kind is pure data about *what* happened; where it happened and how
/// loud it is decided later, by [`crate::audio`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SoundKind {
    /// A vehicle was destroyed on the ground (rules.md section 9).
    ExplosionGround,
    /// A helicopter was destroyed, in the air (rules.md section 5.2).
    ExplosionAir,
    /// A turret fired: the kind is the turret kind (rules.md section 10).
    TurretShot(crate::constants::TurretKind),
    /// A vehicle shot at an opponent in a duel (rules.md section 9).
    VehicleFire,
    /// A vehicle shot at a wall it is stuck against (rules.md section 4).
    WallHit,
    /// A projectile hit its target: the kind is the turret kind that fired it
    /// (rules.md section 10).
    Impact(crate::constants::TurretKind),
}

/// Mix one layer into `buf`: a sine sliding from `tone_start` to `tone_end`
/// Hz, multiplied by `level` and an exponential decay of `decay` 1/s.
fn add_tone(
    buf: &mut [Sample],
    tone_start: f64,
    tone_end: f64,
    decay: f64,
    level: f64,
    phase: &mut f64,
) {
    let len = buf.len();
    if len == 0 {
        return;
    }
    let rate = constants::AUDIO_SAMPLE_RATE as f64;
    for (i, slot) in buf.iter_mut().enumerate() {
        // Position inside the sound in 0..1 drives both the sweep and the decay.
        let t = i as f64 / (len - 1).max(1) as f64;
        let freq = tone_start + (tone_end - tone_start) * t;
        *phase += std::f64::consts::TAU * freq / rate;
        let env = (-decay * t).exp();
        *slot += level * env * phase.sin();
    }
}

/// Mix one layer into `buf`: white noise through a one-pole low-pass whose
/// cutoff slides from `noise_start` to `noise_end` Hz, multiplied by `level`
/// and an exponential decay of `decay` 1/s.
///
/// The low-pass is what separates a boom from a hiss: raw white noise sounds
/// like static, while sliding its cutoff down turns into a rumble.
fn add_noise(
    buf: &mut [Sample],
    noise_start: f64,
    noise_end: f64,
    decay: f64,
    level: f64,
    noise: &Noise,
) {
    let len = buf.len();
    if len == 0 {
        return;
    }
    let rate = constants::AUDIO_SAMPLE_RATE as f64;
    // Low-pass state of the one-pole filter, carried between samples so the
    // noise is filtered rather than reset every sample. It starts at silence,
    // which keeps the onset free of a click.
    let mut filtered = 0.0_f64;
    for (i, slot) in buf.iter_mut().enumerate() {
        let t = i as f64 / (len - 1).max(1) as f64;
        // One-pole coefficient from the cutoff of this sample, which slides
        // from noise_start down to noise_end (or up, for a rocket motor).
        let freq = (noise_start + (noise_end - noise_start) * t).max(1.0);
        let cutoff = 1.0 - (-std::f64::consts::TAU * freq / rate).exp();
        filtered = cutoff * (noise.next() - filtered) + filtered;
        let env = (-decay * t).exp();
        *slot += level * env * filtered;
    }
}

/// Peak-normalise `buf` to `peak` and scale it to 16-bit PCM samples.
///
/// Normalising by the loudest sample (rather than scaling by a fixed gain)
/// keeps every sound at a comparable level regardless of how many layers it
/// has or how loud the noise happened to be, so the constants in
/// `constants.rs` only have to describe the *shape* of a sound, not its exact
/// loudness. The soft `tanh` clip keeps peaks inside the range while rounding
/// the corners, instead of the harsh buzz of a hard clip at +-1.0.
fn to_pcm16(buf: &[Sample], peak: f64) -> Vec<i16> {
    let loudest = buf.iter().fold(0.0_f64, |acc, s| acc.max(s.abs()));
    let gain = if loudest > f64::MIN_POSITIVE {
        peak / loudest
    } else {
        0.0
    };
    buf.iter()
        .map(|s| (s * gain).clamp(-1.0, 1.0).tanh() * i16::MAX as f64)
        .map(|s| s.round() as i16)
        .collect()
}

/// Wrap PCM samples in a mono 16-bit WAV container.
///
/// This is the only place that knows the container format; everything above
/// works on bare samples.
fn wav_container(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    // RIFF chunk: magic, file size - 8, "WAVE".
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    // fmt chunk: PCM (1), mono, sample rate, byte rate, block align, 16 bits.
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&constants::AUDIO_SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(constants::AUDIO_SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    // data chunk: the samples themselves.
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// An explosion of a destroyed vehicle.
///
/// Two layers: a wide noise burst falling from a hiss to a rumble, and a low
/// tone falling to a sub-bass thump. Ground vehicles get the long version;
/// a helicopter gets a shorter, brighter one, matching the fact that it
/// explodes in the air and leaves no ground wave (see `fx.rs`).
fn explosion(ground: bool) -> Vec<Sample> {
    let seconds = if ground {
        constants::AUDIO_EXPLOSION_GROUND_SECONDS
    } else {
        constants::AUDIO_EXPLOSION_AIR_SECONDS
    };
    let mut buf = vec![0.0; sample_count(seconds)];
    let rng = Noise::new(0x5EED_A710);
    add_noise(
        &mut buf,
        constants::AUDIO_EXPLOSION_NOISE_START_HZ,
        constants::AUDIO_EXPLOSION_NOISE_END_HZ,
        constants::AUDIO_EXPLOSION_NOISE_DECAY,
        constants::AUDIO_EXPLOSION_NOISE_LEVEL,
        &rng,
    );
    if ground {
        // The low thump only makes sense for something hitting the ground.
        let mut phase = 0.0;
        add_tone(
            &mut buf,
            constants::AUDIO_EXPLOSION_TONE_START_HZ,
            constants::AUDIO_EXPLOSION_TONE_END_HZ,
            constants::AUDIO_EXPLOSION_TONE_DECAY,
            constants::AUDIO_EXPLOSION_TONE_LEVEL,
            &mut phase,
        );
    }
    buf
}

/// A shot of a turret (rules.md section 10).
///
/// Each turret kind gets its own shape: a heavy `Normal` gun, a short snappy
/// `Rapid` burst, and a long hissy `Rocket` whose noise sweeps *up*, the way
/// a rocket motor does.
fn turret_shot(kind: crate::constants::TurretKind) -> Vec<Sample> {
    use crate::constants::TurretKind;
    let (seconds, tone_start, tone_end, decay) = match kind {
        TurretKind::Normal => (
            constants::AUDIO_TURRET_NORMAL_SECONDS,
            constants::AUDIO_TURRET_NORMAL_TONE_START_HZ,
            constants::AUDIO_TURRET_NORMAL_TONE_END_HZ,
            18.0,
        ),
        TurretKind::Rapid => (
            constants::AUDIO_TURRET_RAPID_SECONDS,
            constants::AUDIO_TURRET_RAPID_TONE_START_HZ,
            constants::AUDIO_TURRET_RAPID_TONE_END_HZ,
            30.0,
        ),
        TurretKind::Rocket => (
            constants::AUDIO_TURRET_ROCKET_SECONDS,
            constants::AUDIO_EXPLOSION_TONE_END_HZ,
            constants::AUDIO_EXPLOSION_TONE_START_HZ,
            constants::AUDIO_TURRET_ROCKET_DECAY,
        ),
    };
    let mut buf = vec![0.0; sample_count(seconds)];
    let mut phase = 0.0;
    add_tone(&mut buf, tone_start, tone_end, decay, 1.0, &mut phase);
    // The noise band gives each gun its character: a bright crack for the
    // rapid gun, a wide whoosh rising into a roar for the rocket launcher.
    let (noise_start, noise_end, level) = match kind {
        TurretKind::Normal => (
            constants::AUDIO_VEHICLE_FIRE_NOISE_START_HZ,
            constants::AUDIO_VEHICLE_FIRE_NOISE_END_HZ,
            constants::AUDIO_VEHICLE_FIRE_NOISE_LEVEL,
        ),
        TurretKind::Rapid => (
            constants::AUDIO_EXPLOSION_NOISE_START_HZ,
            constants::AUDIO_EXPLOSION_NOISE_START_HZ * 0.5,
            constants::AUDIO_VEHICLE_FIRE_NOISE_LEVEL,
        ),
        TurretKind::Rocket => (
            constants::AUDIO_TURRET_ROCKET_NOISE_START_HZ,
            constants::AUDIO_TURRET_ROCKET_NOISE_END_HZ,
            constants::AUDIO_TURRET_ROCKET_NOISE_LEVEL,
        ),
    };
    let rng = Noise::new(0x5EED_A711 + kind as u64);
    add_noise(&mut buf, noise_start, noise_end, decay, level, &rng);
    buf
}

/// A shot of a vehicle at an opponent (rules.md section 9).
fn vehicle_fire() -> Vec<Sample> {
    let decay = constants::AUDIO_VEHICLE_FIRE_DECAY;
    let mut buf = vec![0.0; sample_count(constants::AUDIO_VEHICLE_FIRE_SECONDS)];
    let mut phase = 0.0;
    add_tone(
        &mut buf,
        constants::AUDIO_VEHICLE_FIRE_TONE_START_HZ,
        constants::AUDIO_VEHICLE_FIRE_TONE_END_HZ,
        decay,
        1.0,
        &mut phase,
    );
    let rng = Noise::new(0x5EED_A712);
    add_noise(
        &mut buf,
        constants::AUDIO_VEHICLE_FIRE_NOISE_START_HZ,
        constants::AUDIO_VEHICLE_FIRE_NOISE_END_HZ,
        decay,
        constants::AUDIO_VEHICLE_FIRE_NOISE_LEVEL,
        &rng,
    );
    buf
}

/// A vehicle shooting at a wall (rules.md section 4): duller than a real shot.
fn wall_hit() -> Vec<Sample> {
    let decay = constants::AUDIO_WALL_HIT_DECAY;
    let mut buf = vec![0.0; sample_count(constants::AUDIO_WALL_HIT_SECONDS)];
    let mut phase = 0.0;
    add_tone(
        &mut buf,
        constants::AUDIO_WALL_HIT_TONE_START_HZ,
        constants::AUDIO_WALL_HIT_TONE_END_HZ,
        decay,
        1.0,
        &mut phase,
    );
    let rng = Noise::new(0x5EED_A713);
    add_noise(
        &mut buf,
        constants::AUDIO_WALL_HIT_TONE_START_HZ * 4.0,
        constants::AUDIO_WALL_HIT_TONE_END_HZ * 4.0,
        decay,
        constants::AUDIO_WALL_HIT_NOISE_LEVEL,
        &rng,
    );
    buf
}

/// A projectile hitting its target (rules.md section 10).
///
/// The shape is the same for every turret kind; what differs is how loud the
/// event is, which [`crate::audio`] decides when it plays the sound.
fn impact(kind: crate::constants::TurretKind) -> Vec<Sample> {
    let decay = constants::AUDIO_IMPACT_DECAY;
    let mut buf = vec![0.0; sample_count(constants::AUDIO_IMPACT_SECONDS)];
    let mut phase = 0.0;
    add_tone(
        &mut buf,
        constants::AUDIO_IMPACT_TONE_START_HZ,
        constants::AUDIO_IMPACT_TONE_END_HZ,
        decay,
        1.0,
        &mut phase,
    );
    let rng = Noise::new(0x5EED_A714 + kind as u64);
    add_noise(
        &mut buf,
        constants::AUDIO_IMPACT_TONE_START_HZ,
        constants::AUDIO_IMPACT_TONE_END_HZ,
        decay,
        constants::AUDIO_IMPACT_NOISE_LEVEL,
        &rng,
    );
    buf
}

/// Fade the last [`constants::AUDIO_FADE_OUT_SECONDS`] of `buf` to silence.
///
/// The decay envelopes leave a few percent of the amplitude on the final
/// sample, which is heard as a click when the buffer ends. Fading the tail to
/// zero costs nothing and removes it.
fn fade_tail(buf: &mut [Sample]) {
    let tail =
        (constants::AUDIO_FADE_OUT_SECONDS * constants::AUDIO_SAMPLE_RATE as f64).round() as usize;
    let len = buf.len();
    if tail == 0 || len == 0 || tail >= len {
        buf.iter_mut().for_each(|s| *s = 0.0);
        return;
    }
    let start = len - tail;
    for (i, s) in buf[start..].iter_mut().enumerate() {
        *s *= 1.0 - i as f64 / tail as f64;
    }
    // The very last sample of the ramp is already zero mathematically; make
    // it exactly zero here so the waveform cannot end on a rounding step.
    if let Some(last) = buf.last_mut() {
        *last = 0.0;
    }
}

/// Synthesise `kind` and return it as a WAV byte stream.
///
/// The result is deterministic: the same kind always yields the same bytes.
pub fn synthesise(kind: SoundKind) -> Vec<u8> {
    let mut samples = match kind {
        SoundKind::ExplosionGround => explosion(true),
        SoundKind::ExplosionAir => explosion(false),
        SoundKind::TurretShot(t) => turret_shot(t),
        SoundKind::VehicleFire => vehicle_fire(),
        SoundKind::WallHit => wall_hit(),
        SoundKind::Impact(t) => impact(t),
    };
    fade_tail(&mut samples);
    wav_container(&to_pcm16(&samples, constants::AUDIO_PEAK))
}

/// One sample of a synthesised sound, in the range -1..1.
type Sample = f64;

/// Seconds of audio at the configured sample rate, at least one sample.
fn sample_count(seconds: f64) -> usize {
    let n = (seconds * constants::AUDIO_SAMPLE_RATE as f64).round() as usize;
    n.max(1)
}

/// Deterministic white noise in -1..1 for one sound kind.
///
/// Seeded from the kind, so every explosion of a ground vehicle has the same
/// noise texture while a shot has a different one. A fixed stream (rather than
/// fresh entropy) is what makes the synthesised output reproducible.
struct Noise(std::cell::Cell<u64>);

impl Noise {
    /// Create the noise stream for one seed.
    fn new(seed: u64) -> Self {
        Self(std::cell::Cell::new(Rng::new(seed).next_u64()))
    }
    /// Next sample in -1..1 (splitmix64, the same core as [`Rng`]).
    fn next(&self) -> Sample {
        let mut z = self.0.get().wrapping_add(0x9E3779B97F4A7C15);
        self.0.set(z);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        let v = z ^ (z >> 31);
        ((v >> 11) as f64) / ((1u64 << 53) as f64) * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::TurretKind;

    /// Every sound kind the game can emit.
    const ALL: [SoundKind; 8] = [
        SoundKind::ExplosionGround,
        SoundKind::ExplosionAir,
        SoundKind::TurretShot(TurretKind::Normal),
        SoundKind::TurretShot(TurretKind::Rapid),
        SoundKind::TurretShot(TurretKind::Rocket),
        SoundKind::VehicleFire,
        SoundKind::WallHit,
        SoundKind::Impact(TurretKind::Normal),
    ];

    /// Number of PCM samples a WAV of `bytes` carries, from its data chunk.
    fn data_samples(bytes: &[u8]) -> usize {
        let data_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        data_len / 2
    }

    /// The PCM samples of a synthesised WAV, skipping the 44-byte header.
    // `array_chunks` needs an array, not a slice, so read the 16-bit samples
    // two bytes at a time here.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn samples_of(bytes: &[u8]) -> Vec<i16> {
        bytes[44..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    /// Loudest absolute sample in `samples`.
    fn peak_of(samples: &[i16]) -> u16 {
        samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0)
    }

    #[test]
    fn every_sound_is_a_valid_mono_16bit_wav() {
        for kind in ALL {
            let wav = synthesise(kind);
            assert_eq!(&wav[0..4], b"RIFF", "{kind:?}: RIFF magic");
            assert_eq!(&wav[8..12], b"WAVE", "{kind:?}: WAVE magic");
            assert_eq!(&wav[12..16], b"fmt ", "{kind:?}: fmt chunk");
            // PCM format 1, one channel, 16 bits, our sample rate.
            assert_eq!(u16::from_le_bytes(wav[20..22].try_into().unwrap()), 1);
            assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
            assert_eq!(
                u32::from_le_bytes(wav[24..28].try_into().unwrap()),
                constants::AUDIO_SAMPLE_RATE
            );
            assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
            assert_eq!(&wav[36..40], b"data", "{kind:?}: data chunk");
            // The declared sizes must agree with the real length of the buffer,
            // otherwise the audio backend would read past the end of it.
            let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
            assert_eq!(44 + data_len, wav.len(), "{kind:?}: RIFF size");
            assert!(data_samples(&wav) > 0, "{kind:?}: no samples");
        }
    }

    #[test]
    fn synthesis_is_deterministic() {
        for kind in ALL {
            assert_eq!(synthesise(kind), synthesise(kind), "{kind:?} differs");
        }
    }

    #[test]
    fn different_sounds_are_different_sounds() {
        // Two sounds that collapsed to the same waveform would be a silent bug
        // in the synthesis (a constant buffer, say).
        let mut seen: Vec<(SoundKind, Vec<u8>)> = Vec::new();
        for kind in ALL {
            let wav = synthesise(kind);
            for (other, bytes) in seen.iter() {
                assert_ne!(*other, kind, "{kind:?} is indistinguishable from it");
                assert_ne!(&wav, bytes, "{kind:?} is byte-identical to {other:?}");
            }
            seen.push((kind, wav));
        }
    }

    #[test]
    fn sounds_last_the_time_their_constants_ask_for() {
        for kind in ALL {
            let wav = synthesise(kind);
            let seconds = data_samples(&wav) as f64 / constants::AUDIO_SAMPLE_RATE as f64;
            // One sound buffer must be short and audible, never a click.
            assert!(seconds > 0.05, "{kind:?} too short: {seconds}s");
            assert!(seconds < 2.0, "{kind:?} too long: {seconds}s");
        }
    }

    #[test]
    fn sounds_are_audible_but_not_clipped() {
        for kind in ALL {
            let samples = samples_of(&synthesise(kind));
            let peak = peak_of(&samples);
            // A silent buffer would pass every other check, so require sound.
            assert!(peak > 0, "{kind:?} is silent");
            // The soft clip keeps the loudest sample just under full scale: a
            // waveform pinned at +-32767 means hard clipping, which buzzes.
            assert!(
                peak < i16::MAX as u16,
                "{kind:?} hits full scale ({peak}), so it clips"
            );
        }
    }

    #[test]
    fn sounds_fade_out_instead_of_ending_on_a_click() {
        // A sound that is still loud in its last samples would click when the
        // sample buffer runs out; the decay envelope and the fade-out must
        // bring it all the way to silence.
        for kind in ALL {
            let samples = samples_of(&synthesise(kind));
            let last = *samples.last().unwrap();
            assert_eq!(last, 0, "{kind:?} ends on {last}, which is a click");
            let head_peak = peak_of(&samples[..2000.min(samples.len())]);
            let tail_peak = peak_of(&samples[samples.len() - 200..]);
            assert!(
                tail_peak * 2 < head_peak,
                "{kind:?} does not decay: head={head_peak} tail={tail_peak}"
            );
        }
    }

    #[test]
    fn a_helicopter_dies_differently_from_a_tank() {
        // The air explosion has no ground thump, so it is both shorter and
        // differently shaped than the ground one.
        let ground = synthesise(SoundKind::ExplosionGround);
        let air = synthesise(SoundKind::ExplosionAir);
        assert!(data_samples(&air) < data_samples(&ground));
        assert_ne!(ground, air);
    }
}
