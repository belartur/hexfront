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
from hexfront.depth import DepthBuffer, DepthCamera
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


def test_far_building_does_not_cover_near_high_terrain():
    """Far buildings/vehicles must not cover nearer high terrain.

    Regression test: tiles used to paint in one pass (skirts + tops)
    and buildings/vehicles in later passes, so a building or vehicle
    behind a nearer hill was painted *over* that hill. Depth testing
    now compares the actual surface fragments, so the nearer
    high tile covers the farther object's body, while a nearer
    object still covers terrain behind it (checked below both ways).
    """
    from hexfront.entities import Building, BuildingKind, Vehicle
    from hexfront.constants import VehicleKind
    pygame.init()
    screen = pygame.display.set_mode((640, 480))
    renderer = Renderer(screen)
    camera = Camera(screen.get_size())
    ref = Board(10, 10)
    camera.center_on_world(*ref.center_world((4, 4)))
    scene_of = lambda board, buildings, vehicles: \
        SimpleNamespace(board=board, buildings=buildings, vehicles=vehicles)

    def frame(board, buildings, vehicles):
        scene = scene_of(board, buildings, vehicles)
        screen.fill((0, 0, 0))
        renderer._draw_tiles(scene, camera)
        renderer._draw_objects(scene, camera, None, None)
        return pygame.surfarray.array3d(screen).copy()

    def body_pixels(draw_fn, scene):
        screen.fill((0, 0, 0))
        renderer._scene = DepthBuffer(screen)
        depth_camera = DepthCamera(camera)
        draw_fn(scene, depth_camera)
        renderer._scene = None
        alone = pygame.surfarray.array3d(screen).copy()
        return (np.abs(alone.astype(int)).sum(axis=2) > 30)

    def visible_body_pixels(composed, bare, body):
        diff = (np.abs(composed.astype(int)
                       - bare.astype(int)).sum(axis=2) > 30)
        return int((body & diff).sum())

    # Far building (4, 5) behind the near hill (5, 5, h=8): most of the
    # body must be hidden by the hill painted after it.
    board = Board(10, 10)
    for t in board.tiles.values():
        t.height = 1
    far, near = (4, 5), (5, 5)
    board.tiles[near].height = 8
    b = Building(BuildingKind.BASE_TANK, 1, *far, units=10)
    scene = scene_of(board, [b], [])
    body = body_pixels(lambda s, c: renderer._draw_building(s, c, b,
                                                         False), scene)
    assert int(body.sum()) > 1000
    visible = visible_body_pixels(frame(board, [b], []),
                                  frame(board, [], []), body)
    assert visible < int(body.sum()) // 2, (visible, int(body.sum()))

    # Reverse: a building on the near tile covers the terrain behind.
    board2 = Board(10, 10)
    for t in board2.tiles.values():
        t.height = 1
    b2 = Building(BuildingKind.BASE_TANK, 1, *near, units=10)
    scene2 = scene_of(board2, [b2], [])
    body2 = body_pixels(lambda s, c: renderer._draw_building(s, c, b2,
                                                          False), scene2)
    visible2 = visible_body_pixels(frame(board2, [b2], []),
                                   frame(board2, [], []), body2)
    assert visible2 == int(body2.sum()), (visible2, int(body2.sum()))
    # The vehicle is already ON the high tile, not behind its cliff.
    fx, fy = board.center_world(far)
    nx, ny = board.center_world(near)
    vx, vy = fx + (nx - fx) * 0.7, fy + (ny - fy) * 0.7
    v = Vehicle(VehicleKind.TANK, 1, 10.0, [], (vx, vy), src_tile=far)
    v.x, v.y = vx, vy
    scenev = scene_of(board, [], [v])
    bodyv = body_pixels(lambda s, c: renderer._draw_vehicle(s, c, v),
                        scenev)
    assert int(bodyv.sum()) > 300
    visiblev = visible_body_pixels(frame(board, [], [v]),
                                   frame(board, [], []), bodyv)
    assert board.world_to_tile(v.x, v.y) == near
    assert visiblev == int(bodyv.sum()), (visiblev, int(bodyv.sum()))
    # Same vehicle on open flat ground stays fully visible.
    board3 = Board(10, 10)
    for t in board3.tiles.values():
        t.height = 1
    ox, oy = board3.center_world((4, 4))
    vo = Vehicle(VehicleKind.TANK, 1, 10.0, [], (ox, oy), src_tile=(4, 4))
    vo.x, vo.y = ox, oy
    sceneo = scene_of(board3, [], [vo])
    bodyo = body_pixels(lambda s, c: renderer._draw_vehicle(s, c, vo),
                        sceneo)
    visibleo = visible_body_pixels(frame(board3, [], [vo]),
                                   frame(board3, [], []), bodyo)
    assert visibleo == int(bodyo.sum()), (visibleo, int(bodyo.sum()))
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


def test_vehicle_at_far_edge_of_own_tile():
    """A flat tile must never erase the vehicle standing on its far half."""
    from hexfront.entities import Vehicle
    from hexfront.constants import VehicleKind
    screen = pygame.display.set_mode((640, 480))
    renderer = Renderer(screen)
    board = Board(10, 10)
    camera = Camera(screen.get_size())
    cx, cy = board.center_world((4, 4))
    camera.center_on_world(cx, cy)
    for zoom in (0.5, 1.0, 2.0):
        camera.zoom = zoom
        camera.x += 0.75
        camera.y -= 0.25
        for kind in VehicleKind:
            for dx, dy in ((-12, -12), (0, -24), (-24, 0), (12, 12)):
                v = Vehicle(kind, 0, 10.0, [], (cx + dx, cy + dy))
                scene = SimpleNamespace(board=board, buildings=[], vehicles=[v])
                assert board.world_to_tile(v.x, v.y) == (4, 4)
                screen.fill((0, 0, 0))
                renderer._scene = DepthBuffer(screen)
                renderer._draw_vehicle(scene, DepthCamera(camera), v)
                renderer._scene = None
                alone = pygame.surfarray.array3d(screen)
                body = alone.any(axis=2)
                screen.fill((0, 0, 0))
                renderer._draw_tiles(scene, camera)
                composed = pygame.surfarray.array3d(screen)
                assert np.array_equal(composed[body], alone[body]), \
                    ("Vehicle overwritten by flat ground", kind, zoom, dx, dy)
    pygame.quit()



def test_depth_visibility_is_independent_of_submission_order():
    """Intersecting planes exchange front/back within the same polygon."""
    from hexfront.depth import ProjectedPoint
    screen = pygame.display.set_mode((100, 100))
    frames = []
    flat = [ProjectedPoint(x, y, 0) for x, y in
            ((10, 10), (90, 10), (90, 90), (10, 90))]
    slope = [ProjectedPoint(x, y, x - 50) for x, y in
             ((10, 10), (90, 10), (90, 90), (10, 90))]
    primitives = [((255, 0, 0), flat), ((0, 255, 0), slope)]
    for order in (primitives, primitives[::-1]):
        screen.fill((0, 0, 0))
        depth = DepthBuffer(screen)
        for color, polygon in order:
            depth.polygon(color, polygon)
        frames.append(pygame.surfarray.array3d(screen))
    assert np.array_equal(*frames)
    assert tuple(frames[0][20, 50]) == (255, 0, 0)
    assert tuple(frames[0][80, 50]) == (0, 255, 0)
    pygame.quit()


def test_near_high_tile_occludes_ramp():
    """A ramp submitted last cannot paint over a nearer high cliff/top."""
    from hexfront import hexgrid
    screen = pygame.display.set_mode((640, 480))
    renderer = Renderer(screen)
    board = Board(10, 10)
    p = (4, 4)
    camera = Camera(screen.get_size())
    scene = SimpleNamespace(board=board, buildings=[], vehicles=[])
    for direction in range(6):
        for t in board.tiles.values():
            t.height = 1
        high = hexgrid.neighbor(*p, direction)
        low = hexgrid.neighbor(*p, (direction + 3) % 6)
        board.tiles[high].height = 8
        board.set_ramp(p, high, low)
        for zoom in (0.5, 1.0, 2.0):
            camera.zoom = zoom
            camera.center_on_world(*board.center_world(p))
            c = DepthCamera(camera)

            def frame(order):
                screen.fill((0, 0, 0))
                renderer._scene = DepthBuffer(screen)
                try:
                    for item in order:
                        if item == "ramp":
                            renderer._draw_ramp(scene, c, p, board.tiles[p])
                        else:
                            renderer._draw_skirts(scene, c, high, board.tiles[high])
                            renderer._draw_top(scene, c, high, board.tiles[high])
                    return (pygame.surfarray.array3d(screen),
                            renderer._scene.values.copy())
                finally:
                    renderer._scene = None

            ramp, rd = frame(["ramp"])
            hill, hd = frame(["hill"])
            forward, _ = frame(["ramp", "hill"])
            reverse, _ = frame(["hill", "ramp"])
            overlap = np.isfinite(rd) & np.isfinite(hd)
            front = overlap & (hd > rd + C.DEPTH_EPSILON)
            behind = overlap & (rd > hd + C.DEPTH_EPSILON)
            assert np.array_equal(forward[front], hill[front])
            assert np.array_equal(reverse[front], hill[front])
            assert np.array_equal(forward[behind], ramp[behind])
            assert np.array_equal(reverse[behind], ramp[behind])
            # For the most frontal axis, ensure the assertions are not vacuous.
            if direction == 0:
                assert int(front.sum()) > 10
        board.remove_ramp(p)
    pygame.quit()


def test_terrain_cache_invalidates_after_edit():
    """Cached colour and depth must match a fresh renderer after map edits."""
    from hexfront import hexgrid
    screen = pygame.display.set_mode((640, 480))
    renderer = Renderer(screen)
    board = Board(10, 10)
    camera = Camera(screen.get_size())
    camera.center_on_world(*board.center_world((4, 4)))
    scene = SimpleNamespace(board=board, buildings=[], vehicles=[])
    renderer._draw_tiles(scene, camera)
    for zoom in (0.5, 1.0, 2.0):
        camera.zoom = zoom
        board.tiles[(5, 4)].height += 1
        board.set_ramp((4, 4), (5, 4), hexgrid.neighbor(4, 4, 3))
        renderer._draw_tiles(scene, camera)
        cached = pygame.surfarray.array3d(screen)
        Renderer(screen)._draw_tiles(scene, camera)
        assert np.array_equal(cached, pygame.surfarray.array3d(screen))
        renderer._draw_tiles(scene, camera)
        assert np.array_equal(cached, pygame.surfarray.array3d(screen))
    pygame.quit()


def test_bridge_and_vehicle_pixel_occlusion():
    """Deck fragments cover traffic below, never traffic on the deck."""
    from hexfront.entities import Vehicle
    from hexfront.constants import VehicleKind
    screen = pygame.display.set_mode((640, 480))
    renderer = Renderer(screen)
    board = Board(10, 10)
    for tile in board.tiles.values():
        tile.height = 0
    board.tiles[(4, 3)].height = board.tiles[(4, 6)].height = 3
    bridge = board.add_bridge((4, 3), (4, 6), 1)
    assert bridge is not None
    scene = SimpleNamespace(board=board, buildings=[], vehicles=[])
    camera = Camera(screen.get_size())
    pos = board.center_world((4, 4))
    camera.center_on_world(*pos)
    tank = Vehicle(VehicleKind.TANK, 0, 10.0, [(4, 4), (4, 5)],
                   pos, src_tile=(4, 3))
    hover = Vehicle(VehicleKind.HOVERCRAFT, 1, 10.0, [(3, 4)],
                    pos, src_tile=(5, 4))
    for zoom in (0.5, 1.0, 2.0):
        camera.zoom = zoom
        c = DepthCamera(camera)
        for vehicle in (tank, hover):
            def frame(deck):
                screen.fill((0, 0, 0))
                renderer._scene = DepthBuffer(screen)
                try:
                    renderer._draw_vehicle(scene, c, vehicle)
                    if deck:
                        renderer._draw_bridge_fragment(scene, c, bridge, (4, 4))
                    return pygame.surfarray.array3d(screen)
                finally:
                    renderer._scene = None
            alone = frame(False)
            composed = frame(True)
            body = alone.any(axis=2)
            changed = np.any(alone != composed, axis=2) & body
            if vehicle is tank:
                assert not changed.any(), (zoom, int(changed.sum()))
            else:
                assert changed.any(), "The deck must hide part of the hovercraft"
    pygame.quit()


def test_surface_details_keep_full_width_but_respect_occlusion():
    """A roof must not cut its own mark; a nearer plane must still hide it."""
    screen = pygame.display.set_mode((240, 200))
    camera = Camera(screen.get_size())
    for zoom in (0.5, 1.0, 2.0):
        camera.zoom = zoom
        camera.x, camera.y = 0.25, -0.75
        c = DepthCamera(camera)
        roof = [c.world_to_screen(x, y, 0) for x, y in
                ((-40, -40), (40, -40), (40, 40), (-40, 40))]
        a, b = c.world_to_screen(-12, 0, 0), c.world_to_screen(12, 0, 0)
        mask = pygame.Surface(screen.get_size())
        mask.fill((0, 0, 0))
        pygame.draw.line(mask, (255, 255, 255), a, b, 3)
        expected = pygame.surfarray.array3d(mask).any(axis=2)
        screen.fill((0, 0, 0))
        depth = DepthBuffer(screen)
        depth.polygon((80, 80, 80), roof)
        depth.line((255, 255, 255), a, b, 3)
        assert (pygame.surfarray.array3d(screen)[expected] == 255).all()
        # A plane closer along the viewing ray covers the same screen area.
        from hexfront.depth import ProjectedPoint
        front = [ProjectedPoint(*p.projected, p.depth + 20) for p in roof]
        depth.polygon((80, 80, 80), front)
        depth.line((255, 255, 255), a, b, 3)
        assert (pygame.surfarray.array3d(screen)[expected] == 80).all()
    pygame.quit()


def test_batched_grid_and_view_changes():
    """Grid strokes survive adjacent fills and fractional pan/zoom changes."""
    from hexfront import hexgrid
    from unittest.mock import patch
    screen = pygame.display.set_mode((320, 240))
    board = Board(20, 20)
    scene = SimpleNamespace(board=board, buildings=[], vehicles=[])
    renderer = Renderer(screen)
    camera = Camera(screen.get_size())
    camera.center_on_world(*board.center_world((8, 8)))
    for zoom in (0.5, 1.0, 1.1, 2.0):
        camera.zoom = zoom
        for shift in (0, 1, 0.375, -2.75):
            camera.x += shift
            camera.y -= shift
            with patch.object(DepthBuffer, 'line', side_effect=AssertionError(
                    'Flat grid must not rasterize edges individually')):
                renderer._draw_tiles(scene, camera)
            cached = pygame.surfarray.array3d(screen)
            Renderer(screen)._draw_tiles(scene, camera)
            assert np.array_equal(cached, pygame.surfarray.array3d(screen))
            mask = pygame.Surface(screen.get_size())
            mask.fill((0, 0, 0))
            for tile in board.tiles:
                pts = [camera.world_to_screen(x, y, C.ELEVATION_PX)
                       for x, y in hexgrid.hex_corners(*tile, board.side)]
                pygame.draw.polygon(mask, (255, 255, 255), pts, C.GRID_LINE_WIDTH)
            edges = pygame.surfarray.array3d(mask).any(axis=2)
            assert edges.any()
            assert (cached[edges] == C.LAND_EDGE).all()
    pygame.quit()


def test_projectile_contrast_outline():
    """Shots remain identifiable even on a background of their own colour."""
    screen = pygame.display.set_mode((240, 200))
    board = Board(4, 4)
    renderer = Renderer(screen)
    camera = Camera(screen.get_size())
    pos = board.center_world((1, 1))
    camera.center_on_world(*pos)
    color = (230, 210, 60)
    shot = {"t": 0.5, "dur": 1.0, "from": pos, "to": pos,
            "to_tile": (1, 1), "color": color}
    scene = SimpleNamespace(board=board, projectiles=[shot])
    for rocket in (False, True):
        shot['rocket'] = rocket
        screen.fill(color)
        renderer._draw_projectiles(scene, camera)
        pixels = pygame.surfarray.array3d(screen)
        assert (pixels == C.PROJECTILE_OUTLINE_COLOR).all(axis=2).any()
    pygame.quit()


def test_vehicle_shadows_do_not_darken_hulls():
    """Every vehicle's shadow changes the ground, never its own body."""
    from unittest.mock import patch
    from hexfront.entities import Vehicle
    screen = pygame.display.set_mode((480, 360))
    board = Board(10, 10)
    pos = board.center_world((4, 4))
    camera = Camera(screen.get_size())
    camera.center_on_world(*pos)
    for kind in C.VehicleKind:
        for zoom in (0.5, 1.0, 2.0):
            camera.zoom = zoom
            renderer = Renderer(screen)
            v = Vehicle(kind, 0, 10, [], pos)
            scene = SimpleNamespace(board=board, buildings=[], vehicles=[v],
                                    projectiles=[])
            screen.fill((0, 0, 0))
            renderer._scene = DepthBuffer(screen)
            renderer._draw_vehicle(scene, DepthCamera(camera), v)
            renderer._scene = None
            body = pygame.surfarray.array3d(screen).any(axis=2)
            with patch.object(renderer, '_draw_badges'), \
                    patch.object(renderer, '_draw_ranges'):
                with patch.object(renderer, '_draw_shadows'):
                    renderer._rotor_phase = -0.2
                    renderer.draw_world(scene, camera)
                    bare = pygame.surfarray.array3d(screen)
                renderer._rotor_phase = -0.2
                renderer.draw_world(scene, camera)
                shaded = pygame.surfarray.array3d(screen)
                assert np.array_equal(bare[body], shaded[body]), (kind, zoom)
                assert np.any(bare != shaded), (kind, zoom, 'missing ground shadow')
                renderer._rotor_phase = -0.2
                renderer.draw_world(scene, camera)
                assert np.array_equal(shaded, pygame.surfarray.array3d(screen)), \
                    'Shadows must not accumulate in the terrain cache'
    pygame.quit()


def test_shadows_follow_receivers_and_preserve_depth():
    """Water, inclined ramps and bridge decks receive depth-neutral shadows."""
    from hexfront.entities import Vehicle
    screen = pygame.display.set_mode((640, 480))
    pos_tile = (4, 4)
    for surface in ('water', 'ramp', 'deck', 'under_bridge'):
        board = Board(10, 10)
        for tile in board.tiles.values():
            tile.height = 0
        if surface == 'ramp':
            board.tiles[(4, 3)].height = 2
            board.set_ramp(pos_tile, (4, 3), (4, 5))
        if surface in ('deck', 'under_bridge'):
            board.tiles[(4, 3)].height = board.tiles[(4, 6)].height = 3
            bridge = board.add_bridge((4, 3), (4, 6), 1)
            assert bridge is not None
        pos = board.center_world(pos_tile)
        camera = Camera(screen.get_size())
        camera.center_on_world(*pos)
        for zoom in (0.5, 1.0, 2.0):
            camera.zoom = zoom
            for kind in C.VehicleKind:
                renderer = Renderer(screen)
                vehicle = Vehicle(kind, 0, 10, [(4, 5)], pos,
                                  src_tile=pos_tile)
                if surface != 'deck':
                    vehicle.route = []
                    vehicle.src_tile = None
                scene = SimpleNamespace(board=board, vehicles=[vehicle])
                c = DepthCamera(camera)
                screen.fill(C.WATER_COLOR)
                renderer._scene = DepthBuffer(screen)
                renderer._draw_top(scene, c, pos_tile, board.tiles[pos_tile])
                if surface == 'ramp':
                    renderer._draw_ramp(scene, c, pos_tile, board.tiles[pos_tile])
                if surface in ('deck', 'under_bridge'):
                    renderer._draw_bridge_fragment(scene, c, bridge, pos_tile)
                before = pygame.surfarray.array3d(screen)
                depths = renderer._scene.values.copy()
                renderer._draw_shadows(scene, c)
                after = pygame.surfarray.array3d(screen)
                changed = np.any(before != after, axis=2)
                assert np.array_equal(depths, renderer._scene.values)
                if surface != 'under_bridge' or kind == C.VehicleKind.HELICOPTER:
                    assert changed.any(), (surface, kind, zoom)
                if surface == 'under_bridge' and kind != C.VehicleKind.HELICOPTER:
                    ys = np.arange(screen.get_height())[None, :] + 0.5
                    water_depth = ys / zoom + camera.y - screen.get_height() / (2 * zoom)
                    assert not (changed & (depths > water_depth + C.DEPTH_EPSILON)).any()
                renderer._scene = None
    pygame.quit()


def test_decal_rejects_occluders_and_missing_receivers():
    """A shadow shades equal depth only, not higher/lower faces or empty space."""
    from hexfront.depth import ProjectedPoint
    screen = pygame.display.set_mode((100, 100))
    points = [(10, 10), (90, 10), (90, 90), (10, 90)]
    shadow = [ProjectedPoint(x, y, 0) for x, y in points]
    for receiver in (None, -10, 0, 10):
        screen.fill((120, 120, 120))
        depth = DepthBuffer(screen)
        if receiver is not None:
            depth.polygon((120, 120, 120),
                          [ProjectedPoint(x, y, receiver) for x, y in points])
        before = pygame.surfarray.array3d(screen)
        old_depth = depth.values.copy()
        depth.decal(C.SHADOW_COLOR, shadow)
        changed = np.any(before != pygame.surfarray.array3d(screen))
        assert changed == (receiver == 0)
        assert np.array_equal(old_depth, depth.values)
    pygame.quit()



if __name__ == "__main__":
    test_shadows_follow_receivers_and_preserve_depth()
    test_decal_rejects_occluders_and_missing_receivers()
    test_vehicle_shadows_do_not_darken_hulls()
    test_surface_details_keep_full_width_but_respect_occlusion()
    test_batched_grid_and_view_changes()
    test_projectile_contrast_outline()
    test_bridge_and_vehicle_pixel_occlusion()
    test_depth_visibility_is_independent_of_submission_order()
    test_near_high_tile_occludes_ramp()
    test_terrain_cache_invalidates_after_edit()
    test_vehicle_at_far_edge_of_own_tile()
    test_view_clears_on_pan_and_zoom()
    test_view_culling_covers_screen()
    test_far_building_does_not_cover_near_high_terrain()
    test_vehicle_on_bridge_deck_not_on_ground()
    test_ramp_frame_and_height()
    test_ramp_strip_stays_visible_over_high_skirt()
    test_turret_barrels_differ()
    test_editor_draws_turret_and_heal_ranges()
    print("OK   test_view_clears_on_pan_and_zoom")
    print("OK   test_view_culling_covers_screen")
    print("OK   test_far_building_does_not_cover_near_high_terrain")
    print("OK   test_vehicle_on_bridge_deck_not_on_ground")
    print("OK   test_ramp_frame_and_height")
    print("OK   test_ramp_strip_stays_visible_over_high_skirt")
    print("OK   test_turret_barrels_differ")
    print("OK   test_editor_draws_turret_and_heal_ranges")
    print("\nAll render tests passed.")
