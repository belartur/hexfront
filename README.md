# War Regions

A real-time strategy game on a hexagonal board, implemented in Python with
**pygame**.  The game rules live in [rules.md](rules.md), the implementation
specification in [specification.md](specification.md).

## Running

```bash
python3 main.py          # requires pygame (pip install pygame)
python3 editor.py        # the board editor (separate application)
```

## Gameplay

* 2-4 players per level: you (blue) plus AI opponents.
* Real-time combat.  Units spawn in your bases (5 per 10 s, up to capacity).
* Click **RMB** on your building (or **LMB** when nothing is selected yet)
  to select it; then **LMB** on any other building sends *all* its units
  there as a vehicle — your own buildings included, so you can transfer
  units between them.  **Esc** (or **RMB** off-building) cancels.
* Capture every building and destroy every enemy vehicle to win.
* Ground vehicles move only between tiles of equal height; ramps join
  different heights and bridges cross water.  Helicopters fly anywhere.

### Controls

| Input                          | Action                          |
|--------------------------------|---------------------------------|
| LMB drag / arrows / WASD / edge| pan the view                    |
| mouse wheel / `+` / `-`        | zoom (0.5x - 2x)                |
| LMB, RMB, Esc                 | selecting & sending vehicles    |
| `P`                            | pause                           |
| `Esc`                          | back to menu (unless selecting) |

## Levels and the board editor

Levels live as binary map files in the `maps/` directory; the menu lists
every `maps/*.map` file and shows the file name as the level name.  The
file format (dimensions, 4-bit heights, buildings/ramps/bridges/obstacles)
is specified in `specification.md` ("Format pliku planszy") and implemented
in `war_regions/mapfile.py`.

The board editor is a separate application sharing the game's board
renderer.  It offers terrain raise/lower, building, ramp, bridge and
obstacle tools (with the placement rules of rules.md sec. 1, 7, 8
enforced), saves to and loads from `maps/`:

```bash
python3 editor.py                # empty 20x13 board
python3 editor.py maps/Zatoka.map
```

`python3 make_maps.py` regenerates the bundled sample maps from the
procedural generator in `war_regions/levels.py`.

## Code layout

```
war_regions/
  constants.py   every tunable value (documented; rules.md units "j")
  hexgrid.py     flat-top hex geometry (odd-q offset coordinates)
  board.py       tiles, obstacles, ramps, bridges, path-finding
  entities.py    players, buildings, vehicles
  game.py        real-time simulation (production, combat, turrets, ...)
  ai.py          AI decision loop (rules.md sec. 13)
  levels.py      procedural map generator (also feeds make_maps.py)
  mapfile.py     binary map file format: save / load / list maps
  camera.py      isometric projection and view transforms
  render.py      code-drawn isometric renderer (no raster assets)
  app.py         menu, loading screen, input handling, HUD
main.py          entry point
editor.py        board editor entry point
make_maps.py     regenerates the sample maps in maps/
tests/test_logic.py   headless rule tests:      python3 -m tests.test_logic
tests/test_render.py  rendering regression test: python3 -m tests.test_render
tests/test_mapfile.py  map format & editor tests: python3 -m tests.test_mapfile
```

The conversion **1 j = 1 px** at 1:1 zoom is defined once in
`war_regions/constants.py` (`UNIT_J_TO_PX`); zoom and window scaling affect
rendering only.  The hexagon side is 36 j (flat-top layout).
