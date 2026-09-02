"""Headless logic tests for War Regions.

Run with:  python3 -m tests.test_logic
Uses controlled, hand-built boards so every rule is verified exactly.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
os.environ.setdefault("SDL_VIDEODRIVER", "dummy")

from war_regions import constants as C                     # noqa: E402
from war_regions.board import Board, Obstacle              # noqa: E402
from war_regions.constants import VehicleKind              # noqa: E402
from war_regions.entities import (Building, BuildingKind, Player,  # noqa: E402
                                  Vehicle)
from war_regions.game import Game                          # noqa: E402
from war_regions import hexgrid                            # noqa: E402


def make_game(board):
    """Game with two players (0 = human, 1 = AI) on ``board``."""
    return Game(board, [Player(0, True), Player(1, False)], [], 1)


def run(game, seconds):
    """Advance the simulation by ``seconds`` of fixed steps."""
    for _ in range(round(seconds / C.SIM_DT)):
        game.update(C.SIM_DT)


# ----------------------------------------------------------------------
def test_hex_math():
    """World<->hex round trip and distances."""
    for q, r in [(0, 0), (3, 2), (1, 4), (5, 5)]:
        x, y = hexgrid.hex_to_world(q, r, C.HEX_SIDE)
        assert hexgrid.world_to_hex(x, y, C.HEX_SIDE) == (q, r), (q, r)
    assert hexgrid.hex_distance(0, 0, 2, 1) == 2
    # every neighbour is exactly one step away
    for d in range(6):
        n = hexgrid.neighbor(2, 3, d)
        assert hexgrid.hex_distance(2, 3, *n) == 1
    # opposite directions lead back
    for d in range(6):
        assert hexgrid.neighbor(*hexgrid.neighbor(2, 3, d),
                                hexgrid.OPPOSITE_DIR[d]) == (2, 3)


def test_movement_rules():
    """Heights, ramps and bridges restrict ground movement (sec. 4, 7, 8)."""
    b = Board(12, 12)
    # tank: same-height land only
    assert b.passable((0, 0), (1, 0), VehicleKind.TANK)
    b.tiles[(1, 0)].height = 2
    assert not b.passable((0, 0), (1, 0), VehicleKind.TANK)
    assert b.passable((0, 0), (1, 0), VehicleKind.HELICOPTER)
    # hovercraft crosses water<->land only at height 1
    b.tiles[(2, 1)].height = 0
    assert not b.passable((1, 0), (2, 1), VehicleKind.HOVERCRAFT)
    b.tiles[(1, 0)].height = 1
    assert b.passable((1, 0), (2, 1), VehicleKind.HOVERCRAFT)
    assert not b.passable((1, 0), (2, 1), VehicleKind.TANK)
    # ramp joins opposite neighbours regardless of height (sec. 7)
    b2 = Board(12, 12)
    for t in b2.tiles.values():
        t.height = 1
    for t in [(5, 5), (5, 4), (6, 4), (7, 4), (8, 4)]:
        b2.tiles[t].height = 4
    a, p, hi = (4, 5), (5, 5), (5, 4)
    b2.set_ramp(p, a, hi)
    assert b2.tiles[p].height == 1
    assert b2.passable(a, p, VehicleKind.TANK)
    assert b2.passable(p, hi, VehicleKind.TANK)
    assert not b2.passable((4, 4), p, VehicleKind.TANK)  # off-axis blocked
    assert b2.passable((4, 4), p, VehicleKind.HELICOPTER)
    # bridge over water between equal height >= 3 land (sec. 8);
    # direction 1 = 90 deg = straight down a column
    b3 = Board(14, 14)
    for t in b3.tiles.values():
        t.height = 3
    run_tiles = [(5, 6), (5, 7)]
    for t in run_tiles:
        b3.tiles[t].height = 0
    b3.tiles[(4, 6)].height = 3
    bridge = b3.add_bridge((5, 5), (5, 8), 1)
    assert bridge is not None and bridge.fragments == run_tiles, bridge
    assert b3.passable((5, 5), run_tiles[0], VehicleKind.TANK)
    assert b3.passable(run_tiles[0], run_tiles[1], VehicleKind.TANK)
    assert b3.passable(run_tiles[1], (5, 8), VehicleKind.TANK)
    assert not b3.passable((4, 6), run_tiles[0], VehicleKind.TANK)
    assert b3.find_path((5, 5), (5, 8), VehicleKind.TANK) ==         [(5, 6), (5, 7), (5, 8)]
    # bridge needs height >= 3 on both ends
    b3.tiles[(7, 9)].height = 2
    b3.tiles[(7, 10)].height = 0
    b3.tiles[(7, 11)].height = 2
    assert b3.add_bridge((7, 9), (7, 11), 1) is None


def test_production_and_capture():
    """Base production, capture arithmetic, overcrowding (sec. 3, 4)."""
    b = Board(10, 10)
    game = make_game(b)
    base = Building(BuildingKind.BASE_TANK, 0, 2, 2, units=10.0)
    target = Building(BuildingKind.HEAL_TOWER, None, 6, 2, units=5.0)
    game.buildings = [base, target]
    game.building_at = {x.tile: x for x in game.buildings}
    run(game, 10.0)
    assert base.units == 15.0, base.units          # +5 after 10 s
    # capture: p > b  ->  owner changes, units = p - b
    assert game.try_send(0, base.tile, target.tile)
    run(game, 12.0)
    assert target.owner == 0 and abs(target.units - 10.0) < 1e-6
    # reinforcement: same owner adds up
    base.units = 10.0
    assert game.try_send(0, base.tile, target.tile)
    run(game, 12.0)
    assert abs(target.units - 20.0) < 1e-6, target.units
    # overcrowding kills 1/s above capacity (sec. 3)
    target.units = target.capacity + 5
    run(game, 2.0)
    assert abs(target.units - (target.capacity + 3)) < 1e-6, target.units


def test_repelled_attack():
    """p <= b: building keeps owner and loses p units (sec. 4)."""
    b = Board(10, 10)
    game = make_game(b)
    src = Building(BuildingKind.BASE_TANK, 0, 1, 1, units=10.0)
    dst = Building(BuildingKind.BASE_TANK, 1, 6, 1, units=30.0)
    game.buildings = [src, dst]
    game.building_at = {x.tile: x for x in game.buildings}
    assert game.try_send(0, src.tile, dst.tile)
    run(game, 9.0)      # arrival ~5 s; the base spawns only at t = 10 s
    assert dst.owner == 1
    assert abs(dst.units - 20.0) < 1e-6, dst.units


def test_combat():
    """Vehicles stop, duel with ceil(x/5) damage, winner resumes (sec. 9)."""
    b = Board(20, 20)
    game = make_game(b)
    v0 = Vehicle(VehicleKind.TANK, 0, 30.0, [], (100.0, 100.0))
    v1 = Vehicle(VehicleKind.TANK, 1, 10.0, [], (140.0, 100.0))
    game.vehicles = [v0, v1]
    run(game, 1.5)
    assert v0.combat_target is v1 and v1.combat_target is v0
    # v1 (10 units) hits for ceil(10/5)=2, v0 (30) hits for 6
    assert abs(v0.units - (30.0 - 2.0)) < 1e-6, v0.units
    assert abs(v1.units - (10.0 - 6.0)) < 1e-6, v1.units
    run(game, 3.0)
    assert v1.dead and not v0.dead          # v1 loses 6/s and dies first
    assert v0.combat_target is None         # duel over -> resumes


def test_joiner_no_retaliation():
    """A third vehicle joins a duel without receiving fire (sec. 9)."""
    b = Board(20, 20)
    game = make_game(b)
    v0 = Vehicle(VehicleKind.TANK, 0, 40.0, [], (100.0, 100.0))
    v1 = Vehicle(VehicleKind.TANK, 1, 40.0, [], (130.0, 100.0))
    v2 = Vehicle(VehicleKind.TANK, 0, 20.0, [], (145.0, 105.0))
    game.vehicles = [v0, v1, v2]
    run(game, 0.4)
    assert v0.combat_target is v1 and v1.combat_target is v0
    assert v2.combat_target is v1            # joiner attacks nearest enemy
    run(game, 0.7)                            # shots fire at t = 1.0 s
    assert v2.units == 20.0                   # v1 never answered the joiner
    assert v1.units < 40.0 - 8.0              # both opponents hit v1


def test_turrets():
    """Turret range, damage x and neutral turrets (sec. 10, 12)."""
    b = Board(20, 20)
    game = make_game(b)
    t = Building(BuildingKind.TURRET_NORMAL, None, 5, 5, units=10.0)
    game.buildings = [t]
    game.building_at = {t.tile: t}
    v = Vehicle(VehicleKind.TANK, 0, 50.0, [],
                (t.pos[0] + 100.0, t.pos[1]))
    game.vehicles = [v]
    run(game, 5.5)
    assert abs(v.units - 40.0) < 1e-6, v.units   # 10 dmg after 5 s
    # out of range (250 j): no damage
    v2 = Vehicle(VehicleKind.TANK, 0, 50.0, [],
                 (t.pos[0] + 400.0, t.pos[1]))
    game.vehicles = [v2]
    run(game, 5.0)
    assert v2.units == 50.0
    # rapid turret: ceil(x/4) every second
    t2 = Building(BuildingKind.TURRET_RAPID, 1, 8, 5, units=12.0)
    game.buildings.append(t2)
    game.building_at[t2.tile] = t2
    v3 = Vehicle(VehicleKind.TANK, 0, 40.0, [], (t2.pos[0] + 100, t2.pos[1]))
    game.vehicles = [v3]
    run(game, 3.0)
    assert abs(v3.units - (40.0 - 3 * 3)) < 1e-6, v3.units  # ceil(12/4)=3


def test_heal_tower_and_buffer():
    """Healing tower +2/3s within 15x; buffer 0.5/s within 160 j."""
    b = Board(20, 20)
    game = make_game(b)
    tower = Building(BuildingKind.HEAL_TOWER, 0, 5, 5, units=2.0)
    game.buildings = [tower]
    game.building_at = {tower.tile: tower}
    v = Vehicle(VehicleKind.TANK, 0, 10.0, [], (tower.pos[0] + 25,
                                                tower.pos[1]))
    game.vehicles = [v]
    run(game, 3.2)
    assert v.units >= 12.0, v.units             # +2 after the first pulse
    # buffer heals friendly vehicles nearby (sec. 5.4)
    buf = Vehicle(VehicleKind.BUFFER, 0, 30.0, [], (v.x + 100, v.y))
    game.vehicles = [buf, v]
    v.units = 10.0
    run(game, 4.0)
    # 0.5/s * 4 s from the buffer + 2 from the tower pulse at t = 6 s
    assert abs(v.units - 14.0) < 1e-6, v.units


def test_wall_mine_traps():
    """Walls block, mines explode once, fire trap stays (sec. 1, 4)."""
    b = Board(20, 12)
    game = make_game(b)
    src = Building(BuildingKind.BASE_TANK, 0, 1, 5, units=50.0)
    dst = Building(BuildingKind.TURRET_NORMAL, None, 12, 5, units=10.0)
    game.buildings = [src, dst]
    game.building_at = {src.tile: src, dst.tile: dst}
    path = b.find_path(src.tile, dst.tile, VehicleKind.TANK)
    wall_tile, mine_tile, fire_tile = path[3], path[7], path[10]
    b.tiles[wall_tile].obstacle = Obstacle(Obstacle.WALL)
    b.tiles[mine_tile].obstacle = Obstacle(Obstacle.MINE)
    b.tiles[fire_tile].obstacle = Obstacle(Obstacle.TRAP_FIRE)
    assert game.try_send(0, src.tile, dst.tile)
    v = game.vehicles[0]
    run(game, 1.0)
    # vehicle stopped before the wall
    assert v.route_index <= 3, v.route_index
    run(game, 25.0)                              # 20 s to smash the wall
    assert b.tiles[wall_tile].obstacle is None
    run(game, 15.0)
    assert v.dead or v.route_index >= len(v.route)
    assert b.tiles[mine_tile].obstacle is None      # mine exploded once
    assert b.tiles[fire_tile].obstacle is not None  # fire trap remains


def test_ice_trap_stays():
    """Ice trap slows but is never removed (sec. 4)."""
    b = Board(20, 12)
    game = make_game(b)
    src = Building(BuildingKind.BASE_TANK, 0, 1, 5, units=30.0)
    dst = Building(BuildingKind.BASE_TANK, 0, 12, 5, units=0.0)
    game.buildings = [src, dst]
    game.building_at = {src.tile: src, dst.tile: dst}
    path = b.find_path(src.tile, dst.tile, VehicleKind.TANK)
    b.tiles[path[2]].obstacle = Obstacle(Obstacle.TRAP_ICE)
    assert game.try_send(0, src.tile, dst.tile)
    run(game, 30.0)
    assert b.tiles[path[2]].obstacle.kind == Obstacle.TRAP_ICE
    # 30 delivered + 3 production cycles (+5 each) in the target base
    assert dst.units == 45.0, dst.units          # arrived despite the slow


def test_elimination_and_victory():
    """A player with no buildings and no vehicles is out (sec. 2)."""
    b = Board(10, 10)
    game = make_game(b)
    base = Building(BuildingKind.BASE_TANK, 0, 2, 2, units=10.0)
    game.buildings = [base]
    game.building_at = {base.tile: base}
    assert not game.over
    base.owner = None
    game._check_elimination()
    assert game.human_id in game.eliminated
    assert game.over and game.winner == "ai"


def test_ai_sends_units():
    """AI decides on its interval and sends vehicles (sec. 13)."""
    from war_regions.levels import build_level, LEVELS
    from war_regions.ai import AIController, AI_DIFFICULTIES
    game = build_level(LEVELS[0])
    base = next(b for b in game.buildings if b.owner == 1)
    base.units = 90.0                 # enough to survive en-route turret fire
    ai = AIController(game, 1, AI_DIFFICULTIES["normal"], 42)
    sent = 0
    for _ in range(int(12 / C.SIM_DT)):
        ai.update(C.SIM_DT)
        sent = max(sent, len(game.vehicles))
    assert sent >= 1, "AI never sent a vehicle"


def test_full_sims():
    """Two minutes of every level with AI - no crashes, sane state."""
    from war_regions.levels import build_level, LEVELS
    from war_regions.ai import AIController, AI_DIFFICULTIES
    for cfg in LEVELS:
        game = build_level(cfg)
        ais = [AIController(game, p.id, AI_DIFFICULTIES[cfg.ai_difficulty],
                            cfg.seed * 31 + p.id)
               for p in game.players if not p.is_human]
        for _ in range(int(120 / C.SIM_DT)):
            game.update(C.SIM_DT)
            for ai in ais:
                ai.update(C.SIM_DT)
        total = sum(b.units for b in game.buildings) + \
            sum(v.units for v in game.vehicles)
        assert total >= 0
        for v in game.vehicles:
            assert v.units > 0, "dead vehicle still in the list"


TESTS = [test_hex_math, test_movement_rules, test_production_and_capture,
         test_repelled_attack, test_combat, test_joiner_no_retaliation,
         test_turrets, test_heal_tower_and_buffer, test_wall_mine_traps,
         test_ice_trap_stays, test_elimination_and_victory,
         test_ai_sends_units, test_full_sims]

if __name__ == "__main__":
    failures = 0
    for t in TESTS:
        try:
            t()
            print(f"OK   {t.__name__}")
        except AssertionError as exc:
            failures += 1
            print(f"FAIL {t.__name__}: {exc}")
    print()
    if failures:
        print(f"{failures} test group(s) FAILED")
        sys.exit(1)
    print(f"All {len(TESTS)} test groups passed.")
