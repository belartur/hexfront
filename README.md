# War Regions

A real-time strategy game on a hexagonal board, implemented in Python with
**pygame**.  The game rules live in [rules.md](rules.md), the implementation
specification in [specification.md](specification.md).

## Running

```bash
python3 main.py          # requires pygame (pip install pygame)
```

## Gameplay

* 2-4 players per level: you (blue) plus AI opponents.
* Real-time combat.  Units spawn in your bases (5 per 10 s, up to capacity).
* Click **LMB** on your building to select it, **LMB** on a target building to
  preview the route, then press **Enter** (or click **LMB** a third time) to
  send *all* units as a vehicle.  **RMB**/**Esc** cancels.
* Capture every building and destroy every enemy vehicle to win.
* Ground vehicles move only between tiles of equal height; ramps join
  different heights and bridges cross water.  Helicopters fly anywhere.

### Controls

| Input                          | Action                          |
|--------------------------------|---------------------------------|
| LMB drag / arrows / WASD / edge| pan the view                    |
| mouse wheel / `+` / `-`        | zoom (0.5x - 2x)                |
| LMB, Enter, RMB, Esc           | sending vehicles (see above)    |
| `P`                            | pause                           |
| `Esc`                          | back to menu (unless selecting) |

## Code layout

```
war_regions/
  constants.py   every tunable value (documented; rules.md units "j")
  hexgrid.py     flat-top hex geometry (odd-q offset coordinates)
  board.py       tiles, obstacles, ramps, bridges, path-finding
  entities.py    players, buildings, vehicles
  game.py        real-time simulation (production, combat, turrets, ...)
  ai.py          AI decision loop (rules.md sec. 13)
  levels.py      the menu levels, generated from code with fixed seeds
  camera.py      isometric projection and view transforms
  render.py      code-drawn isometric renderer (no raster assets)
  app.py         menu, loading screen, input handling, HUD
main.py          entry point
tests/test_logic.py   headless rule tests:      python3 -m tests.test_logic
tests/test_render.py  rendering regression test: python3 -m tests.test_render
```

The conversion **1 j = 1 px** at 1:1 zoom is defined once in
`war_regions/constants.py` (`UNIT_J_TO_PX`); zoom and window scaling affect
rendering only.  The hexagon side is 36 j (flat-top layout).
