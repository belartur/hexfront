"""Global constants of *War Regions*.

Every gameplay value (distances, ranges, speeds, radii, amounts) is taken
from ``rules.md`` and expressed in distance units **j**.  The conversion
factor from *j* to pixels is defined exactly once, in ``UNIT_J_TO_PX``:
zoom and window scaling affect *rendering only*, never the simulation.

Each constant carries a short documentation comment pointing at the rule
it implements, so values can be tweaked easily when experimenting.
"""

from enum import Enum

# --------------------------------------------------------------------------
# Units and world scale
# --------------------------------------------------------------------------
#: 1 j == 1 px at 1:1 view scale (specification: "Parametry").
UNIT_J_TO_PX = 1.0

#: Side length of a flat-top hexagon in j (specification: "Parametry").
HEX_SIDE = 36.0 * UNIT_J_TO_PX

#: Pixels of screen elevation per one unit of tile height (rendering only;
#: heights themselves are integers 0..15 per rules.md section 1).
ELEVATION_PX = 9.0 * UNIT_J_TO_PX

#: cos(30 deg) - horizontal factor of the isometric projection.
ISO_COS = 0.8660254037844387

#: Vertical squash factor of the isometric projection (2:1 isometric).
ISO_SIN = 0.5

# --------------------------------------------------------------------------
# Simulation
# --------------------------------------------------------------------------
#: Fixed simulation frame rate (specification: FPS = 1/60).
FPS = 60

#: Length of one simulation step in seconds.
SIM_DT = 1.0 / FPS

# --------------------------------------------------------------------------
# Vehicles (rules.md sections 4-5)
# --------------------------------------------------------------------------
class VehicleKind(Enum):
    """Kinds of vehicles that can travel over the map (rules.md sec. 4)."""

    TANK = "tank"
    HELICOPTER = "helicopter"
    HOVERCRAFT = "hovercraft"
    BUFFER = "buffer"


#: Cruise speed of each vehicle kind in j/s (rules.md sec. 5).
VEHICLE_SPEED = {
    VehicleKind.TANK: 60.0,          # sec. 5.1
    VehicleKind.HELICOPTER: 90.0,    # sec. 5.2
    VehicleKind.HOVERCRAFT: 48.0,    # sec. 5.3 (20% slower)
    VehicleKind.BUFFER: 60.0,        # sec. 5.4 (moves like a tank)
}

#: Radius of the healing aura of a buffer in j (rules.md sec. 5.4).
BUFFER_HEAL_RADIUS = 160.0

#: Units restored per second by a buffer (1 unit / 2 s, rules.md sec. 5.4).
BUFFER_HEAL_RATE = 0.5

# --------------------------------------------------------------------------
# Obstacles (rules.md sections 1, 4)
# --------------------------------------------------------------------------
#: Damage dealt by a mine that explodes under a vehicle (sec. 4).
MINE_DAMAGE = 25

#: Distance in j from a mined tile's centre at which the mine explodes
#: ("srodek grafiki pojazdu znajduje sie na srodku pola" - sec. 1).
MINE_TRIGGER_RADIUS = 8.0

#: Damage per second while a vehicle sits on a fire trap (sec. 4).
FIRE_TRAP_DPS = 1.0

#: Speed multiplier while a ground vehicle is on an ice trap (sec. 4).
ICE_TRAP_SLOWDOWN = 0.5

#: Number of hits a wall can take before it collapses (sec. 4).
WALL_HP = 20

#: Interval between consecutive shots of a vehicle at a wall (sec. 4).
WALL_ATTACK_INTERVAL = 1.0

#: Damage of one shot at a wall - always exactly 1 (sec. 4).
WALL_ATTACK_DAMAGE = 1

# --------------------------------------------------------------------------
# Combat (rules.md section 9)
# --------------------------------------------------------------------------
#: Detection radius in j - vehicles stop and fight within this circle.
DETECTION_RADIUS = 80.0

#: Interval between consecutive shots in vehicle-vs-vehicle combat.
FIRE_INTERVAL = 1.0

# --------------------------------------------------------------------------
# Turrets (rules.md section 10)
# --------------------------------------------------------------------------
class TurretKind(Enum):
    """Kinds of gun emplacements (rules.md sec. 10)."""

    NORMAL = "normal"
    RAPID = "rapid"
    ROCKET = "rocket"


#: Per-kind turret parameters.  ``range`` and ``splash`` in j,
#: ``cooldown`` in seconds, ``damage_div``: damage is ceil(x / damage_div)
#: where x is the number of units stored in the turret.
TURRET_STATS = {
    TurretKind.NORMAL: {"range": 250.0, "cooldown": 5.0,
                        "damage_div": 1.0, "splash": 0.0},   # sec. 10.1
    TurretKind.RAPID: {"range": 190.0, "cooldown": 1.0,
                       "damage_div": 4.0, "splash": 0.0},    # sec. 10.3
    TurretKind.ROCKET: {"range": 320.0, "cooldown": 5.0,
                        "damage_div": 1.0, "splash": 80.0},  # sec. 10.2
}

# --------------------------------------------------------------------------
# Healing tower (rules.md section 11)
# --------------------------------------------------------------------------
#: Range of a healing tower equals this factor times its unit count.
HEAL_TOWER_RANGE_PER_UNIT = 15.0

#: Interval between healing pulses of a healing tower, in seconds.
HEAL_TOWER_INTERVAL = 3.0

#: Units granted per pulse to every friendly vehicle in range.
HEAL_TOWER_AMOUNT = 2

# --------------------------------------------------------------------------
# Buildings (rules.md sections 2-3)
# --------------------------------------------------------------------------
#: Capacity of every building except bases (sec. 3).
BUILDING_CAPACITY = 50

#: Capacity of bases (sec. 3).
BASE_CAPACITY = 100

#: Units dying per second in an overcrowded building (sec. 3).
OVERCROWD_DEATH_RATE = 1.0

#: Units arriving in a base at the end of each production cycle (sec. 3).
BASE_SPAWN_AMOUNT = 5

#: Length of one base production cycle in seconds (sec. 3).
BASE_SPAWN_INTERVAL = 10.0

# --------------------------------------------------------------------------
# Camera / input (specification: "Sterowanie")
# --------------------------------------------------------------------------
ZOOM_MIN = 0.5
ZOOM_MAX = 2.0
ZOOM_STEP = 1.1                    # multiplicative factor per wheel notch
PAN_SPEED = 700.0                  # screen px per second via keys/edges
EDGE_PAN_MARGIN = 12               # px of screen edge that pans the view
DRAG_THRESHOLD = 5                 # px of movement before a drag starts
LOADING_TIME = 1.0                 # s the "showing the map" screen lasts

# --------------------------------------------------------------------------
# Floating combat text (specification: "Grafika i interfejs uzytkownika")
# --------------------------------------------------------------------------
FLOAT_TEXT_LIFETIME = 2.0          # seconds a -x / +x number stays visible
FLOAT_TEXT_SPEED = 26.0            # px/s of upward drift

# --------------------------------------------------------------------------
# Colours (specification: land is grey, water light blue; players differ)
# --------------------------------------------------------------------------
WATER_COLOR = (110, 170, 225)
WATER_EDGE = (90, 150, 205)
LAND_COLOR = (152, 152, 152)
LAND_VARIANT = (140, 140, 140)
LAND_EDGE = (110, 110, 110)

#: One distinct colour per player, indexed by player id (max 4 players).
PLAYER_COLORS = [
    (70, 135, 250),    # blue  - human
    (230, 85, 70),     # red   - AI
    (245, 195, 55),    # yellow- AI
    (115, 200, 95),    # green - AI
]

#: Colour of objects that belong to no player.
NEUTRAL_COLOR = (165, 165, 165)

#: Translucent overlays: turret ranges white, healing ranges light green.
RANGE_TURRET_COLOR = (255, 255, 255, 42)
RANGE_HEAL_COLOR = (150, 245, 150, 46)

#: Preview path colours.
PATH_COLOR = (255, 255, 255)
PATH_PREVIEW_COLOR = (255, 240, 120)

UI_TEXT_COLOR = (235, 235, 235)
UI_BACKGROUND = (24, 26, 34)

# --------------------------------------------------------------------------
# AI difficulty (rules.md sec. 13.5, 13.8)
# --------------------------------------------------------------------------
from dataclasses import dataclass


@dataclass(frozen=True)
class AIDifficulty:
    """Tunable parameters of one AI difficulty level (rules.md sec. 13.8)."""

    name: str
    interval: float        # seconds between decisions (sec. 13.2)
    noise: float           # std-dev of the score noise
    reaction_delay: float  # s before a fresh threat is reacted to
    threshold: float       # minimum score to perform an action (sec. 13.5)
    w1: float              # weight: chance of capturing the target
    w2: float              # weight: value of the target building
    w3: float              # weight: defence need / evacuation
    w4: float              # weight: travel time
    w5: float              # weight: route danger
    w6: float              # weight: risk of losing the source


#: Difficulty presets (rules.md sec. 13.8); tweak freely to experiment.
AI_DIFFICULTIES = {
    "easy": AIDifficulty("easy", 2.6, 1.4, 7.0, 1.2,
                         6.0, 3.0, 4.0, 1.2, 2.0, 2.0),
    "normal": AIDifficulty("normal", 2.0, 0.7, 3.5, 0.9,
                           8.0, 4.0, 5.0, 1.8, 3.0, 3.0),
    "hard": AIDifficulty("hard", 1.5, 0.25, 1.5, 0.6,
                         10.0, 5.0, 6.0, 2.2, 4.0, 4.0),
}
