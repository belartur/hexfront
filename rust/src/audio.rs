//! Sound playback: the only module that talks to the audio backend.
//!
//! The simulation reports *what* happened ([`crate::entities::SoundEvent`]),
//! [`crate::sound`] says which recording answers it, and this module is what
//! actually makes a noise: it loads the Ogg Vorbis files from `sounds/` once,
//! works out how loud each event should be given where the camera is looking,
//! and hands it to macroquad's audio backend (enabled by the `audio` feature
//! of the crate).
//!
//! Because a battle can report dozens of events in a single simulation step,
//! this module is deliberately selective about what it plays:
//!
//! * sounds farther than [`constants::AUDIO_MAX_DISTANCE_J`] are dropped
//!   outright, and the rest fade off linearly with distance, so the fight the
//!   player is watching is louder than the one across the map,
//! * at most [`constants::AUDIO_MAX_VOICES_PER_STEP`] sounds start per step,
//!   the closest ones first,
//! * the same sound cannot retrigger within
//!   [`constants::AUDIO_RETRIGGER_INTERVAL`], which stops one rapid-fire
//!   turret from stacking its shots into a continuous buzz.
//!
//! The attenuation is measured from the centre of the view, in world units
//! (j) -- the same distance space the simulation uses, so zooming the view
//! does not change what is audible.

use std::collections::HashMap;

use macroquad::audio::{self, PlaySoundParams};

use crate::constants;
use crate::decode;
use crate::entities::SoundEvent;
use crate::rng::Rng;
use crate::sound::{self, SoundKind};

/// The sound played for one event, including how loud and which recording.
///
/// Split from [`SoundEvent`] because neither the loudness nor the variant is
/// part of what the simulation reports: they depend on the camera and on a
/// level-seeded stream, both of which live in the presentation layer.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    /// Which family of recordings to take from.
    kind: SoundKind,
    /// Which recording of that family.
    variant: usize,
    /// Final volume, already multiplied by the master volume.
    volume: f32,
}

/// The audio subsystem: decoded sounds plus the bookkeeping of what is playing.
pub struct Audio {
    /// Decoded recordings, flattened: for each kind of
    /// [`sound::ALL_KINDS`], its variants in order. The index of a recording
    /// is resolved by [`Audio::slot`]. Empty until [`Audio::load`] has run,
    /// which also means sound is muted.
    sounds: Vec<Option<audio::Sound>>,
    /// Time of the last start of each kind, used for retrigger limiting.
    last_played: HashMap<SoundKind, f64>,
    /// Stream that picks the recording variant, seeded with the level seed so
    /// a match always replays the same sounds while two blasts within one
    /// battle still differ.
    variants: Rng,
    /// Current clock in seconds, advanced once per frame.
    time: f64,
    /// True once the sounds failed to load (no audio device, for instance).
    failed: bool,
}

impl Audio {
    /// Create a silent audio subsystem; call [`Audio::load`] to enable it.
    pub fn new() -> Self {
        Self {
            sounds: Vec::new(),
            last_played: HashMap::new(),
            variants: Rng::new(constants::AUDIO_VARIANT_SEED),
            time: 0.0,
            failed: false,
        }
    }

    /// Re-seed the variant stream, so a level always plays its sounds the same
    /// way. Called when a match or an editor playtest starts, next to
    /// `Fx::reseed()`.
    pub fn reseed(&mut self, seed: u64) {
        self.variants = Rng::new(seed ^ constants::AUDIO_VARIANT_SEED);
        self.last_played.clear();
    }

    /// Load and decode every recording, so the game has something to play.
    ///
    /// The files come from `sounds/` (see `sounds/README.md` for provenance).
    /// Each one is decoded and resampled here by [`crate::decode`], and the
    /// backend receives WAV bytes in memory rather than the file: its own
    /// resampler shortens a recording and adds aliasing (see `decode.rs`).
    ///
    /// Safe to call more than once; a failure only disables sound, it never
    /// stops the game. A machine with no working audio device (a bare CI
    /// runner, a container without a sound card) or a build without the `sounds/`
    /// directory must still be able to start Hexfront, so a failure is
    /// remembered instead of panicking.
    pub async fn load(&mut self) {
        if self.failed || !self.sounds.is_empty() {
            return;
        }
        for kind in sound::ALL_KINDS {
            for path in sound::variant_paths(kind) {
                let name = path.display().to_string();
                let bytes = match std::fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        eprintln!("hexfront: audio disabled, cannot read {name}: {err}");
                        self.fail();
                        return;
                    }
                };
                let decoded = match decode::to_wav(&bytes, constants::AUDIO_MIX_RATE) {
                    Ok(decoded) => decoded,
                    Err(err) => {
                        eprintln!("hexfront: audio disabled, cannot decode {name}: {err}");
                        self.fail();
                        return;
                    }
                };
                match audio::load_sound_from_bytes(&decoded.wav).await {
                    Ok(loaded) => {
                        // The backend resamples anything that is not already at
                        // the mix rate -- with artefacts, as `decode.rs`
                        // explains. Reaching this branch with a different rate
                        // means the resampler refused to convert the file, and
                        // the recording would play wrong; say so instead of
                        // leaving the player to wonder.
                        if decoded.sample_rate != constants::AUDIO_MIX_RATE || decoded.channels > 2
                        {
                            eprintln!(
                                "hexfront: {name} decoded to {} Hz / {} channels, \
                                 not {} Hz stereo -- it will sound wrong",
                                decoded.sample_rate,
                                decoded.channels,
                                constants::AUDIO_MIX_RATE
                            );
                        }
                        self.sounds.push(Some(loaded));
                    }
                    Err(err) => {
                        eprintln!("hexfront: audio disabled, cannot load {name}: {err}");
                        self.fail();
                        return;
                    }
                }
            }
        }
    }

    /// Give up on sound and drop everything loaded so far.
    fn fail(&mut self) {
        self.failed = true;
        self.sounds.clear();
    }

    /// True while the game can actually make a sound.
    pub fn is_enabled(&self) -> bool {
        !self.failed && !self.sounds.is_empty()
    }

    /// Advance the clock used for retrigger limiting, once per frame.
    pub fn update(&mut self, dt: f32) {
        self.time += dt as f64;
    }

    /// Forget the retrigger history (entering the menu, leaving a level).
    pub fn stop_all(&mut self) {
        self.last_played.clear();
    }

    /// Play the sounds of one simulation step, as seen from the view centre.
    ///
    /// `centre` is the world position the camera looks at and `events` are the
    /// sounds the simulation reported since the last call. The events are
    /// filtered by distance and loudness, sorted so the closest ones win the
    /// limited number of voices, and then played.
    pub fn play_events(&mut self, centre: (f64, f64), events: &[SoundEvent]) {
        if !self.is_enabled() || events.is_empty() {
            return;
        }
        // Work out what each event would sound like, and how far away it is.
        // The recording variant is drawn only for events that will really
        // play, so that skipped (too far away) events do not shift the
        // sequence of the ones that do.
        let mut candidates: Vec<(f64, SoundKind, f32)> = Vec::with_capacity(events.len());
        for event in events {
            let Some((volume, distance)) = volume_at(centre, (event.x, event.y), event.kind) else {
                continue;
            };
            candidates.push((distance, event.kind, volume));
        }
        // Closest first, so the voices we do spend are the visible ones.
        candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut started = 0;
        for (_, kind, volume) in candidates {
            if started >= constants::AUDIO_MAX_VOICES_PER_STEP {
                break;
            }
            if !self.may_play(kind) {
                continue;
            }
            let variant = sound::pick_variant(kind, &mut self.variants);
            self.play(Placed {
                kind,
                variant,
                volume,
            });
            started += 1;
        }
    }

    /// True when this kind may start again: the retrigger gap has passed.
    fn may_play(&self, kind: SoundKind) -> bool {
        match self.last_played.get(&kind) {
            Some(last) => self.time - last >= constants::AUDIO_RETRIGGER_INTERVAL,
            None => true,
        }
    }

    /// Index of a recording in the flattened [`Audio::sounds`] table.
    ///
    /// Returns `None` for a kind whose family is not in
    /// [`sound::ALL_KINDS`], which would mean the loading pass and this lookup
    /// disagree about what exists.
    fn slot(&self, kind: SoundKind, variant: usize) -> Option<usize> {
        let family = sound::ALL_KINDS
            .iter()
            .position(|k| *k == kind)
            .or_else(|| {
                // A kind that shares files with a listed one (a rocket impact
                // shares with a plain impact) resolves through that entry.
                sound::ALL_KINDS
                    .iter()
                    .position(|k| sound::file_name(*k, 0) == sound::file_name(kind, 0))
            })?;
        // Offsets: how many recordings the earlier families hold.
        let offset: usize = sound::ALL_KINDS[..family]
            .iter()
            .map(|k| sound::variant_count(*k))
            .sum();
        Some(offset + variant)
    }

    /// Hand one recording to the mixer, remembering when it started.
    fn play(&mut self, placed: Placed) {
        let Some(index) = self.slot(placed.kind, placed.variant) else {
            return;
        };
        let Some(sound) = self.sounds.get(index).and_then(|s| s.as_ref()) else {
            return;
        };
        audio::play_sound(
            sound,
            PlaySoundParams {
                looped: false,
                volume: placed.volume,
            },
        );
        self.last_played.insert(placed.kind, self.time);
    }
}

/// The volume of `kind` at world position `pos`, seen from `centre`.
///
/// Returns the final volume and the distance in j, or `None` when the event
/// is too far away (or too quiet) to be worth a mixer voice.
fn volume_at(centre: (f64, f64), pos: (f64, f64), kind: SoundKind) -> Option<(f32, f64)> {
    let dx = pos.0 - centre.0;
    let dy = pos.1 - centre.1;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance >= constants::AUDIO_MAX_DISTANCE_J {
        return None;
    }
    // Full volume nearby, falling linearly to silence at the maximum range.
    let falloff = 1.0
        - ((distance - constants::AUDIO_FULL_DISTANCE_J)
            / (constants::AUDIO_MAX_DISTANCE_J - constants::AUDIO_FULL_DISTANCE_J))
            .clamp(0.0, 1.0);
    let volume = constants::AUDIO_MASTER_VOLUME * base_level(kind) * falloff as f32;
    if volume < constants::AUDIO_MIN_VOLUME {
        return None;
    }
    Some((volume, distance))
}

/// How loud a sound is on its own, before the distance falloff.
///
/// A rocket impact reads as a small blast and a plain gun hit as a light tick,
/// so a firefight has a rhythm instead of a wall of equal clicks.
fn base_level(kind: SoundKind) -> f32 {
    match kind {
        SoundKind::ExplosionGround | SoundKind::ExplosionAir => 1.0,
        SoundKind::TurretShot(_) | SoundKind::WallHit => 0.8,
        // Vehicles are a little quieter than turrets: the player mostly hears
        // their own guns, and a duel in the corner should not drown them out.
        SoundKind::VehicleFire => constants::AUDIO_VEHICLE_FIRE_LEVEL,
        SoundKind::Impact(crate::constants::TurretKind::Rocket) => {
            constants::AUDIO_IMPACT_ROCKET_LEVEL
        }
        SoundKind::Impact(_) => constants::AUDIO_IMPACT_TURRET_LEVEL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::TurretKind;
    use crate::sound::ALL_KINDS;

    #[test]
    fn nearby_sounds_are_heard_and_distant_ones_are_not() {
        let centre = (0.0, 0.0);
        // Right on top of the camera: at full volume.
        let (volume, distance) =
            volume_at(centre, (10.0, 0.0), SoundKind::ExplosionGround).unwrap();
        assert!(distance < 11.0);
        assert!((volume - constants::AUDIO_MASTER_VOLUME).abs() < 1e-6);
        // Beyond the audible range: dropped, so it costs no mixer voice.
        assert!(volume_at(centre, (5000.0, 0.0), SoundKind::ExplosionGround).is_none());
    }

    #[test]
    fn volume_falls_off_with_distance() {
        let centre = (0.0, 0.0);
        let near = volume_at(centre, (0.0, 0.0), SoundKind::ExplosionGround)
            .unwrap()
            .0;
        let mid = volume_at(centre, (500.0, 0.0), SoundKind::ExplosionGround)
            .unwrap()
            .0;
        let far = volume_at(centre, (850.0, 0.0), SoundKind::ExplosionGround)
            .unwrap()
            .0;
        assert!(near > mid, "{near} !> {mid}");
        assert!(mid > far, "{mid} !> {far}");
        assert!(far > 0.0);
    }

    #[test]
    fn a_rocket_hit_is_louder_than_a_plain_gun_hit() {
        let centre = (0.0, 0.0);
        let rocket = volume_at(centre, (0.0, 0.0), SoundKind::Impact(TurretKind::Rocket))
            .unwrap()
            .0;
        let plain = volume_at(centre, (0.0, 0.0), SoundKind::Impact(TurretKind::Normal))
            .unwrap()
            .0;
        assert!(rocket > plain, "{rocket} !> {plain}");
    }

    #[test]
    fn no_volume_ever_exceeds_full_scale() {
        // Sounds overlap in the mixer, so any single one above 1.0 would let
        // the sum distort.
        for kind in ALL_KINDS {
            for distance in [0.0, 100.0, 300.0, 600.0, 850.0] {
                if let Some((volume, _)) = volume_at((0.0, 0.0), (distance, 0.0), kind) {
                    assert!(
                        (0.0..=1.0).contains(&volume),
                        "{kind:?} at {distance} j is {volume}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_kind_resolves_to_its_own_slot() {
        // A kind the mixer cannot look up would play silence forever, and two
        // kinds sharing a slot would play each other's recordings.
        let audio = Audio::new();
        let mut seen = std::collections::HashSet::new();
        for kind in ALL_KINDS {
            for variant in 0..sound::variant_count(kind) {
                let slot = audio
                    .slot(kind, variant)
                    .unwrap_or_else(|| panic!("{kind:?} variant {variant} has no slot"));
                assert!(seen.insert(slot), "{kind:?} collides on slot {slot}");
            }
        }
        // The slots must be exactly the recordings the loading pass reads,
        // one per file, with no gaps and nothing past the end.
        let expected: usize = ALL_KINDS.iter().map(|k| sound::variant_count(*k)).sum();
        assert_eq!(seen.len(), expected);
        assert_eq!(seen.iter().copied().max(), Some(expected - 1));
    }

    #[test]
    fn a_kind_sharing_recordings_resolves_them_too() {
        // A rocket impact shares its files with a plain impact; it must still
        // find them, otherwise rockets would hit silently.
        let audio = Audio::new();
        let plain = sound::variant_count(SoundKind::Impact(TurretKind::Normal));
        for variant in 0..plain {
            assert_eq!(
                audio.slot(SoundKind::Impact(TurretKind::Rocket), variant),
                audio.slot(SoundKind::Impact(TurretKind::Normal), variant),
                "a rocket impact must land on the shared recordings"
            );
        }
    }

    #[test]
    fn an_unloaded_subsystem_stays_silent() {
        // Before `load` (or after a failure) the game must still run, quietly.
        let audio = Audio::new();
        assert!(!audio.is_enabled());
        // Playing into a disabled subsystem is a no-op, not a panic.
        let mut audio = audio;
        audio.play_events(
            (0.0, 0.0),
            &[SoundEvent {
                kind: SoundKind::ExplosionGround,
                x: 0.0,
                y: 0.0,
            }],
        );
    }
}
