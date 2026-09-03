"""Game entities: players, buildings and vehicles.

Pure data + small helpers; all rule logic lives in :mod:`war_regions.game`.
"""

from dataclasses import dataclass
from enum import Enum

from . import constants as C
from . import hexgrid
from .constants import TurretKind, VehicleKind


class BuildingKind(Enum):
    """All building kinds (rules.md sec. 3)."""

    BASE_TANK = "base_tank"
    BASE_HELICOPTER = "base_helicopter"
    BASE_HOVERCRAFT = "base_hovercraft"
    BASE_BUFFER = "base_buffer"
    TURRET_NORMAL = "turret_normal"
    TURRET_RAPID = "turret_rapid"
    TURRET_ROCKET = "turret_rocket"
    HEAL_TOWER = "heal_tower"


#: Bases spawn the vehicle kind matching their type (rules.md sec. 4).
BASE_VEHICLE = {
    BuildingKind.BASE_TANK: VehicleKind.TANK,
    BuildingKind.BASE_HELICOPTER: VehicleKind.HELICOPTER,
    BuildingKind.BASE_HOVERCRAFT: VehicleKind.HOVERCRAFT,
    BuildingKind.BASE_BUFFER: VehicleKind.BUFFER,
}

#: Turret kind behind each turret building (rules.md sec. 10).
TURRET_BUILDING = {
    BuildingKind.TURRET_NORMAL: TurretKind.NORMAL,
    BuildingKind.TURRET_RAPID: TurretKind.RAPID,
    BuildingKind.TURRET_ROCKET: TurretKind.ROCKET,
}


def is_base(kind: BuildingKind) -> bool:
    """True for the four base kinds."""
    return kind in BASE_VEHICLE


def is_turret(kind: BuildingKind) -> bool:
    """True for the three turret kinds."""
    return kind in TURRET_BUILDING


def vehicle_kind_of(kind: BuildingKind) -> VehicleKind:
    """Vehicle kind spawned by a building: base type or a tank (sec. 4)."""
    return BASE_VEHICLE.get(kind, VehicleKind.TANK)


def turret_kind_of(kind: BuildingKind):
    """TurretKind of a turret building, else ``None``."""
    return TURRET_BUILDING.get(kind)


def capacity_of(kind: BuildingKind) -> int:
    """Building capacity: bases 100, everything else 50 (sec. 3)."""
    return C.BASE_CAPACITY if is_base(kind) else C.BUILDING_CAPACITY


@dataclass
class Player:
    """One of the 2-4 players (exactly one is human, rules.md sec. 1)."""

    id: int
    is_human: bool
    eliminated: bool = False

    @property
    def color(self) -> tuple:
        """Player colour used by the renderer."""
        return C.PLAYER_COLORS[self.id % len(C.PLAYER_COLORS)]


class Building:
    """A structure standing on one tile: base, turret or healing tower."""

    def __init__(self, kind: BuildingKind, owner, q: int, r: int,
                 units: float = 0.0):
        self.kind = kind
        self.owner = owner            # player id or None (neutral)
        self.tile = (q, r)
        self.units = float(units)
        self.capacity = capacity_of(kind)
        self.production_timer = 0.0   # base spawn cycle progress
        self.fire_timer = 0.0         # turret cooldown progress
        self.heal_timer = 0.0         # healing-tower pulse progress
        self.last_target_pos = None   # world pos of last turret target
        # floating -x / +x aggregation (specification: floating numbers)
        self.loss_acc = 0.0
        self.gain_acc = 0.0
        self.text_timer = 0.0
        self.texts = []               # [ [amount, age], ... ]

    @property
    def pos(self) -> tuple:
        """World position of the tile centre."""
        return hexgrid.hex_to_world(self.tile[0], self.tile[1], C.HEX_SIDE)

    def __repr__(self):  # pragma: no cover - debugging aid
        return f"<Building {self.kind.value} owner={self.owner} " \
               f"tile={self.tile} units={self.units:.0f}>"


class Vehicle:
    """A moving group of units travelling along a fixed route (sec. 4)."""

    _next_id = 1

    def __init__(self, kind: VehicleKind, owner: int, units: float,
                 route: list, start_pos: tuple):
        self.id = Vehicle._next_id
        Vehicle._next_id += 1
        self.kind = kind
        self.owner = owner
        self.units = float(units)
        self.route = list(route)          # tiles after source, incl. target
        self.route_index = 0              # next waypoint into ``route``
        self.x, self.y = start_pos        # continuous world position
        self.combat_target = None         # Vehicle | None (sec. 9)
        self.last_opponent = None         # last vehicle we shot at
        self.fire_timer = 0.0             # combat shot cooldown progress
        self.wall_timer = 0.0             # wall-attack shot cooldown progress
        self.wall_target = None           # tile of the wall being shot
        self.wall_shots = 0               # shots fired at that wall so far
        self.loss_acc = 0.0
        self.gain_acc = 0.0
        self.text_timer = 0.0
        self.texts = []
        self.dead = False                 # set when destroyed / arrived

    @property
    def pos(self) -> tuple:
        """World position (centre of the sprite)."""
        return (self.x, self.y)

    @property
    def dest_tile(self):
        """Final tile of the route (or ``None``)."""
        return self.route[-1] if self.route else None

    def __repr__(self):  # pragma: no cover - debugging aid
        return f"<Vehicle {self.kind.value} owner={self.owner} " \
               f"units={self.units:.0f} id={self.id}>"
