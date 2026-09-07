"""Camera and the isometric world<->screen projection.

The projection maps world (x, y, z) to the screen::

    px = (x - y) * ISO_COS
    py = (x + y) * ISO_SIN - z

where z is the rendered elevation (tile height * ELEVATION_PX).  The
camera stores a pan offset in projected space and a zoom factor; zooming
affects rendering only, never simulation distances.
"""

import math

from . import constants as C
from .hexgrid import SQRT3


def clamp(value: float, low: float, high: float) -> float:
    """Clamp ``value`` into [low, high]."""
    return max(low, min(high, value))


class Camera:
    """View position and zoom of the isometric viewport."""

    def __init__(self, screen_size: tuple):
        self.x = 0.0            # pan offset in projected space
        self.y = 0.0
        self.zoom = 1.0
        self.screen_size = tuple(screen_size)
        #: World-space bounds ``(x_min, x_max, y_min, y_max)`` the view
        #: centre may not leave, or ``None`` for unlimited panning
        #: (set by :meth:`limit_to_board`).
        self.bounds = None

    # ------------------------------------------------------------------
    def world_to_screen(self, wx: float, wy: float, wz: float = 0.0) -> tuple:
        """Project a world point to integer screen coordinates."""
        px = (wx - wy) * C.ISO_COS
        py = (wx + wy) * C.ISO_SIN - wz
        return (round((px - self.x) * self.zoom + self.screen_size[0] / 2),
                round((py - self.y) * self.zoom + self.screen_size[1] / 2))

    def screen_to_world(self, sx: float, sy: float, wz: float = 0.0) -> tuple:
        """Inverse of :meth:`world_to_screen` for a known elevation ``wz``."""
        px = (sx - self.screen_size[0] / 2) / self.zoom + self.x
        py = (sy - self.screen_size[1] / 2) / self.zoom + self.y + wz
        wx = (px / C.ISO_COS + py / C.ISO_SIN) / 2
        wy = (py / C.ISO_SIN - px / C.ISO_COS) / 2
        return (wx, wy)

    def pan(self, dx: float, dy: float) -> None:
        """Pan the view by a screen-space delta (already zoom-corrected)."""
        self.x -= dx / self.zoom
        self.y -= dy / self.zoom
        self._clamp_to_bounds()

    def pan_projected(self, dx: float, dy: float) -> None:
        """Pan the view directly in projected space."""
        self.x += dx
        self.y += dy
        self._clamp_to_bounds()

    def zoom_at(self, factor: float, sx: float, sy: float) -> None:
        """Multiplicative zoom keeping the world point under the cursor."""
        wx, wy = self.screen_to_world(sx, sy)
        self.zoom = clamp(self.zoom * factor, C.ZOOM_MIN, C.ZOOM_MAX)
        px = (wx - wy) * C.ISO_COS
        py = (wx + wy) * C.ISO_SIN
        self.x = px - (sx - self.screen_size[0] / 2) / self.zoom
        self.y = py - (sy - self.screen_size[1] / 2) / self.zoom
        self._clamp_to_bounds()

    def center_on_world(self, wx: float, wy: float, wz: float = 0.0) -> None:
        """Center the view on a world point."""
        self.x = (wx - wy) * C.ISO_COS
        self.y = (wx + wy) * C.ISO_SIN - wz
        self._clamp_to_bounds()

    # ------------------------------------------------------------------
    # Panning limits (specification: panning stays within the board, with
    # the farthest board tile allowed to reach the screen centre)
    # ------------------------------------------------------------------
    def limit_to_board(self, board) -> None:
        """Constrain the view centre to the area of ``board``.

        The screen centre is the world point projected at elevation 0,
        so panning is limited by clamping that world point into the
        bounding box of the board's tile centres: at the farthest pan an
        extreme board tile sits at the centre of the screen
        (specification: "Przesuwanie jest ograniczone do granic
        planszy").  Stored as ``(x_min, x_max, y_min, y_max)``.
        """
        x_max = 1.5 * board.side * (board.cols - 1)
        y_max = (SQRT3 * board.side
                 * (board.rows - 1 + 0.5 * ((board.cols - 1) & 1)))
        self.bounds = (0.0, x_max, 0.0, y_max)

    def _clamp_to_bounds(self) -> None:
        """Keep the view centre inside :attr:`bounds`, if they are set.

        The pan offset lives in projected space, so the constraint is
        applied to the world point it corresponds to (the inverse of the
        projection at elevation 0).
        """
        if self.bounds is None:
            return
        x_min, x_max, y_min, y_max = self.bounds
        wx = (self.x / C.ISO_COS + self.y / C.ISO_SIN) / 2.0
        wy = (self.y / C.ISO_SIN - self.x / C.ISO_COS) / 2.0
        wx = clamp(wx, x_min, x_max)
        wy = clamp(wy, y_min, y_max)
        self.x = (wx - wy) * C.ISO_COS
        self.y = (wx + wy) * C.ISO_SIN

    # ------------------------------------------------------------------
    def screen_circle_poly(self, cx: float, cy: float, radius: float,
                           wz: float = 0.0, n: int = 40) -> list:
        """Project a ground-plane circle as a polygon (an iso ellipse)."""
        pts = []
        for i in range(n):
            a = 2.0 * math.pi * i / n
            pts.append(self.world_to_screen(cx + radius * math.cos(a),
                                            cy + radius * math.sin(a), wz))
        return pts
