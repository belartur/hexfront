# Sound files

The combat event sounds the game plays. All of them come from the **"25 CC0
bang / firework SFX"** pack by **rubberduck**, published on
[OpenGameArt.org](https://opengameart.org/content/25-CC0-bang-firework-sfx)
(the author released it under **CC0 1.0** — public domain dedication, so there
are no restrictions on use, modification or redistribution, commercial
included).

## Files

| File | Source recording | Used for |
|---|---|---|
| `explosion_ground_1..3.ogg` | `bang_06`, `bang_01`, `bang_09` | ground vehicle destroyed (3 variants) |
| `explosion_air_1..2.ogg` | `bang_10`, `bang_07` | helicopter destroyed (2 variants) |
| `turret_normal_1..2.ogg` | `cannon_01`, `cannon_04` | normal turret firing (2 variants) |
| `turret_rapid_1..2.ogg` | `shot_02`, `shot_01` | rapid turret firing (2 variants) |
| `turret_rocket_1..2.ogg` | `cannon_02`, `cannon_03` | rocket turret firing (2 variants) |
| `vehicle_fire_1..2.ogg` | `cannon_05`, `shot_01` | vehicle firing in a duel (2 variants) |
| `impact_1..2.ogg` | `shot_03`, `shot_02` | projectile hitting its target (2 variants) |
| `wall_hit_1.ogg` | `shot_03` | vehicle shooting at a wall |

## How the files were prepared

**The files are stored exactly as they were published** — the original 48 kHz
stereo recordings in Ogg Vorbis, with no processing at all. After downloading
the pack, each recording is copied under the name above (`cp bang_06.ogg
explosion_ground_1.ogg`, and so on) and committed as is.

Everything else happens at start-up in
[`rust/src/decode.rs`](../rust/src/decode.rs): the file is decoded (`lewton`),
its sample rate is corrected to 44100 Hz (`resampler`), and the result reaches
the mixer as a WAV held in memory.

### Why the files are not re-encoded to 44.1 kHz

Vorbis is a lossy format, and every extra generation of encoding costs real
quality. Measured on `bang_06`: re-encoding at quality 3 alone costs about
0.13 dB RMS, and adding filters on top of that would take nearly 3 dB more.
Since the game has to decode the file to samples anyway, doing that once, in
one step, is better than doing it twice with a loss in between.

### Why the backend is not allowed to do it

The audio backend would decode the file just fine, but its resampler is a
**nearest-neighbour copy with no filtering**. For a 48 kHz recording that
shortens the file to half its length (a 1.364 s explosion becomes 0.682 s) and
raises the high-frequency energy by about 8.6 dB, so the explosion would sound
like a short hissy thud instead. `decode.rs` therefore resamples the audio
itself, and the backend receives 44100 Hz already, which makes it skip that
flawed path entirely.

### Other sample rates

The rate is read from each file's header rather than assumed, so a recording
sampled at a different rate (44.1 kHz, 96 kHz, anything the `resampler` crate
models) works without any code change. A file already at 44.1 kHz is passed
through without conversion. A file outside the rates the crate models, or with
more than two channels, ends in a readable message in the log rather than in
silence.

### Channels

The recordings are stereo and stay that way. The backend always plays two
channels, so a mono file would be split across both anyway — a downmix would
only make the file smaller on disk, which is no longer a goal now that the
files are not re-encoded.

