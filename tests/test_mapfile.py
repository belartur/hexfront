"""Map file format and editor tests (headless, dummy video driver).

Run with:  python3 -m tests.test_mapfile
"""

import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
os.environ.setdefault("SDL_VIDEODRIVER", "dummy")

import pygame                                                    # noqa: E402
from hexfront import mapfile                                  # noqa: E402
from hexfront.board import Board, Obstacle                    # noqa: E402
from hexfront.camera import Camera                            # noqa: E402
from hexfront.constants import VehicleKind                    # noqa: E402
from hexfront.constants import (                              # noqa: E402
    EDITOR_DIGIT_COMMIT_DELAY, EDITOR_LAND_HEIGHT, EDITOR_LAND_SIZE,
    EDITOR_NEW_SIZE)
from hexfront.entities import Building, BuildingKind          # noqa: E402
from editor import Editor, EditorScene, pad_map, trim_map        # noqa: E402


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
        # 2 header bytes + ceil(48/2) height bytes + 4 building records
        # (2+1+2 B each) + 1 ramp + 2 bridge fragments + 5 obstacles
        # (2+1 B each)  (specification: "Format pliku planszy")
        assert os.path.getsize(path) == 2 + 24 + 4 * 5 + 8 * 3

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


def test_unit_count_and_owner_bits():
    """Units 0-999 ride the 10 low bits, the owner the 6 high ones."""
    board = Board(2, 1)
    for t in board.tiles.values():
        t.height = 1
    buildings = [
        Building(BuildingKind.BASE_TANK, 3, 0, 0, units=999),
        Building(BuildingKind.BASE_TANK, None, 1, 0, units=1500),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "bits.map")
        mapfile.save_map(path, board, buildings)
        # Building record at offset 3 (2 header bytes + 1 height byte):
        # 2 coord bytes + 1 type + 2 property bytes.  Owner 3 -> code 4
        # = 0b000100, units 999 = 0b1111100111, so the 16-bit word is
        # 0b000100_1111100111 = 0x13E7.
        data = open(path, "rb").read()
        assert data[6:8] == b"\x13\xe7"
        loaded_board, loaded = mapfile.load_board(path)
    by_tile = {b.tile: b for b in loaded}
    assert by_tile[(0, 0)].units == 999 and by_tile[(0, 0)].owner == 3
    # 1500 does not fit the format: clamped to 999 on save
    assert by_tile[(1, 0)].units == 999 and by_tile[(1, 0)].owner is None


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


def test_editor_actions():
    """Editor key actions build a board and save/load it (editor spec)."""
    from editor import Editor, EditorScene

    ed = Editor()
    ed.scene = EditorScene(Board(10, 8), [])
    for t in ed.scene.board.tiles.values():
        t.height = 1
    board = ed.scene.board

    # b: place a building, again: cycle the kind
    ed._action_building((2, 2))
    b = ed._building_at((2, 2))
    assert b.kind == BuildingKind.BASE_TANK
    assert b.owner is None and b.units == 0.0
    ed._action_building((2, 2))
    assert ed._building_at((2, 2)).kind == BuildingKind.BASE_HELICOPTER

    # o: cycle the owner (no action without a building)
    ed._action_owner((2, 2))
    assert ed._building_at((2, 2)).owner == 0     # neutral -> blue
    ed._action_owner((1, 1))
    assert ed._building_at((1, 1)) is None

    # digits: three digits commit immediately; short entries need a delay
    ed._action_digit((2, 2), "1")
    ed._action_digit((2, 2), "2")
    assert ed._building_at((2, 2)).units == 0.0   # not committed yet
    ed._action_digit((2, 2), "3")
    assert ed._building_at((2, 2)).units == 123.0
    ed._action_digit((2, 2), "2")
    ed._digit_tick(EDITOR_DIGIT_COMMIT_DELAY)
    assert ed._building_at((2, 2)).units == 2.0
    ed._action_digit((2, 2), "9")
    ed._action_digit((2, 2), "9")
    ed._action_digit((2, 2), "9")
    assert ed._building_at((2, 2)).units == 999.0
    # pointing at another tile accepts the pending entry (editor spec)
    ed._action_digit((2, 2), "7")
    assert ed._building_at((2, 2)).units == 999.0  # not committed yet
    ed._action_digit((5, 5), "4")
    assert ed._building_at((2, 2)).units == 7.0   # accepted on re-point
    # digits on a tile without a building: no effect at all
    ed._action_digit((0, 0), "5")
    ed._digit_tick(EDITOR_DIGIT_COMMIT_DELAY)
    assert ed._building_at((0, 0)) is None

    # t: place an obstacle, again: cycle the kind; overwrites the building.
    # New obstacles reuse the remembered kind (editor spec).
    ed._action_obstacle((3, 3))
    assert board.tiles[(3, 3)].obstacle.kind == Obstacle.WALL
    ed._action_obstacle((3, 3))
    assert board.tiles[(3, 3)].obstacle.kind == Obstacle.MINE
    ed._action_obstacle((2, 2))
    assert ed._building_at((2, 2)) is None
    assert board.tiles[(2, 2)].obstacle.kind == Obstacle.MINE

    # r: a new ramp prefers the axis with differing opposite neighbours
    board.tiles[(4, 4)].height = 3                # (5, 4): dir0 -> (6, 5)
    ed._action_ramp((5, 4))
    ramp = board.tiles[(5, 4)].ramp
    assert ramp is not None and set(ramp) == {(6, 5), (4, 4)}
    assert board.height((5, 4)) == 1              # min of the ends (sec. 7)
    ed._action_ramp((5, 4))                       # again: rotate
    assert board.tiles[(5, 4)].ramp is not None \
        and set(board.tiles[(5, 4)].ramp) != {(6, 5), (4, 4)}

    # m: place a bridge fragment, again: rotate
    board.tiles[(2, 6)].height = 3                # (3, 6) odd col, axis 0:
    board.tiles[(4, 7)].height = 3                # (2, 6) and (4, 7)
    board.tiles[(3, 6)].height = 0
    ed._action_bridge((3, 6))
    assert board.tiles[(3, 6)].bridge.direction == 0
    ed._action_bridge((3, 6))
    assert board.tiles[(3, 6)].bridge.direction == 1
    # a neighbouring bridge directed at the tile imposes its axis
    ed._action_bridge((3, 7))                     # (3, 6) axis 1 -> (3, 7)
    assert board.tiles[(3, 7)].bridge.direction == 1
    ed._action_bridge((3, 6))
    assert board.tiles[(3, 6)].bridge.direction == 2

    # [ / ]: terrain stops at 0 and 15 (no wrap-around)
    ed._change_height((0, 0), -1)                 # 1 -> 0 (water)
    ed._change_height((0, 0), -1)                 # no-op at 0
    assert board.height((0, 0)) == 0
    ed._change_height((0, 0), +1)
    assert board.height((0, 0)) == 1

    # Del deletes the object but not the terrain
    ed._delete_object((3, 6))
    assert board.tiles[(3, 6)].bridge is None
    assert board.tiles[(3, 7)].bridge is not None  # remaining fragment

    # dirty tracking, save and load round trip; the saved file is
    # trimmed (this board is all land, so nothing is cut) and the loaded
    # board is padded up to the standard new map size (editor spec)
    assert ed.dirty
    cwd = os.getcwd()
    try:
        os.chdir(tempfile.mkdtemp())
        ed._save("editor_test")
        assert not ed.dirty and ed.map_name == "editor_test"
        expected_buildings = pad_map(*trim_map(board,
                                               ed.scene.buildings))[1]
        ed2 = Editor()
        ed2._load("editor_test")
        assert not ed2.dirty and ed2.map_name == "editor_test"
        assert (ed2.scene.board.cols, ed2.scene.board.rows) == \
            EDITOR_NEW_SIZE
        for exp in expected_buildings:
            got = ed2._building_at(exp.tile)
            assert got is not None, exp.tile
            assert got.kind == exp.kind and got.owner == exp.owner
            assert got.units == exp.units
        # the obstacles and the remaining bridge fragment survive, shifted
        obstacles = [t.obstacle for t in ed2.scene.board.tiles.values()
                     if t.obstacle is not None]
        assert [o.kind for o in obstacles] == [Obstacle.MINE, Obstacle.MINE]
        fragments = [t for t in ed2.scene.board.tiles.values()
                     if t.bridge is not None]
        assert len(fragments) == 1
        ed2._new_map()                  # ctrl+n starts the standard map
        assert not ed2.scene.buildings
        assert (ed2.scene.board.cols, ed2.scene.board.rows) == EDITOR_NEW_SIZE
        assert not ed2.map_name and not ed2.dirty
        lw, lh = EDITOR_LAND_SIZE
        mid = (ed2.scene.board.cols // 2, ed2.scene.board.rows // 2)
        assert ed2.scene.board.height(mid) == EDITOR_LAND_HEIGHT
        assert ed2.scene.board.height((0, 0)) == 0   # mostly water
        assert ed2.scene.board.height((mid[0] + lw // 2 + 1, mid[1])) == 0
    finally:
        os.chdir(cwd)
    pygame.quit()


def test_trim_and_pad():
    """trim_map cuts empty borders (min 1x1); pad_map pads back evenly."""
    board = Board(6, 5)
    for t in board.tiles.values():
        t.height = 0                           # all water
    board.tiles[(2, 2)].height = 3
    buildings = [Building(BuildingKind.BASE_TANK, 0, 2, 2, units=5)]
    tboard, tbuildings = trim_map(board, buildings)
    assert (tboard.cols, tboard.rows) == (1, 1)
    assert tboard.height((0, 0)) == 3
    assert tbuildings[0].tile == (0, 0) and tbuildings[0].units == 5
    assert tbuildings[0].owner == 0           # originals stay untouched
    assert buildings[0].tile == (2, 2)

    pboard, pbuildings = pad_map(tboard, tbuildings, size=(6, 5))
    assert (pboard.cols, pboard.rows) == (6, 5)
    # even padding: 5 extra columns -> 2 before, 3 after; 4 rows -> 2/2
    assert pbuildings[0].tile == (2, 2)
    assert pboard.height((2, 2)) == 3 and pboard.height((0, 0)) == 0

    # a fully empty board trims to the 1 x 1 minimum
    empty = Board(4, 4)
    for t in empty.tiles.values():
        t.height = 0
    eboard, ebuildings = trim_map(empty, [])
    assert (eboard.cols, eboard.rows) == (1, 1) and not ebuildings

    # objects are carried over: obstacle and ramp coordinates shift
    board2 = Board(5, 4)
    for t in board2.tiles.values():
        t.height = 0
    board2.tiles[(1, 1)].obstacle = Obstacle(Obstacle.WALL)
    board2.tiles[(2, 1)].height = 1
    board2.tiles[(4, 1)].height = 1
    board2.set_ramp((3, 1), (4, 1), (2, 1))
    b2, _ = trim_map(board2, [])
    assert b2.cols == 4 and b2.rows == 1      # only column 0 and rows 0/2-3
    assert b2.tiles[(0, 0)].obstacle.kind == Obstacle.WALL
    assert set(b2.tiles[(2, 0)].ramp) == {(3, 0), (1, 0)}
    assert b2.height((2, 0)) == 1             # min of the ends (sec. 7)


def test_editor_errors():
    """_errors lists the rule violations of the edited map (editor spec)."""
    ed = Editor()
    board = Board(6, 6)
    ed.scene = EditorScene(board, [])
    # an empty map lacks both bases
    assert ed._errors() == ["brak bazy gracza",
                            "brak bazy przynajmniej jednego przeciwnika"]

    # a building on water
    ed.scene.buildings.append(
        Building(BuildingKind.BASE_TANK, 0, 0, 0, units=5))
    board.tiles[(0, 0)].height = 0
    errors = ed._errors()
    assert any(e.startswith("budynek na wodzie (0, 0)") for e in errors)
    assert "brak bazy gracza" not in errors   # the blue base exists now
    assert "brak bazy przynajmniej jednego przeciwnika" in errors

    # ramp joining equal heights, then with a wrong tile height
    board.set_ramp((3, 2), (4, 3), (2, 2))    # all heights equal here
    assert any("tych samych wysokościach (3, 2)" in e for e in ed._errors())
    board.tiles[(3, 2)].height = 2            # wrong: must be min(a, b)
    assert any("innej wysokości niż niższe z łączonych pól (3, 2)" in e
               for e in ed._errors())

    # bridge over too-high land and joining different heights
    board.tiles[(5, 2)].height = 3
    board.tiles[(5, 5)].height = 3
    board.tiles[(5, 3)].height = 0
    board.tiles[(5, 4)].height = 0
    assert board.add_bridge((5, 2), (5, 5), 1) is not None
    board.tiles[(5, 3)].height = 2            # fragment must be < w - 2
    assert any("za wysokim lądem (5, 3)" in e for e in ed._errors())
    board.tiles[(5, 3)].height = 0
    board.tiles[(5, 5)].height = 4            # ends must share a height
    assert any("różnej wysokości (5, 2)-(5, 5)" in e for e in ed._errors())

    # an opponent base clears the last error
    ed.scene.buildings.append(
        Building(BuildingKind.BASE_TANK, 1, 2, 2, units=5))
    errors = ed._errors()
    assert "brak bazy przynajmniej jednego przeciwnika" not in errors
    pygame.quit()


def test_editor_ctrl_keys():
    """ctrl+s saves under the current name; ctrl+n starts the new map."""
    ed = Editor()
    ed.scene = EditorScene(Board(4, 4), [])
    for t in ed.scene.board.tiles.values():
        t.height = 1
    ed._action_building((1, 1))
    ed.map_name = "editor_ctrl"
    cwd = os.getcwd()
    try:
        os.chdir(tempfile.mkdtemp())
        pygame.key.set_mods(pygame.KMOD_CTRL)
        ed._key(pygame.event.Event(pygame.KEYDOWN, key=pygame.K_s))
        assert not ed.dirty
        assert os.path.isfile(os.path.join("maps", "editor_ctrl.map"))
        ed._action_building((2, 2))
        assert ed.dirty
        ed._key(pygame.event.Event(pygame.KEYDOWN, key=pygame.K_n))
        pygame.key.set_mods(0)
        assert not ed.dirty and ed.map_name is None
        assert (ed.scene.board.cols, ed.scene.board.rows) == EDITOR_NEW_SIZE
    finally:
        os.chdir(cwd)
        pygame.key.set_mods(0)
    pygame.quit()


def test_pick_tile_flat():
    """Board.pick_tile(flat=True) picks as if all heights were zero."""
    board = Board(3, 3)
    board.tiles[(1, 1)].height = 5
    camera = Camera((800, 600))
    camera.center_on_world(*board.center_world((1, 1)))
    # Screen centre: the ground-level projection of the tall tile.  The
    # flat pick reports (1, 1) itself, while the elevation-refined pick
    # follows the raised terrain.
    pos = camera.world_to_screen(*board.center_world((1, 1)))
    assert board.pick_tile(camera, pos, flat=True) == (1, 1)
    assert board.pick_tile(camera, pos) == (2, 2)


TESTS = [test_round_trip, test_unit_count_and_owner_bits, test_load_game,
         test_list_maps_and_seed, test_trim_and_pad, test_editor_actions,
         test_editor_errors, test_editor_ctrl_keys, test_pick_tile_flat]

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
