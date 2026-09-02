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

    def pan_projected(self, dx: float, dy: float) -> None:
        """Pan the view directly in projected space."""
        self.x += dx
        self.y += dy

    def zoom_at(self, factor: float, sx: float, sy: float) -> None:
        """Multiplicative zoom keeping the world point under the cursor."""
        wx, wy = self.screen_to_world(sx, sy)
        self.zoom = clamp(self.zoom * factor, C.ZOOM_MIN, C.ZOOM_MAX)
        px = (wx - wy) * C.ISO_COS
        py = (wx + wy) * C.ISO_SIN
        self.x = px - (sx - self.screen_size[0] / 2) / self.zoom
        self.y = py - (sy - self.screen_size[1] / 2) / self.zoom

    def center_on_world(self, wx: float, wy: float, wz: float = 0.0) -> None:
        """Center the view on a world point."""
        self.x = (wx - wy) * C.ISO_COS
        self.y = (wx + wy) * C.ISO_SIN - wz

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
