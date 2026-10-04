# Hexfront

A real-time strategy game on a hexagonal board.  The game rules live in
[rules.md](rules.md); [specification.md](specification.md) is the
specification shared by all implementations.  The implementation is
**Rust** (macroquad), specified in
[specification_rust.md](specification_rust.md), with the game and tests in
the `rust/` directory.  The rules, the specifications and the `maps/`
directory stay in the repository root, so further language implementations
can be added next to `rust/` without touching them.

## Running

```bash
cd rust && cargo run --release  # the game (requires the Rust toolchain)
```

## Gameplay

* 2-4 players per level: you (blue) plus AI opponents.
* Real-time combat.  Units spawn in your bases (5 per 10 s, up to capacity).
* Click **RMB** on your building (or **LMB** when nothing is selected yet)
  to select it; then **LMB** on any other building — or on a bonus field —
  sends *all* its units there as a vehicle.  Your own buildings are
  valid targets too, so you can transfer units between them.  **Esc**
  (or **RMB** off-building) cancels.
* Bonuses are one-shot fields marked with a yellow ring: send a vehicle to
  one for `+x` units, `*x` units, or a drone that guards its carrier (and
  rebuilds the field if the carrier dies).  The vehicle drives back to the
  building it came from with the effect applied.
* Capture every building and destroy every enemy vehicle to win.
* Ground vehicles move only between tiles of equal height; ramps join
  different heights and bridges cross water.  Helicopters fly anywhere.

### Controls

| Input                          | Action                          |
|--------------------------------|---------------------------------|
| LMB drag / arrows / WASD / edge| pan the view (within the board) |
| mouse wheel / `+` / `-`        | zoom (0.5x - 2x)                |
| `Alt`                          | pick the tile as if all fields stood at height 0 |
| LMB, RMB, Esc                 | selecting & sending vehicles    |
| `P`                            | pause                           |
| `Esc`                          | back to menu (unless selecting) |

## Levels and the board editor

Levels live as binary map files in the `maps/` directory; the menu lists
every `maps/*.map` file and shows the file name as the level name.  The
file format (dimensions, 4-bit heights,
buildings/ramps/bridges/obstacles/bonuses)
is specified in `specification_of_map_format.md` and implemented in
`rust/src/mapfile.rs`.  The bundled maps are versioned in the repository;
new ones are created with the built-in editor.

The board editor is part of the game, not a separate program: press `add
map` in the main menu to open it with a new 256x256 board, or right-click a
map in the menu to edit it (left-click starts the game instead).  It shares
the game's board renderer and tile picking.  Editing is key-driven: point
a tile with the mouse and press a key — `b` building (again: cycle kind;
new buildings reuse the last kind/owner/units), `i` bonus (again: cycle
`+x` / `*x` / drone; new bonuses reuse the last kind and value), digits
units 0-999 or bonus value 1-999 (`+x`) / 2-99 (`*x`), `o`
owner, `t` obstacle (again: cycle kind; new obstacles reuse the last
kind), `m` bridge, `r` ramp, `[`/`]` terrain -/+ (no wrap), `Del`/RMB
delete, `l` load, `s` save (empty water borders are trimmed on save),
ctrl+S save, ctrl+N new 256x256 board, `Alt` pick at height 0 (placing
overwrites the previous object; a legend is shown on screen; rule
violations are listed in red; the editor asks about unsaved changes on
exit), and `p` starts a playtest game on the edited level (see
`specification_rust.md`).

## Code layout

```text
rust/                           the implementation (macroquad; this repository
                                root holds rules.md, specification*.md and maps/)
  src/main.rs           entry point (cd rust && cargo run --release)
  src/constants.rs      values shared by several modules (rules.md units "j";
                        single-module constants live next to their code)
  src/hexgrid.rs        flat-top hex geometry (odd-q offset coordinates)
  src/math.rs           scalar helpers shared by simulation and mesh builders
  src/board.rs          tiles, obstacles, ramps, bridges, path-finding, picking
  src/entities.rs       players, buildings, vehicles, bonuses, drones
  src/game.rs           real-time simulation (production, combat, turrets,
                        bonus missions and drones, ...)
  src/ai.rs             AI decision loop (rules.md sec. 14; bonuses sec. 13)
  src/rng.rs            deterministic PRNG for the AI noise (no extra crates)
  src/mapfile.rs        binary map file format: save / load / list maps
  src/fx.rs             explosion particle system (presentation only, headless)
  src/sound.rs          sound catalogue: event -> Ogg Vorbis file (headless)
  src/decode.rs         sound decoding and resampling to 44100 Hz (headless)
  src/audio.rs          sound playback through the macroquad backend
  src/camera.rs         isometric projection and view transforms
  src/iso.rs            orthographic GPU camera reproducing the 2D projection
  src/mesh/             CPU mesh building without macroquad, split by subject:
                        mod (vertex types, solid primitives, DynamicMesh,
                        fx particle geometry), terrain, surface, buildings,
                        obstacles, vehicles, overlays, tests
  src/render.rs         code-drawn isometric renderer (no raster assets)
  src/editor.rs         map editor as a game state (no separate program)
  src/app.rs            menu, input handling, HUD
  unit tests            headless rule + map tests:   cd rust && cargo test
```

The conversion **1 j = 1 px** at 1:1 zoom is defined once in
`rust/src/constants.rs` (`UNIT_J_TO_PX`); zoom and window scaling affect
rendering only.  The hexagon side is 36 j (flat-top layout).

The cost of building the meshes is measured by `src/render_baseline.rs`
(`cd rust && cargo test --release render_baseline -- --ignored --nocapture`).
These are measurements, not timing assertions or a guarantee of interactive FPS.
