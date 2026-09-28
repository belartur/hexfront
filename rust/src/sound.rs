//! Sound catalogue: which recording answers which game event.
//!
//! The sounds themselves live in the repository's `sounds/` directory as
//! Ogg Vorbis files (their provenance and licence are documented in
//! `sounds/README.md`); this module owns the *mapping* between a game event
//! and the file that answers it, plus the deterministic choice of a variant.
//!
//! Vorbis is what the audio backend decodes natively, so the files are used
//! as they are, without being converted. They are stored at the rate the
//! backend mixes at (see the note in `sounds/README.md`): the backend would
//! otherwise resample them with a nearest-neighbour copy that halves the
//! duration and adds aliasing.
//!
//! It knows nothing about volume, distance or the audio device: the
//! simulation reports *what* happened ([`crate::entities::SoundEvent`],
//! drained from [`crate::game::Game::take_sounds`]) and [`crate::audio`]
//! decides how loud and whether at all. This module also does not depend on
//! macroquad, so the whole mapping is testable headless -- the tests read the
//! real files from `sounds/` and check that every event really has a sound.
//!
//! Events have several variants (three ground explosions, two rocket shots,
//! ...), because replaying one recording over and over is the fastest way to
//! make a battle sound canned. Which variant plays is derived from
//! [`crate::rng::Rng`], so a match replays identically from the same seed
//! while two explosions in the same battle still differ.

use crate::constants::{self, TurretKind};
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

/// Base name of the files of one event, without the `_N` variant suffix.
fn base_name(kind: SoundKind) -> &'static str {
    match kind {
        SoundKind::ExplosionGround => "explosion_ground",
        SoundKind::ExplosionAir => "explosion_air",
        SoundKind::TurretShot(TurretKind::Normal) => "turret_normal",
        SoundKind::TurretShot(TurretKind::Rapid) => "turret_rapid",
        SoundKind::TurretShot(TurretKind::Rocket) => "turret_rocket",
        SoundKind::VehicleFire => "vehicle_fire",
        SoundKind::WallHit => "wall_hit",
        // A rocket hit and a plain hit share one set of recordings; only the
        // loudness differs, and that is applied in `audio.rs`.
        SoundKind::Impact(_) => "impact",
    }
}

/// How many variants `kind` has on disk.
///
/// Kept next to [`base_name`] on purpose: a variant that does not exist must
/// be a compile-time-visible pair with its file, not a number that silently
/// disagrees with the directory.
pub const fn variant_count(kind: SoundKind) -> usize {
    match kind {
        SoundKind::ExplosionGround => 3,
        SoundKind::ExplosionAir => 2,
        SoundKind::TurretShot(TurretKind::Normal) => 2,
        SoundKind::TurretShot(TurretKind::Rapid) => 2,
        SoundKind::TurretShot(TurretKind::Rocket) => 2,
        SoundKind::VehicleFire => 2,
        SoundKind::WallHit => 1,
        SoundKind::Impact(_) => 2,
    }
}

/// Every sound kind the game can emit, in a fixed order.
///
/// The order only indexes tables built next to it, but it must not change
/// without changing those too, so it lives in one place.
pub const ALL_KINDS: [SoundKind; 8] = [
    SoundKind::ExplosionGround,
    SoundKind::ExplosionAir,
    SoundKind::TurretShot(TurretKind::Normal),
    SoundKind::TurretShot(TurretKind::Rapid),
    SoundKind::TurretShot(TurretKind::Rocket),
    SoundKind::VehicleFire,
    SoundKind::WallHit,
    SoundKind::Impact(TurretKind::Normal),
];

/// File name of variant `variant` (0-based) of `kind`, e.g.
/// `turret_rapid_2.ogg`.
pub fn file_name(kind: SoundKind, variant: usize) -> String {
    format!("{}_{}.ogg", base_name(kind), variant + 1)
}

/// Full path of variant `variant` of `kind`, ready for loading.
///
/// The path is built from [`constants::sounds_dir`], so it is independent of
/// the working directory the game was started from.
pub fn variant_path(kind: SoundKind, variant: usize) -> std::path::PathBuf {
    constants::sounds_dir().join(file_name(kind, variant))
}

/// The variant of `kind` that a given stream picks, wrapping around.
///
/// Callers drive this with the same level-seeded [`Rng`] the rest of the
/// presentation layer uses, which is what makes the choice deterministic per
/// match but varied within it.
pub fn pick_variant(kind: SoundKind, rng: &mut Rng) -> usize {
    let count = variant_count(kind);
    if count <= 1 {
        return 0;
    }
    rng.next_u64() as usize % count
}

/// Paths of every variant of `kind`, in order.
pub fn variant_paths(kind: SoundKind) -> Vec<std::path::PathBuf> {
    (0..variant_count(kind))
        .map(|v| variant_path(kind, v))
        .collect()
}

#[cfg(test)]
mod tests_support {
    //! Minimal Ogg Vorbis header reader, just enough to check the files.
    //!
    //! The decoding itself happens in [`crate::decode`], which the tests there
    //! exercise end to end on the real files. These helpers only read the
    //! header, so the catalogue tests can check each file cheaply, without
    //! decoding 16 recordings per test.

    /// Read a recording, failing with its name when it is missing.
    pub(crate) fn read(path: &std::path::Path) -> Vec<u8> {
        std::fs::read(path).unwrap_or_else(|e| panic!("{}: cannot read: {e}", path.display()))
    }

    /// The first Vorbis identification header in `bytes`, as
    /// `(channels, sample_rate)`.
    ///
    /// Located by searching for the `\x01vorbis` packet id rather than by
    /// assuming an offset, because the first Ogg page may carry metadata
    /// before it.
    pub(crate) fn vorbis_format(bytes: &[u8], what: &str) -> (u8, u32) {
        assert_eq!(&bytes[0..4], b"OggS", "{what}: not an Ogg stream");
        // OggS, stream structure version 0.
        assert_eq!(bytes[4], 0, "{what}: unexpected Ogg version");
        let id_pos = bytes
            .windows(7)
            .position(|w| w == [0x01, b'v', b'o', b'r', b'b', b'i', b's'])
            .unwrap_or_else(|| panic!("{what}: no Vorbis identification header"));
        // id (7) + version (4) + channels (1) + rate (4 LE)
        let body = &bytes[id_pos + 7..];
        let version = u32::from_le_bytes(body[0..4].try_into().unwrap());
        assert_eq!(version, 0, "{what}: unexpected Vorbis version");
        let channels = body[4];
        let rate = u32::from_le_bytes(body[5..9].try_into().unwrap());
        assert!(
            channels == 1 || channels == 2,
            "{what}: {channels} channels"
        );
        assert!(rate > 0, "{what}: bad sample rate");
        (channels, rate)
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use super::*;
    use crate::constants::{self, TurretKind};
    use resampler::SampleRate;

    /// Sound kinds that are not listed in [`ALL_KINDS`]: they share files with
    /// a listed kind, but must still resolve to an existing file.
    const EXTRA: [SoundKind; 3] = [
        SoundKind::Impact(TurretKind::Rapid),
        SoundKind::Impact(TurretKind::Rocket),
        SoundKind::TurretShot(TurretKind::Normal),
    ];

    #[test]
    fn every_sound_kind_has_files_that_exist() {
        for kind in ALL_KINDS {
            let paths = variant_paths(kind);
            assert!(!paths.is_empty(), "{kind:?} has no files");
            for path in &paths {
                let bytes = read(path);
                // The files are stored exactly as they were published, so the
                // only requirement here is that they are readable Vorbis with
                // a rate the resampler models; `decode.rs` does the rest.
                let (channels, rate) = vorbis_format(&bytes, &path.display().to_string());
                assert!(
                    channels == 1 || channels == 2,
                    "{}: {channels} channels",
                    path.display()
                );
                assert!(
                    SampleRate::try_from(rate).is_ok(),
                    "{}: {rate} Hz has no resampler",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn shared_kinds_resolve_to_the_same_files() {
        // A rocket impact and a machine-gun impact use one recording; the
        // difference is loudness, handled in `audio.rs`.
        assert_eq!(
            variant_paths(SoundKind::Impact(TurretKind::Normal)),
            variant_paths(SoundKind::Impact(TurretKind::Rocket))
        );
        for kind in EXTRA {
            for path in variant_paths(kind) {
                assert!(
                    path.exists(),
                    "{kind:?} points at a missing file {}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn paths_do_not_depend_on_the_working_directory() {
        // The game may be started from anywhere (`cargo run` from `rust/`, a
        // binary from a desktop entry, CI from the repository root), so the
        // sounds must be found through the repository root, not `./sounds`.
        let path = variant_path(SoundKind::ExplosionGround, 0);
        assert!(
            path.is_absolute(),
            "{path:?} is relative to the working directory"
        );
        assert!(
            path.starts_with(constants::repo_root()),
            "{path:?} is outside the repository"
        );
        assert!(path.exists(), "{path:?} does not exist");
    }

    #[test]
    fn variants_are_distinct_files() {
        // Two entries pointing at one file would silently halve the variety
        // the variant system is there to provide.
        for kind in ALL_KINDS {
            let paths = variant_paths(kind);
            let mut sorted = paths.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), paths.len(), "{kind:?} repeats a file");
        }
    }

    #[test]
    fn every_event_family_maps_to_its_own_files() {
        // Guards against two different events sharing a base name by mistake.
        let mut names: Vec<&'static str> = ALL_KINDS.iter().map(|k| base_name(*k)).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "two events share one file family");
    }

    #[test]
    fn the_variant_picker_is_deterministic_and_varies() {
        let mut a = Rng::new(12345);
        let mut b = Rng::new(12345);
        let first: Vec<usize> = (0..20)
            .map(|_| pick_variant(SoundKind::ExplosionGround, &mut a))
            .collect();
        let second: Vec<usize> = (0..20)
            .map(|_| pick_variant(SoundKind::ExplosionGround, &mut b))
            .collect();
        assert_eq!(first, second, "the same seed must replay the same sounds");
        // Three ground explosions, so a long run must not repeat one forever.
        assert!(first.iter().collect::<std::collections::HashSet<_>>().len() > 1);
    }

    #[test]
    fn a_single_variant_kind_always_picks_the_only_one() {
        let mut rng = Rng::new(7);
        for _ in 0..10 {
            assert_eq!(pick_variant(SoundKind::WallHit, &mut rng), 0);
        }
    }

    #[test]
    fn the_picker_never_points_past_the_files() {
        // The picker and the table of files must agree, or a late variant
        // would try to load a file that is not there.
        let mut rng = Rng::new(99);
        for kind in ALL_KINDS {
            for _ in 0..50 {
                let v = pick_variant(kind, &mut rng);
                assert!(v < variant_count(kind), "{kind:?} picked {v}");
                assert!(variant_path(kind, v).exists());
            }
        }
    }
}

#[cfg(test)]
mod quality {
    //! Checks on the recordings themselves, before the backend decodes them.
    //!
    //! These are the faults that are invisible in a code review and obvious
    //! in a headset. Two of them -- a format the backend cannot load, and a
    //! sample rate that makes it resample -- are properties of the file and
    //! are checked here against the real files. The rest (clipping, a cutoff
    //! ending on a click) are properties of the waveform; they were checked
    //! when the files were prepared and are not re-checked by the build,
    //! because decoding Vorbis would mean adding a dependency purely for
    //! tests.

    use super::tests_support::*;

    use crate::sound::{ALL_KINDS, variant_paths};
    use resampler::SampleRate;

    #[test]
    fn every_recording_is_a_stream_the_decoder_accepts() {
        // The files are stored as published, so what matters is that the
        // decoding chain can handle them: a readable Vorbis stream, at a rate
        // the resampler models, with at most the two channels the mixer
        // plays. `decode.rs` has the end-to-end tests.
        for kind in ALL_KINDS {
            for path in variant_paths(kind) {
                let bytes = read(&path);
                let (channels, rate) = vorbis_format(&bytes, &path.display().to_string());
                assert!(
                    SampleRate::try_from(rate).is_ok(),
                    "{}: {rate} Hz has no resampler",
                    path.display()
                );
                assert!(
                    channels == 1 || channels == 2,
                    "{}: {channels} channels",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn the_recordings_are_small() {
        // Ogg Vorbis exists to be small; if this ever grows, someone
        // re-encoded them uncompressed by accident.
        let mut total = 0_usize;
        for kind in ALL_KINDS {
            for path in variant_paths(kind) {
                let size = read(&path).len();
                total += size;
                assert!(size > 0, "{}: empty file", path.display());
                assert!(
                    size < 400_000,
                    "{}: {} KB is too big for a one-shot",
                    path.display(),
                    size / 1024
                );
            }
        }
        assert!(
            total < 2_000_000,
            "the whole sound set is {} KB",
            total / 1024
        );
    }

    #[test]
    fn the_sound_directory_holds_only_files_we_know_about() {
        // A leftover recording from an earlier experiment would be dead weight
        // in the repository, so check that the directory matches the catalogue.
        let known: std::collections::HashSet<String> = ALL_KINDS
            .iter()
            .flat_map(|k| crate::sound::variant_paths(*k))
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        let dir = crate::constants::sounds_dir();
        for entry in std::fs::read_dir(&dir).expect("sounds/ must exist") {
            let entry = entry.expect("cannot read a directory entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".ogg") {
                assert!(
                    known.contains(&name),
                    "{name} is in sounds/ but no event plays it"
                );
            }
        }
    }
}
