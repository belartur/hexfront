"""Level definitions and procedural board generation.

Every level is generated from code (specification: "Generowanie plansz")
from a fixed seed, so each named level is always the same map.  Generation
guarantees a playable board: all buildings are tank-reachable from every
player's base; otherwise the map is regenerated with a derived seed.
"""

import random
from dataclasses import dataclass

from . import hexgrid
from .board import Board, Obstacle
from .constants import VehicleKind
from .entities import Building, BuildingKind, Player
from .game import Game


@dataclass
class LevelConfig:
    """Static description of one menu level."""

    name: str            # name displayed in the menu
    size: tuple          # board size in hexes (cols, rows)
    seed: int            # base randomness seed (rules sec. 13.2)
    players: int         # total players, 2..4 (incl. the human)
    ai_difficulty: str   # key into ai.AI_DIFFICULTIES
    plateaus: int        # number of elevated blobs
    lakes: int           # number of water blobs
    neutral: dict        # BuildingKind -> count of neutral buildings
    obstacles: dict      # Obstacle kind -> count


#: Neutral building sets shared by the levels.
_NEUTRAL_STANDARD = {
    BuildingKind.TURRET_NORMAL: 2,
    BuildingKind.TURRET_ROCKET: 1,
    BuildingKind.HEAL_TOWER: 1,
    BuildingKind.BASE_TANK: 1,
}

#: Obstacle sets shared by the levels.
_OBSTACLES_STANDARD = {
    Obstacle.MINE: 4,
    Obstacle.MINE_WATER: 3,
    Obstacle.TRAP_FIRE: 2,
    Obstacle.TRAP_ICE: 2,
    Obstacle.WALL: 3,
}

#: The three levels offered by the menu.
LEVELS = [
    LevelConfig("Zatoka", (20, 13), 101, 2, "easy", 2, 2,
                dict(_NEUTRAL_STANDARD), dict(_OBSTACLES_STANDARD)),
    LevelConfig("Przełęcz", (24, 15), 202, 3, "normal", 3, 1,
                {**_NEUTRAL_STANDARD, BuildingKind.TURRET_RAPID: 1},
                {**_OBSTACLES_STANDARD, Obstacle.WALL: 5}),
    LevelConfig("Archipelag", (26, 16), 303, 4, "hard", 2, 4,
                dict(_NEUTRAL_STANDARD),
                {**_OBSTACLES_STANDARD, Obstacle.MINE: 6,
                 Obstacle.MINE_WATER: 5}),
]


def build_level(cfg: LevelConfig) -> Game:
    """Generate and return a ready-to-run :class:`Game` for ``cfg``."""
    for attempt in range(40):
        seed = cfg.seed + attempt * 7919
        result = _generate(cfg, seed)
        if result is not None:
            board, players, buildings = result
            return Game(board, players, buildings, seed)
    raise RuntimeError(f"could not generate level {cfg.name!r}")


# ----------------------------------------------------------------------
# Generation pipeline
# ----------------------------------------------------------------------
def _generate(cfg: LevelConfig, seed: int):
    """One generation attempt; returns (board, players, buildings) or None."""
    rng = random.Random(seed)
    cols, rows = cfg.size
    board = Board(cols, rows)
    heights = {t: 1 for t in board.tiles}

    # Elevated plateaus (sec. 1: heights 1..15).
    for _ in range(cfg.plateaus):
        center = (rng.randrange(2, cols - 2), rng.randrange(2, rows - 2))
        radius = rng.randint(2, 3)
        h = rng.randint(3, 5)
        for t in board.tiles:
            if hexgrid.hex_distance(t[0], t[1], center[0], center[1]) <= radius:
                heights[t] = max(heights[t], h)

    # Lakes (height 0 = water).
    for _ in range(cfg.lakes):
        center = (rng.randrange(2, cols - 2), rng.randrange(2, rows - 2))
        radius = rng.randint(1, 3)
        for t in board.tiles:
            if hexgrid.hex_distance(t[0], t[1], center[0], center[1]) <= radius:
                heights[t] = 0

    # The border ring is always plain height-1 land: it guarantees that
    # ground vehicles can reach any shore of the map.
    for (q, r) in board.tiles:
        if q in (0, cols - 1) or r in (0, rows - 1):
            heights[(q, r)] = 1

    for t, h in heights.items():
        board.tiles[t].height = h

    _connect_components(board, rng, cols, rows)
    _make_bridges(board, rng, limit=2)

    reach = board.reachable((0, 0), VehicleKind.TANK)
    buildings, used = _place_buildings(board, cfg, rng, reach)
    if buildings is None:
        return None
    _place_obstacles(board, cfg, rng, reach, used)

    players = [Player(0, True)]
    players += [Player(i, False) for i in range(1, cfg.players)]
    return board, players, buildings


# ----------------------------------------------------------------------
# Connectivity (ramps, sec. 7)
# ----------------------------------------------------------------------
def _land_components(board: Board):
    """Label land tiles with height-connected component ids."""
    comp = {}
    cid = 0
    for start in board.tiles:
        if start in comp or board.height(start) <= 0:
            continue
        comp[start] = cid
        stack = [start]
        while stack:
            cur = stack.pop()
            for n in board.neighbors(cur):
                if n not in comp and board.height(n) > 0 and \
                        board.height(n) == board.height(cur):
                    comp[n] = cid
                    stack.append(n)
        cid += 1
    return comp, cid


def _connect_components(board: Board, rng, cols: int, rows: int) -> None:
    """Add ramps until every elevated land area joins the main component.

    The main component is the border ring; any other component containing
    land of height >= 2 gets up to two ramps per pass.  Components that
    cannot be bridged by a ramp are flattened to height 1.
    """
    for _ in range(6):
        comp, ncomp = _land_components(board)
        main = comp[(0, 0)]
        others = set()
        for t, c in comp.items():
            if c != main and board.height(t) >= 2:
                others.add(c)
        if not others:
            return
        fixed = False
        for c in others:
            candidates = []
            for p in board.tiles:
                q, r = p
                if q in (0, cols - 1) or r in (0, rows - 1):
                    continue                       # keep the ring intact
                if board.tiles[p].bridge is not None:
                    continue
                for d in range(3):                 # 3 opposite-pair axes
                    a = hexgrid.neighbor(q, r, d)
                    b = hexgrid.neighbor(q, r, d + 3)
                    if not (board.contains(a) and board.contains(b)):
                        continue
                    ca, cb = comp.get(a), comp.get(b)
                    if ca is None or cb is None or ca == cb:
                        continue
                    if c not in (ca, cb):
                        continue
                    candidates.append((p, a, b))
            rng.shuffle(candidates)
            for p, a, b in candidates[:2]:
                board.set_ramp(p, a, b)
                fixed = True
        if not fixed:
            # Nothing can be connected by ramps: flatten leftovers.
            comp, ncomp = _land_components(board)
            main = comp[(0, 0)]
            for t, c in comp.items():
                if c != main and board.height(t) >= 2:
                    board.tiles[t].height = 1


# ----------------------------------------------------------------------
# Bridges (sec. 8)
# ----------------------------------------------------------------------
def _make_bridges(board: Board, rng, limit: int = 2) -> None:
    """Scan for straight water runs flanked by equal high land."""
    seen = set()
    for a in sorted(board.tiles):                 # deterministic order
        if len(board.bridges) >= limit:
            return
        w = board.height(a)
        if w < 3:
            continue
        for d in range(6):
            run = []
            cur = hexgrid.neighbor(a[0], a[1], d)
            while board.contains(cur) and board.height(cur) == 0:
                run.append(cur)
                cur = hexgrid.neighbor(cur[0], cur[1], d)
            if not run or not board.contains(cur):
                continue
            if board.height(cur) != w or len(run) > board.cols:
                continue
            key = frozenset((a, cur))
            if key in seen:
                continue
            seen.add(key)
            if board.add_bridge(a, cur, d) is not None:
                break


# ----------------------------------------------------------------------
# Buildings and obstacles
# ----------------------------------------------------------------------
def _place_buildings(board: Board, cfg: LevelConfig, rng, reach: set):
    """Place player bases and neutral buildings; returns (list, used)."""
    candidates = [t for t in sorted(reach)
                  if board.tiles[t].ramp is None
                  and board.tiles[t].bridge is None]
    rng.shuffle(candidates)
    used = set()
    chosen = []
    buildings = []

    def take(min_dist: int, predicate=lambda t: True):
        """First unused candidate far enough from every placed building."""
        for t in candidates:
            if t in used or not predicate(t):
                continue
            if all(hexgrid.hex_distance(t[0], t[1], c[0], c[1]) >= min_dist
                   for c in chosen):
                used.add(t)
                chosen.append(t)
                return t
        return None

    # Player bases, well spread over the map (rules sec. 1: the human and
    # at least one AI start with at least one base).
    base_kinds = [BuildingKind.BASE_TANK, BuildingKind.BASE_HELICOPTER,
                  BuildingKind.BASE_HOVERCRAFT, BuildingKind.BASE_BUFFER]
    for i in range(cfg.players):
        kind = base_kinds[0] if i == 0 else base_kinds[i % 4]
        tile = None
        for min_dist in (7, 5, 3):
            tile = take(min_dist)
            if tile is not None:
                break
        if tile is None:
            return None, used
        board.tiles[tile].obstacle = None
        buildings.append(Building(kind, i, tile[0], tile[1], units=20.0))

    # Neutral buildings (rules sec. 12).
    start_units = {BuildingKind.HEAL_TOWER: 10.0, BuildingKind.BASE_TANK: 15.0,
                   BuildingKind.BASE_BUFFER: 15.0}
    for kind, count in cfg.neutral.items():
        for _ in range(count):
            tile = take(2)
            if tile is None:
                return None, used
            board.tiles[tile].obstacle = None
            buildings.append(Building(kind, None, tile[0], tile[1],
                                      units=start_units.get(kind, 15.0)))
    return buildings, used


def _place_obstacles(board: Board, cfg: LevelConfig, rng, reach: set,
                     used: set) -> None:
    """Scatter mines, traps and walls (rules sec. 1, 4)."""
    buildings = used  # tiles occupied by buildings
    for kind, count in cfg.obstacles.items():
        for _ in range(count):
            for _try in range(60):
                t = rng.choice(sorted(board.tiles))
                tile = board.tiles[t]
                if t in buildings or tile.obstacle is not None or \
                        tile.ramp is not None or tile.bridge is not None:
                    continue
                if any(hexgrid.hex_distance(t[0], t[1], b[0], b[1]) < 2
                       for b in buildings):
                    continue                    # never right next to a base
                if kind == Obstacle.MINE and tile.height < 1:
                    continue
                if kind == Obstacle.MINE_WATER and tile.height != 0:
                    continue
                if kind in (Obstacle.TRAP_FIRE, Obstacle.TRAP_ICE,
                            Obstacle.WALL) and tile.height < 1:
                    continue
                tile.obstacle = Obstacle(kind)
                break
