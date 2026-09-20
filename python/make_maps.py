#!/usr/bin/env python3
"""Regenerate the sample maps in the ``maps/`` directory of the repository.

Run with ``python3 python/make_maps.py`` (from anywhere - the ``maps``
directory lives in the repository root, see ``python/hexfront/constants.py``).
The menu of the game lists every ``.map`` file of that directory
(specification: "Planszy i edytor plansz"), so this script just
materialises the levels from :mod:`hexfront.levels` as map files; the
editor can modify them and save copies under new names.
"""

import os

from hexfront import constants as C
from hexfront.levels import LEVELS, build_level
from hexfront.mapfile import save_map


def main() -> None:
    """Generate every configured level and save it into ``maps/``."""
    os.makedirs(C.MAPS_DIR, exist_ok=True)
    for cfg in LEVELS:
        game = build_level(cfg)
        path = os.path.join(C.MAPS_DIR, cfg.name + C.MAP_EXTENSION)
        save_map(path, game.board, game.buildings)
        print(f"saved {path}")


if __name__ == "__main__":
    main()
