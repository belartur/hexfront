"""Map file format and editor tests (headless, dummy video driver).

Run with:  python3 -m tests.test_mapfile
"""

import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
os.environ.setdefault("SDL_VIDEODRIVER", "dummy")

import pygame                                                    # noqa: E402
from war_regions import mapfile                                  # noqa: E402
from war_regions.board import Board, Obstacle                    # noqa: E402
from war_regions.constants import VehicleKind                    # noqa: E402
from war_regions.entities import Building, BuildingKind          # noqa: E402


def sample_board():
    """A small board exercising every feature of the map format."""
    board = Board(8, 6)
    for t in board.tiles.values():
        t.height = 2
    # water and heights (odd tile count -> padding nibble)
    board.tiles[(0, 0)].height = 0
    board.tiles[(7, 5)].height = 7
    # ramp along axis 0 (sec. 7); (2, 2) and (4, 3) are the opposite
    # neighbours of (3, 2) (odd column: dir 3 and dir 0)
    board.set_ramp((3, 2), (4, 3), (2, 2))
    # bridge over water straight down column 5 between height-3 ends
    # (sec. 8: direction 1 steps (0, +1) in even columns)
    board.tiles[(5, 2)].height = 3
    board.tiles[(5, 5)].height = 3
    board.tiles[(5, 3)].height = 0
    board.tiles[(5, 4)].height = 0
    assert board.add_bridge((5, 2), (5, 5), 1) is not None
    # obstacles on their legal terrain (rules sec. 1)
    board.tiles[(5, 0)].obstacle = Obstacle(Obstacle.WALL)
    board.tiles[(6, 0)].obstacle = Obstacle(Obstacle.TRAP_ICE)
    board.tiles[(0, 0)].obstacle = Obstacle(Obstacle.MINE_WATER)
    board.tiles[(6, 5)].obstacle = Obstacle(Obstacle.TRAP_FIRE)
    board.tiles[(5, 5)].obstacle = Obstacle(Obstacle.MINE)
    return board


def sample_buildings():
    """Buildings of every owner, including a neutral one."""
    return [
        Building(BuildingKind.BASE_TANK, 0, 1, 1, units=20),
        Building(BuildingKind.TURRET_ROCKET, 1, 4, 1, units=15),
        Building(BuildingKind.HEAL_TOWER, None, 6, 1, units=10),
        Building(BuildingKind.BASE_HELICOPTER, 3, 7, 0, units=5),
    ]


def test_round_trip():
    """save_map/load_board preserve heights, objects and buildings."""
    board, buildings = sample_board(), sample_buildings()
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "test.map")
        mapfile.save_map(path, board, buildings)
        # 4 header bytes + ceil(48/2) height bytes + 4 building records
        # (4+1+2 B each) + 1 ramp + 2 bridge fragments + 5 obstacles
        # (4+1 B each)  (specification: "Format pliku planszy")
        assert os.path.getsize(path) == 4 + 24 + 4 * 7 + 8 * 5

        loaded_board, loaded_buildings = mapfile.load_board(path)
    assert loaded_board.cols == 8 and loaded_board.rows == 6
    for tile, t in board.tiles.items():
        lt = loaded_board.tiles[tile]
        assert lt.height == t.height, tile
        assert (lt.obstacle.kind if lt.obstacle else None) == \
            (t.obstacle.kind if t.obstacle else None), tile
    # ramp survives with its ends and the enforced min height
    lr = loaded_board.tiles[(3, 2)].ramp
    assert lr is not None and set(lr) == {(4, 3), (2, 2)}
    assert loaded_board.tiles[(3, 2)].height == 2
    # bridge survives as a whole object with the same passable pairs
    lb = loaded_board.tiles[(5, 3)].bridge
    assert lb is not None and lb.w == 3 and lb.direction == 1
    assert loaded_board.passable((5, 2), (5, 3), VehicleKind.TANK)
    assert loaded_board.passable((5, 4), (5, 5), VehicleKind.TANK)
    assert not loaded_board.passable((4, 3), (5, 3), VehicleKind.TANK)
    # buildings survive with kind, owner and units
    by_tile = {b.tile: b for b in loaded_buildings}
    assert len(loaded_buildings) == 4
    assert by_tile[(1, 1)].kind == BuildingKind.BASE_TANK
    assert by_tile[(1, 1)].owner == 0 and by_tile[(1, 1)].units == 20
    assert by_tile[(4, 1)].owner == 1
    assert by_tile[(6, 1)].owner is None
    assert by_tile[(7, 0)].owner == 3


def test_load_game():
    """load_game wires owners into players, human = blue = id 0."""
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "owners.map")
        mapfile.save_map(path, sample_board(), sample_buildings())
        game = mapfile.load_game(path)
    assert game.human_id == 0
    assert [p.id for p in game.players] == [0, 1, 2, 3]
    assert game.players[0].is_human and not game.players[1].is_human
    assert game.building_at_tile((4, 1)).owner == 1
    assert len(game.building_at) == 4
    game.update(0.1)               # a loaded map simulates without errors
    assert game.time > 0.0


def test_list_maps_and_seed():
    """list_maps finds .map files; the seed only depends on the name."""
    with tempfile.TemporaryDirectory() as tmp:
        open(os.path.join(tmp, "b.map"), "wb").close()
        open(os.path.join(tmp, "a.map"), "wb").close()
        open(os.path.join(tmp, "c.txt"), "wb").close()
        maps = mapfile.list_maps(tmp)
        assert [os.path.basename(m) for m in maps] == ["a.map", "b.map"]
    assert mapfile.level_seed("maps/x.map") == \
        mapfile.level_seed("other/dir/x.map")


def test_editor_tools():
    """Editor tools build a rules-conforming board and save/load it."""
    from editor import (Editor, EditorScene, TOOL_BRIDGE, TOOL_BUILDING,
                        TOOL_ERASE, TOOL_RAMP, TOOL_RAISE)

    ed = Editor()
    ed.scene = EditorScene(Board(10, 8), [])
    for t in ed.scene.board.tiles.values():
        t.height = 1

    # terrain tool (and protected occupied tiles)
    ed.tool, ed.scene.board.tiles[(2, 2)].height = TOOL_RAISE, 0
    ed._apply_tool((2, 2))
    assert ed.scene.board.height((2, 2)) == 1

    # buildings only on land
    ed.tool = TOOL_BUILDING
    ed.scene.board.tiles[(1, 1)].height = 0
    ed._apply_tool((1, 1))
    assert not ed.scene.buildings
    ed._apply_tool((2, 1))
    assert len(ed.scene.buildings) == 1

    # ramp requires both opposite neighbours on the board
    ed.tool = TOOL_RAMP
    ed._apply_tool((0, 0))          # axis-0 neighbour lies off the board
    assert ed.scene.board.tiles[(0, 0)].ramp is None
    ed.scene.board.tiles[(4, 4)].height = 3
    ed.scene.board.tiles[(6, 5)].height = 3
    ed._apply_tool((5, 4))
    assert ed.scene.board.tiles[(5, 4)].ramp is not None
    assert ed.scene.board.height((5, 4)) == 3

    # bridge: fragment between equal high ends forms a real bridge
    # (odd column axis 0: the opposite neighbours of (3, 6) are
    # (2, 6) and (4, 7))
    ed.tool = TOOL_BRIDGE
    ed.scene.board.tiles[(2, 6)].height = 3
    ed.scene.board.tiles[(4, 7)].height = 3
    ed.scene.board.tiles[(3, 6)].height = 0
    ed._apply_tool((3, 6))
    assert ed.scene.board.tiles[(3, 6)].bridge is not None

    # invalid bridge fragment (low ends) is rejected
    ed._apply_tool((8, 6))
    assert ed.scene.board.tiles[(8, 6)].bridge is None

    # erase removes buildings (and bridge fragments)
    ed.tool = TOOL_ERASE
    ed._apply_tool((2, 1))
    assert not ed.scene.buildings

    # save + load round trip through the editor itself
    ed.tool = TOOL_BUILDING
    ed._apply_tool((1, 5))              # one building for the round trip
    cwd = os.getcwd()
    try:
        os.chdir(tempfile.mkdtemp())
        ed._save("editor_test")
        assert os.path.isfile("maps/editor_test.map")
        ed2 = Editor()
        ed2._load("editor_test")
        assert ed2.scene.board.cols == 10
        assert ed2.scene.board.rows == 8
        assert len(ed2.scene.buildings) == 1
        assert ed2.bridge_marks
    finally:
        os.chdir(cwd)
    pygame.quit()


TESTS = [test_round_trip, test_load_game, test_list_maps_and_seed,
         test_editor_tools]

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
