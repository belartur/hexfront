"""Binary map file format (specification: "Format pliku planszy").

Layout of a ``.map`` file, all integers big-endian:

* 4 header bytes: number of columns ``k`` (2 B) and rows ``w`` (2 B);
* ``k * w`` 4-bit heights, two tiles per byte (the *high* nibble holds the
  earlier tile of each pair, tiles ordered row-major: ``r * k + q``).
  When ``k * w`` is odd the final low nibble stores zero;
* objects, one record each: 2 B column + 2 B row + 1 B type, followed by
  2 extra bytes for buildings (owner 0-4, where 0 is neutral, and the
  starting unit count).  Type codes:

  - 0-19 building kinds (part of the range unused), see ``BUILDING_CODES``,
  - 20-22 bridge fragment, code - 20 = the geometric axis (0-2) of the
    bridge; a whole bridge is stored as one record per deck fragment,
  - 23-25 ramp, code - 23 = the geometric axis of the two connected
    opposite neighbours,
  - 26 and up obstacle kinds, see ``OBSTACLE_CODES``.

Bridge fragments are reassembled into whole :class:`~war_regions.board.Bridge`
objects by :func:`rebuild_bridges` (shared with the editor).
"""

import os
import struct
import zlib

from . import hexgrid
from . import constants as C
from .board import Board, Bridge, Obstacle
from .entities import Building, BuildingKind, Player
from .game import Game

# ----------------------------------------------------------------------
# Code tables (specification: "Format pliku planszy")
# ----------------------------------------------------------------------
#: Building kind behind each building type code 0-19 (rest unused).
BUILDING_CODES = {
    0: BuildingKind.BASE_TANK,
    1: BuildingKind.BASE_HELICOPTER,
    2: BuildingKind.BASE_HOVERCRAFT,
    3: BuildingKind.BASE_BUFFER,
    4: BuildingKind.TURRET_NORMAL,
    5: BuildingKind.TURRET_RAPID,
    6: BuildingKind.TURRET_ROCKET,
    7: BuildingKind.HEAL_TOWER,
}

#: Inverse of :data:`BUILDING_CODES`.
CODE_BUILDINGS = {kind: code for code, kind in BUILDING_CODES.items()}

#: First type code of bridges (code - BRIDGE_CODE_BASE = axis 0-2).
BRIDGE_CODE_BASE = 20

#: First type code of ramps (code - RAMP_CODE_BASE = axis 0-2).
RAMP_CODE_BASE = 23

#: Obstacle kind behind each obstacle type code 26 and up.
OBSTACLE_CODES = {
    26: Obstacle.WALL,
    27: Obstacle.MINE,
    28: Obstacle.MINE_WATER,
    29: Obstacle.TRAP_FIRE,
    30: Obstacle.TRAP_ICE,
}

#: Inverse of :data:`OBSTACLE_CODES`.
CODE_OBSTACLES = {kind: code for code, kind in OBSTACLE_CODES.items()}

#: Building owner codes (specification): 0 neutral, 1 blue (the human
#: player), 2 red, 3 green, 4 yellow.
OWNER_CODE_NEUTRAL = 0

# ----------------------------------------------------------------------
# Saving
# ----------------------------------------------------------------------
def save_map(path: str, board: Board, buildings: list) -> None:
    """Write ``board`` and its ``buildings`` to the binary file ``path``."""
    nibbles = []
    for r in range(board.rows):                     # row-major order
        for q in range(board.cols):
            nibbles.append(max(0, min(15, board.tiles[(q, r)].height)))
    if len(nibbles) % 2:
        nibbles.append(0)                           # padding low nibble
    heights = bytearray()
    for i in range(0, len(nibbles), 2):
        heights.append((nibbles[i] << 4) | nibbles[i + 1])

    records = []
    for b in sorted(buildings, key=lambda b: b.tile):
        code = CODE_BUILDINGS.get(b.kind)
        if code is None:
            continue
        owner = (OWNER_CODE_NEUTRAL if b.owner is None else b.owner + 1)
        units = max(0, min(255, int(round(b.units))))
        records.append(struct.pack(">HHBBB", b.tile[0], b.tile[1],
                                   code, owner, units))
    for tile in sorted(board.tiles):
        t = board.tiles[tile]
        if t.ramp is not None:
            axis = _ramp_axis(tile, t.ramp)
            if axis is not None:
                records.append(struct.pack(">HHB", tile[0], tile[1],
                                           RAMP_CODE_BASE + axis))
        elif t.bridge is not None:
            records.append(struct.pack(">HHB", tile[0], tile[1],
                                       BRIDGE_CODE_BASE
                                       + t.bridge.direction % 3))
        elif t.obstacle is not None:
            code = CODE_OBSTACLES.get(t.obstacle.kind)
            if code is not None:
                records.append(struct.pack(">HHB", tile[0], tile[1], code))

    with open(path, "wb") as f:
        f.write(struct.pack(">HH", board.cols, board.rows))
        f.write(bytes(heights))
        for rec in records:
            f.write(rec)


def _ramp_axis(tile: tuple, ramp: tuple):
    """Axis 0-2 of a ramp whose neighbour ``a`` is ``ramp[0]``, else None."""
    a = ramp[0]
    for d in range(6):
        if hexgrid.neighbor(tile[0], tile[1], d) == a:
            return d % 3
    return None



# ----------------------------------------------------------------------
# Loading
# ----------------------------------------------------------------------
def load_board(path: str, validate: bool = True):
    """Read a map file; returns ``(board, buildings)``.

    Structural corruption raises :class:`ValueError`; objects that violate
    the placement rules of rules.md sec. 1 are skipped with a warning.
    With ``validate=False`` (used by the board editor) bridge fragments
    are kept and previewed even when their geometry violates sec. 8.
    """
    with open(path, "rb") as f:
        data = f.read()
    if len(data) < 4:
        raise ValueError(f"{path}: file too short for the header")
    cols, rows = struct.unpack_from(">HH", data, 0)
    if cols == 0 or rows == 0:
        raise ValueError(f"{path}: empty board")
    board = Board(cols, rows)

    n = cols * rows
    nibbles = []
    for byte in data[4:4 + (n + 1) // 2]:
        nibbles.append(byte >> 4)
        nibbles.append(byte & 0x0F)
    if len(nibbles) < n:
        raise ValueError(f"{path}: truncated height data")
    for r in range(rows):
        for q in range(cols):
            board.tiles[(q, r)].height = nibbles[r * cols + q]

    buildings = []
    frag_marks = {}
    off = 4 + (n + 1) // 2
    while off + 5 <= len(data):
        q, r, code = struct.unpack_from(">HHB", data, off)
        off += 5
        tile = (q, r)
        if not board.contains(tile):
            raise ValueError(f"{path}: object outside the board at {tile}")
        if code <= 19:
            if off + 2 > len(data):
                raise ValueError(f"{path}: truncated building at {tile}")
            owner, units = struct.unpack_from(">BB", data, off)
            off += 2
            kind = BUILDING_CODES.get(code)
            if kind is None:
                _warn(path, f"unused building code {code} at {tile}")
                continue
            buildings.append(Building(
                kind, None if owner == OWNER_CODE_NEUTRAL else owner - 1,
                q, r, units=float(units)))
        elif code < RAMP_CODE_BASE:
            frag_marks[tile] = code - BRIDGE_CODE_BASE
        elif code < 26:
            axis = code - RAMP_CODE_BASE
            a = hexgrid.neighbor(q, r, axis)
            b = hexgrid.neighbor(q, r, axis + 3)
            if board.contains(a) and board.contains(b) \
                    and not any(bd.tile == tile for bd in buildings):
                board.set_ramp(tile, a, b)
            else:
                _warn(path, f"invalid ramp at {tile}")
        else:
            kind = OBSTACLE_CODES.get(code)
            if kind is None:
                _warn(path, f"unknown obstacle code {code} at {tile}")
            elif _obstacle_allowed(board, buildings, tile, kind):
                board.tiles[tile].obstacle = Obstacle(kind)
            else:
                _warn(path, f"obstacle {kind} not allowed at {tile}")

    rebuild_bridges(board, frag_marks, validate)
    return board, buildings


def load_game(path: str) -> Game:
    """Load a map file as a ready-to-run :class:`Game`.

    Players derive from the building owners found in the file (owner byte
    1 is the blue, human player); the AI randomness seed derives from the
    file name so every level is deterministic (rules.md sec. 13.2).
    """
    board, buildings = load_board(path)
    owners = {b.owner for b in buildings if b.owner is not None}
    top = max(owners) if owners else 0
    players = [Player(i, i == 0) for i in range(top + 1)]
    return Game(board, players, buildings, level_seed(path))


def level_seed(path: str) -> int:
    """Deterministic AI seed of a level, derived from its file name."""
    return zlib.crc32(os.path.basename(path).encode("utf-8"))


def list_maps(directory: str = None) -> list:
    """Sorted paths of all map files in ``directory`` (default MAPS_DIR)."""
    directory = directory or C.MAPS_DIR
    if not os.path.isdir(directory):
        return []
    return [os.path.join(directory, name)
            for name in sorted(os.listdir(directory))
            if name.endswith(C.MAP_EXTENSION)]


def rebuild_bridges(board: Board, frag_marks: dict,
                    validate: bool = True) -> None:
    """(Re)build whole bridges from per-tile fragment marks.

    ``frag_marks`` maps a deck tile to the geometric axis 0-2 of its
    bridge.  Every maximal straight run of same-axis fragments becomes one
    :class:`~war_regions.board.Bridge` via :meth:`Board.add_bridge`,
    which validates the geometry of rules.md sec. 8 (equal end heights
    >= 3, fragments low enough); invalid runs are silently dropped.

    With ``validate=False`` (the board editor's preview mode) geometry-
    invalid runs still get a bridge object, rendered at the highest
    involved terrain elevation; the game keeps validating on load.
    """
    board.bridges.clear()
    for t in board.tiles:
        board.tiles[t].bridge = None
    remaining = dict(frag_marks)
    while remaining:
        tile, axis = next(iter(remaining.items()))
        back = (axis + 3) % 6
        start = tile
        prev = hexgrid.neighbor(start[0], start[1], back)
        while prev in remaining and remaining[prev] == axis:
            start = prev
            prev = hexgrid.neighbor(start[0], start[1], back)
        run = [start]
        cur = hexgrid.neighbor(start[0], start[1], axis)
        while cur in remaining and remaining[cur] == axis:
            run.append(cur)
            cur = hexgrid.neighbor(cur[0], cur[1], axis)
        for f in run:
            del remaining[f]
        a = hexgrid.neighbor(start[0], start[1], back)
        bridge = board.add_bridge(a, cur, axis) \
            if board.contains(a) and board.contains(cur) else None
        if bridge is None and not validate:
            w = max([board.height(a), board.height(cur)]
                    + [board.height(f) for f in run])
            bridge = Bridge(a, cur, w, axis, run)
            for f in run:
                board.tiles[f].bridge = bridge
            board.bridges.append(bridge)


def _obstacle_allowed(board: Board, buildings: list, tile: tuple,
                      kind: str) -> bool:
    """Placement rules of rules.md sec. 1 for one obstacle."""
    t = board.tiles[tile]
    if t.obstacle is not None or t.ramp is not None or t.bridge is not None:
        return False
    if any(b.tile == tile for b in buildings):
        return False
    if kind == Obstacle.MINE_WATER:
        return t.height == 0
    return t.height > 0                      # wall, mines, traps: land only


def _warn(path: str, message: str) -> None:
    """Print a non-fatal map-loading warning."""
    print(f"warning: {path}: {message}")

