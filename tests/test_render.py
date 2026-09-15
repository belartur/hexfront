"""Rendering regression tests (headless, dummy video driver).

Run with:  python3 -m tests.test_render
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
os.environ.setdefault("SDL_VIDEODRIVER", "dummy")

import numpy as np                                          # noqa: E402
import pygame                                               # noqa: E402

from types import SimpleNamespace                           # noqa: E402

from hexfront import constants as C                      # noqa: E402
from hexfront.app import Application                     # noqa: E402
from hexfront.board import Board                         # noqa: E402
from hexfront.camera import Camera                       # noqa: E402
from hexfront.constants import WATER_COLOR               # noqa: E402
from hexfront.render import Renderer                     # noqa: E402


def water_fraction(surf: pygame.Surface) -> float:
    """Fraction of pixels that are plain open-water colour."""
    a = pygame.surfarray.array3d(surf).astype(int)
    water = np.array(WATER_COLOR)
    return float((np.abs(a - water).sum(axis=2) < 12).mean())


def test_view_clears_on_pan_and_zoom():
    """Outside the board only open water may render (rules.md sec. 1).

    Regression test: the renderer never cleared the screen, so panning or
    zooming left stale pixels outside the board area.
    """
    app = Application()
    app._draw()                                    # populate the menu rects
    app._click(app.menu_rects[0][0].center)
    for _ in range(80):
        app._update(1 / 60)
        app._draw()
    assert app.state == 2                          # now PLAYING

    # Panning is limited to the board (specification): a huge pan clamps
    # so that an extreme board tile sits at the screen centre.  The view
    # still shows board plus open sea, with no stale pixels.
    app.camera.pan_projected(-50000, -50000)
    x_min, x_max, y_min, y_max = app.camera.bounds
    wx = (app.camera.x / C.ISO_COS + app.camera.y / C.ISO_SIN) / 2.0
    wy = (app.camera.y / C.ISO_SIN - app.camera.x / C.ISO_COS) / 2.0
    assert x_min <= wx <= x_max and y_min <= wy <= y_max
    app.renderer.draw_world(app.game, app.camera)
    pygame.display.flip()
    frac = water_fraction(pygame.display.get_surface())
    assert 0.0 < frac < 1.0, "expected board + sea at the pan limit"

    # Back on the board, zoom to both extremes: mixed view, no crash and
    # still no stale pixels from earlier frames.
    app.camera.center_on_world(*app.game.board.center_world((10, 6)))
    for zoom in (0.5, 1.0, 2.0, 0.5, 2.0):
        app.camera.zoom = zoom
        app.renderer.draw_world(app.game, app.camera)
        pygame.display.flip()
        frac = water_fraction(pygame.display.get_surface())
        assert 0.0 < frac < 1.0, "expected board + sea in the view"


def test_view_culling_covers_screen():
    """_visible_tiles contains every tile whose projection reaches the
    screen, at any zoom (huge boards are drawn tile-culled)."""
    screen = pygame.display.set_mode((1180, 720))
    renderer = Renderer(screen)
    board = Board(60, 40)
    import random
    rng = random.Random(7)
    for t in board.tiles.values():
        t.height = rng.randrange(16)
    camera = Camera(screen.get_size())
    camera.limit_to_board(board)
    scene = SimpleNamespace(board=board)     # what the painter accesses
    w, h = screen.get_size()
    for zoom in (0.5, 1.0, 2.0):
        camera.zoom = zoom
        camera.center_on_world(*board.center_world((30, 20)))
        visible = renderer._visible_tiles(scene, camera)
        assert len(visible) < len(board.tiles)      # culling actually culls
        for tile, t in board.tiles.items():
            x, y = board.center_world(tile)
            for wz in (0.0, t.height * C.ELEVATION_PX):
                sx, sy = camera.world_to_screen(x, y, wz)
                if 0 <= sx < w and 0 <= sy < h:
                    assert tile in visible, (tile, zoom, (sx, sy))
    pygame.quit()


def test_vehicle_on_bridge_deck_not_on_ground():
    """Vehicles travelling along a bridge stand on the deck (rules.md sec. 8).

    Regression test: ground (non-helicopter) vehicles crossing a bridge
    were drawn at the terrain height under the deck (e.g. at the bottom
    of the water they span), instead of on the deck itself.
    """
    from hexfront.entities import Vehicle
    from hexfront.constants import VehicleKind
    screen = pygame.display.set_mode((640, 480))
    renderer = Renderer(screen)
    board = Board(14, 14)
    for t in board.tiles.values():
        t.height = 3
    for t in [(5, 6), (5, 7)]:
        board.tiles[t].height = 0
    bridge = board.add_bridge((5, 5), (5, 8), 1)
    assert bridge is not None
    scene = SimpleNamespace(board=board)
    deck_z = bridge.w * C.ELEVATION_PX
    # A tank driving along the deck: source -> fragments -> far end.
    route = [(5, 6), (5, 7), (5, 8)]
    for pos_tile, index in [((5, 5), 0), ((5, 6), 1), ((5, 7), 2)]:
        x, y = board.center_world(pos_tile)
        v = Vehicle(VehicleKind.TANK, 0, 10.0, route,
                    board.center_world((5, 5)), src_tile=(5, 5))
        v.route_index = index
        v.x, v.y = x, y
        tile = board.world_to_tile(x, y)
        prev, nxt = renderer._route_endpoints(v)
        assert renderer._ground_z(scene, tile, (x, y), prev, nxt) \
            == deck_z, (pos_tile, tile, prev, nxt)
        assert renderer._vehicle_z(scene, v) == deck_z + 6.0
    # A hovercraft sailing *under* the bridge stays at water level.
    h = Vehicle(VehicleKind.HOVERCRAFT, 1, 10.0, [(4, 6)],
                board.center_world((6, 6)), src_tile=(6, 6))
    h.x, h.y = board.center_world((5, 6))
    tile = board.world_to_tile(h.x, h.y)
    assert renderer._ground_z(scene, tile, (h.x, h.y),
                              *renderer._route_endpoints(h)) == 0.0
    assert renderer._vehicle_z(scene, h) == 6.0
    pygame.quit()


def test_ramp_frame_and_height():
    """Ramp edges face the joined neighbours; deck height interpolates.

    Regression test: the strip used centre-based anchors and there was
    no Renderer._ramp_z at all (AttributeError for vehicles/shadows on
    ramps); the frame must run edge-midpoint to edge-midpoint and the
    deck height must span ha..hb (rules.md sec. 7).
    """
    import math
    from hexfront import hexgrid
    screen = pygame.display.set_mode((640, 480))
    renderer = Renderer(screen)
    board = Board(14, 12)
    for t in board.tiles.values():
        t.height = 1
    p = (7, 6)
    for d in range(3):
        a = hexgrid.neighbor(p[0], p[1], d)
        b = hexgrid.neighbor(p[0], p[1], (d + 3) % 6)
        board.tiles[a].height = 1
        board.tiles[b].height = 4
        board.set_ramp(p, a, b)
        t = board.tiles[p]
        axis, edge_a, edge_b, ha, hb = renderer._ramp_frame(board, p, t)
        assert (ha, hb) == (1, 4)
        # Edges face their neighbours: midpoint nearest to its centre.
        ac = board.center_world(a)
        bc = board.center_world(b)
        assert math.hypot(edge_a[0] - ac[0], edge_a[1] - ac[1]) < 32.0
        assert math.hypot(edge_b[0] - bc[0], edge_b[1] - bc[1]) < 32.0
        # Axis points a -> b and is perpendicular to both hex edges.
        assert (edge_b[0] - edge_a[0]) * axis[0] +             (edge_b[1] - edge_a[1]) * axis[1] > 0
        scene = SimpleNamespace(board=board)
        assert abs(renderer._ground_z(scene, p, edge_a)
                   - ha * C.ELEVATION_PX) < 1e-6
        assert abs(renderer._ground_z(scene, p, edge_b)
                   - hb * C.ELEVATION_PX) < 1e-6
        mid = ((edge_a[0] + edge_b[0]) / 2.0,
               (edge_a[1] + edge_b[1]) / 2.0)
        assert abs(renderer._ground_z(scene, p, mid)
                   - (ha + hb) / 2.0 * C.ELEVATION_PX) < 1e-6
        board.remove_ramp(p)
    pygame.quit()


def test_ramp_strip_stays_visible_over_high_skirt():
    """The strip end in front is not covered by the high wall behind.

    Regression test: with a large height gap the high neighbour (drawn
    later under a single per-tile depth key) painted its full-width
    skirt over the narrower strip lying in front of it.
    """
    from hexfront import hexgrid
    screen = pygame.display.set_mode((800, 600))
    renderer = Renderer(screen)
    board = Board(14, 12)
    for t in board.tiles.values():
        t.height = 1
    p = (7, 6)
    a = hexgrid.neighbor(p[0], p[1], 0)
    b = hexgrid.neighbor(p[0], p[1], 3)
    board.tiles[a].height = 1
    board.tiles[b].height = 6
    board.set_ramp(p, a, b)
    scene = SimpleNamespace(board=board, buildings=[], vehicles=[])
    camera = Camera(screen.get_size())
    camera.center_on_world(*board.center_world(p))
    renderer._draw_tiles(scene, camera)
    axis, edge_a, edge_b, ha, hb = renderer._ramp_frame(
        board, p, board.tiles[p])
    za = ha * C.ELEVATION_PX
    mx = (edge_a[0] + edge_b[0]) / 2.0
    my = (edge_a[1] + edge_b[1]) / 2.0
    mza = (ha + hb) / 2.0 * C.ELEVATION_PX
    mid = camera.world_to_screen(mx, my, mza)
    surf = pygame.surfarray.array3d(screen)
    got = tuple(int(v) for v in surf[mid[0], mid[1]])
    assert got == (172, 158, 120), (mid, got)
    # ... and the tall grey wall of the high neighbour is still there.
    top_z = hb * C.ELEVATION_PX
    bcx, bcy = board.center_world(b)
    skirt_probe = camera.world_to_screen(bcx, bcy, (hb - 1) * C.ELEVATION_PX)
    sx = max(0, min(screen.get_width() - 1, skirt_probe[0]))
    sy = max(0, min(screen.get_height() - 1, skirt_probe[1]))
    got2 = tuple(int(v) for v in surf[sx, sy])
    assert got2 != (172, 158, 120), ((sx, sy), got2)
    _ = (za, top_z)
    pygame.quit()


def test_turret_barrels_differ():
    """The three turret kinds draw different barrels (Wariant A)."""
    from hexfront.entities import Building, BuildingKind
    pygame.init()
    screen = pygame.display.set_mode((640, 480))
    renderer = Renderer(screen)
    board = Board(10, 10)
    for t in board.tiles.values():
        t.height = 1
    camera = Camera(screen.get_size())
    camera.center_on_world(*board.center_world((5, 5)))
    scene = SimpleNamespace(board=board, buildings=[], vehicles=[])
    kinds = [BuildingKind.TURRET_NORMAL, BuildingKind.TURRET_RAPID,
             BuildingKind.TURRET_ROCKET]
    shots = []
    for kind in kinds:
        screen.fill((0, 0, 0))
        b = Building(kind, 0, 5, 5, units=10)
        scene.buildings = [b]
        renderer._draw_tiles(scene, camera)
        renderer._draw_objects(scene, camera, None, None)
        shots.append(pygame.surfarray.array3d(screen).copy())
    import numpy as _np
    for i in range(3):
        for j in range(i + 1, 3):
            diff = _np.abs(shots[i].astype(int) - shots[j].astype(int)).sum()
            assert diff > 0, (kinds[i], kinds[j])
    pygame.quit()


def test_editor_draws_turret_and_heal_ranges():
    """Editor shows turret + owned-heal ranges (editor spec, sec. 30)."""
    from editor import EditorScene
    from hexfront.entities import Building, BuildingKind
    pygame.init()
    screen = pygame.display.set_mode((640, 480))
    renderer = Renderer(screen)
    board = Board(16, 12)
    for t in board.tiles.values():
        t.height = 1
    buildings = [
        Building(BuildingKind.TURRET_NORMAL, 0, 4, 6, units=10),
        Building(BuildingKind.TURRET_RAPID, 0, 8, 6, units=10),
        Building(BuildingKind.TURRET_ROCKET, 0, 12, 6, units=10),
        Building(BuildingKind.HEAL_TOWER, 0, 6, 8, units=10),
        Building(BuildingKind.HEAL_TOWER, None, 10, 8, units=10),
    ]
    scene = EditorScene(board, buildings)
    camera = Camera(screen.get_size())
    camera.center_on_world(*board.center_world((8, 6)))
    # 3 turrets + 1 owned heal tower; the neutral heal tower has no
    # range overlay (editor spec).
    assert len(renderer._range_circles(scene, camera)) == 4
    # The translucent fills must survive draw_editor (regression: the
    # overlay used to be painted before the tiles, so the opaque tiles
    # covered the fills and only the outlines stayed visible).  Compare
    # the full editor frame against bare tiles: the ranges must tint a
    # significant part of the view.
    import numpy as _np2
    renderer.draw_editor(scene, camera)
    with_ranges = pygame.surfarray.array3d(screen).astype(int).copy()
    scene.buildings = []
    renderer.draw_editor(scene, camera)
    bare = pygame.surfarray.array3d(screen).astype(int)
    changed = (_np2.abs(with_ranges - bare).sum(axis=2) > 30).mean()
    assert changed > 0.05, changed
    pygame.quit()


if __name__ == "__main__":
    test_view_clears_on_pan_and_zoom()
    test_view_culling_covers_screen()
    test_vehicle_on_bridge_deck_not_on_ground()
    test_ramp_frame_and_height()
    test_ramp_strip_stays_visible_over_high_skirt()
    test_turret_barrels_differ()
    test_editor_draws_turret_and_heal_ranges()
    print("OK   test_view_clears_on_pan_and_zoom")
    print("OK   test_view_culling_covers_screen")
    print("OK   test_vehicle_on_bridge_deck_not_on_ground")
    print("OK   test_ramp_frame_and_height")
    print("OK   test_ramp_strip_stays_visible_over_high_skirt")
    print("OK   test_turret_barrels_differ")
    print("OK   test_editor_draws_turret_and_heal_ranges")
    print("\nAll render tests passed.")
