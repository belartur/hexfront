"""Isometric renderer: every graphic is drawn from code (no raster assets).

Objects of individual players differ by colour (specification: "Grafika i
interfejs użytkownika"); all drawing helpers accept the colour as an
argument so swapping in raster art later stays easy.
"""

import math

import pygame

from . import constants as C
from . import hexgrid
from .board import Obstacle
from .constants import TurretKind, VehicleKind
from .camera import Camera
from .depth import DepthBuffer, DepthCamera
from .entities import (BuildingKind, is_base, is_turret, turret_kind_of)


def _shade(color, factor: float) -> tuple:
    """Multiply an RGB colour by ``factor`` (clamped)."""
    return tuple(max(0, min(255, int(c * factor))) for c in color[:3])


def _lerp(a: tuple, b: tuple, t: float) -> tuple:
    """Linear interpolation between two points."""
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)


class Renderer:
    """Draws a whole game state onto the pygame screen."""

    def __init__(self, screen: pygame.Surface):
        self.screen = screen
        self._fonts = {}
        self._rotor_phase = 0.0
        self._scene = None
        self._terrain_key = None
        self._terrain_surface = None
        self._terrain_depth = None

    # ------------------------------------------------------------------
    def font(self, size: int) -> pygame.font.Font:
        """Cached font of a given pixel size."""
        f = self._fonts.get(size)
        if f is None:
            f = pygame.font.Font(None, size)
            self._fonts[size] = f
        return f

    def draw_world(self, game, camera: Camera, selection=None,
                   hover_tile=None) -> None:
        """Render one frame of the running game."""
        # Clear the view with open water: the board is surrounded by
        # water on all sides (rules.md sec. 1), so anything outside the
        # generated tiles renders as the sea.
        self.screen.fill(C.WATER_COLOR)
        self._rotor_phase += 0.2
        overlay = pygame.Surface(self.screen.get_size(), pygame.SRCALPHA)
        self._draw_shadows(game, camera, overlay)
        self._draw_ranges(game, camera, overlay)
        self._draw_tiles(game, camera)
        self.screen.blit(overlay, (0, 0))
        self._draw_paths(game, camera, selection)
        self._draw_objects(game, camera, selection, hover_tile)
        self._draw_projectiles(game, camera)
        self._draw_float_texts(game, camera)
        self._draw_badges(game, camera)

    # ------------------------------------------------------------------
    # Terrain (land grey, water light blue - specification)
    # ------------------------------------------------------------------
    def _draw_skirts(self, game, camera: Camera, tile: tuple, t) -> None:
        """Cliff skirts of one tile, wherever a neighbour (or void) is
        lower; every face participates in the shared depth buffer."""
        board = game.board
        q, r = tile
        corners = hexgrid.hex_corners(q, r, board.side)
        z = t.height * C.ELEVATION_PX
        pts = [camera.world_to_screen(x, y, z) for x, y in corners]
        for k in range(6):
            d = hexgrid.edge_dir_index(q, k)
            n = hexgrid.neighbor(q, r, d)
            nh = board.height(n) if board.contains(n) else 0
            if nh < t.height:
                nz = nh * C.ELEVATION_PX
                b1 = camera.world_to_screen(*corners[k], nz)
                b2 = camera.world_to_screen(*corners[(k + 1) % 6], nz)
                skirt = _shade(_shade(C.LAND_COLOR,
                                      1.0 + 0.05 * t.height), 0.62)
                self._polygon(
                    self.screen, skirt,
                    [pts[k], pts[(k + 1) % 6], b2, b1])

    def _draw_top(self, game, camera: Camera, tile: tuple, t) -> None:
        """Hex top surface of one tile."""
        board = game.board
        q, r = tile
        corners = hexgrid.hex_corners(q, r, board.side)
        z = t.height * C.ELEVATION_PX
        pts = [camera.world_to_screen(x, y, z) for x, y in corners]
        if t.height == 0:
            fill, edge = C.WATER_COLOR, C.WATER_EDGE
        else:
            fill = _shade(C.LAND_COLOR, 1.0 + 0.05 * t.height)
            edge = C.LAND_EDGE
        self._polygon(self.screen, fill, pts)
        self._polygon(self.screen, edge, pts, 1)

    # ------------------------------------------------------------------
    # Depth-tested scene with view culling.
    # ------------------------------------------------------------------
    def _visible_tiles(self, game, camera: Camera) -> set:
        """Tiles that can appear on the screen (view culling).

        The four screen corners are mapped back to the world plane at
        zero and at the maximum elevation; the bounding box of the eight
        world points, padded by two hexes, bounds every tile whose hex
        or cliff could reach the screen.  Drawing only this subset keeps
        huge boards (e.g. the editor's 256 x 256) bounded by view size.
        """
        board = game.board
        w, h = self.screen.get_size()
        xs, ys = [], []
        for wz in (0.0, 15 * C.ELEVATION_PX):
            for sx, sy in ((0, 0), (w, 0), (0, h), (w, h)):
                wx, wy = camera.screen_to_world(sx, sy, wz)
                xs.append(wx)
                ys.append(wy)
        pad = 2.0 * board.side
        col = 1.5 * board.side
        row = math.sqrt(3.0) * board.side
        q_lo = int(math.floor((min(xs) - pad) / col)) - 1
        q_hi = int(math.ceil((max(xs) + pad) / col)) + 1
        r_lo = int(math.floor((min(ys) - pad) / row)) - 1
        r_hi = int(math.ceil((max(ys) + pad) / row)) + 1
        return {(q, r) for q in range(q_lo, q_hi + 1)
                for r in range(r_lo, r_hi + 1)
                if board.contains((q, r))}

    def _polygon(self, surface, color, points, width=0) -> None:
        """Draw scene geometry with depth, or an ordinary UI polygon."""
        if self._scene is not None:
            self._scene.polygon(color, points, width)
        else:
            pygame.draw.polygon(surface, color, points, width)

    def _line(self, surface, color, a, b, width=1) -> None:
        """Draw scene geometry with depth, or an ordinary UI line."""
        if self._scene is not None:
            self._scene.line(color, a, b, width)
        else:
            pygame.draw.line(surface, color, a, b, width)

    def _draw_tiles(self, game, camera: Camera) -> None:
        """Render opaque geometry with per-pixel visibility, also in editor."""
        board = game.board
        building_at = {b.tile: b for b in game.buildings}
        camera = DepthCamera(camera)
        visible = sorted(self._visible_tiles(game, camera))
        # Neighbour heights also affect visible cliff bottoms and ramps.
        relevant = set(visible)
        for tile in visible:
            relevant.update(board.neighbors(tile))
        terrain_key = (board, camera.x, camera.y, camera.zoom,
                       camera.screen_size, self.screen.get_size(), board.side,
                       tuple((tile, board.tiles[tile].height,
                              board.tiles[tile].ramp)
                             for tile in sorted(relevant)))
        self._scene = DepthBuffer(self.screen)
        try:
            if terrain_key != self._terrain_key:
                self.screen.fill(C.WATER_COLOR)
                for tile in visible:
                    t = board.tiles[tile]
                    self._draw_skirts(game, camera, tile, t)
                    self._draw_top(game, camera, tile, t)
                    if t.ramp is not None:
                        self._draw_ramp(game, camera, tile, t)
                self._terrain_surface = self.screen.copy()
                self._terrain_depth = self._scene.values.copy()
                self._terrain_key = terrain_key
            else:
                self.screen.blit(self._terrain_surface, (0, 0))
                self._scene.values[:] = self._terrain_depth
            # Buildings can change ownership/aim; obstacles can disappear.
            # Only terrain is cached, never moving objects or UI overlays.
            for tile in visible:
                t = board.tiles[tile]
                if t.obstacle is not None:
                    self._draw_obstacle(game, camera, tile, t.obstacle)
                if tile in building_at:
                    self._draw_building(game, camera, building_at[tile], False)
                if t.bridge is not None and t.bridge.a is not None:
                    self._draw_bridge_fragment(game, camera, t.bridge, tile)
            for vehicle in game.vehicles:
                self._draw_vehicle(game, camera, vehicle)
        finally:
            self._scene = None

    # ------------------------------------------------------------------
    # Bridge-deck aware ground elevation
    # ------------------------------------------------------------------
    def _route_endpoints(self, v) -> tuple:
        """Tiles before/after the vehicle on its route: ``(prev, next)``.

        ``prev`` is the tile the vehicle comes from: the last visited
        waypoint, or the source tile stored on the vehicle right after
        departure.  ``next`` is the waypoint the vehicle heads to
        (``None`` when the route is empty/finished).  Used to tell whether
        the vehicle travels *along* a bridge deck or passes *under* it.
        """
        prev = getattr(v, "src_tile", None)
        nxt = None
        if v.route:
            if 0 < v.route_index <= len(v.route):
                prev = v.route[v.route_index - 1]
            if 0 <= v.route_index < len(v.route):
                nxt = v.route[v.route_index]
        return (prev, nxt)

    def _deck_z(self, board, tile, prev, nxt):
        """Deck elevation when the segment travels along a bridge.

        Returns ``None`` when neither the ``prev`` -> ``nxt`` segment nor
        the ``tile`` adjoining ``prev``/``nxt`` is a bridge-deck pair, i.e.
        the vehicle is not on the deck (it may e.g. sail under the bridge).
        """
        if prev is None and nxt is None:
            return None
        for br in board.bridges:
            pairs = br.pairs
            if prev is not None and nxt is not None \
                    and frozenset((prev, nxt)) in pairs:
                return br.w * C.ELEVATION_PX
            if tile is not None:
                if prev is not None \
                        and frozenset((prev, tile)) in pairs:
                    return br.w * C.ELEVATION_PX
                if nxt is not None \
                        and frozenset((tile, nxt)) in pairs:
                    return br.w * C.ELEVATION_PX
        return None

    def _ground_z(self, game, tile, pos, prev=None, nxt=None) -> float:
        """Walkable-surface elevation at ``pos`` (bridge decks included).

        A vehicle travelling along a bridge deck stands on the deck
        (rules.md sec. 8); anything else stands on the terrain (ramps
        interpolated).  Helicopters ignore it for their bodies but reuse
        it for their shadows.
        """
        board = game.board
        deck = self._deck_z(board, tile, prev, nxt)
        if deck is not None:
            return deck
        if tile is None:
            return 0.0
        t = board.tile(tile)
        if t is None:
            return 0.0
        if t.ramp is not None and pos is not None:
            return self._ramp_z(board, tile, pos)
        return t.height * C.ELEVATION_PX

    # ------------------------------------------------------------------
    # Shadows of flying and ground vehicles
    # ------------------------------------------------------------------
    def _draw_shadows(self, game, camera: Camera, overlay) -> None:
        for v in game.vehicles:
            tile = game.board.world_to_tile(v.x, v.y)
            prev, nxt = self._route_endpoints(v)
            z = self._ground_z(game, tile, (v.x, v.y), prev, nxt)
            pts = camera.screen_circle_poly(v.x, v.y, 14.0, z, 14)
            pygame.draw.polygon(overlay, (0, 0, 0, 70), pts)

    # ------------------------------------------------------------------
    # Ramp, bridge, obstacle and building drawing primitives
    # ------------------------------------------------------------------

    def _ramp_frame(self, board, tile: tuple, t):
        """World-space frame of the ramp strip on ``tile``.

        Returns ``(axis, edge_a, edge_b, ha, hb)`` where ``axis`` is the
        unit world vector from the edge facing neighbour ``a`` towards
        the edge facing neighbour ``b``, ``edge_a``/``edge_b`` are the
        world (x, y) midpoints of those hex edges, and ``ha``/``hb``
        are the terrain heights of the joined neighbours.

        The edge facing a neighbour is the one whose midpoint is
        nearest to that neighbour's centre (the neighbour centre lies
        on the edge-midpoint ray); the axis therefore coincides with
        the line joining the two neighbour centres.
        """
        from . import hexgrid as _hexgrid
        q, r = tile
        corners = _hexgrid.hex_corners(q, r, board.side)
        mids = []
        for k in range(6):
            c1 = corners[k]
            c2 = corners[(k + 1) % 6]
            mx = (c1[0] + c2[0]) / 2.0
            my = (c1[1] + c2[1]) / 2.0
            mids.append((mx, my))

        def _nearest(target):
            tx, ty = board.center_world(target)
            best, best_d = 0, None
            for k in range(6):
                d2 = (mids[k][0] - tx) ** 2 + (mids[k][1] - ty) ** 2
                if best_d is None or d2 < best_d:
                    best, best_d = k, d2
            return best

        edge_a = mids[_nearest(t.ramp[0])]
        edge_b = mids[_nearest(t.ramp[1])]
        dx = edge_b[0] - edge_a[0]
        dy = edge_b[1] - edge_a[1]
        length = math.hypot(dx, dy) or 1.0
        axis = (dx / length, dy / length)
        a, b = t.ramp
        ha = board.height(a) if board.contains(a) else 0
        hb = board.height(b) if board.contains(b) else 0
        return (axis, edge_a, edge_b, ha, hb)

    def _ramp_z(self, board, tile: tuple, pos: tuple) -> float:
        """Elevation of the ramp deck under world point ``pos``.

        Linear interpolation between the heights of the joined
        neighbours along the ramp axis, clamped to the strip ends.
        """
        t = board.tiles.get(tile)
        if t is None or t.ramp is None or pos is None:
            return board.height(tile) * C.ELEVATION_PX
        (axis, edge_a, edge_b, ha, hb) = self._ramp_frame(board, tile, t)
        dx = edge_b[0] - edge_a[0]
        dy = edge_b[1] - edge_a[1]
        length = math.hypot(dx, dy) or 1.0
        frac = ((pos[0] - edge_a[0]) * axis[0]
                + (pos[1] - edge_a[1]) * axis[1]) / length
        frac = max(0.0, min(1.0, frac))
        return ((1.0 - frac) * ha + frac * hb) * C.ELEVATION_PX

    def _draw_ramp(self, game, camera: Camera, tile: tuple, t) -> None:
        """A ramp tile: a solid inclined *strip* along the ramp axis.

        The strip's short edges lie on the midpoints of the hex edges
        facing neighbours a and b, drawn at their respective heights,
        so the tilt is exactly the joined fields' height difference.  The body below the top face is
        filled with darker dirt down to the base elevation -- no empty
        space is visible under the ramp.  The strip is narrower than the
        hex; normal ground of tile ``p`` stays visible at both sides.
        """
        board = game.board
        (axis, edge_a, edge_b, ha, hb) = self._ramp_frame(board, tile, t)
        za = ha * C.ELEVATION_PX
        zb = hb * C.ELEVATION_PX
        z_lo = min(za, zb)
        ux, uy = axis                               # points a -> b (world)
        hw = board.side * 0.45                    # strip half-width (<= s/2)
        # Long edges stay parallel to the tilted axis in world space
        # (a sheared projection of the flat strip), so the strip always
        # runs edge to edge and never drifts past the hex border.
        px, py = -uy, ux
        a1 = camera.world_to_screen(edge_a[0] + px * hw,
                                    edge_a[1] + py * hw, za)
        a2 = camera.world_to_screen(edge_a[0] - px * hw,
                                    edge_a[1] - py * hw, za)
        b1 = camera.world_to_screen(edge_b[0] + px * hw,
                                    edge_b[1] + py * hw, zb)
        b2 = camera.world_to_screen(edge_b[0] - px * hw,
                                    edge_b[1] - py * hw, zb)
        # Solid body: both sides filled from the tilted top edges down to
        # the base elevation -- the union covers everything under the ramp.
        ground_a1 = camera.world_to_screen(edge_a[0] + px * hw,
                                           edge_a[1] + py * hw, z_lo)
        ground_a2 = camera.world_to_screen(edge_a[0] - px * hw,
                                           edge_a[1] - py * hw, z_lo)
        ground_b1 = camera.world_to_screen(edge_b[0] + px * hw,
                                           edge_b[1] + py * hw, z_lo)
        ground_b2 = camera.world_to_screen(edge_b[0] - px * hw,
                                           edge_b[1] - py * hw, z_lo)
        skirt = _shade((172, 158, 120), 0.62)
        self._polygon(self.screen, skirt, [a1, b1, ground_b1, ground_a1])
        self._polygon(self.screen, skirt, [a2, b2, ground_b2, ground_a2])
        # Rectangular top face, tilted by the height difference.
        pts = [a1, b1, b2, a2]
        self._polygon(self.screen, (172, 158, 120), pts)
        self._polygon(self.screen, C.LAND_EDGE, pts, 1)

    def _draw_bridge_fragment(self, game, camera: Camera, bridge,
                              frag: tuple) -> None:
        """One deck segment of a bridge, elevated at the bridge height."""
        board = game.board
        cx, cy = board.center_world(frag)
        nxt = hexgrid.neighbor(frag[0], frag[1], bridge.direction)
        if board.contains(nxt):
            nx, ny = board.center_world(nxt)
        else:
            nx, ny = board.center_world(bridge.b)
        ax, ay = nx - cx, ny - cy
        length = math.hypot(ax, ay) or 1.0
        ux, uy = ax / length, ay / length
        px, py = -uy, ux
        s = board.side
        L, W = 0.52 * length, 0.34 * s
        deck_z = bridge.w * C.ELEVATION_PX + 5
        ground_z = board.height(frag) * C.ELEVATION_PX
        corners = [(cx + ux * L + px * W, cy + uy * L + py * W),
                   (cx + ux * L - px * W, cy + uy * L - py * W),
                   (cx - ux * L - px * W, cy - uy * L - py * W),
                   (cx - ux * L + px * W, cy - uy * L + py * W)]
        top = [camera.world_to_screen(x, y, deck_z) for x, y in corners]
        # Support pillars from the deck down to the tile surface.
        for x, y in corners:
            g = camera.world_to_screen(x, y, ground_z)
            d = camera.world_to_screen(x, y, deck_z)
            self._line(self.screen, (95, 70, 45), g, d, 2)
        self._polygon(self.screen, (150, 112, 72), top)
        self._polygon(self.screen, (110, 80, 50), top, 2)

    def _draw_obstacle(self, game, camera: Camera, tile: tuple,
                       obs: Obstacle) -> None:
        board = game.board
        cx, cy = board.center_world(tile)
        z = board.height(tile) * C.ELEVATION_PX
        if obs.kind == Obstacle.WALL:
            self._iso_box(camera, cx, cy, z, 30, 30, 14, (95, 95, 105))
        elif obs.kind in (Obstacle.MINE, Obstacle.MINE_WATER):
            pts = camera.screen_circle_poly(cx, cy, 8.0, z, 12)
            self._polygon(self.screen, (40, 40, 45), pts)
            dot = camera.screen_circle_poly(cx, cy, 3.0, z, 8)
            self._polygon(self.screen, (200, 60, 50), dot)
        elif obs.kind == Obstacle.TRAP_FIRE:
            pts = camera.screen_circle_poly(cx, cy, 20.0, z, 16)
            self._polygon(self.screen, (235, 120, 40, 110), pts)
            flame = camera.world_to_screen(cx, cy, z + 10)
            base = camera.world_to_screen(cx, cy, z)
            self._line(self.screen, (250, 170, 60), base, flame, 3)
        elif obs.kind == Obstacle.TRAP_ICE:
            pts = camera.screen_circle_poly(cx, cy, 20.0, z, 16)
            self._polygon(self.screen, (170, 220, 250, 120), pts)
            for dx, dy in ((-8, -4), (4, 6)):
                p1 = camera.world_to_screen(cx + dx, cy + dy, z)
                p2 = camera.world_to_screen(cx - dx, cy - dy, z)
                self._line(self.screen, (240, 250, 255), p1, p2, 2)

    # ------------------------------------------------------------------
    # Shared shape helpers
    # ------------------------------------------------------------------
    def _iso_box(self, camera: Camera, x: float, y: float, z: float,
                 w: float, d: float, h: float, color: tuple) -> None:
        """Axis-aligned box centred on (x, y), base at elevation ``z``."""
        top = [camera.world_to_screen(x - w / 2, y - d / 2, z + h),
               camera.world_to_screen(x + w / 2, y - d / 2, z + h),
               camera.world_to_screen(x + w / 2, y + d / 2, z + h),
               camera.world_to_screen(x - w / 2, y + d / 2, z + h)]
        face_x = [camera.world_to_screen(x + w / 2, y - d / 2, z + h),
                  camera.world_to_screen(x + w / 2, y + d / 2, z + h),
                  camera.world_to_screen(x + w / 2, y + d / 2, z),
                  camera.world_to_screen(x + w / 2, y - d / 2, z)]
        face_y = [camera.world_to_screen(x - w / 2, y + d / 2, z + h),
                  camera.world_to_screen(x + w / 2, y + d / 2, z + h),
                  camera.world_to_screen(x + w / 2, y + d / 2, z),
                  camera.world_to_screen(x - w / 2, y + d / 2, z)]
        self._polygon(self.screen, _shade(color, 0.62), face_y)
        self._polygon(self.screen, _shade(color, 0.8), face_x)
        self._polygon(self.screen, color, top)

    # ------------------------------------------------------------------
    # Range overlays (white turrets, light-green healers - specification)
    # ------------------------------------------------------------------
    def _range_circles(self, game, camera: Camera) -> list:
        """Polygons + colours of every range overlay of the state."""
        circles = []
        for b in game.buildings:
            z = game.board.height(b.tile) * C.ELEVATION_PX
            tk = turret_kind_of(b.kind)
            if tk is not None:
                circles.append((camera.screen_circle_poly(
                    b.pos[0], b.pos[1],
                    C.TURRET_STATS[tk]["range"], z),
                    C.RANGE_TURRET_COLOR, C.RANGE_TURRET_OUTLINE))
            elif b.kind == BuildingKind.HEAL_TOWER and b.owner is not None:
                rng = C.HEAL_TOWER_RANGE_PER_UNIT * b.units
                circles.append((camera.screen_circle_poly(
                    b.pos[0], b.pos[1], rng, z),
                    C.RANGE_HEAL_COLOR, C.RANGE_HEAL_OUTLINE))
        for v in game.vehicles:
            if v.kind != VehicleKind.BUFFER:
                continue
            tile = game.board.world_to_tile(v.x, v.y)
            prev, nxt = self._route_endpoints(v)
            z = self._ground_z(game, tile, (v.x, v.y), prev, nxt)
            circles.append((camera.screen_circle_poly(
                v.x, v.y, C.BUFFER_HEAL_RADIUS, z),
                C.RANGE_HEAL_COLOR, C.RANGE_HEAL_OUTLINE))
        return circles

    def _paint_range_circles(self, camera: Camera, circles: list,
                             overlay) -> None:
        """Every translucent fill first, then every outline (same hue,
        less transparent), so an outline covers the fills of other
        ranges instead of blending into them."""
        for pts, fill, _outline in circles:
            pygame.draw.polygon(overlay, fill, pts)
        for pts, _fill, outline in circles:
            pygame.draw.polygon(overlay, outline, pts,
                                C.RANGE_OUTLINE_WIDTH)

    def _draw_ranges(self, game, camera: Camera, overlay) -> None:
        """Range overlays: white turrets, light-green healers (spec)."""
        self._paint_range_circles(camera, self._range_circles(game, camera),
                                  overlay)

    # ------------------------------------------------------------------
    # Static board rendering (shared with the board editor)
    # ------------------------------------------------------------------
    def draw_editor(self, scene, camera: Camera, hover_tile=None) -> None:
        """Render a static board without a running game.

        ``scene`` only needs the ``board`` and ``buildings`` attributes
        used by the tile painter, so the editor draws its maps with
        exactly the same code as the game (specification: the editor
        shares the board-drawing code with the game).
        """
        self.screen.fill(C.WATER_COLOR)
        self._draw_tiles(scene, camera)
        # Ranges of turrets and (owned) healing towers are shown in the
        # editor too, drawn exactly like in the game (editor spec); the
        # translucent layer is painted after the tiles so the fills stay
        # visible (before only the outlines were drawn, the tiles were
        # painted over the fills later). The translucent layer is only
        # built when there is anything to draw.
        circles = self._range_circles(scene, camera)
        if circles:
            overlay = pygame.Surface(self.screen.get_size(), pygame.SRCALPHA)
            self._paint_range_circles(camera, circles, overlay)
            self.screen.blit(overlay, (0, 0))
        self._draw_badges(scene, camera)
        if hover_tile is not None and scene.board.contains(hover_tile):
            z = scene.board.height(hover_tile) * C.ELEVATION_PX
            pts = [camera.world_to_screen(x, y, z) for x, y in
                   hexgrid.hex_corners(hover_tile[0], hover_tile[1],
                                       scene.board.side)]
            self._polygon(self.screen, (255, 255, 255), pts, 2)

    # ------------------------------------------------------------------
    # Dashed travel paths (vanish behind the vehicle - specification)
    # ------------------------------------------------------------------
    def _waypoint_z(self, board, seq, i, on_deck=False) -> float:
        """Elevation of waypoint ``seq[i]`` along a tile route (deck aware).

        A waypoint with a bridge fragment sits on the deck only when an
        adjacent step of the route travels along the deck (rules.md
        sec. 8); crossing *under* a bridge keeps the terrain elevation.
        ``on_deck`` covers the first waypoint whose previous step left the
        known sequence (e.g. the source tile of a vehicle route): when the
        vehicle itself is on the deck, its next waypoint on a bridge
        fragment stays on the deck.
        """
        tile = seq[i]
        for pair in ([(seq[i - 1], tile)] if i > 0 else []) + \
                ([(tile, seq[i + 1])] if i + 1 < len(seq) else []):
            for br in board.bridges:
                if frozenset(pair) in br.pairs:
                    return br.w * C.ELEVATION_PX
        t = board.tile(tile)
        if t is not None and t.bridge is not None and on_deck:
            return t.bridge.w * C.ELEVATION_PX
        if t is not None and t.ramp is not None:
            return self._ramp_z(board, tile, board.center_world(tile))
        return board.height(tile) * C.ELEVATION_PX

    def _draw_paths(self, game, camera: Camera, selection) -> None:
        if selection is not None and selection.get("path"):
            seq = [selection["src"]] + list(selection["path"])
            pts = [camera.world_to_screen(*game.board.center_world(t),
                                          self._waypoint_z(game.board, seq,
                                                           i))
                   for i, t in enumerate(seq)]
            self._dashed_polyline(pts, C.PATH_PREVIEW_COLOR)
        for v in game.vehicles:
            if not v.route or v.route_index >= len(v.route):
                continue
            tile = game.board.world_to_tile(v.x, v.y)
            prev, nxt = self._route_endpoints(v)
            on_deck = self._deck_z(game.board, tile, prev, nxt) is not None
            seq = v.route[v.route_index:]
            pts = [camera.world_to_screen(v.x, v.y, self._vehicle_z(
                game, v))]
            for i in range(len(seq)):
                pts.append(camera.world_to_screen(
                    *game.board.center_world(seq[i]),
                    self._waypoint_z(game.board, seq, i,
                                     on_deck=on_deck)))
            self._dashed_polyline(pts, C.PATH_COLOR)

    def _dashed_polyline(self, pts, color, dash=9.0, gap=6.0) -> None:
        """Draw a dashed polyline in screen space."""
        if len(pts) < 2:
            return
        drawing, left = True, dash
        for a, b in zip(pts, pts[1:]):
            seg = math.hypot(b[0] - a[0], b[1] - a[1])
            if seg < 1e-6:
                continue
            ux, uy = (b[0] - a[0]) / seg, (b[1] - a[1]) / seg
            pos = 0.0
            while pos < seg:
                run = min(left, seg - pos)
                if drawing:
                    p1 = (a[0] + ux * pos, a[1] + uy * pos)
                    p2 = (a[0] + ux * (pos + run), a[1] + uy * (pos + run))
                    self._line(self.screen, color, p1, p2, 2)
                pos += run
                left -= run
                if left <= 0:
                    drawing = not drawing
                    left = dash if drawing else gap

    # ------------------------------------------------------------------
    # Vehicles, merged into the tile stream (buildings are drawn with
    # their tiles)
    # ------------------------------------------------------------------
    def _draw_objects(self, game, camera: Camera, selection,
                      hover_tile) -> None:
        board = game.board
        if hover_tile is not None and board.contains(hover_tile):
            z = board.height(hover_tile) * C.ELEVATION_PX
            pts = [camera.world_to_screen(x, y, z) for x, y in
                   hexgrid.hex_corners(hover_tile[0], hover_tile[1],
                                       board.side)]
            self._polygon(self.screen, (255, 255, 255), pts, 1)
        # Vehicles already depth-tested inside _draw_tiles();
        # _draw_objects() only adds the selection marker on top.
        if selection is not None and selection.get("src") is not None:
            src = game.building_at_tile(selection["src"])
            if src is not None:
                z = board.height(src.tile) * C.ELEVATION_PX
                pts = camera.screen_circle_poly(src.pos[0], src.pos[1],
                                                30.0, z, 24)
                self._polygon(self.screen, (255, 255, 255), pts, 2)

    def _draw_badges(self, game, camera: Camera) -> None:
        """Unit-count badges of every building and vehicle, drawn as the
        very last pass so they are never hidden by terrain or objects."""
        board = game.board
        for b in game.buildings:
            self._draw_badge(game, camera, b.pos,
                             board.height(b.tile) * C.ELEVATION_PX,
                             b.units,
                             maxed=b.units >= b.capacity,
                             production_frac=(b.production_timer
                                              / C.BASE_SPAWN_INTERVAL
                                              if is_base(b.kind)
                                              and b.owner is not None
                                              else None))
        for v in game.vehicles:
            self._draw_badge(game, camera, v.pos,
                             self._vehicle_z(game, v), v.units,
                             maxed=False, production_frac=None)

    def _vehicle_z(self, game, v) -> float:
        """Render elevation of a vehicle (helicopters hover higher)."""
        tile = game.board.world_to_tile(v.x, v.y)
        prev, nxt = self._route_endpoints(v)
        ground = self._ground_z(game, tile, (v.x, v.y), prev, nxt)
        return ground + (C.ELEVATION_PX * 2.2
                         if v.kind == VehicleKind.HELICOPTER else 6.0)

    def _draw_building(self, game, camera: Camera, b, selected: bool) -> None:
        x, y = b.pos
        z = game.board.height(b.tile) * C.ELEVATION_PX
        color = (C.NEUTRAL_COLOR if b.owner is None
                 else C.PLAYER_COLORS[b.owner % 4])
        if is_base(b.kind):
            self._iso_box(camera, x, y, z, 34, 34, 24, color)
            top = z + 24
            if b.kind == BuildingKind.BASE_TANK:
                p1 = camera.world_to_screen(x - 4, y, top + 12)
                p2 = camera.world_to_screen(x + 14, y, top + 12)
                self._line(self.screen, _shade(color, 0.5), p1, p2, 3)
            elif b.kind == BuildingKind.BASE_HELICOPTER:
                pts = camera.screen_circle_poly(x, y, 9.0, top, 14)
                self._polygon(self.screen, (240, 240, 240), pts, 2)
            elif b.kind == BuildingKind.BASE_HOVERCRAFT:
                pts = camera.screen_circle_poly(x, y, 12.0, top, 14)
                self._polygon(self.screen, _shade(color, 0.7), pts, 3)
            else:  # buffer base
                self._draw_cross(camera, x, y, top, (110, 220, 120))
        elif is_turret(b.kind):
            self._iso_box(camera, x, y, z, 24, 24, 12, color)
            self._draw_turret_barrel(camera, b, x, y, z, color)
        else:  # healing tower
            base = camera.world_to_screen(x, y, z)
            top = camera.world_to_screen(x, y, z + 34)
            self._line(self.screen, _shade(color, 0.8), base, top, 5)
            self._draw_cross(camera, x, y, z + 34, (150, 245, 150))

    def _turret_aim_angle(self, b, x: float, y: float):
        """Aim angle of a turret barrel (follows the last target)."""
        if b.last_target_pos is None:
            return None
        tx, ty = b.last_target_pos
        return math.atan2((tx - x) * C.ISO_SIN, (tx - x) * C.ISO_COS)

    def _draw_turret_barrel(self, camera: Camera, b, x: float, y: float,
                            z: float, color) -> None:
        """Barrel drawing that tells the three turret kinds apart."""
        dark = _shade(color, 0.55)
        tk = turret_kind_of(b.kind)
        ang = self._turret_aim_angle(b, x, y)
        if ang is None:
            ang = -math.pi / 2.0
        dx, dy = math.cos(ang), math.sin(ang)
        if tk == TurretKind.RAPID:
            base = camera.world_to_screen(x, y, z + 12)
            hub = camera.world_to_screen(x, y, z + 22)
            for side in (-1.0, 1.0):
                px, py = -dy * side * 5.0, dx * side * 5.0
                p0 = camera.world_to_screen(x + px, y + py, z + 22)
                p1 = camera.world_to_screen(x + px + dx * 10.0,
                                            y + py + dy * 10.0, z + 22)
                self._line(self.screen, dark, p0, p1, 3)
            self._line(self.screen, dark, base, hub, 4)
        elif tk == TurretKind.ROCKET:
            cx = x + dx * 6.0
            cy = y + dy * 6.0
            pts = camera.screen_circle_poly(cx, cy, 8.0, z + 22, 12)
            self._polygon(self.screen, dark, pts, 3)
            tip = camera.world_to_screen(cx + dx * 8.0, cy + dy * 8.0,
                                         z + 22)
            hub = camera.world_to_screen(x, y, z + 22)
            self._line(self.screen, dark, hub, tip, 2)
        else:  # normal turret: one long barrel
            barrel = camera.world_to_screen(x, y, z + 22)
            base = camera.world_to_screen(x, y, z + 12)
            self._line(self.screen, dark, base, barrel, 4)
            tip = camera.world_to_screen(x + dx * 18.0, y + dy * 18.0,
                                         z + 22)
            self._line(self.screen, dark, barrel, tip, 3)

    def _draw_cross(self, camera: Camera, x: float, y: float, z: float,
                    color) -> None:
        """Small standing plus-sign (marker of buffers / heal towers)."""
        h = 6.0
        p1 = camera.world_to_screen(x - h, y, z)
        p2 = camera.world_to_screen(x + h, y, z)
        p3 = camera.world_to_screen(x, y - h, z)
        p4 = camera.world_to_screen(x, y + h, z)
        self._line(self.screen, color, p1, p2, 3)
        self._line(self.screen, color, p3, p4, 3)

    def _draw_vehicle(self, game, camera: Camera, v) -> None:
        color = C.PLAYER_COLORS[v.owner % 4]
        z = self._vehicle_z(game, v)
        x, y = v.x, v.y
        if v.kind == VehicleKind.TANK:
            self._iso_box(camera, x, y, z, 16, 22, 9, color)
            self._iso_box(camera, x, y, z + 9, 10, 10, 5,
                          _shade(color, 0.7))
        elif v.kind == VehicleKind.HELICOPTER:
            pts = camera.screen_circle_poly(x, y, 10.0, z, 12)
            self._polygon(self.screen, color, pts)
            tail = camera.world_to_screen(x - 16, y, z)
            body = camera.world_to_screen(x, y, z)
            self._line(self.screen, _shade(color, 0.7), body, tail, 3)
            ang = self._rotor_phase
            r = 22.0
            p1 = camera.world_to_screen(x + r * math.cos(ang),
                                        y + r * math.sin(ang), z + 5)
            p2 = camera.world_to_screen(x - r * math.cos(ang),
                                        y - r * math.sin(ang), z + 5)
            self._line(self.screen, (210, 210, 210), p1, p2, 2)
        elif v.kind == VehicleKind.HOVERCRAFT:
            pts = camera.screen_circle_poly(x, y, 13.0, z, 14)
            self._polygon(self.screen, color, pts)
            inner = camera.screen_circle_poly(x, y, 7.0, z + 4, 12)
            self._polygon(self.screen, _shade(color, 0.7), inner)
        else:  # buffer
            self._iso_box(camera, x, y, z, 16, 20, 9, color)
            self._draw_cross(camera, x, y, z + 9, (130, 235, 140))

    # ------------------------------------------------------------------
    # Unit-count badges (circle + number + MAX + spawn ring)
    # ------------------------------------------------------------------
    def _draw_badge(self, game, camera: Camera, world_pos, ground_z,
                    units: float, maxed: bool,
                    production_frac) -> None:
        zoom = camera.zoom
        sx, sy = camera.world_to_screen(world_pos[0], world_pos[1], ground_z)
        r = max(8, int(12 * zoom))
        cx, cy = sx + int(r * 1.5), sy + int(r * 1.1)
        pygame.draw.circle(self.screen, (28, 28, 34), (cx, cy), r)
        pygame.draw.circle(self.screen, (245, 245, 245), (cx, cy), r, 2)
        fsize = max(10, int(17 * zoom))
        text = self.font(fsize).render(str(int(round(units))), True,
                                       (255, 255, 255))
        ty = cy - text.get_height() // 2 - (int(4 * zoom) if maxed else 0)
        self.screen.blit(text, (cx - text.get_width() // 2, ty))
        if maxed:
            small = self.font(max(9, int(fsize * 0.62))).render(
                "MAX", True, (255, 255, 255))
            self.screen.blit(small, (cx - small.get_width() // 2,
                                     ty + text.get_height() - 2))
        if production_frac is not None and production_frac > 0:
            # White ring completing over 10 s; never overlaps the number.
            rect = pygame.Rect(cx - r - 3, cy - r - 3, 2 * (r + 3),
                               2 * (r + 3))
            start = math.pi / 2
            end = start + 2 * math.pi * min(1.0, production_frac)
            pygame.draw.arc(self.screen, (255, 255, 255), rect, start, end, 2)

    # ------------------------------------------------------------------
    # Turret projectiles (homing shots - damage resolves on impact, sec. 10)
    # ------------------------------------------------------------------
    def _draw_projectiles(self, game, camera: Camera) -> None:
        for p in game.projectiles:
            t = p["t"] / p["dur"]
            x, y = _lerp(p["from"], p["to"], t)
            from_tile = game.board.world_to_tile(*p["from"])
            from_z = self._ground_z(game, from_tile, p["from"]) + 14.0
            to_z = self._ground_z(game, p["to_tile"], p["to"],
                                  p.get("to_prev"), p.get("to_next")) + 14.0
            z = (1.0 - t) * from_z + t * to_z
            pos = camera.world_to_screen(x, y, z)
            radius = 4 if p.get("rocket") else 2
            pygame.draw.circle(self.screen, p["color"], pos, radius)

    # ------------------------------------------------------------------
    # Floating -x / +x numbers (2 s, drifting upwards - specification)
    # ------------------------------------------------------------------
    def _draw_float_texts(self, game, camera: Camera) -> None:
        entities = list(game.buildings) + list(game.vehicles)
        for ent in entities:
            if not ent.texts:
                continue
            if hasattr(ent, "tile"):
                pos, z = ent.pos, game.board.height(ent.tile) \
                    * C.ELEVATION_PX
            else:
                pos = ent.pos
                z = self._vehicle_z(game, ent)
            sx, sy = camera.world_to_screen(pos[0], pos[1], z)
            bx, by = sx + int(18 * camera.zoom), sy + int(13 * camera.zoom)
            for t in ent.texts:
                amount, age = t[0], t[1]
                count = len(t) > 2 and t[2]     # wall-shot ordinal flag
                alpha = max(0, 255 - int(255 * age / C.FLOAT_TEXT_LIFETIME))
                color = ((255, 255, 255) if (amount < 0 or count)
                         else (170, 245, 170))
                fsize = max(10, int(18 * camera.zoom))
                if count:
                    label = str(amount)
                else:
                    label = f"{amount:+d}".replace("+0", "+") \
                        if amount > 0 else str(amount)
                surf = self.font(fsize).render(label, True, color)
                surf.set_alpha(alpha)
                dy = int(age * C.FLOAT_TEXT_SPEED * camera.zoom)
                self.screen.blit(surf, (bx - surf.get_width() // 2,
                                        by - 2 * int(18 * camera.zoom) - dy))
