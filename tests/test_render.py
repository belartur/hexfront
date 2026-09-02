"""Rendering regression tests (headless, dummy video driver).

Run with:  python3 -m tests.test_render
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
os.environ.setdefault("SDL_VIDEODRIVER", "dummy")

import numpy as np                                          # noqa: E402
import pygame                                               # noqa: E402

from war_regions.app import Application                     # noqa: E402
from war_regions.constants import WATER_COLOR               # noqa: E402


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

    # The whole board panned far off-screen: every pixel must be water.
    app.camera.pan_projected(-50000, -50000)
    app.renderer.draw_world(app.game, app.camera)
    pygame.display.flip()
    frac = water_fraction(pygame.display.get_surface())
    assert frac > 0.999, f"artefacts outside the board after pan ({frac})"

    # Back on the board, zoom to both extremes: mixed view, no crash and
    # still no stale pixels from earlier frames.
    app.camera.center_on_world(*app.game.board.center_world((10, 6)))
    for zoom in (0.5, 1.0, 2.0, 0.5, 2.0):
        app.camera.zoom = zoom
        app.renderer.draw_world(app.game, app.camera)
        pygame.display.flip()
        frac = water_fraction(pygame.display.get_surface())
        assert 0.0 < frac < 1.0, "expected board + sea in the view"


if __name__ == "__main__":
    test_view_clears_on_pan_and_zoom()
    print("OK   test_view_clears_on_pan_and_zoom")
    print("\nAll render tests passed.")
