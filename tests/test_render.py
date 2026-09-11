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

from war_regions import constants as C                      # noqa: E402
from war_regions.app import Application                     # noqa: E402
from war_regions.board import Board                         # noqa: E402
from war_regions.camera import Camera                       # noqa: E402
from war_regions.constants import WATER_COLOR               # noqa: E402
from war_regions.render import Renderer                     # noqa: E402


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
    from war_regions.entities import Vehicle
    from war_regions.constants import VehicleKind
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


if __name__ == "__main__":
    test_view_clears_on_pan_and_zoom()
    test_view_culling_covers_screen()
    test_vehicle_on_bridge_deck_not_on_ground()
    print("OK   test_view_clears_on_pan_and_zoom")
    print("OK   test_view_culling_covers_screen")
    print("OK   test_vehicle_on_bridge_deck_not_on_ground")
    print("\nAll render tests passed.")
