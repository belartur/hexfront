"""Hexagonal grid helpers for a flat-top hex layout.

The board uses *odd-q* offset coordinates (flat-top hexagons, odd columns
pushed down on screen):

    world.x = 1.5 * side * q
    world.y = sqrt(3) * side * (r + 0.5 * (q & 1))

All functions here are pure geometry; they know nothing about game rules.
"""

import math

#: sqrt(3), the height/width ratio constant of a flat-top hexagon.
SQRT3 = math.sqrt(3.0)

#: Geometric directions, shared by both parities, ordered by angle:
#: 30, 90, 150, 210, 270, 330 deg (screen y grows downwards).  Stepping
#: repeatedly in one direction follows a straight hex corridor.
GEO_DIRS_EVEN = ((1, 0), (0, 1), (-1, 0), (-1, -1), (0, -1), (1, -1))

#: Same angles for tiles in odd columns (the odd-q stagger swaps offsets).
GEO_DIRS_ODD = ((1, 1), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, 0))

#: Index of the opposite (180 deg) geometric direction.
OPPOSITE_DIR = (3, 4, 5, 0, 1, 2)


def dirs(q: int) -> tuple:
    """The 6 geometric direction offsets for a tile in column ``q``."""
    return GEO_DIRS_ODD if q & 1 else GEO_DIRS_EVEN


def neighbor(q: int, r: int, direction: int) -> tuple:
    """Return the (q, r) coordinates of the neighbour in ``direction``.

    Directions are *geometric* (see GEO_DIRS_*): stepping repeatedly in
    one direction follows a straight line of hexes, and direction
    (d + 3) % 6 always leads back.
    """
    dq, dr = dirs(q)[direction % 6]
    return (q + dq, r + dr)


def neighbors(q: int, r: int) -> tuple:
    """Return the (q, r) coordinates of all six neighbours."""
    return tuple((q + dq, r + dr) for dq, dr in dirs(q))


def hex_to_world(q: int, r: int, side: float) -> tuple:
    """Convert offset hex coordinates to world (x, y) of the tile centre."""
    x = 1.5 * side * q
    y = SQRT3 * side * (r + 0.5 * (q & 1))
    return (x, y)


def _axial_round(x: float, y: float) -> tuple:
    """Round fractional axial coordinates (q, r) to the nearest hex."""
    xc = x
    zc = y
    yc = -xc - zc
    rx, ry, rz = round(xc), round(yc), round(zc)
    dx, dy, dz = abs(rx - xc), abs(ry - yc), abs(rz - zc)
    if dx > dy and dx > dz:
        rx = -ry - rz
    elif dy > dz:
        ry = -rx - rz
    else:
        rz = -rx - ry
    return (int(rx), int(rz))


def world_to_hex(x: float, y: float, side: float) -> tuple:
    """Convert world coordinates to the offset (q, r) of the containing hex."""
    qf = 2.0 / 3.0 * x / side
    rf = SQRT3 / 3.0 * y / side - qf / 2.0
    qa, ra = _axial_round(qf, rf)
    q = qa
    r = ra + (q - (q & 1)) // 2
    return (q, r)


def offset_to_axial(q: int, r: int) -> tuple:
    """Convert odd-q offset coordinates to axial (q, r)."""
    return (q, r - (q - (q & 1)) // 2)


def hex_distance(q1: int, r1: int, q2: int, r2: int) -> int:
    """Hex-grid distance (number of steps) between two tiles."""
    qa1, ra1 = offset_to_axial(q1, r1)
    qa2, ra2 = offset_to_axial(q2, r2)
    dx = qa1 - qa2
    dz = ra1 - ra2
    dy = -dx - dz
    return (abs(dx) + abs(dy) + abs(dz)) // 2


def hex_corners(q: int, r: int, side: float) -> list:
    """Return the six corners of a tile as world (x, y) pairs.

    Corners are ordered clockwise starting at angle 0 deg (east).  The edge
    between corner ``k`` and corner ``k + 1`` faces the direction returned
    by :func:`edge_dir_index`.
    """
    cx, cy = hex_to_world(q, r, side)
    pts = []
    for k in range(6):
        ang = math.radians(60.0 * k)
        pts.append((cx + side * math.cos(ang), cy + side * math.sin(ang)))
    return pts


def edge_dir_index(q: int, k: int) -> int:
    """Map the edge between corner ``k`` and ``k + 1`` to a direction.

    Edge 0 (corners 0-1) faces 30 deg = direction 0; the geometric
    direction indices are ordered by angle, so the answer is simply ``k``.
    """
    return k % 6
