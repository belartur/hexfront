"""The game board: tiles, obstacles, ramps, bridges and path-finding.

Ground-movement rules implemented in :meth:`Board.passable` follow
rules.md sections 4, 5, 7 (ramps) and 8 (bridges).
"""

from collections import deque

from . import constants as C
from . import hexgrid
from .constants import VehicleKind


class Obstacle:
    """A static obstacle standing on a tile (rules.md sec. 1, 4)."""

    WALL = "wall"            # 20 hp, blocks ground vehicles until destroyed
    MINE = "mine"            # 25 damage once, then removed (land)
    MINE_WATER = "mine_water"
    TRAP_FIRE = "trap_fire"  # 1 dmg/s while on it, never removed
    TRAP_ICE = "trap_ice"    # halves speed while on it, never removed

    def __init__(self, kind: str):
        self.kind = kind
        #: Only walls have hit points; everything else is indestructible
        #: by direct damage (mines explode themselves, traps stay).
        self.hp = C.WALL_HP if kind == Obstacle.WALL else 0


class Tile:
    """One hexagonal field of the board."""

    __slots__ = ("height", "obstacle", "ramp", "bridge")

    def __init__(self, height: int = 1):
        #: Immutable in play; 0 = water, 1..15 = land (rules.md sec. 1).
        self.height = height
        self.obstacle = None   # Obstacle | None
        #: Ramp: pair (a, b) of *opposite* neighbour tiles it connects
        #: (rules.md sec. 7).  Its height equals min(height(a), height(b)).
        self.ramp = None
        #: Bridge deck fragment flying over this tile (rules.md sec. 8).
        self.bridge = None


class Bridge:
    """A straight bridge connecting two non-adjacent equal-height tiles."""

    def __init__(self, a: tuple, b: tuple, w: int, direction: int,
                 fragments: list):
        self.a = a                  # land tile at height w
        self.b = b                  # land tile at height w
        self.w = w                  # shared height of both ends
        self.direction = direction  # hex direction from a towards b
        self.fragments = fragments  # deck tiles, ordered a -> b
        #: Adjacent tile pairs connected *along the deck*; these bypass
        #: normal terrain-height rules (sec. 8).
        self.pairs = set()
        seq = [a] + fragments + [b]
        for u, v in zip(seq, seq[1:]):
            self.pairs.add(frozenset((u, v)))


class Board:
    """Rectangular (odd-q) board of hexagonal tiles."""

    def __init__(self, cols: int, rows: int, side: float = C.HEX_SIDE):
        self.cols = cols
        self.rows = rows
        self.side = side
        self.tiles = {(q, r): Tile() for q in range(cols)
                      for r in range(rows)}
        self.bridges = []

    # ------------------------------------------------------------------
    # Basic queries
    # ------------------------------------------------------------------
    def contains(self, tile: tuple) -> bool:
        """True when ``tile`` lies on the board."""
        q, r = tile
        return 0 <= q < self.cols and 0 <= r < self.rows

    def tile(self, tile: tuple):
        """Tile object or ``None`` when outside the board."""
        return self.tiles.get(tile)

    def height(self, tile: tuple) -> int:
        """Height of a tile (0 = water); outside tiles are treated as 0."""
        t = self.tiles.get(tile)
        return t.height if t is not None else 0

    def neighbors(self, tile: tuple) -> list:
        """In-board neighbours of a tile, in fixed direction order."""
        return [n for n in hexgrid.neighbors(*tile) if self.contains(n)]

    def center_world(self, tile: tuple) -> tuple:
        """World coordinates of the tile centre."""
        return hexgrid.hex_to_world(tile[0], tile[1], self.side)

    def world_to_tile(self, x: float, y: float):
        """Tile containing the world point, or ``None`` when outside."""
        tile = hexgrid.world_to_hex(x, y, self.side)
        return tile if self.contains(tile) else None

    # ------------------------------------------------------------------
    # Map features
    # ------------------------------------------------------------------
    def set_ramp(self, p: tuple, a: tuple, b: tuple) -> None:
        """Turn tile ``p`` into a ramp joining opposite neighbours a, b.

        The ramp tile's height becomes min(height(a), height(b)) - sec. 7.
        """
        t = self.tiles[p]
        t.ramp = (a, b)
        t.height = min(self.height(a), self.height(b))
        t.obstacle = None
        t.bridge = None

    def add_bridge(self, a: tuple, b: tuple, direction: int):
        """Try to build a bridge from ``a`` towards ``b`` in ``direction``.

        Returns the new :class:`Bridge` or ``None`` when the geometry does
        not permit one (sec. 8): the ends must share a height w >= 3, the
        straight corridor between them must stay on the board and every
        fragment tile must be lower than w - 2.
        """
        fragments = []
        cur = a
        while True:
            cur = hexgrid.neighbor(cur[0], cur[1], direction)
            if cur == b:
                break
            if not self.contains(cur):
                return None
            t = self.tiles[cur]
            if t.ramp is not None or t.bridge is not None or t.obstacle:
                return None
            fragments.append(cur)
            if len(fragments) > self.cols + self.rows:
                return None
        w = self.height(a)
        if w < 3 or w != self.height(b):
            return None
        if any(self.height(t) > w - 3 for t in fragments):
            return None
        bridge = Bridge(a, b, w, direction, fragments)
        for t in fragments:
            self.tiles[t].bridge = bridge
        self.bridges.append(bridge)
        return bridge

    # ------------------------------------------------------------------
    # Movement rules
    # ------------------------------------------------------------------
    def passable(self, u: tuple, v: tuple, kind: VehicleKind) -> bool:
        """True when a vehicle of ``kind`` may drive directly u -> v."""
        if u == v:
            return False
        if kind == VehicleKind.HELICOPTER:
            return True                      # sec. 5.2: flies everywhere
        tu, tv = self.tiles[u], self.tiles[v]

        # Ramps (sec. 7): enterable only along the a/b axis; heights may
        # differ, but the non-ramp end must be drivable terrain.
        if tu.ramp is not None or tv.ramp is not None:
            if tu.ramp is not None and v in tu.ramp:
                return self._drivable(v, kind)
            if tv.ramp is not None and u in tv.ramp:
                return self._drivable(u, kind)
            return False                     # off-axis move onto a ramp

        # Bridge decks (sec. 8): travelling along the bridge ignores
        # terrain heights; crossing under follows the normal rules.
        br = tu.bridge or tv.bridge
        if br is not None and frozenset((u, v)) in br.pairs:
            return True

        hu, hv = tu.height, tv.height
        if kind == VehicleKind.HOVERCRAFT:   # sec. 5.3
            if hu == hv:
                return True                  # water-water / same land
            lo, hi = min(hu, hv), max(hu, hv)
            return lo == 0 and hi == 1       # shore crossing only at h=1
        # Tank / buffer (sec. 5.1, 5.4): equal-height land only.
        return hu == hv and hu > 0

    def _drivable(self, tile: tuple, kind: VehicleKind) -> bool:
        """Terrain check ignoring heights (used for ramp ends)."""
        if tile[0] is None:  # pragma: no cover - defensive
            return False
        t = self.tiles.get(tile)
        if t is None:
            return False
        if t.ramp is not None:
            return True                      # ramp decks carry any vehicle
        if kind == VehicleKind.HOVERCRAFT:
            return True                      # hovercraft: land or water
        return t.height > 0                  # tank / buffer: land only

    # ------------------------------------------------------------------
    # Path-finding (rules.md sec. 6: shortest road, obstacles ignored)
    # ------------------------------------------------------------------
    def find_path(self, src: tuple, dst: tuple, kind: VehicleKind):
        """Breadth-first shortest route src -> dst for vehicle ``kind``.

        Returns the list of tiles *after* the source, including the
        destination, or ``None`` when no road exists.  Mines, traps and
        walls are deliberately ignored (sec. 6); ramps and bridges are
        respected because they change the road graph itself.
        """
        if src == dst or not self.contains(src) or not self.contains(dst):
            return None
        prev = {src: None}
        queue = deque((src,))
        while queue:
            cur = queue.popleft()
            for n in self.neighbors(cur):
                if n in prev or not self.passable(cur, n, kind):
                    continue
                prev[n] = cur
                if n == dst:
                    path = [n]
                    while prev[path[-1]] is not None:
                        path.append(prev[path[-1]])
                    path.reverse()
                    return path[1:]          # drop the source tile
                queue.append(n)
        return None

    def reachable(self, src: tuple, kind: VehicleKind) -> set:
        """Set of tiles reachable from ``src`` by vehicle ``kind``."""
        seen = {src}
        queue = deque((src,))
        while queue:
            cur = queue.popleft()
            for n in self.neighbors(cur):
                if n not in seen and self.passable(cur, n, kind):
                    seen.add(n)
                    queue.append(n)
        return seen

    def path_world_length(self, path: list, start: tuple = None) -> float:
        """World-space length of a tile path (for travel-time estimates).

        When ``start`` is given, the hop from ``start`` to ``path[0]``
        is included (useful because routes exclude their source tile).
        """
        total = 0.0
        prev = self.center_world(start) if start is not None else None
        for t in path:
            pos = self.center_world(t)
            if prev is not None:
                total += ((pos[0] - prev[0]) ** 2 +
                          (pos[1] - prev[1]) ** 2) ** 0.5
            prev = pos
        return total
