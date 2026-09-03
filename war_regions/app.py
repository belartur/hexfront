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

import pygame

from . import constants as C
from .camera import Camera
from .entities import vehicle_kind_of
from .levels import LEVELS, build_level
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
        self.menu_rects = []          # [(rect, LevelConfig), ...]
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
                elif ev.button == 3 and self.state == STATE_PLAYING:
                    self._select_rmb(ev.pos)       # RMB always selects
                elif ev.button in (4, 5) and self.camera is not None:
                    self.camera.zoom_at(
                        C.ZOOM_STEP if ev.button == 4 else 1.0 / C.ZOOM_STEP,
                        *ev.pos)
            elif ev.type == pygame.MOUSEWHEEL:
                if self.camera is not None:
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
                    self.state = STATE_MENU
            elif ev.key == pygame.K_p:
                self.paused = not self.paused
            elif ev.key in (pygame.K_PLUS, pygame.K_EQUALS,
                            pygame.K_KP_PLUS):
                self.camera.zoom_at(C.ZOOM_STEP, *self.mouse_pos)
            elif ev.key in (pygame.K_MINUS, pygame.K_KP_MINUS):
                self.camera.zoom_at(1.0 / C.ZOOM_STEP, *self.mouse_pos)
        elif self.state == STATE_MENU and ev.key == pygame.K_ESCAPE:
            self.running = False

    def _click(self, pos) -> None:
        if self.state == STATE_MENU:
            for rect, level in self.menu_rects:
                if rect.collidepoint(pos):
                    self._start_level(level)
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
        """Tile under the cursor, refined against terrain elevation."""
        if self.game is None or self.camera is None:
            return None
        wx, wy = self.camera.screen_to_world(*pos)
        tile = self.game.board.world_to_tile(wx, wy)
        for _ in range(3):
            if tile is None:
                break
            wz = self.game.board.height(tile) * C.ELEVATION_PX
            wx, wy = self.camera.screen_to_world(pos[0], pos[1], wz)
            refined = self.game.board.world_to_tile(wx, wy)
            if refined == tile:
                break
            tile = refined
        return tile

    def _start_level(self, level) -> None:
        """Build the level and show the map (loading state, spec)."""
        self.game = build_level(level)
        self.camera = Camera(self.screen.get_size())
        mid = (level.size[0] // 2, level.size[1] // 2)
        self.camera.center_on_world(*self.game.board.center_world(mid))
        self.ai = [AIController(self.game, p.id,
                                AI_DIFFICULTIES[level.ai_difficulty],
                                level.seed * 31 + p.id)
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
                 "Esc: menu"]
        for i, line in enumerate(lines):
            surf = self.renderer.font(18).render(line, True,
                                                 C.UI_TEXT_COLOR)
            self.screen.blit(surf, (10, 8 + i * 20))

    def _draw_menu(self) -> None:
        self.screen.fill(C.UI_BACKGROUND)
        w, h = self.screen.get_size()
        title = self.renderer.font(72).render("WAR REGIONS", True,
                                              C.UI_TEXT_COLOR)
        self.screen.blit(title, (w // 2 - title.get_width() // 2,
                                 int(h * 0.14)))
        sub = self.renderer.font(26).render("choose a level", True,
                                            (150, 155, 170))
        self.screen.blit(sub, (w // 2 - sub.get_width() // 2,
                               int(h * 0.14) + title.get_height() + 6))
        self.menu_rects = []
        y = int(h * 0.36)
        mouse = pygame.mouse.get_pos()
        for level in LEVELS:
            text = f"{level.name}   ({level.players} players, " \
                   f"{level.ai_difficulty} AI)"
            surf = self.renderer.font(40).render(text, True,
                                                 C.UI_TEXT_COLOR)
            rect = surf.get_rect(center=(w // 2, y))
            rect = rect.inflate(48, 20)
            if rect.collidepoint(mouse):
                pygame.draw.rect(self.screen, (52, 58, 78), rect,
                                 border_radius=8)
                pygame.draw.rect(self.screen, (120, 140, 200), rect, 2,
                                 border_radius=8)
            self.screen.blit(surf, (rect.centerx - surf.get_width() // 2,
                                    rect.centery - surf.get_height() // 2))
            self.menu_rects.append((rect, level))
            y += int(h * 0.13)
        hint = self.renderer.font(20).render(
            "click a level to play - LMB select, Esc menu, P pause", True,
            (120, 125, 140))
        self.screen.blit(hint, (w // 2 - hint.get_width() // 2, h - 50))
