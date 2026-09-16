"""Depth-tested pygame primitives for the shared game/editor renderer.

Convex faces use pixel-centre coverage; pygame provides batched hex coverage
and strokes. Surface-bound strokes sample their supporting depth plane.
NumPy evaluates affine depth inside clipped rectangles. Larger depth wins.
"""

import numpy as np
import pygame

from . import constants as C
from .camera import Camera


class ProjectedPoint(tuple):
    """Integer pygame coordinates with unrounded projection and ray depth."""

    def __new__(cls, sx: float, sy: float, depth: float):
        point = super().__new__(cls, (round(sx), round(sy)))
        point.projected = (sx, sy)
        point.depth = depth
        return point


class DepthCamera:
    """Projection adapter retaining depth without changing the UI camera."""

    def __init__(self, camera: Camera):
        self.camera = camera

    def __getattr__(self, name: str):
        return getattr(self.camera, name)

    def world_to_screen(self, x: float, y: float, z: float = 0.0) -> tuple:
        """Project a world vertex and retain its affine ray coordinate."""
        c = self.camera
        point = ProjectedPoint(
            ((x - y) * C.ISO_COS - c.x) * c.zoom + c.screen_size[0] / 2,
            ((x + y) * C.ISO_SIN - z - c.y) * c.zoom + c.screen_size[1] / 2,
            (x + y) * C.ISO_SIN + z)
        # Horizontal details belong to this plane across their entire width.
        point.ground_plane = (1 / c.zoom,
                              c.y - c.screen_size[1] / (2 * c.zoom) + 2 * z)
        return point

    def screen_circle_poly(self, cx: float, cy: float, radius: float,
                           wz: float = 0.0, n: int = 40) -> list:
        """Retain depth on every vertex of a projected ground circle."""
        return Camera.screen_circle_poly(self, cx, cy, radius, wz, n)


class DepthBuffer:
    """Frame-local colour/depth target; coplanar details win exact ties."""

    def __init__(self, screen: pygame.Surface):
        self.screen = screen
        self.values = np.full(screen.get_size(), -np.inf)
        self.mask = pygame.Surface(screen.get_size(), depth=8)

    def _region(self, points: list, width: int) -> pygame.Rect:
        """Clip raster work to the primitive, including thick-line coverage."""
        xs, ys = zip(*points)
        return pygame.Rect(min(xs) - width, min(ys) - width,
                           max(xs) - min(xs) + 2 * width + 1,
                           max(ys) - min(ys) + 2 * width + 1).clip(
                               self.screen.get_rect())

    def _paint(self, rect: pygame.Rect, color: tuple, depths,
               coverage=None) -> None:
        """Commit covered pixels that are not behind an existing surface."""
        x, y, w, h = rect
        if coverage is None:
            coverage = pygame.surfarray.array2d(self.mask.subsurface(rect)) != 0
        old = self.values[x:x + w, y:y + h]
        visible = coverage & (depths >= old - C.DEPTH_EPSILON)
        if visible.any():
            pixels = pygame.surfarray.pixels2d(self.screen)
            pixels[x:x + w, y:y + h][visible] = self.screen.map_rgb(color[:3])
            old[visible] = np.broadcast_to(depths, old.shape)[visible]
            del pixels

    def horizontal_faces(self, faces: list, fill: tuple, edge: tuple,
                         width: int) -> None:
        """Batch coplanar hex tops and outlines in two native pygame passes.

        Every covered pixel uses the SAME plane equation, including the
        outline's width. No per-edge NumPy arrays or depth biases are needed.
        """
        if not faces:
            return
        slope, intercept = faces[0][0].ground_plane
        rect = self.screen.get_clip()
        depths = (np.arange(rect.top, rect.bottom)[None, :] + 0.5) * slope + intercept
        for color, stroke in ((fill, 0), (edge, width)):
            self.mask.fill(0)
            for points in faces:
                pygame.draw.polygon(self.mask, 1, points, stroke)
            self._paint(rect, color, depths)

    def polygon(self, color: tuple, points: list, width: int = 0) -> None:
        """Draw a planar convex polygon or its depth-tested outline."""
        rect = self._region(points, width).clip(self.screen.get_clip())
        if not rect:
            return
        origin = points[0]
        # Find the best-conditioned triangle, including edge-on faces.
        candidates = []
        for a, b in zip(points[1:], points[2:]):
            ax = a.projected[0] - origin.projected[0]
            ay = a.projected[1] - origin.projected[1]
            bx = b.projected[0] - origin.projected[0]
            by = b.projected[1] - origin.projected[1]
            candidates.append((abs(ax * by - ay * bx), ax, ay, bx, by, a, b))
        area, ax, ay, bx, by, a, b = max(candidates, key=lambda v: v[0])
        if area <= C.DEPTH_EPSILON:
            if width:
                for a, b in zip(points, points[1:] + points[:1]):
                    self.line(color, a, b, width)
            return
        det = ax * by - ay * bx
        da, db = a.depth - origin.depth, b.depth - origin.depth
        dx, dy = (da * by - db * ay) / det, (ax * db - bx * da) / det
        xs = np.arange(rect.left, rect.right)[:, None] + 0.5
        ys = np.arange(rect.top, rect.bottom)[None, :] + 0.5
        depths = (origin.depth + dx * (xs - origin.projected[0])
                  + dy * (ys - origin.projected[1]))
        if width:
            self.mask.fill(0, rect)
            pygame.draw.polygon(self.mask, 1, points, width)
            self._paint(rect, color, depths)
            return
        # Sample the actual projected polygon, not its rounded pygame
        # boundary: extrapolation outside a cliff creates false occluders.
        positive = np.ones((rect.w, rect.h), dtype=bool)
        negative = positive.copy()
        for a, b in zip(points, points[1:] + points[:1]):
            ax, ay = a.projected
            bx, by = b.projected
            cross = (bx - ax) * (ys - ay) - (by - ay) * (xs - ax)
            positive &= cross >= -C.DEPTH_EPSILON
            negative &= cross <= C.DEPTH_EPSILON
        self._paint(rect, color, depths, positive | negative)

    def line(self, color: tuple, a: ProjectedPoint, b: ProjectedPoint,
             width: int = 1) -> None:
        """Draw a line with depth interpolated along its projected segment."""
        rect = self._region([a, b], width).clip(self.screen.get_clip())
        if not rect:
            return
        ax, ay = a.projected
        dx, dy = b.projected[0] - ax, b.projected[1] - ay
        length_sq = dx * dx + dy * dy
        xs = np.arange(rect.left, rect.right)[:, None] + 0.5 - ax
        ys = np.arange(rect.top, rect.bottom)[None, :] + 0.5 - ay
        fraction = (np.clip((xs * dx + ys * dy) / length_sq, 0, 1)
                    if length_sq > C.DEPTH_EPSILON else 0.0)
        depths = a.depth + fraction * (b.depth - a.depth)
        plane = getattr(a, "ground_plane", None)
        if plane is not None and plane == getattr(b, "ground_plane", None):
            slope, intercept = plane
            depths = (ys + ay) * slope + intercept
        self.mask.fill(0, rect)
        pygame.draw.line(self.mask, 1, a, b, width)
        self._paint(rect, color, depths)
