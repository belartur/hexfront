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
from .constants import VehicleKind
from .camera import Camera
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
        lower.  Called for every tile before any hex top is drawn."""
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
                pygame.draw.polygon(
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
        pygame.draw.polygon(self.screen, fill, pts)
        pygame.draw.polygon(self.screen, edge, pts, 1)

    # ------------------------------------------------------------------
    # One painter pass: each tile is drawn together with everything that
    # stands on it (ramp, bridge deck, obstacle, building), all sorted by
    # the same depth key, so tall terrain properly occludes the contents
    # of lower tiles behind it.
    # ------------------------------------------------------------------
    # ------------------------------------------------------------------
    # Board painter: tiles back-to-front, each with its cliff skirts, top
    # and contents drawn together, so walls stay attached to their tile.
    # ------------------------------------------------------------------
    def _tile_depth(self, game, tile: tuple) -> float:
        """Painter depth key of a tile: bigger = closer to the camera.

        From the projection (camera.py: ``py = (x + y) * ISO_SIN - z``)
        two points land on one view ray when a step of one height unit
        offsets ``1 / ISO_SIN`` steps of the world diagonal -- so the
        elevation must enter the depth with weight ``ELEVATION_PX /
        ISO_SIN`` relative to ``(x + y)``.  Higher ground is therefore
        painted after lower ground in front of it and stays visible.
        """
        x, y = game.board.center_world(tile)
        return ((x + y) * C.ISO_SIN
                + game.board.height(tile) * C.ELEVATION_PX)

    def _draw_tiles(self, game, camera: Camera) -> None:
        board = game.board
        building_at = {b.tile: b for b in game.buildings}
        for tile in sorted(board.tiles, key=lambda t: self._tile_depth(game, t)):
            t = board.tiles[tile]
            # Skirts first, then the top: the wall fills the gap below
            # its own hex and covers the lower terrain behind it.
            self._draw_skirts(game, camera, tile, t)
            self._draw_top(game, camera, tile, t)
            if t.ramp is not None:
                self._draw_ramp(game, camera, tile, t)
            if t.obstacle is not None:
                self._draw_obstacle(game, camera, tile, t.obstacle)
            b = building_at.get(tile)
            if b is not None:
                self._draw_building(game, camera, b, False)
        # Bridge decks float above every terrain surface.
        for bridge in board.bridges:
            for frag in sorted(bridge.fragments):
                self._draw_bridge_fragment(game, camera, bridge, frag)

    # ------------------------------------------------------------------
    # Shadows of flying and ground vehicles
    # ------------------------------------------------------------------
    def _draw_shadows(self, game, camera: Camera, overlay) -> None:
        for v in game.vehicles:
            tile = game.board.world_to_tile(v.x, v.y)
            z = (game.board.height(tile) * C.ELEVATION_PX) if tile else 0
            pts = camera.screen_circle_poly(v.x, v.y, 14.0, z, 14)
            pygame.draw.polygon(overlay, (0, 0, 0, 70), pts)

    # ------------------------------------------------------------------
    # Ramp, bridge, obstacle and building drawing primitives
    # ------------------------------------------------------------------

    def _draw_ramp(self, game, camera: Camera, tile: tuple, t) -> None:
        """A ramp tile: dirt-coloured hex with chevrons along its axis."""
        q, r = tile
        corners = hexgrid.hex_corners(q, r, game.board.side)
        z = t.height * C.ELEVATION_PX
        pts = [camera.world_to_screen(x, y, z) for x, y in corners]
        pygame.draw.polygon(self.screen, (172, 158, 120), pts)
        pygame.draw.polygon(self.screen, C.LAND_EDGE, pts, 1)
        a, b = t.ramp
        ax, ay = game.board.center_world(a)
        bx, by = game.board.center_world(b)
        cx, cy = game.board.center_world(tile)
        length = math.hypot(bx - ax, by - ay) or 1.0
        ux, uy = (bx - ax) / length, (by - ay) / length
        for off in (-10.0, 10.0):
            tipx, tipy = cx + ux * off, cy + uy * off
            left = camera.world_to_screen(tipx - uy * 7 - ux * 6,
                                          tipy + ux * 7 - uy * 6, z)
            right = camera.world_to_screen(tipx + uy * 7 - ux * 6,
                                           tipy - ux * 7 - uy * 6, z)
            tip = camera.world_to_screen(tipx + ux * 8, tipy + uy * 8, z)
            pygame.draw.polygon(self.screen, (240, 235, 220),
                                [left, tip, right])

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
            pygame.draw.line(self.screen, (95, 70, 45), g, d, 2)
        pygame.draw.polygon(self.screen, (150, 112, 72), top)
        pygame.draw.polygon(self.screen, (110, 80, 50), top, 2)

    def _draw_obstacle(self, game, camera: Camera, tile: tuple,
                       obs: Obstacle) -> None:
        board = game.board
        cx, cy = board.center_world(tile)
        z = board.height(tile) * C.ELEVATION_PX
        if obs.kind == Obstacle.WALL:
            self._iso_box(camera, cx, cy, z, 30, 30, 14, (95, 95, 105))
        elif obs.kind in (Obstacle.MINE, Obstacle.MINE_WATER):
            pts = camera.screen_circle_poly(cx, cy, 8.0, z, 12)
            pygame.draw.polygon(self.screen, (40, 40, 45), pts)
            dot = camera.screen_circle_poly(cx, cy, 3.0, z, 8)
            pygame.draw.polygon(self.screen, (200, 60, 50), dot)
        elif obs.kind == Obstacle.TRAP_FIRE:
            pts = camera.screen_circle_poly(cx, cy, 20.0, z, 16)
            pygame.draw.polygon(self.screen, (235, 120, 40, 110), pts)
            flame = camera.world_to_screen(cx, cy, z + 10)
            base = camera.world_to_screen(cx, cy, z)
            pygame.draw.line(self.screen, (250, 170, 60), base, flame, 3)
        elif obs.kind == Obstacle.TRAP_ICE:
            pts = camera.screen_circle_poly(cx, cy, 20.0, z, 16)
            pygame.draw.polygon(self.screen, (170, 220, 250, 120), pts)
            for dx, dy in ((-8, -4), (4, 6)):
                p1 = camera.world_to_screen(cx + dx, cy + dy, z)
                p2 = camera.world_to_screen(cx - dx, cy - dy, z)
                pygame.draw.line(self.screen, (240, 250, 255), p1, p2, 2)

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
        pygame.draw.polygon(self.screen, _shade(color, 0.62), face_y)
        pygame.draw.polygon(self.screen, _shade(color, 0.8), face_x)
        pygame.draw.polygon(self.screen, color, top)

    # ------------------------------------------------------------------
    # Range overlays (white turrets, light-green healers - specification)
    # ------------------------------------------------------------------
    def _draw_ranges(self, game, camera: Camera, overlay) -> None:
        """Range overlays: every translucent fill first, then every
        outline (same hue, less transparent), so an outline covers the
        fills of other ranges instead of blending into them."""
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
            z = (game.board.height(tile) * C.ELEVATION_PX) if tile else 0
            circles.append((camera.screen_circle_poly(
                v.x, v.y, C.BUFFER_HEAL_RADIUS, z),
                C.RANGE_HEAL_COLOR, C.RANGE_HEAL_OUTLINE))
        for pts, fill, _outline in circles:
            pygame.draw.polygon(overlay, fill, pts)
        for pts, _fill, outline in circles:
            pygame.draw.polygon(overlay, outline, pts,
                                C.RANGE_OUTLINE_WIDTH)

    # ------------------------------------------------------------------
    # Dashed travel paths (vanish behind the vehicle - specification)
    # ------------------------------------------------------------------
    def _draw_paths(self, game, camera: Camera, selection) -> None:
        if selection is not None and selection.get("path"):
            pts = [camera.world_to_screen(*game.board.center_world(t),
                                          game.board.height(t)
                                          * C.ELEVATION_PX)
                   for t in selection["path"]]
            src = game.building_at_tile(selection["src"])
            if src is not None:
                z = game.board.height(src.tile) * C.ELEVATION_PX
                pts.insert(0, camera.world_to_screen(*src.pos, z))
            self._dashed_polyline(pts, C.PATH_PREVIEW_COLOR)
        for v in game.vehicles:
            if not v.route or v.route_index >= len(v.route):
                continue
            pts = [camera.world_to_screen(v.x, v.y, self._vehicle_z(
                game, v))]
            for tile in v.route[v.route_index:]:
                z = game.board.height(tile) * C.ELEVATION_PX
                pts.append(camera.world_to_screen(
                    *game.board.center_world(tile), z))
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
                    pygame.draw.line(self.screen, color, p1, p2, 2)
                pos += run
                left -= run
                if left <= 0:
                    drawing = not drawing
                    left = dash if drawing else gap

    # ------------------------------------------------------------------
    # Vehicles, depth-sorted (buildings are drawn with their tiles)
    # ------------------------------------------------------------------
    def _draw_objects(self, game, camera: Camera, selection,
                      hover_tile) -> None:
        board = game.board
        if hover_tile is not None and board.contains(hover_tile):
            z = board.height(hover_tile) * C.ELEVATION_PX
            pts = [camera.world_to_screen(x, y, z) for x, y in
                   hexgrid.hex_corners(hover_tile[0], hover_tile[1],
                                       board.side)]
            pygame.draw.polygon(self.screen, (255, 255, 255), pts, 1)
        items = []
        for v in game.vehicles:
            items.append((v.x + v.y + 0.1, "v", v))
        items.sort(key=lambda it: it[0])
        for _, kind, obj in items:
            self._draw_vehicle(game, camera, obj)
        if selection is not None and selection.get("src") is not None:
            src = game.building_at_tile(selection["src"])
            if src is not None:
                z = board.height(src.tile) * C.ELEVATION_PX
                pts = camera.screen_circle_poly(src.pos[0], src.pos[1],
                                                30.0, z, 24)
                pygame.draw.polygon(self.screen, (255, 255, 255), pts, 2)

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
        ground = (game.board.height(tile) * C.ELEVATION_PX) if tile else 0
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
                pygame.draw.line(self.screen, _shade(color, 0.5), p1, p2, 3)
            elif b.kind == BuildingKind.BASE_HELICOPTER:
                pts = camera.screen_circle_poly(x, y, 9.0, top, 14)
                pygame.draw.polygon(self.screen, (240, 240, 240), pts, 2)
            elif b.kind == BuildingKind.BASE_HOVERCRAFT:
                pts = camera.screen_circle_poly(x, y, 12.0, top, 14)
                pygame.draw.polygon(self.screen, _shade(color, 0.7), pts, 3)
            else:  # buffer base
                self._draw_cross(camera, x, y, top, (110, 220, 120))
        elif is_turret(b.kind):
            self._iso_box(camera, x, y, z, 24, 24, 12, color)
            barrel = camera.world_to_screen(x, y, z + 22)
            base = camera.world_to_screen(x, y, z + 12)
            pygame.draw.line(self.screen, _shade(color, 0.55), base,
                             barrel, 4)
            if b.last_target_pos is not None:
                tx, ty = b.last_target_pos
                ang = math.atan2((tx - x) * C.ISO_SIN,
                                 (tx - x) * C.ISO_COS)
                tip = camera.world_to_screen(
                    x + math.cos(ang) * 12 / C.ISO_COS,
                    y + math.sin(ang) * 12 / C.ISO_COS, z + 22)
                pygame.draw.line(self.screen, _shade(color, 0.55),
                                 barrel, tip, 3)
        else:  # healing tower
            base = camera.world_to_screen(x, y, z)
            top = camera.world_to_screen(x, y, z + 34)
            pygame.draw.line(self.screen, _shade(color, 0.8), base, top, 5)
            self._draw_cross(camera, x, y, z + 34, (150, 245, 150))

    def _draw_cross(self, camera: Camera, x: float, y: float, z: float,
                    color) -> None:
        """Small standing plus-sign (marker of buffers / heal towers)."""
        h = 6.0
        p1 = camera.world_to_screen(x - h, y, z)
        p2 = camera.world_to_screen(x + h, y, z)
        p3 = camera.world_to_screen(x, y - h, z)
        p4 = camera.world_to_screen(x, y + h, z)
        pygame.draw.line(self.screen, color, p1, p2, 3)
        pygame.draw.line(self.screen, color, p3, p4, 3)

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
            pygame.draw.polygon(self.screen, color, pts)
            tail = camera.world_to_screen(x - 16, y, z)
            body = camera.world_to_screen(x, y, z)
            pygame.draw.line(self.screen, _shade(color, 0.7), body, tail, 3)
            ang = self._rotor_phase
            r = 22.0
            p1 = camera.world_to_screen(x + r * math.cos(ang),
                                        y + r * math.sin(ang), z + 5)
            p2 = camera.world_to_screen(x - r * math.cos(ang),
                                        y - r * math.sin(ang), z + 5)
            pygame.draw.line(self.screen, (210, 210, 210), p1, p2, 2)
        elif v.kind == VehicleKind.HOVERCRAFT:
            pts = camera.screen_circle_poly(x, y, 13.0, z, 14)
            pygame.draw.polygon(self.screen, color, pts)
            inner = camera.screen_circle_poly(x, y, 7.0, z + 4, 12)
            pygame.draw.polygon(self.screen, _shade(color, 0.7), inner)
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
    # Turret projectiles (purely visual - damage is instant, sec. 10)
    # ------------------------------------------------------------------
    def _draw_projectiles(self, game, camera: Camera) -> None:
        for p in game.projectiles:
            t = p["t"] / p["dur"]
            x, y = _lerp(p["from"], p["to"], t)
            tile = game.board.world_to_tile(x, y)
            z = (game.board.height(tile) * C.ELEVATION_PX + 14) if tile \
                else 14
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
