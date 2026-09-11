"""Application layer: window, menu, level loading, input and HUD.

Implements the controls from the specification:
  * view: LMB drag, arrow keys, WASD, screen-edge hover, wheel/+/- zoom
    (0.5x - 2x);
  * selecting a source building: RMB always (re)selects the clicked own
    building with units inside; RMB anywhere else cancels;
  * sending a vehicle: LMB on own building selects it when nothing is
    selected; with an active selection LMB on any other building sends
    a vehicle there (own buildings included, i.e. unit transfers).
    The route preview follows the cursor; Esc cancels the selection;
  * Esc returns to the menu unless a building is selected;
  * P pauses the game.
"""

import math
import os

import pygame

from . import constants as C
from .camera import Camera
from .entities import vehicle_kind_of
from .mapfile import list_maps, load_game, level_seed
from .ai import AIController, AI_DIFFICULTIES
from .render import Renderer

STATE_MENU, STATE_LOADING, STATE_PLAYING = range(3)


class Application:
    """Owns the pygame window and drives the whole program."""

    def __init__(self, size=(1180, 720)):
        pygame.init()
        self.screen = pygame.display.set_mode(size, pygame.RESIZABLE)
        pygame.display.set_caption("War Regions")
        self.clock = pygame.time.Clock()
        self.renderer = Renderer(self.screen)
        self.state = STATE_MENU
        self.running = True
        self.menu_rects = []          # [(rect, map file path), ...]
        self.maps = list_maps()       # levels shown in the menu
        self.menu_scroll = 0          # vertical scroll of the menu grid (px)
        self.mouse_pos = (0, 0)
        self.game = None
        self.camera = None
        self.ai = []
        self.paused = False
        self.selection = None         # {"src": tile, "path": preview}
        self._preview_tile = None     # hover tile the preview was built for
        self.load_timer = 0.0
        self._sim_acc = 0.0
        self._down_pos = None
        self._dragging = False

    # ------------------------------------------------------------------
    def run(self) -> None:
        """Main loop; exits when the window is closed."""
        while self.running:
            dt = min(self.clock.tick(C.FPS) / 1000.0, 0.1)
            self._handle_events()
            self._update(dt)
            self._draw()
            pygame.display.flip()
        pygame.quit()

    # ------------------------------------------------------------------
    # Events
    # ------------------------------------------------------------------
    def _handle_events(self) -> None:
        for ev in pygame.event.get():
            if ev.type == pygame.QUIT:
                self.running = False
            elif ev.type == pygame.VIDEORESIZE:
                self.screen = pygame.display.set_mode(ev.size,
                                                      pygame.RESIZABLE)
                self.renderer.screen = self.screen
                if self.camera is not None:
                    self.camera.screen_size = tuple(ev.size)
            elif ev.type == pygame.MOUSEMOTION:
                self.mouse_pos = ev.pos
                if (self.state == STATE_PLAYING and ev.buttons[0]
                        and self._down_pos is not None
                        and self._dragging_or_far(ev.pos)):
                    self._dragging = True
                    self.camera.pan(ev.rel[0], ev.rel[1])
            elif ev.type == pygame.MOUSEBUTTONDOWN:
                if ev.button == 1:
                    self._down_pos = ev.pos
                    self._dragging = False
                elif self.state == STATE_MENU and ev.button in (4, 5):
                    self._scroll_menu(C.MENU_SCROLL_STEP
                                      if ev.button == 5
                                      else -C.MENU_SCROLL_STEP)
                elif ev.button == 3 and self.state == STATE_PLAYING:
                    self._select_rmb(ev.pos)       # RMB always selects
                elif ev.button in (4, 5) and self.camera is not None:
                    self.camera.zoom_at(
                        C.ZOOM_STEP if ev.button == 4 else 1.0 / C.ZOOM_STEP,
                        *ev.pos)
            elif ev.type == pygame.MOUSEWHEEL:
                if self.state == STATE_MENU:
                    self._scroll_menu(-ev.y * C.MENU_SCROLL_STEP)
                elif self.camera is not None:
                    self.camera.zoom_at(
                        C.ZOOM_STEP if ev.y > 0 else 1.0 / C.ZOOM_STEP,
                        *self.mouse_pos)
            elif ev.type == pygame.MOUSEBUTTONUP and ev.button == 1:
                down, self._down_pos = self._down_pos, None
                if down is not None and not self._dragging:
                    self._click(ev.pos)
            elif ev.type == pygame.KEYDOWN:
                self._key(ev)

    def _dragging_or_far(self, pos) -> bool:
        """True once the held mouse moved beyond the click threshold."""
        if self._dragging:
            return True
        dx = pos[0] - self._down_pos[0]
        dy = pos[1] - self._down_pos[1]
        return math.hypot(dx, dy) > C.DRAG_THRESHOLD

    def _key(self, ev: pygame.event.Event) -> None:
        if self.state == STATE_PLAYING:
            if ev.key == pygame.K_ESCAPE:
                if self.selection is not None:
                    self._set_selection(None)  # first cancel the selection
                else:
                    self._enter_menu()
            elif ev.key == pygame.K_p:
                self.paused = not self.paused
            elif ev.key in (pygame.K_PLUS, pygame.K_EQUALS,
                            pygame.K_KP_PLUS):
                self.camera.zoom_at(C.ZOOM_STEP, *self.mouse_pos)
            elif ev.key in (pygame.K_MINUS, pygame.K_KP_MINUS):
                self.camera.zoom_at(1.0 / C.ZOOM_STEP, *self.mouse_pos)
        elif self.state == STATE_MENU and ev.key == pygame.K_ESCAPE:
            self.running = False
        elif self.state == STATE_MENU and ev.key in (pygame.K_UP,
                                                     pygame.K_PAGEUP):
            self._scroll_menu(-C.MENU_SCROLL_STEP)
        elif self.state == STATE_MENU and ev.key in (pygame.K_DOWN,
                                                     pygame.K_PAGEDOWN):
            self._scroll_menu(C.MENU_SCROLL_STEP)

    def _enter_menu(self) -> None:
        """Return to the level menu, refreshing the map file list."""
        self.state = STATE_MENU
        self.maps = list_maps()
        self.menu_scroll = 0

    def _menu_grid_metrics(self) -> dict:
        """Geometry of the three-column menu grid for the current window.

        Returns column count/width, row stride, visible area and the
        maximum scroll offset, so scrolling and drawing stay consistent.
        """
        w, h = self.screen.get_size()
        n = len(self.maps)
        cols = min(C.MENU_COLUMNS, n) if n else 1
        cols = max(1, cols)
        avail_w = max(1, w - 2 * C.MENU_SIDE_MARGIN)
        col_w = max(1, avail_w // cols)
        font = self.renderer.font(C.MENU_FONT_SIZE)
        cell_h = font.get_linesize() + 2 * C.MENU_CELL_PAD_Y
        stride = cell_h + C.MENU_ROW_GAP
        rows = (n + cols - 1) // cols if n else 0
        content_h = rows * stride - (C.MENU_ROW_GAP if rows else 0)
        grid_top = int(h * C.MENU_GRID_TOP_FRACTION)
        grid_bottom = h - C.MENU_GRID_BOTTOM_MARGIN
        visible_h = max(1, grid_bottom - grid_top)
        max_scroll = max(0, content_h - visible_h)
        return {"cols": cols, "col_w": col_w, "cell_h": cell_h,
                "stride": stride, "rows": rows, "content_h": content_h,
                "grid_top": grid_top, "grid_bottom": grid_bottom,
                "visible_h": visible_h, "max_scroll": max_scroll}

    def _scroll_menu(self, delta: int) -> None:
        """Scroll the menu grid by ``delta`` pixels, clamped to content."""
        m = self._menu_grid_metrics()
        self.menu_scroll = max(0, min(m["max_scroll"],
                                      self.menu_scroll + delta))

    def _fit_menu_text(self, font: pygame.font.Font, text: str,
                       max_w: int) -> pygame.Surface:
        """Render ``text`` truncated with an ellipsis to fit ``max_w``."""
        if font.size(text)[0] <= max_w:
            return font.render(text, True, C.UI_TEXT_COLOR)
        ellipsis = "\u2026"
        lo, hi = 0, len(text)
        while lo < hi:
            mid = (lo + hi + 1) // 2
            if font.size(text[:mid] + ellipsis)[0] <= max_w:
                lo = mid
            else:
                hi = mid - 1
        return font.render(text[:lo] + ellipsis, True, C.UI_TEXT_COLOR)

    def _click(self, pos) -> None:
        if self.state == STATE_MENU:
            for rect, map_path in self.menu_rects:
                if rect.collidepoint(pos):
                    self._start_map(map_path)
                    return
        elif self.state == STATE_PLAYING:
            self._game_click(pos)

    def _game_click(self, pos) -> None:
        """LMB click during play: select when idle, otherwise send (spec).

        With no building selected, LMB selects an own building with units
        inside.  With an active selection, LMB on any *other* building
        sends a vehicle from the selected building to the clicked one --
        own buildings included, which enables unit transfers.  A failed
        send (no route) keeps the selection.
        """
        tile = self._pick_tile(pos)
        building = (self.game.building_at_tile(tile)
                    if tile is not None else None)
        human = self.game.human_id
        sel = self.selection
        if sel is None:
            if (building is not None and building.owner == human
                    and building.units > 0):
                self._set_selection(building.tile)
        elif building is not None and building.tile != sel["src"]:
            # LMB with an active selection always means "send there".
            if self.game.try_send(human, sel["src"], building.tile):
                self._set_selection(None)

    def _select_rmb(self, pos) -> None:
        """RMB click during play: always (re)selects the clicked own
        building with units inside; anywhere else it cancels (spec)."""
        tile = self._pick_tile(pos)
        building = (self.game.building_at_tile(tile)
                    if tile is not None else None)
        human = self.game.human_id
        if (building is not None and building.owner == human
                and building.units > 0):
            self._set_selection(building.tile)
        else:
            self._set_selection(None)

    def _set_selection(self, src_tile) -> None:
        """Select ``src_tile`` as the sending source; ``None`` cancels."""
        self._preview_tile = None
        self.selection = ({"src": src_tile, "path": None}
                          if src_tile is not None else None)

    def _update_preview(self, hover_tile) -> None:
        """Refresh the dashed route preview for the selected source.

        The preview targets the building under the cursor and is
        recomputed only when the hovered tile changes (path-finding is
        not free).
        """
        sel = self.selection
        if sel is None or self.game is None:
            self._preview_tile = None
            return
        if hover_tile == self._preview_tile:
            return                          # preview still valid
        self._preview_tile = hover_tile
        sel["path"] = None
        if hover_tile is not None and hover_tile != sel["src"]:
            if self.game.building_at_tile(hover_tile) is not None:
                sel["path"] = self._route_preview(sel["src"], hover_tile)

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------
    def _route_preview(self, src_tile, dst_tile):
        """Route the selected send would take, or None when impossible."""
        src = self.game.building_at_tile(src_tile)
        if src is None:
            return None
        kind = vehicle_kind_of(src.kind)
        return self.game.board.find_path(src_tile, dst_tile, kind)

    def _pick_tile(self, pos):
        """Tile under the cursor, refined against terrain elevation.

        With the alt key held the tile is picked as if every field stood
        at height zero (specification: "Lecz gdy jest przyciśnięty
        klawisz alt...").  Delegates to the shared :meth:`Board.pick_tile`,
        so the game and the editor point at tiles with exactly the same
        code.
        """
        if self.game is None or self.camera is None:
            return None
        keys = pygame.key.get_pressed()
        flat = keys[pygame.K_LALT] or keys[pygame.K_RALT]
        return self.game.board.pick_tile(self.camera, pos, flat=flat)

    def _start_map(self, map_path: str) -> None:
        """Load a map file and show it (loading state, spec).

        The level name shown in the menu equals the map file name; the
        players derive from the building owners in the file and the AI
        plays with the default difficulty from the constants module.
        """
        self.game = load_game(map_path)
        self.camera = Camera(self.screen.get_size())
        mid = (self.game.board.cols // 2, self.game.board.rows // 2)
        self.camera.limit_to_board(self.game.board)
        self.camera.center_on_world(*self.game.board.center_world(mid))
        seed = level_seed(map_path)
        difficulty = AI_DIFFICULTIES[C.MAP_DEFAULT_AI_DIFFICULTY]
        self.ai = [AIController(self.game, p.id, difficulty,
                                seed * 31 + p.id)
                   for p in self.game.players if not p.is_human]
        self.paused = False
        self._set_selection(None)
        self._sim_acc = 0.0
        self.load_timer = C.LOADING_TIME
        self.state = STATE_LOADING

    # ------------------------------------------------------------------
    # Update
    # ------------------------------------------------------------------
    def _update(self, dt: float) -> None:
        if self.state == STATE_LOADING:
            self.load_timer -= dt
            if self.load_timer <= 0:
                self.state = STATE_PLAYING
        elif self.state == STATE_PLAYING:
            self._pan_keys(dt)
            if not self.paused and not self.game.over:
                for ai in self.ai:
                    ai.update(dt)
                self._sim_acc += dt
                while self._sim_acc >= C.SIM_DT:
                    self.game.update(C.SIM_DT)
                    self._sim_acc -= C.SIM_DT

    def _pan_keys(self, dt: float) -> None:
        """Arrow keys, WASD and screen-edge panning."""
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
            self.camera.pan(-dx, -dy)   # keys move the *view*

    # ------------------------------------------------------------------
    # Drawing
    # ------------------------------------------------------------------
    def _draw(self) -> None:
        if self.state == STATE_MENU:
            self._draw_menu()
            return
        hover = self._pick_tile(self.mouse_pos) \
            if self.state == STATE_PLAYING else None
        self._update_preview(hover)
        self.renderer.draw_world(self.game, self.camera, self.selection,
                                 hover)
        if self.state == STATE_LOADING:
            self._banner("Loading...")
        elif self.paused and not self.game.over:
            self._banner("PAUSED  (P resumes)")
        elif self.game.over:
            self._banner("VICTORY!" if self.game.winner == "human"
                         else "DEFEAT")
        self._hud()

    def _banner(self, text: str) -> None:
        """Large centred banner (loading / pause / game result)."""
        surf = self.renderer.font(64).render(text, True, C.UI_TEXT_COLOR)
        pos = (self.screen.get_width() // 2 - surf.get_width() // 2,
               self.screen.get_height() // 2 - surf.get_height() // 2)
        pygame.draw.rect(self.screen, (20, 20, 26, 120),
                         surf.get_rect().inflate(40, 24).move(pos))
        self.screen.blit(surf, pos)

    def _hud(self) -> None:
        """Small control hints in the top-left corner."""
        lines = ["RMB: select building   LMB: select / send units   "
                 "Esc: cancel",
                 "Drag/WASD/arrows/edge: pan   wheel/+/-: zoom   P: pause   "
                 "Alt: flat pick   Esc: menu"]
        for i, line in enumerate(lines):
            surf = self.renderer.font(18).render(line, True,
                                                 C.UI_TEXT_COLOR)
            self.screen.blit(surf, (10, 8 + i * 20))

    def _draw_menu(self) -> None:
        self.screen.fill(C.UI_BACKGROUND)
        w, h = self.screen.get_size()
        title_size = 64 if h < 620 or w < 640 else 72
        title = self.renderer.font(title_size).render("WAR REGIONS", True,
                                                      C.UI_TEXT_COLOR)
        self.screen.blit(title, (w // 2 - title.get_width() // 2,
                                 int(h * 0.08)))
        sub = self.renderer.font(24).render("choose a level", True,
                                            (150, 155, 170))
        self.screen.blit(sub, (w // 2 - sub.get_width() // 2,
                               int(h * 0.08) + title.get_height() + 4))
        m = self._menu_grid_metrics()
        self.menu_scroll = max(0, min(m["max_scroll"], self.menu_scroll))
        scroll = self.menu_scroll
        cols, col_w = m["cols"], m["col_w"]
        font = self.renderer.font(C.MENU_FONT_SIZE)
        max_text_w = max(1, col_w - 2 * C.MENU_CELL_PAD_X - 12)
        mouse = pygame.mouse.get_pos()
        self.menu_rects = []
        for i, map_path in enumerate(self.maps):
            col, row = i % cols, i // cols
            cx = C.MENU_SIDE_MARGIN + col * col_w + col_w // 2
            cy = m["grid_top"] + row * m["stride"] + m["cell_h"] // 2 \
                - scroll
            text = os.path.splitext(os.path.basename(map_path))[0]
            surf = self._fit_menu_text(font, text, max_text_w)
            rect = pygame.Rect(0, 0,
                               surf.get_width() + 2 * C.MENU_CELL_PAD_X,
                               m["cell_h"])
            rect.center = (cx, cy)
            self.menu_rects.append((rect, map_path))
            if cy + m["cell_h"] // 2 < m["grid_top"] \
                    or cy - m["cell_h"] // 2 > m["grid_bottom"]:
                continue
            if rect.collidepoint(mouse):
                pygame.draw.rect(self.screen, (52, 58, 78), rect,
                                 border_radius=8)
                pygame.draw.rect(self.screen, (120, 140, 200), rect, 2,
                                 border_radius=8)
            self.screen.blit(surf, (rect.centerx - surf.get_width() // 2,
                                    rect.centery - surf.get_height() // 2))
        if m["max_scroll"] > 0:
            bar_h = max(24, int(m["visible_h"] * m["visible_h"]
                                / max(1, m["content_h"])))
            bar_y = m["grid_top"] + int(
                (m["visible_h"] - bar_h) * scroll / m["max_scroll"])
            pygame.draw.rect(self.screen, (52, 58, 78),
                             pygame.Rect(w - 14, m["grid_top"], 6,
                                         m["visible_h"]),
                             border_radius=3)
            pygame.draw.rect(self.screen, (120, 140, 200),
                             pygame.Rect(w - 14, bar_y, 6, bar_h),
                             border_radius=3)
            hint_text = "click a level to play - wheel/Up/Down scrolls"
        else:
            hint_text = "click a level to play"
        hint = self.renderer.font(20).render(hint_text, True,
                                             (120, 125, 140))
        self.screen.blit(hint, (w // 2 - hint.get_width() // 2, h - 40))
