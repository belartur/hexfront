#!/usr/bin/env python3
"""Board editor of *War Regions* (a separate application, specification:
"Planszy i edytor plansz").  Run with ``python3 editor.py [map file]``.

The editor draws boards with the same renderer code as the game
(:meth:`war_regions.render.Renderer.draw_editor`) and saves them in the
binary map format described in the specification ("Format pliku planszy").

Controls
--------
LMB              apply the selected tool (paint while dragging)
RMB drag         pan the view
wheel / + / -    zoom
WASD / arrows    pan the view
1..0, i          select a tool (the palette in the top-left corner)
TAB              next building kind            O  cycle building owner
[ / ]            change starting units         R  cycle ramp/bridge axis
N                new board (type the size as COLSxROWS, e.g. 24x15)
Ctrl+S           save to maps/<name>.map       Ctrl+L / Ctrl+O  load
Esc              cancel text input / quit
"""

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

# ----------------------------------------------------------------------
# Tools
# ----------------------------------------------------------------------
TOOL_RAISE = "raise"        # raise the terrain by 1 (up to 15)
TOOL_LOWER = "lower"        # lower the terrain by 1 (down to 0 = water)
TOOL_BUILDING = "building"  # place the currently configured building
TOOL_ERASE = "erase"        # remove everything from a tile
TOOL_RAMP = "ramp"          # ramp along the current axis (rules sec. 7)
TOOL_BRIDGE = "bridge"      # bridge deck fragment (rules sec. 8)
TOOL_WALL = "wall"          # obstacles (rules sec. 1, 4):
TOOL_MINE = "mine"
TOOL_MINE_WATER = "mine_water"
TOOL_TRAP_FIRE = "trap_fire"
TOOL_TRAP_ICE = "trap_ice"

#: Palette entries: (hotkey, label, tool).
TOOLS = [
    ("1", "terrain +", TOOL_RAISE),
    ("2", "terrain -", TOOL_LOWER),
    ("3", "building", TOOL_BUILDING),
    ("4", "erase", TOOL_ERASE),
    ("5", "ramp", TOOL_RAMP),
    ("6", "bridge", TOOL_BRIDGE),
    ("7", "wall", TOOL_WALL),
    ("8", "mine", TOOL_MINE),
    ("9", "water mine", TOOL_MINE_WATER),
    ("0", "fire trap", TOOL_TRAP_FIRE),
    ("i", "ice trap", TOOL_TRAP_ICE),
]

#: Hotkey -> tool lookup.
KEY_TOOLS = {key: tool for key, _label, tool in TOOLS}

#: Building kinds placed by the building tool, in TAB-cycling order.
BUILDING_ORDER = list(BuildingKind)

#: Owner choices of the building tool: None = neutral, then player ids
#: (0 = blue/human, 1 = red, 2 = green, 3 = yellow; specification owner
#: bytes are id + 1).
OWNER_ORDER = [None, 0, 1, 2, 3]
OWNER_LABELS = {None: "neutral", 0: "blue", 1: "red", 2: "green",
                3: "yellow"}


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
    """Owns the editor window, the edited board and all tool state."""

    def __init__(self, size=(1180, 720)):
        pygame.init()
        self.screen = pygame.display.set_mode(size, pygame.RESIZABLE)
        pygame.display.set_caption("War Regions - Board Editor")
        self.clock = pygame.time.Clock()
        self.renderer = Renderer(self.screen)
        self.camera = Camera(self.screen.get_size())
        self.scene = None
        self.bridge_marks = {}       # tile -> axis 0-2 (deck fragments)
        self._new_board(20, 13)
        self.tool = TOOL_RAISE
        self.building_index = 0      # index into BUILDING_ORDER
        self.owner_index = 1         # index into OWNER_ORDER
        self.units = 20              # starting units of placed buildings
        self.axis = 0                # ramp/bridge axis 0-2
        self.status = ""             # last status message
        self.status_timer = 0.0
        self.palette_rects = []      # [(rect, tool), ...]
        self.input_mode = None       # None | "save" | "load" | "new"
        self.input_text = ""
        self.painting = False
        self.panning = False
        self._last_painted = None
        self.mouse_pos = (0, 0)

    # ------------------------------------------------------------------
    def run(self) -> None:
        """Main loop; exits when the window is closed."""
        while True:
            dt = min(self.clock.tick(C.FPS) / 1000.0, 0.1)
            if not self._handle_events():
                break
            self._update(dt)
            self._draw()
            pygame.display.flip()
        pygame.quit()

    # ------------------------------------------------------------------
    # Board mutations
    # ------------------------------------------------------------------
    def _new_board(self, cols: int, rows: int) -> None:
        """Replace the edited board with an empty cols x rows one."""
        cols, rows = max(2, cols), max(2, rows)
        board = Board(cols, rows)
        for t in board.tiles.values():
            t.height = 1
        self.scene = EditorScene(board, [])
        self.bridge_marks = {}
        mid = (cols // 2, rows // 2)
        self.camera.center_on_world(*board.center_world(mid))

    def _building_at(self, tile: tuple):
        """Building standing on ``tile`` or ``None``."""
        for b in self.scene.buildings:
            if b.tile == tile:
                return b
        return None

    def _tile_free(self, tile: tuple) -> bool:
        """True when nothing stands on ``tile``."""
        t = self.scene.board.tiles[tile]
        return (t.obstacle is None and t.ramp is None and t.bridge is None
                and self._building_at(tile) is None)

    def _say(self, message: str) -> None:
        """Show a status message for a few seconds."""
        self.status = message
        self.status_timer = 4.0

    def _apply_tool(self, tile: tuple) -> None:
        """Apply the current tool to ``tile`` (rules-constrained)."""
        if not self.scene.board.contains(tile):
            return
        if self.tool == TOOL_RAISE:
            self._change_height(tile, +1)
        elif self.tool == TOOL_LOWER:
            self._change_height(tile, -1)
        elif self.tool == TOOL_BUILDING:
            self._place_building(tile)
        elif self.tool == TOOL_ERASE:
            self._erase(tile)
        elif self.tool == TOOL_RAMP:
            self._place_ramp(tile)
        elif self.tool == TOOL_BRIDGE:
            self._place_bridge_fragment(tile)
        else:
            self._place_obstacle(tile)

    def _change_height(self, tile: tuple, delta: int) -> None:
        """Raise/lower terrain; tiles with objects are protected."""
        t = self.scene.board.tiles[tile]
        if not self._tile_free(tile):
            self._say("tile occupied - erase it first")
            return
        new = max(0, min(15, t.height + delta))
        if new != t.height:
            t.height = new

    def _place_building(self, tile: tuple) -> None:
        """Place the configured building on a free land tile (sec. 1)."""
        if not self._tile_free(tile):
            self._say("tile occupied - erase it first")
            return
        if self.scene.board.height(tile) == 0:
            self._say("buildings need land (height > 0)")
            return
        kind = BUILDING_ORDER[self.building_index]
        owner = OWNER_ORDER[self.owner_index]
        self.scene.buildings.append(
            Building(kind, owner, tile[0], tile[1], units=float(self.units)))

    def _place_ramp(self, tile: tuple) -> None:
        """Ramp joining the two opposite neighbours along the axis."""
        if not self._tile_free(tile):
            self._say("tile occupied - erase it first")
            return
        board = self.scene.board
        a = hexgrid.neighbor(tile[0], tile[1], self.axis)
        b = hexgrid.neighbor(tile[0], tile[1], self.axis + 3)
        if not (board.contains(a) and board.contains(b)):
            self._say("both opposite neighbours must lie on the board")
            return
        board.set_ramp(tile, a, b)   # height becomes min(a, b), sec. 7

    def _place_bridge_fragment(self, tile: tuple) -> None:
        """Add a deck fragment and (re)form the whole bridge (sec. 8)."""
        if not self._tile_free(tile):
            self._say("tile occupied - erase it first")
            return
        self.bridge_marks[tile] = self.axis
        mapfile.rebuild_bridges(self.scene.board, self.bridge_marks)
        bridge = self.scene.board.tiles[tile].bridge
        if bridge is None:
            del self.bridge_marks[tile]
            self._say("invalid bridge - the ends must be equal land of "
                      "height >= 3 with low ground between")
        else:
            self.bridge_marks[tile] = bridge.direction % 3

    def _place_obstacle(self, tile: tuple) -> None:
        """Place an obstacle honouring the placement rules (sec. 1)."""
        if not self._tile_free(tile):
            self._say("tile occupied - erase it first")
            return
        t = self.scene.board.tiles[tile]
        if self.tool == TOOL_MINE_WATER:
            if t.height != 0:
                self._say("water mines go on water (height 0)")
                return
        elif t.height == 0:
            self._say("this obstacle needs land (height > 0)")
            return
        t.obstacle = Obstacle(self.tool)

    def _erase(self, tile: tuple) -> None:
        """Remove the building, obstacle, ramp or bridge fragment."""
        board = self.scene.board
        building = self._building_at(tile)
        if building is not None:
            self.scene.buildings.remove(building)
        board.tiles[tile].obstacle = None
        board.tiles[tile].ramp = None
        if tile in self.bridge_marks:
            del self.bridge_marks[tile]
            mapfile.rebuild_bridges(board, self.bridge_marks)

    # ------------------------------------------------------------------
    # Files
    # ------------------------------------------------------------------
    def _save(self, name: str) -> None:
        """Save the board as ``maps/<name>.map``."""
        path = os.path.join(C.MAPS_DIR, self._map_name(name))
        os.makedirs(C.MAPS_DIR, exist_ok=True)
        try:
            mapfile.save_map(path, self.scene.board, self.scene.buildings)
            self._say(f"saved {path}")
        except OSError as exc:
            self._say(f"save failed: {exc}")

    def _load(self, name: str) -> None:
        """Load the board from ``maps/<name>.map`` (or a full path)."""
        path = name if os.path.isfile(name) \
            else os.path.join(C.MAPS_DIR, self._map_name(name))
        try:
            board, buildings = mapfile.load_board(path)
        except (OSError, ValueError) as exc:
            self._say(f"load failed: {exc}")
            return
        self.scene = EditorScene(board, buildings)
        self.bridge_marks = {
            t: board.tiles[t].bridge.direction % 3
            for t in board.tiles if board.tiles[t].bridge is not None}
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
    # Events
    # ------------------------------------------------------------------
    def _handle_events(self) -> bool:
        """Process one frame of events; False ends the editor."""
        for ev in pygame.event.get():
            if ev.type == pygame.QUIT:
                return False
            elif ev.type == pygame.VIDEORESIZE:
                self.screen = pygame.display.set_mode(ev.size,
                                                      pygame.RESIZABLE)
                self.renderer.screen = self.screen
                self.camera.screen_size = tuple(ev.size)
            elif ev.type == pygame.MOUSEWHEEL:
                self.camera.zoom_at(
                    C.ZOOM_STEP if ev.y > 0 else 1.0 / C.ZOOM_STEP,
                    *pygame.mouse.get_pos())
            elif ev.type == pygame.MOUSEBUTTONDOWN:
                if ev.button == 1:
                    if not self._palette_click(ev.pos):
                        self.painting = True
                        self._last_painted = None
                        self._paint_at(ev.pos)
                elif ev.button == 3:
                    self.panning = True
                elif ev.button in (4, 5):        # legacy wheel buttons
                    self.camera.zoom_at(
                        C.ZOOM_STEP if ev.button == 4
                        else 1.0 / C.ZOOM_STEP, *ev.pos)
            elif ev.type == pygame.MOUSEBUTTONUP:
                if ev.button == 1:
                    self.painting = False
                    self._last_painted = None
                elif ev.button == 3:
                    self.panning = False
            elif ev.type == pygame.MOUSEMOTION:
                self.mouse_pos = ev.pos
                if self.panning and ev.buttons[2]:
                    self.camera.pan(ev.rel[0], ev.rel[1])
                elif self.painting and ev.buttons[0]:
                    self._paint_at(ev.pos, drag=True)
            elif ev.type == pygame.TEXTINPUT and self.input_mode:
                self.input_text += ev.text
            elif ev.type == pygame.KEYDOWN:
                if not self._key(ev):
                    return False
        return True

    def _paint_at(self, pos, drag: bool = False) -> None:
        """Apply the tool to the tile under the cursor (once per drag)."""
        if self.input_mode is not None:
            return
        wx, wy = self.camera.screen_to_world(*pos)
        tile = self.scene.board.world_to_tile(wx, wy)
        if tile is None or (drag and tile == self._last_painted):
            return
        self._last_painted = tile
        self._apply_tool(tile)

    def _palette_click(self, pos) -> bool:
        """Select a palette tool under the cursor; True when hit."""
        for rect, tool in self.palette_rects:
            if rect.collidepoint(pos):
                self.tool = tool
                return True
        return False

    def _key(self, ev: pygame.event.Event) -> bool:
        """Keyboard handling; False ends the editor."""
        if self.input_mode is not None:
            if ev.key == pygame.K_RETURN:
                self._confirm_input()
            elif ev.key == pygame.K_ESCAPE:
                self.input_mode = None
            elif ev.key == pygame.K_BACKSPACE:
                self.input_text = self.input_text[:-1]
            return True

        mods = pygame.key.get_mods()
        if ev.key == pygame.K_ESCAPE:
            return False
        elif ev.key in (pygame.K_PLUS, pygame.K_EQUALS, pygame.K_KP_PLUS):
            self.camera.zoom_at(C.ZOOM_STEP, *self.mouse_pos)
        elif ev.key in (pygame.K_MINUS, pygame.K_KP_MINUS):
            self.camera.zoom_at(1.0 / C.ZOOM_STEP, *self.mouse_pos)
        elif ev.key == pygame.K_TAB:
            self.building_index = (self.building_index + 1) \
                % len(BUILDING_ORDER)
        elif ev.key == pygame.K_o and not mods & pygame.KMOD_CTRL:
            self.owner_index = (self.owner_index + 1) % len(OWNER_ORDER)
        elif ev.key == pygame.K_r:
            self.axis = (self.axis + 1) % 3
        elif ev.key == pygame.K_LEFTBRACKET:
            self.units = max(0, self.units - 5)
        elif ev.key == pygame.K_RIGHTBRACKET:
            self.units = min(255, self.units + 5)
        elif ev.key == pygame.K_n:
            self.input_mode, self.input_text = "new", ""
        elif ev.key == pygame.K_s and mods & pygame.KMOD_CTRL:
            self.input_mode, self.input_text = "save", ""
        elif ev.key in (pygame.K_l, pygame.K_o) \
                and mods & pygame.KMOD_CTRL:
            self.input_mode, self.input_text = "load", ""
        else:
            name = pygame.key.name(ev.key)
            if name in KEY_TOOLS:
                self.tool = KEY_TOOLS[name]
        return True

    def _confirm_input(self) -> None:
        """Finish a text input (save / load / new board)."""
        mode, text = self.input_mode, self.input_text.strip()
        self.input_mode = None
        if not text:
            return
        if mode == "save":
            self._save(text)
        elif mode == "load":
            self._load(text)
        elif mode == "new":
            try:
                cols, rows = (int(part) for part in
                              text.lower().split("x", 1))
                self._new_board(cols, rows)
                self._say(f"new {cols}x{rows} board")
            except ValueError:
                self._say("size must look like 24x15")

    # ------------------------------------------------------------------
    # Update / draw
    # ------------------------------------------------------------------
    def _update(self, dt: float) -> None:
        """Continuous pans and timers, with key panning like in the game."""
        self.mouse_pos = pygame.mouse.get_pos()
        speed = C.PAN_SPEED * dt
        dx = dy = 0
        keys = pygame.key.get_pressed()
        if keys[pygame.K_LEFT] or keys[pygame.K_a]:
            dx -= speed
        if keys[pygame.K_RIGHT] or keys[pygame.K_d]:
            dx += speed
        if keys[pygame.K_UP] or keys[pygame.K_w]:
            dy -= speed
        if keys[pygame.K_DOWN] or keys[pygame.K_s]:
            dy += speed
        if dx or dy:
            self.camera.pan(-dx, -dy)
        if self.status_timer > 0:
            self.status_timer -= dt

    def _draw(self) -> None:
        """One editor frame: board, palette, HUD and text input."""
        hover = self._hover_tile()
        self.renderer.draw_editor(self.scene, self.camera, hover)
        self._draw_palette()
        self._draw_hud()
        if self.input_mode is not None:
            self._draw_input()

    def _hover_tile(self):
        """Tile under the cursor or ``None``."""
        wx, wy = self.camera.screen_to_world(*self.mouse_pos)
        return self.scene.board.world_to_tile(wx, wy)

    def _draw_palette(self) -> None:
        """Clickable tool palette in the top-left corner."""
        self.palette_rects = []
        f = self.renderer.font(22)
        for i, (key, label, tool) in enumerate(TOOLS):
            surf = f.render(f"{key}  {label}", True, C.UI_TEXT_COLOR)
            rect = surf.get_rect(topleft=(10, 10 + i * 26)).inflate(10, 4)
            active = tool == self.tool
            pygame.draw.rect(self.screen,
                             (52, 58, 78) if active else (30, 32, 40),
                             rect, border_radius=6)
            if active:
                pygame.draw.rect(self.screen, (120, 140, 200), rect, 2,
                                 border_radius=6)
            self.screen.blit(surf, (rect.x + 5, rect.y + 2))
            self.palette_rects.append((rect, tool))

    def _draw_hud(self) -> None:
        """Current tool parameters and the status message."""
        f = self.renderer.font(22)
        w, _h = self.screen.get_size()
        kind = BUILDING_ORDER[self.building_index]
        owner = OWNER_ORDER[self.owner_index]
        lines = [
            f"building: {kind.value}   owner: "
            f"{OWNER_LABELS[owner]}   units: {self.units}",
            f"axis (R): {self.axis}   N: new   Ctrl+S: save   "
            f"Ctrl+L: load   Esc: quit",
        ]
        if self.status and self.status_timer > 0:
            lines.append(self.status)
        for i, line in enumerate(lines):
            surf = f.render(line, True, C.UI_TEXT_COLOR)
            self.screen.blit(surf, (w - surf.get_width() - 10, 10 + i * 24))

    def _draw_input(self) -> None:
        """Modal text input box (file name / board size)."""
        f = self.renderer.font(30)
        w, h = self.screen.get_size()
        prompts = {"save": "save as (maps/NAME.map):",
                   "load": "load (maps/NAME.map):",
                   "new": "new board size (COLSxROWS):"}
        prompt = f.render(prompts[self.input_mode], True, C.UI_TEXT_COLOR)
        text = f.render(self.input_text + "_", True, (255, 255, 120))
        box = pygame.Rect(0, 0, max(460, text.get_width() + 40), 96)
        box.center = (w // 2, h // 2)
        pygame.draw.rect(self.screen, (30, 32, 40), box, border_radius=8)
        pygame.draw.rect(self.screen, (120, 140, 200), box, 2,
                         border_radius=8)
        self.screen.blit(prompt, (box.x + 16, box.y + 12))
        self.screen.blit(text, (box.x + 16, box.y + 50))


def main() -> None:
    """Start the editor, optionally opening a map file from argv."""
    editor = Editor()
    if len(sys.argv) > 1:
        editor._load(sys.argv[1])
    editor.run()


if __name__ == "__main__":
    main()
