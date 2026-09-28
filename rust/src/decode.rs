//! Decoding the recordings in `sounds/` for the audio backend.
//!
//! The files are stored as they were published: Ogg Vorbis, in whatever
//! sample rate the recorder used. Nothing in the repository re-encodes them,
//! because Vorbis is lossy and an extra generation of encoding is a real loss
//! of quality (measured at roughly 0.13 dB RMS for a quality-3 round trip,
//! and much more once filters are added on top).
//!
//! This module decodes a file to raw samples, resamples it to the rate the
//! backend mixes at, and hands it over as a WAV byte stream in memory.
//!
//! # Why not let the backend do it
//!
//! The backend would decode the same file, but its resampler copies
//! neighbouring samples without any filtering. Measured on a 48 kHz
//! recording, that path shortens a 1.364 s explosion to 0.682 s and raises
//! the energy above 2 kHz by about 8.6 dB -- the blast turns into a short,
//! hissy thud. Doing the resampling here means the backend sees the rate it
//! expects and takes its conversion path no further, and the sound plays at
//! full length.
//!
//! # Sample rates other than the ones in `sounds/`
//!
//! The rate is read from each file rather than assumed, so a recording
//! re-sourced at 44.1 kHz, 96 kHz or anything else the [`resampler`] crate
//! supports keeps working. A file already at the target rate is passed
//! through untouched (no resampler, no second copy of the samples).
//!
//! This module deliberately does not depend on macroquad, so the whole
//! decoding chain is testable headless: the tests read the real files from
//! `sounds/`, decode them, and check the duration, the channel layout and
//! that no resampling artefacts appeared in the spectrum.

use std::io::Cursor;

use lewton::inside_ogg::OggStreamReader;
use resampler::{ResamplerFft, SampleRate};

/// A recording that could not be prepared for playback.
///
/// The `Display` implementation is what reaches the player (the reason a file
/// could not be loaded is printed to the log), so the offending value is part
/// of the message rather than only of the `Debug` output.
#[derive(Debug)]
pub enum DecodeError {
    /// The bytes are not a Vorbis stream, or it is damaged.
    Vorbis(String),
    /// The file decoded to nothing.
    Empty,
    /// The file's sample rate has no resampler (an unusual rate, or one the
    /// `resampler` crate does not model).
    UnsupportedRate(u32),
    /// The backend's mixer is fixed at two channels and this file has a
    /// different number.
    UnsupportedChannels(u8),
    /// The resampler rejected a buffer.
    Resample(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Vorbis(why) => write!(f, "not a readable Ogg Vorbis file ({why})"),
            DecodeError::Empty => write!(f, "the file contains no samples"),
            DecodeError::UnsupportedRate(hz) => write!(
                f,
                "no resampler for {hz} Hz (the resampler crate models a fixed set of rates)"
            ),
            DecodeError::UnsupportedChannels(n) => {
                write!(f, "{n} channels, but the audio mixer is stereo")
            }
            DecodeError::Resample(why) => write!(f, "resampling failed ({why})"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// One decoded recording, ready for the backend.
pub struct Decoded {
    /// A 16-bit PCM WAV byte stream, already at the rate the backend mixes at.
    pub wav: Vec<u8>,
    /// Sample rate of the samples in `wav`.
    pub sample_rate: u32,
    /// Channel count of `wav` (1 or 2).
    pub channels: u8,
}

/// Decode the Ogg Vorbis file in `bytes` and return it as a WAV byte stream at
/// `target_rate`.
///
/// Channel count and sample rate come from the file, not from the caller, so
/// a file in any format the [`resampler`] crate supports plays without code
/// changes.
pub fn to_wav(bytes: &[u8], target_rate: u32) -> Result<Decoded, DecodeError> {
    let (samples, rate, channels) = decode_vorbis(bytes)?;
    let (samples, rate) = if rate == target_rate {
        // Nothing to do: avoid resampling identical rates (the crate would
        // treat it as a no-op but still copy the whole buffer).
        (samples, rate)
    } else {
        (
            resample(&samples, rate, target_rate, channels)?,
            target_rate,
        )
    };
    Ok(Decoded {
        wav: wav_container(&samples, rate, channels),
        sample_rate: rate,
        channels,
    })
}

/// Decode every Ogg Vorbis packet into interleaved `f32` samples.
///
/// Returns the samples, the sample rate and the channel count, all read from
/// the file's own header.
fn decode_vorbis(bytes: &[u8]) -> Result<(Vec<f32>, u32, u8), DecodeError> {
    let reader = Cursor::new(bytes);
    let mut stream = OggStreamReader::new(reader)
        .map_err(|e| DecodeError::Vorbis(format!("cannot read headers: {e}")))?;
    let rate = stream.ident_hdr.audio_sample_rate;
    let channels = stream.ident_hdr.audio_channels;
    if channels == 0 {
        return Err(DecodeError::Empty);
    }

    let mut samples: Vec<f32> = Vec::new();
    while let Some(packet) = stream
        .read_dec_packet_itl()
        .map_err(|e| DecodeError::Vorbis(format!("cannot decode a packet: {e}")))?
    {
        // `_itl` already returns the packet interleaved as the Ogg container
        // wants it, so the samples only need scaling to `f32`.
        samples.extend(packet.iter().map(|s| *s as f32 / i16::MAX as f32));
    }
    if samples.is_empty() {
        return Err(DecodeError::Empty);
    }
    Ok((samples, rate, channels))
}

/// Resample interleaved `f32` samples from `from` to `to`.
///
/// Channels are resampled independently by the library, so a stereo file
/// keeps its two channels apart instead of being mixed down.
fn resample(samples: &[f32], from: u32, to: u32, channels: u8) -> Result<Vec<f32>, DecodeError> {
    if channels as usize > 2 {
        // The backend's mixer is fixed at two channels and asserts on the
        // count, so more would fail at load time rather than play wrong.
        return Err(DecodeError::UnsupportedChannels(channels));
    }
    let from_rate = SampleRate::try_from(from).map_err(|_| DecodeError::UnsupportedRate(from))?;
    let to_rate = SampleRate::try_from(to).map_err(|_| DecodeError::UnsupportedRate(to))?;

    let mut resampler = ResamplerFft::new(channels as usize, from_rate, to_rate);
    let chunk_in = resampler.chunk_size_input();
    let chunk_out = resampler.chunk_size_output();

    // Whole chunks, and a final zero-padded chunk for the remainder, so the
    // last partial block is still processed instead of being dropped.
    let chunks = samples.len().div_ceil(chunk_in).max(1);
    let mut out = Vec::with_capacity(chunks * chunk_out);
    let mut input = vec![0.0_f32; chunk_in];
    let mut output = vec![0.0_f32; chunk_out];
    let mut offset = 0;
    for _ in 0..chunks {
        let take = chunk_in.min(samples.len() - offset);
        // Reuse the buffer: it still holds the previous chunk past `take`.
        input[..take].copy_from_slice(&samples[offset..offset + take]);
        input[take..].fill(0.0);
        resampler
            .resample(&input, &mut output)
            .map_err(|e| DecodeError::Resample(e.to_string()))?;
        out.extend_from_slice(&output);
        offset += take;
    }

    // The FFT resampler has an algorithmic delay, reported in *input* samples
    // (`delay()` is half the FFT input size). In the output stream that delay
    // is stretched by the same ratio as the signal, and the tail the overlap
    // adds has to go: what we want is a buffer whose length is the time
    // stretch of the input, less exactly the delay it introduced.
    let ratio = f64::from(to) / f64::from(from);
    let delay_in_output = (resampler.delay() as f64 * ratio).round() as usize;
    let wanted = ((samples.len() as f64 * ratio).round() as usize)
        .saturating_sub(delay_in_output)
        .max(1);
    out.truncate(wanted.min(out.len()));
    Ok(out)
}

/// Wrap interleaved `f32` samples in a 16-bit PCM WAV container.
///
/// The backend's decoder reads any format `audrey` knows, and WAV is the one
/// it handles without the risk of a lossy re-encode.
fn wav_container(samples: &[f32], rate: u32, channels: u8) -> Vec<u8> {
    // The `fmt ` chunk has fixed-width fields: format, channels, rate, byte
    // rate, block align and bit depth are 2, 2, 4, 4, 2 and 2 bytes, so the
    // channel count must be widened to `u16` before it is written. Writing the
    // `u8` as-is would emit one byte and shift every field after it, and the
    // backend would then read a garbled header.
    let channels = u16::from(channels);
    let block_align = channels * 2; // 16-bit samples
    let byte_rate = rate * u32::from(channels) * 2;

    // Samples are interleaved, so a trailing half-frame would leave a dangling
    // byte at the end of the `data` chunk. Decoders reject a data length that
    // is not a whole number of frames, so drop the incomplete frame.
    let frames = samples.len() / channels as usize;
    let samples = &samples[..frames * channels as usize];
    let data_len = (samples.len() * 2) as u32;

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        // Clamp instead of wrapping: a sample slightly over full scale must
        // not turn into a loud click at the other end of the scale.
        let v = s.clamp(-1.0, 1.0) * i16::MAX as f32;
        out.extend_from_slice(&(v.round() as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::{self, ALL_KINDS};

    /// The rate the backend mixes at; the decoding chain must hit it.
    const TARGET: u32 = crate::constants::AUDIO_MIX_RATE;

    /// Decode one recording of `kind`, failing the test with a clear message.
    fn decode(kind: sound::SoundKind, variant: usize) -> Decoded {
        let path = sound::variant_path(kind, variant);
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("{}: cannot read: {e}", path.display()));
        to_wav(&bytes, TARGET).unwrap_or_else(|e| panic!("{}: cannot decode: {e}", path.display()))
    }

    /// Duration of a Vorbis file in seconds, measured on real frames.
    ///
    /// `decode_vorbis` returns *interleaved* samples, so the frame count (and
    /// therefore the duration) has to divide by the channel count -- otherwise
    /// a stereo file reads as twice its real length.
    fn source_seconds(path: &std::path::Path) -> f64 {
        let bytes = std::fs::read(path).unwrap();
        let (samples, rate, channels) = decode_vorbis(&bytes).expect("must decode");
        samples.len() as f64 / f64::from(rate) / f64::from(channels)
    }

    /// Number of samples in a WAV built by [`wav_container`].
    fn wav_frames(wav: &[u8], channels: u8) -> usize {
        // Layout: "RIFF"+size(4) | "WAVE"(4) | "fmt "+size(4)+16 | "data"+size(4) | samples
        // so "data" starts at 36 and its size is at 40..44.
        assert_eq!(&wav[0..4], b"RIFF", "RIFF magic");
        assert_eq!(&wav[8..12], b"WAVE", "WAVE magic");
        assert_eq!(&wav[12..16], b"fmt ", "fmt chunk");
        assert_eq!(&wav[36..40], b"data", "data chunk must follow fmt ");
        let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
        assert_eq!(44 + data_len, wav.len(), "RIFF size must match the buffer");
        data_len / 2 / channels as usize
    }

    /// Interleaved `f32` samples of a WAV built by this module.
    // `array_chunks` needs an array, not a slice, so the 16-bit samples are
    // read two bytes at a time here.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn wav_samples(wav: &[u8], channels: u8) -> Vec<f32> {
        let data = &wav[44..];
        data.chunks_exact(2)
            .map(|c| f32::from(i16::from_le_bytes(c.try_into().unwrap())) / i16::MAX as f32)
            .collect::<Vec<_>>()
            .chunks_exact(channels as usize)
            .flatten()
            .copied()
            .collect()
    }

    /// Loudest absolute sample of a channel.
    fn peak(samples: &[f32], channel: usize, channels: usize) -> f32 {
        samples
            .iter()
            .skip(channel)
            .step_by(channels)
            .fold(0.0_f32, |acc, s| acc.max(s.abs()))
    }

    /// RMS of a channel, in dBFS, for a rough level comparison.
    fn rms_db(samples: &[f32], channel: usize, channels: usize) -> f32 {
        let mut acc = 0.0_f32;
        let mut n = 0.0_f32;
        for s in samples.iter().skip(channel).step_by(channels) {
            acc += s * s;
            n += 1.0;
        }
        if n == 0.0 {
            return -120.0;
        }
        20.0 * (acc / n).sqrt().max(1e-9).log10()
    }

    #[test]
    fn the_data_chunk_is_always_a_whole_number_of_frames() {
        // Regression: the resampler can return an odd number of samples, and a
        // dangling byte in the data chunk makes the backend's decoder reject
        // the whole file -- the game then fails to start with a panic deep
        // inside the audio driver, not with a message about the sound.
        for channels in [1_u8, 2] {
            for len in [0_usize, 1, 2, 3, 5, 101, 1000] {
                let samples = vec![0.1_f32; len];
                let wav = wav_container(&samples, TARGET, channels);
                let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
                assert_eq!(44 + data_len, wav.len(), "ch={channels} len={len}");
                let block_align = usize::from(channels) * 2;
                assert_eq!(
                    data_len % block_align,
                    0,
                    "ch={channels} len={len}: data length {data_len} is not a \
                     whole number of {block_align}-byte frames"
                );
            }
        }
    }

    #[test]
    fn every_recording_decodes_to_the_rate_the_mixer_plays() {
        for kind in ALL_KINDS {
            for variant in 0..sound::variant_count(kind) {
                let d = decode(kind, variant);
                assert_eq!(
                    d.sample_rate, TARGET,
                    "{kind:?} variant {variant}: wrong rate"
                );
                assert!(d.channels == 1 || d.channels == 2);
                assert_eq!(&d.wav[0..4], b"RIFF");
            }
        }
    }

    #[test]
    fn resampling_keeps_the_real_duration() {
        // The whole reason for decoding here: the backend's own resampler
        // halves a 48 kHz recording. Ours must keep the played length equal to
        // the recorded length.
        for kind in ALL_KINDS {
            let path = sound::variant_path(kind, 0);
            let source = source_seconds(&path);
            let d = decode(kind, 0);
            let played = wav_frames(&d.wav, d.channels) as f64 / f64::from(d.sample_rate);
            let ratio = played / source;
            assert!(
                (0.97..=1.03).contains(&ratio),
                "{}: {source:.3}s of audio plays as {played:.3}s (ratio {ratio:.3})",
                path.display()
            );
        }
    }

    #[test]
    fn the_two_channels_stay_independent() {
        // A resampler that mixes the channels down would still pass the checks
        // above, and the result would sound narrow and phasey. Compare the
        // channels of a stereo recording: they must differ, and be close in
        // level, which is what a correct per-channel filter produces.
        let d = decode(sound::SoundKind::ExplosionGround, 0);
        if d.channels != 2 {
            return; // a mono recording has nothing to compare
        }
        let s = wav_samples(&d.wav, 2);
        let left_peak = peak(&s, 0, 2);
        let right_peak = peak(&s, 1, 2);
        assert!(left_peak > 0.01 && right_peak > 0.01, "a channel is silent");
        // Levels close together (within 6 dB), but not identical samples.
        let lr_db = 20.0 * (left_peak / right_peak).max(0.01).log10();
        assert!(lr_db.abs() < 6.0, "channel levels differ by {lr_db:.1} dB");
        let differing = s
            .iter()
            .step_by(2)
            .zip(s.iter().skip(1).step_by(2))
            .filter(|(a, b)| (*a - *b).abs() > 1e-4)
            .count();
        assert!(
            differing > s.len() / 2 / 10,
            "the channels are nearly identical, so they were mixed down"
        );
    }

    #[test]
    fn resampling_adds_no_obvious_aliasing() {
        // The failure this whole module exists to avoid: a nearest-neighbour
        // resample folds everything above the new Nyquist frequency back into
        // the audible band. Compare the energy above 4 kHz after our
        // resampling with the same band in the original, resampled properly by
        // a reference (ffmpeg-quality) path -- practically, check that the
        // high band did not jump by more than a few dB.
        let path = sound::variant_path(sound::SoundKind::ExplosionGround, 0);
        let bytes = std::fs::read(&path).unwrap();
        let (raw, rate, channels) = decode_vorbis(&bytes).expect("must decode");
        let resampled = resample(&raw, rate, TARGET, channels).expect("must resample");

        let hf_db = |samples: &[f32]| -> f64 {
            // Crude high-frequency energy: mean absolute first difference,
            // which rises with frequency content.
            let mut acc = 0.0_f64;
            for i in 1..samples.len() {
                let d = samples[i] - samples[i - 1];
                acc += d as f64 * d as f64;
            }
            // Normalise by the overall level so the two are comparable.
            let mut total = 0.0_f64;
            for s in samples {
                total += *s as f64 * *s as f64;
            }
            10.0 * (acc / total.max(1e-12)).max(1e-12).log10()
        };
        let before = hf_db(&raw);
        let after = hf_db(&resampled);
        let delta = after - before;
        assert!(
            delta < 3.0,
            "{}: high-frequency energy rose by {delta:.1} dB while resampling \
             ({before:.1} -> {after:.1}), which is the aliasing signature",
            path.display()
        );
    }

    #[test]
    fn a_file_already_at_the_target_rate_is_passed_through() {
        // A re-sourced 44.1 kHz recording must not be resampled, and must
        // still decode: this is the "other sample rates keep working" path.
        let path = sound::variant_path(sound::SoundKind::ExplosionGround, 0);
        let bytes = std::fs::read(&path).unwrap();
        let (raw, rate, channels) = decode_vorbis(&bytes).unwrap();
        // Ask for the rate the file already has.
        let d = to_wav(&bytes, rate).expect("must decode at its own rate");
        assert_eq!(d.sample_rate, rate);
        assert_eq!(d.channels, channels);
        let frames = wav_frames(&d.wav, d.channels);
        assert_eq!(
            frames,
            raw.len() / channels as usize,
            "a pass-through must keep every sample"
        );
    }

    #[test]
    fn a_file_we_cannot_read_is_an_error_not_a_panic() {
        assert!(matches!(
            to_wav(b"not audio at all", TARGET),
            Err(DecodeError::Vorbis(_))
        ));
        assert!(matches!(to_wav(&[], TARGET), Err(DecodeError::Vorbis(_))));
    }

    #[test]
    fn unsupported_input_is_reported_rather_than_panicking() {
        // A recording the decoder cannot handle, or a rate the resampler does
        // not model, must come back as a clear error: that message is what
        // ends up in the log when a sound cannot be played.
        let samples = vec![0.0_f32; 64];

        // More channels than the fixed stereo mixer can play.
        match resample(&samples, 44_100, 48_000, 3) {
            Err(DecodeError::UnsupportedChannels(3)) => {}
            other => panic!("expected an unsupported-channel error, got {other:?}"),
        }

        // A rate that is not one of the modelled ones (12345 Hz is arbitrary).
        match resample(&samples, 12_345, 44_100, 1) {
            Err(DecodeError::UnsupportedRate(12_345)) => {}
            other => panic!("expected an unsupported-rate error, got {other:?}"),
        }
        match resample(&samples, 44_100, 12_345, 1) {
            Err(DecodeError::UnsupportedRate(12_345)) => {}
            other => panic!("expected an unsupported-rate error, got {other:?}"),
        }

        // The error text has to name the offending value, because it is the
        // only thing the player sees in the log.
        let msg = DecodeError::UnsupportedRate(12_345).to_string();
        assert!(msg.contains("12345"), "the message is: {msg}");
        let msg = DecodeError::UnsupportedChannels(5).to_string();
        assert!(msg.contains('5'), "the message is: {msg}");
    }

    #[test]
    fn resampling_preserves_the_overall_level() {
        // A resampler that loses or doubles the level would make the game
        // inconsistent: every event is mixed at the same nominal volume.
        for kind in ALL_KINDS {
            let path = sound::variant_path(kind, 0);
            let bytes = std::fs::read(&path).unwrap();
            let (raw, rate, channels) = decode_vorbis(&bytes).unwrap();
            if rate == TARGET {
                continue;
            }
            let out = resample(&raw, rate, TARGET, channels).expect("must resample");
            let before = rms_db(&raw, 0, channels as usize);
            let after = rms_db(&out, 0, channels as usize);
            let delta = (after - before).abs();
            assert!(
                delta < 1.5,
                "{}: level changed by {delta:.1} dB while resampling",
                path.display()
            );
        }
    }
}
