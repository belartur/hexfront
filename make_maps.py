#!/usr/bin/env python3
"""Regenerate the sample maps in ``maps/`` from the procedural generator.

Run with ``python3 make_maps.py``.  The menu of the game lists every
``.map`` file of the ``maps`` directory (specification: "Planszy i edytor
plansz"), so this script just materialises the levels from
:mod:`war_regions.levels` as map files; the editor can modify them and
save copies under new names.
"""

import os

from war_regions.levels import LEVELS, build_level
from war_regions.mapfile import save_map


def main() -> None:
    """Generate every configured level and save it into ``maps/``."""
    os.makedirs("maps", exist_ok=True)
    for cfg in LEVELS:
        game = build_level(cfg)
        path = os.path.join("maps", cfg.name + ".map")
        save_map(path, game.board, game.buildings)
        print(f"saved {path}")


if __name__ == "__main__":
    main()
