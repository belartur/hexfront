#!/usr/bin/env python3
"""Board editor of *War Regions* (a separate application; specification:
``specification_of_map_editor.md``).  Run with ``python3 editor.py [map]``.

Editing model: point a tile with the mouse (picked by exactly the same
code as in the game) and press a key that modifies the tile or the object
standing on it.  Placing an object overwrites the object previously on
the tile; a legend is displayed on screen; on exit the editor asks
whether to save unsaved changes.

Keys
----
``b``           place a building / cycle the kind of the existing one
digits          type the unit count of the building (0-255); committed
                immediately after the third digit, or shortly after the
                first or second one
``o``           cycle the building owner (needs a building on the tile)
``t``           place an obstacle / cycle its kind
``m``           place a bridge fragment / rotate it (a bridge on a
                neighbouring tile directed at this tile imposes its axis)
``r``           place a ramp / rotate it (prefers the axis whose opposite
                neighbours differ in height, when one exists)
``[`` / ``]``   lower / raise the terrain by 1 (modulo 16)
``Del`` / RMB   delete the object on the tile
``l``           load a map (pick a name from the maps directory)
``s``           save the map (type a name or pick an existing one; the
                current name, if any, is listed first and highlighted)
ctrl+n          clear / start a new map (no confirmation)

View (the editor spec does not describe it; mirrors the game): LMB drag,
arrow keys and screen-edge hover pan, wheel and ``+``/``-`` zoom
(0.5x-2x).  There is no WASD panning because ``s`` saves the map, and no
RMB panning because RMB deletes objects.
"""

import math
import os
import sys

import pygame

from war_regions import constants as C
from war_regions import hexgrid
from war_regions import mapfile
from war_regions.board import Board, Obstacle
from war_regions.camera import Camera
from war_regions.entities import Building, BuildingKind
from war_regions.render import Renderer

#: Building kinds behind the ``b`` key, in cycling order.
BUILDING_ORDER = list(BuildingKind)

#: Building owners behind the ``o`` key, in cycling order: None = neutral,
#: then player ids (0 = blue/human, 1 = red, 2 = green, 3 = yellow).
OWNER_ORDER = [None, 0, 1, 2, 3]
OWNER_LABELS = {None: "neutral", 0: "blue", 1: "red", 2: "green",
                3: "yellow"}

#: Obstacle kinds behind the ``t`` key, in cycling order.
OBSTACLE_ORDER = [Obstacle.WALL, Obstacle.MINE, Obstacle.MINE_WATER,
                  Obstacle.TRAP_FIRE, Obstacle.TRAP_ICE]

#: Digit keys accepted by the units entry (main row and keypad).
DIGIT_KEYS = {getattr(pygame, f"K_{i}"): str(i) for i in range(10)}
DIGIT_KEYS.update({getattr(pygame, f"K_KP{i}"): str(i) for i in range(10)})

#: Legend lines displayed on screen (editor spec: "wyswietla legende").
LEGEND = [
    "b: building (again: cycle kind)    digits: units 0-255",
    "o: cycle owner                     t: obstacle (again: cycle kind)",
    "m: bridge (again: rotate)          r: ramp (again: rotate)",
    "[ / ]: terrain -1 / +1 (mod 16)    Del / RMB: delete object",
    "l: load   s: save   Ctrl+N: new    Esc: quit",
    "LMB drag / arrows / screen edge: pan   wheel / + / -: zoom",
]


class EditorScene:
    """Minimal stand-in for the game state used by the shared renderer.

    The tile painter and the badge painter only access ``board``,
    ``buildings`` and ``vehicles``.
    """

    def __init__(self, board: Board, buildings: list):
        self.board = board
        self.buildings = buildings
        self.vehicles = []           # never any vehicles in the editor


class Editor:
    """Owns the editor window, the edited board and the key actions."""

    def __init__(self, size=(1180, 720)):
        pygame.init()
        self.screen = pygame.display.set_mode(size, pygame.RESIZABLE)
        pygame.display.set_caption("War Regions - Board Editor")
        self.clock = pygame.time.Clock()
        self.renderer = Renderer(self.screen)
        self.camera = Camera(self.screen.get_size())
        self.scene = None
        self.running = True
        self.map_name = None         # current file name (without .map)
        self.dirty = False           # unsaved changes since last save
        self._new_board(*C.EDITOR_DEFAULT_SIZE)
        self.status = ""             # last status message
        self.status_timer = 0.0
        self.overlay = None          # None | "load" | "save" | "exit"
        self.overlay_index = 0       # highlighted row of the overlay list
        self.overlay_items = []      # names listed by load/save overlays
        self.input_text = ""         # name typed in the save overlay
        self._exit_after_save = False
        self._digit_tile = None      # tile the pending digits apply to
        self._digit_text = ""
        self._digit_timer = 0.0
        self._last_axis = 0          # fallback ramp/bridge axis
        self._down_pos = None        # LMB press position (drag panning)
        self._dragging = False
        self.mouse_pos = (0, 0)

    # ------------------------------------------------------------------
    def run(self) -> None:
        """Main loop; exits after the save-changes prompt is answered."""
        while self.running:
            dt = min(self.clock.tick(C.FPS) / 1000.0, 0.1)
            self._handle_events()
            self._update(dt)
            self._draw()
            pygame.display.flip()
        pygame.quit()

    # ------------------------------------------------------------------
    # Small helpers
    # ------------------------------------------------------------------
    def _say(self, message: str) -> None:
        """Show a status message for a few seconds."""
        self.status = message
        self.status_timer = 4.0

    def _mark_dirty(self) -> None:
        """Flag the board as modified (drives the exit save prompt)."""
        self.dirty = True

    def _building_at(self, tile: tuple):
        """Building standing on ``tile`` or ``None``."""
        for b in self.scene.buildings:
            if b.tile == tile:
                return b
        return None

    def _hover_tile(self):
        """Tile under the cursor, picked exactly like in the game."""
        return self.scene.board.pick_tile(self.camera, self.mouse_pos)

    def _clear_digit_buffer(self) -> None:
        """Drop a pending units entry (used by every other action)."""
        self._digit_tile = None
        self._digit_text = ""
        self._digit_timer = 0.0

    def _bridge_marks(self) -> dict:
        """Per-tile bridge fragment axes of the current board."""
        return {tile: t.bridge.direction % 3
                for tile, t in self.scene.board.tiles.items()
                if t.bridge is not None}

    def _rebuild_bridges(self) -> None:
        """Re-derive the board's bridges from its deck fragments.

        Uses the editor's preview mode: fragment runs that violate the
        bridge geometry of rules.md sec. 8 are still rendered (the game
        validates them when loading the saved file).
        """
        mapfile.rebuild_bridges(self.scene.board, self._bridge_marks(),
                                validate=False)

    def _clear_objects(self, tile: tuple) -> None:
        """Remove every object from ``tile``.

        The editor spec says "wstawienie obiektu nadpisuje obiekt ktory
        znajdowal sie na polu wczesniej", so every placing action starts
        by clearing the tile.
        """
        building = self._building_at(tile)
        if building is not None:
            self.scene.buildings.remove(building)
        t = self.scene.board.tiles[tile]
        t.obstacle = None
        t.ramp = None
        t.bridge = None
        self._rebuild_bridges()

    # ------------------------------------------------------------------
    # Key actions (specification_of_map_editor.md)
    # ------------------------------------------------------------------
    def _action_building(self, tile: tuple) -> None:
        """``b``: place a building or cycle the kind of the existing one."""
        self._clear_digit_buffer()
        building = self._building_at(tile)
        if building is not None:
            building.kind = BUILDING_ORDER[
                (BUILDING_ORDER.index(building.kind) + 1)
                % len(BUILDING_ORDER)]
        else:
            self._clear_objects(tile)
            self.scene.buildings.append(Building(
                BUILDING_ORDER[0], C.EDITOR_DEFAULT_OWNER,
                tile[0], tile[1], units=0.0))
        self._mark_dirty()

    def _action_owner(self, tile: tuple) -> None:
        """``o``: cycle the owner of the building on ``tile``."""
        self._clear_digit_buffer()
        building = self._building_at(tile)
        if building is None:
            self._say("no building on the tile")
            return
        building.owner = OWNER_ORDER[
            (OWNER_ORDER.index(building.owner) + 1) % len(OWNER_ORDER)]
        self._mark_dirty()

    def _action_digit(self, tile: tuple, digit: str) -> None:
        """Digits: type the unit count of the building on ``tile``.

        Three digits commit immediately; shorter entries commit after
        ``EDITOR_DIGIT_COMMIT_DELAY`` seconds.
        """
        if self._digit_tile != tile:
            self._digit_tile, self._digit_text = tile, ""
        self._digit_text += digit
        self._digit_timer = 0.0
        if len(self._digit_text) >= 3:
            self._commit_digits()
        else:
            self._say(f"units: {self._digit_text}")

    def _digit_tick(self, dt: float) -> None:
        """Commit a pending 1- or 2-digit entry after the delay."""
        if not self._digit_text:
            return
        self._digit_timer += dt
        if self._digit_timer >= C.EDITOR_DIGIT_COMMIT_DELAY:
            self._commit_digits()

    def _commit_digits(self) -> None:
        """Apply the typed unit count to the building (clamped 0-255)."""
        tile, text = self._digit_tile, self._digit_text
        self._clear_digit_buffer()
        if tile is None:
            return
        building = self._building_at(tile)
        if building is None:
            self._say("no building on the tile")
            return
        building.units = float(max(0, min(255, int(text))))
        self._mark_dirty()

    def _action_obstacle(self, tile: tuple) -> None:
        """``t``: place an obstacle or cycle the kind of the existing one."""
        self._clear_digit_buffer()
        current = self.scene.board.tiles[tile].obstacle
        if current is not None:
            kind = OBSTACLE_ORDER[(OBSTACLE_ORDER.index(current.kind) + 1)
                                  % len(OBSTACLE_ORDER)]
        else:
            kind = OBSTACLE_ORDER[0]
        self._clear_objects(tile)
        self.scene.board.tiles[tile].obstacle = Obstacle(kind)
        self._mark_dirty()

    def _inherited_bridge_axis(self, tile: tuple):
        """Axis of a neighbouring bridge directed at ``tile``, or None."""
        board = self.scene.board
        for d in range(6):
            n = hexgrid.neighbor(tile[0], tile[1], d)
            if not board.contains(n):
                continue
            bridge = board.tiles[n].bridge
            if bridge is None:
                continue
            axis = bridge.direction % 3
            if tile in (hexgrid.neighbor(n[0], n[1], axis),
                        hexgrid.neighbor(n[0], n[1], axis + 3)):
                return axis
        return None

    def _action_bridge(self, tile: tuple) -> None:
        """``m``: place a bridge fragment or rotate the existing one.

        A fragment inherits the axis of a neighbouring bridge directed at
        this tile (editor spec); otherwise the default axis 0 is used.
        """
        self._clear_digit_buffer()
        existing = self.scene.board.tiles[tile].bridge
        if existing is not None:
            axis = (existing.direction + 1) % 3
        else:
            axis = self._inherited_bridge_axis(tile)
            if axis is None:
                axis = 0
        self._clear_objects(tile)
        marks = self._bridge_marks()
        marks[tile] = axis
        mapfile.rebuild_bridges(self.scene.board, marks, validate=False)
        self._last_axis = axis
        self._mark_dirty()

    def _action_ramp(self, tile: tuple) -> None:
        """``r``: place a ramp or rotate the existing one.

        A new ramp prefers the axis whose opposite neighbours differ in
        height (editor spec); when no such pair exists, the last used
        axis is applied.  The ramp tile's height follows rules.md sec. 7
        (min of both ends) via :meth:`Board.set_ramp`.
        """
        self._clear_digit_buffer()
        board = self.scene.board
        existing = board.tiles[tile].ramp
        if existing is not None:
            axis = (_ramp_axis(tile, existing) + 1) % 3
        else:
            axis = None
            for d in range(3):
                a = hexgrid.neighbor(tile[0], tile[1], d)
                b = hexgrid.neighbor(tile[0], tile[1], d + 3)
                if board.contains(a) and board.contains(b) \
                        and board.height(a) != board.height(b):
                    axis = d
                    break
            if axis is None:
                axis = self._last_axis
                self._say("no differing opposite neighbours "
                          "- using the last axis")
        a = hexgrid.neighbor(tile[0], tile[1], axis)
        b = hexgrid.neighbor(tile[0], tile[1], axis + 3)
        if not (board.contains(a) and board.contains(b)):
            self._say("both opposite neighbours must lie on the board")
            return
        self._clear_objects(tile)
        board.set_ramp(tile, a, b)
        self._last_axis = axis
        self._mark_dirty()

    def _change_height(self, tile: tuple, delta: int) -> None:
        """``[`` / ``]``: lower / raise the terrain by 1 (modulo 16)."""
        self._clear_digit_buffer()
        t = self.scene.board.tiles[tile]
        t.height = (t.height + delta) % 16
        if t.ramp is not None:
            self._say("note: a ramp's height follows min(a, b) on load")
        self._mark_dirty()

    def _delete_object(self, tile: tuple) -> None:
        """``Del`` / RMB: remove the object on ``tile`` (terrain stays)."""
        self._clear_digit_buffer()
        t = self.scene.board.tiles[tile]
        building = self._building_at(tile)
        changed = building is not None or t.obstacle is not None \
            or t.ramp is not None or t.bridge is not None
        if building is not None:
            self.scene.buildings.remove(building)
        t.obstacle = None
        t.ramp = None
        t.bridge = None
        self._rebuild_bridges()
        if changed:
            self._mark_dirty()

    def _delete_at(self, pos) -> None:
        """RMB: delete the object under the cursor, if any."""
        tile = self._hover_tile()
        if tile is not None:
            self._delete_object(tile)

    # ------------------------------------------------------------------
    # Files (``l`` / ``s`` / ctrl+n)
    # ------------------------------------------------------------------
    def _new_board(self, cols: int, rows: int) -> None:
        """Replace the edited scene with an empty cols x rows board."""
        cols, rows = max(2, cols), max(2, rows)
        board = Board(cols, rows)
        for t in board.tiles.values():
            t.height = 1
        self.scene = EditorScene(board, [])
        self.map_name = None
        self.dirty = False
        mid = (cols // 2, rows // 2)
        self.camera.center_on_world(*board.center_world(mid))

    def _new_map(self) -> None:
        """ctrl+n: clear the board keeping its dimensions (no prompt)."""
        self._new_board(self.scene.board.cols, self.scene.board.rows)
        self._clear_digit_buffer()
        self._say("new map")

    def _save(self, name: str) -> None:
        """Save the board as ``maps/<name>.map``."""
        name = self._map_name(name)
        path = os.path.join(C.MAPS_DIR, name)
        os.makedirs(C.MAPS_DIR, exist_ok=True)
        try:
            mapfile.save_map(path, self.scene.board, self.scene.buildings)
        except OSError as exc:
            self._say(f"save failed: {exc}")
            return
        self.map_name = os.path.splitext(name)[0]   # stem, like the lists
        self.dirty = False
        self._say(f"saved {path}")

    def _load(self, name: str) -> None:
        """Load the board from ``maps/<name>.map`` (or a full path)."""
        path = name if os.path.isfile(name) \
            else os.path.join(C.MAPS_DIR, self._map_name(name))
        try:
            board, buildings = mapfile.load_board(path, validate=False)
        except (OSError, ValueError) as exc:
            self._say(f"load failed: {exc}")
            return
        self.scene = EditorScene(board, buildings)
        self._rebuild_bridges()          # preview runs, incl. invalid ones
        self.map_name = os.path.splitext(os.path.basename(path))[0]
        self.dirty = False
        self._clear_digit_buffer()
        mid = (board.cols // 2, board.rows // 2)
        self.camera.center_on_world(*board.center_world(mid))
        self._say(f"loaded {path}")

    @staticmethod
    def _map_name(name: str) -> str:
        """Normalise a map file name (append the standard extension)."""
        name = os.path.basename(name.strip())
        if not name.endswith(C.MAP_EXTENSION):
            name += C.MAP_EXTENSION
        return name

    # ------------------------------------------------------------------
    # Exit prompt (editor spec: ask about unsaved changes)
    # ------------------------------------------------------------------
    def _request_exit(self) -> None:
        """Ask whether to save unless everything is already saved."""
        if self.dirty:
            self.overlay = "exit"
        else:
            self.running = False

    # ------------------------------------------------------------------
    # Events
    # ------------------------------------------------------------------
    def _handle_events(self) -> None:
        """Process one frame of events."""
        for ev in pygame.event.get():
            if ev.type == pygame.QUIT:
                self._request_exit()
            elif ev.type == pygame.VIDEORESIZE:
                self.screen = pygame.display.set_mode(ev.size,
                                                      pygame.RESIZABLE)
                self.renderer.screen = self.screen
                self.camera.screen_size = tuple(ev.size)
            elif ev.type == pygame.MOUSEWHEEL:
                self.camera.zoom_at(
                    C.ZOOM_STEP if ev.y > 0 else 1.0 / C.ZOOM_STEP,
                    *self.mouse_pos)
            elif ev.type == pygame.MOUSEBUTTONDOWN:
                if ev.button == 1:
                    if self.overlay is not None:
                        self._overlay_click(ev.pos)
                    else:
                        self._down_pos = ev.pos
                        self._dragging = False
                elif ev.button == 3 and self.overlay is None:
                    self._delete_at(ev.pos)      # RMB deletes the object
                elif ev.button in (4, 5) and self.overlay is None:
                    self.camera.zoom_at(
                        C.ZOOM_STEP if ev.button == 4
                        else 1.0 / C.ZOOM_STEP, *ev.pos)
            elif ev.type == pygame.MOUSEBUTTONUP and ev.button == 1:
                self._down_pos = None
            elif ev.type == pygame.MOUSEMOTION:
                self.mouse_pos = ev.pos
                if (self.overlay is None and ev.buttons[0]
                        and self._down_pos is not None
                        and (self._dragging
                             or math.hypot(ev.pos[0] - self._down_pos[0],
                                           ev.pos[1] - self._down_pos[1])
                             > C.DRAG_THRESHOLD)):
                    self._dragging = True
                    self.camera.pan(ev.rel[0], ev.rel[1])
            elif ev.type == pygame.TEXTINPUT and self.overlay == "save":
                self.input_text += ev.text
            elif ev.type == pygame.KEYDOWN:
                self._key(ev)

    def _key(self, ev: pygame.event.Event) -> None:
        """Dispatch a key press depending on the overlay state."""
        if self.overlay == "exit":
            if ev.key == pygame.K_s:
                self.overlay = None
                if self.map_name is not None:
                    self._save(self.map_name)
                    self.running = False
                else:
                    self._exit_after_save = True
                    self._open_save()
            elif ev.key == pygame.K_n:
                self.running = False             # discard the changes
            elif ev.key == pygame.K_ESCAPE:
                self.overlay = None              # keep editing
            return

        if self.overlay == "load":
            if ev.key == pygame.K_RETURN:
                self._confirm_load()
            elif ev.key == pygame.K_ESCAPE:
                self.overlay = None
            elif ev.key in (pygame.K_UP, pygame.K_DOWN):
                self._move_overlay_selection(ev.key == pygame.K_DOWN)
            return

        if self.overlay == "save":
            if ev.key == pygame.K_RETURN:
                self._confirm_save()
            elif ev.key == pygame.K_ESCAPE:
                self.overlay = None
            elif ev.key == pygame.K_BACKSPACE:
                self.input_text = self.input_text[:-1]
            elif ev.key in (pygame.K_UP, pygame.K_DOWN):
                self._move_overlay_selection(ev.key == pygame.K_DOWN)
            return

        # ---- no overlay: board editing keys ----
        mods = pygame.key.get_mods()
        if ev.key == pygame.K_n and mods & pygame.KMOD_CTRL:
            self._new_map()
            return
        if ev.key == pygame.K_ESCAPE:
            self._request_exit()
            return
        if ev.key in (pygame.K_PLUS, pygame.K_EQUALS, pygame.K_KP_PLUS):
            self.camera.zoom_at(C.ZOOM_STEP, *self.mouse_pos)
            return
        if ev.key in (pygame.K_MINUS, pygame.K_KP_MINUS):
            self.camera.zoom_at(1.0 / C.ZOOM_STEP, *self.mouse_pos)
            return

        tile = self._hover_tile()
        if ev.key == pygame.K_l:
            self._open_load()
        elif ev.key == pygame.K_s:
            self._open_save()
        elif tile is None:
            return                               # keys below need a tile
        elif ev.key == pygame.K_b:
            self._action_building(tile)
        elif ev.key == pygame.K_o:
            self._action_owner(tile)
        elif ev.key == pygame.K_t:
            self._action_obstacle(tile)
        elif ev.key == pygame.K_m:
            self._action_bridge(tile)
        elif ev.key == pygame.K_r:
            self._action_ramp(tile)
        elif ev.key == pygame.K_LEFTBRACKET:
            self._change_height(tile, -1)
        elif ev.key == pygame.K_RIGHTBRACKET:
            self._change_height(tile, +1)
        elif ev.key == pygame.K_DELETE:
            self._delete_object(tile)
        elif ev.key in DIGIT_KEYS:
            self._action_digit(tile, DIGIT_KEYS[ev.key])

    # ------------------------------------------------------------------
    # Overlays (load / save / exit prompt)
    # ------------------------------------------------------------------
    def _open_load(self) -> None:
        """``l``: show the map names of the maps directory."""
        self.overlay_items = [os.path.splitext(os.path.basename(p))[0]
                              for p in mapfile.list_maps()]
        if not self.overlay_items:
            self._say(f"no maps in {C.MAPS_DIR}/")
            return
        self.overlay, self.overlay_index = "load", 0

    def _open_save(self) -> None:
        """``s``: name entry + list of existing maps.

        The current name of the map, if any, is listed first and
        highlighted (editor spec).
        """
        existing = [os.path.splitext(os.path.basename(p))[0]
                    for p in mapfile.list_maps()]
        others = [n for n in existing if n != self.map_name]
        self.overlay_items = ([self.map_name] if self.map_name else []) \
            + others
        self.overlay_index = 0 if self.map_name else -1
        self.input_text = self.map_name or ""
        self.overlay = "save"

    def _move_overlay_selection(self, down: bool) -> None:
        """Move the highlighted row of the overlay list (wraps around)."""
        if not self.overlay_items:
            return
        self.overlay_index = ((self.overlay_index + (1 if down else -1))
                              % len(self.overlay_items))
        if self.overlay == "save":
            self.input_text = self.overlay_items[self.overlay_index]

    def _overlay_click(self, pos) -> None:
        """Click inside an overlay: pick a list row (load/save overlays)."""
        for rect, index in self._item_rects:
            if rect.collidepoint(pos):
                self.overlay_index = index
                if self.overlay == "save":
                    self.input_text = self.overlay_items[index]
                return

    def _confirm_load(self) -> None:
        """Load the highlighted map name."""
        if 0 <= self.overlay_index < len(self.overlay_items):
            name = self.overlay_items[self.overlay_index]
            self.overlay = None
            self._load(name)

    def _confirm_save(self) -> None:
        """Save under the typed / selected name."""
        name = self.input_text.strip()
        if not name:
            self._say("type a map name first")
            return
        self.overlay = None
        self._save(name)
        if self._exit_after_save:
            self.running = False

    # ------------------------------------------------------------------
    # Update / draw
    # ------------------------------------------------------------------
    def _update(self, dt: float) -> None:
        """Timers plus arrow-key and screen-edge panning (like the game)."""
        self._digit_tick(dt)
        if self.status_timer > 0:
            self.status_timer -= dt
        if self.overlay is not None:
            return
        self.mouse_pos = pygame.mouse.get_pos()
        speed = C.PAN_SPEED * dt
        dx = dy = 0
        keys = pygame.key.get_pressed()
        if keys[pygame.K_LEFT]:
            dx -= speed
        if keys[pygame.K_RIGHT]:
            dx += speed
        if keys[pygame.K_UP]:
            dy -= speed
        if keys[pygame.K_DOWN]:
            dy += speed
        mx, my = self.mouse_pos
        w, h = self.camera.screen_size
        if mx < C.EDGE_PAN_MARGIN:
            dx -= speed
        if mx > w - C.EDGE_PAN_MARGIN:
            dx += speed
        if my < C.EDGE_PAN_MARGIN:
            dy -= speed
        if my > h - C.EDGE_PAN_MARGIN:
            dy += speed
        if dx or dy:
            self.camera.pan(-dx, -dy)

    def _draw(self) -> None:
        """One editor frame: board, HUD, legend and an overlay if open."""
        hover = self._hover_tile() if self.overlay is None else None
        self.renderer.draw_editor(self.scene, self.camera, hover)
        self._draw_hud()
        self._draw_legend()
        self._item_rects = []
        if self.overlay == "exit":
            self._draw_exit()
        elif self.overlay == "load":
            self._draw_list("load map", self.overlay_items)
        elif self.overlay == "save":
            self._draw_save()

    def _draw_hud(self) -> None:
        """Map name (with an unsaved-changes marker) and status message."""
        f = self.renderer.font(24)
        w, _h = self.screen.get_size()
        name = self.map_name or "(unnamed)"
        if self.dirty:
            name += " *"
        surf = f.render(name, True, C.UI_TEXT_COLOR)
        self.screen.blit(surf, (w - surf.get_width() - 10, 10))
        if self.status and self.status_timer > 0:
            surf = f.render(self.status, True, (255, 230, 120))
            self.screen.blit(surf, (w - surf.get_width() - 10, 36))

    def _draw_legend(self) -> None:
        """The key legend (editor spec: "wyswietla legende")."""
        f = self.renderer.font(20)
        _w, h = self.screen.get_size()
        y = h - 12 - len(LEGEND) * 22
        for line in LEGEND:
            self.screen.blit(f.render(line, True, C.UI_TEXT_COLOR), (10, y))
            y += 22

    def _panel(self, title: str, height: int) -> pygame.Rect:
        """Centred dialog panel with a title; returns the panel rect."""
        w, h = self.screen.get_size()
        box = pygame.Rect(0, 0, 520, height)
        box.center = (w // 2, h // 2)
        pygame.draw.rect(self.screen, (30, 32, 40), box, border_radius=8)
        pygame.draw.rect(self.screen, (120, 140, 200), box, 2,
                         border_radius=8)
        self.screen.blit(self.renderer.font(28).render(
            title, True, C.UI_TEXT_COLOR), (box.x + 16, box.y + 12))
        return box

    def _draw_list(self, title: str, items: list) -> None:
        """Scrollable name list with a highlighted selection row."""
        self._panel(title, 440)
        f = self.renderer.font(24)
        visible = 12
        start = 0
        if len(items) > visible:
            start = max(0, min(self.overlay_index - visible // 2,
                               len(items) - visible))
        w, h = self.screen.get_size()
        y = h // 2 - 150
        for index in range(start, min(start + visible, len(items))):
            rect = pygame.Rect(w // 2 - 240, y, 480, 30)
            if index == self.overlay_index:
                pygame.draw.rect(self.screen, (52, 58, 78), rect,
                                 border_radius=6)
                pygame.draw.rect(self.screen, (150, 170, 230), rect, 2,
                                 border_radius=6)
            self.screen.blit(f.render(items[index], True, C.UI_TEXT_COLOR),
                             (rect.x + 10, rect.y + 4))
            self._item_rects.append((rect, index))
            y += 32
        hint = self.renderer.font(18).render(
            "arrows / click: choose   Enter: confirm   Esc: cancel", True,
            (130, 135, 150))
        self.screen.blit(hint, hint.get_rect(center=(w // 2, h // 2 + 195)))

    def _draw_save(self) -> None:
        """Save dialog: name entry on top, existing names below.

        The current name (if the map has one) is the first list entry and
        is highlighted (editor spec).
        """
        self._panel("save map", 500)
        f = self.renderer.font(26)
        w, h = self.screen.get_size()
        field = pygame.Rect(w // 2 - 240, h // 2 - 148, 480, 36)
        pygame.draw.rect(self.screen, (18, 20, 26), field, border_radius=6)
        self.screen.blit(f.render(self.input_text + "_", True,
                                  (255, 255, 120)),
                         (field.x + 10, field.y + 4))
        self._draw_list("save map", self.overlay_items)

    def _draw_exit(self) -> None:
        """Prompt shown on exit when there are unsaved changes."""
        self._panel("unsaved changes", 190)
        lines = ["S - save and exit",
                 "N - discard changes and exit",
                 "Esc - keep editing"]
        f = self.renderer.font(24)
        w, h = self.screen.get_size()
        y = h // 2 - 30
        for line in lines:
            self.screen.blit(f.render(line, True, C.UI_TEXT_COLOR),
                             (w // 2 - 120, y))
            y += 34


def _ramp_axis(tile: tuple, ramp: tuple) -> int:
    """Axis 0-2 of a ramp whose neighbour ``a`` is ``ramp[0]``."""
    for d in range(6):
        if hexgrid.neighbor(tile[0], tile[1], d) == ramp[0]:
            return d % 3
    return 0                                     # unreachable for ramps


def main() -> None:
    """Start the editor, optionally opening a map file from argv."""
    editor = Editor()
    if len(sys.argv) > 1:
        editor._load(sys.argv[1])
    editor.run()


if __name__ == "__main__":
    main()
