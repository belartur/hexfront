"""Real-time game simulation (rules.md sections 2-12).

The :class:`Game` object owns the board, the players, all buildings and
vehicles and advances the whole simulation in fixed time steps.  It knows
nothing about pygame - rendering and input live in other modules.
"""

import math

from . import constants as C
from .constants import VehicleKind
from .board import Obstacle
from .entities import (Building, BuildingKind, Player, Vehicle,
                       is_base, turret_kind_of, vehicle_kind_of)


def _dist(a: tuple, b: tuple) -> float:
    """Euclidean distance between two world points (rules.md sec. 9)."""
    return math.hypot(a[0] - b[0], a[1] - b[1])


class Game:
    """Full state of one running match."""

    def __init__(self, board, players: list, buildings: list, seed: int = 0):
        self.board = board
        self.players = players
        self.buildings = buildings
        self.building_at = {b.tile: b for b in buildings}
        self.vehicles = []
        self.projectiles = []     # purely visual shots: dicts from/to/t/dur
        self.time = 0.0
        self.over = False
        self.winner = None        # "human" or "ai"
        self.human_id = next(p.id for p in players if p.is_human)
        self.eliminated = set()

    # ------------------------------------------------------------------
    # Queries used by the UI and the AI
    # ------------------------------------------------------------------
    def human_player(self) -> Player:
        """The human player."""
        return self.players[self.human_id]

    def buildings_owned_by(self, player_id):
        """All buildings currently owned by ``player_id``."""
        return [b for b in self.buildings if b.owner == player_id]

    def building_at_tile(self, tile):
        """Building standing on ``tile`` or ``None``."""
        return self.building_at.get(tile)

    # ------------------------------------------------------------------
    # Sending vehicles (rules.md sec. 6)
    # ------------------------------------------------------------------
    def try_send(self, owner: int, src_tile: tuple, dst_tile: tuple) -> bool:
        """Send one vehicle from ``src_tile`` to ``dst_tile``.

        Returns True on success.  The vehicle takes *all* units from the
        source building (sec. 4); when no road exists nothing happens.
        """
        src = self.building_at.get(src_tile)
        dst = self.building_at.get(dst_tile)
        if src is None or dst is None or src is dst:
            return False
        if src.owner != owner or src.units <= 0:
            return False
        kind = vehicle_kind_of(src.kind)
        route = self.board.find_path(src.tile, dst.tile, kind)
        if route is None:
            return False
        vehicle = Vehicle(kind, owner, src.units, route, src.pos)
        src.units = 0.0
        # Sending a vehicle never shows a floating -x number (spec sec.
        # "Grafika"), so drop any accumulated loss.
        src.loss_acc = 0.0
        self.vehicles.append(vehicle)
        return True

    # ------------------------------------------------------------------
    # Main step
    # ------------------------------------------------------------------
    def update(self, dt: float) -> None:
        """Advance the simulation by ``dt`` seconds (fixed SIM_DT)."""
        self.time += dt
        self._update_buildings(dt)
        self._update_turrets(dt)
        self._update_heal_towers(dt)
        self._update_buffers(dt)
        self._update_vehicles(dt)
        self._update_projectiles(dt)
        self._check_elimination()

    # ------------------------------------------------------------------
    # Buildings: production + overcrowding (rules.md sec. 3)
    # ------------------------------------------------------------------
    def _update_buildings(self, dt: float) -> None:
        for b in self.buildings:
            self._flush_texts(b, dt)
            if is_base(b.kind) and b.owner is not None:
                if b.units < b.capacity:
                    b.production_timer += dt
                    if b.production_timer >= C.BASE_SPAWN_INTERVAL:
                        b.production_timer -= C.BASE_SPAWN_INTERVAL
                        b.units = min(b.capacity,
                                      b.units + C.BASE_SPAWN_AMOUNT)
                else:
                    b.production_timer = 0.0
            if b.units > b.capacity:
                died = min(b.units - b.capacity,
                           C.OVERCROWD_DEATH_RATE * dt)
                b.units -= died
                b.loss_acc += died

    # ------------------------------------------------------------------
    # Turrets (rules.md sec. 10, 12)
    # ------------------------------------------------------------------
    def _update_turrets(self, dt: float) -> None:
        for b in self.buildings:
            tk = turret_kind_of(b.kind)
            if tk is None or b.units < 1:
                continue
            stats = C.TURRET_STATS[tk]
            b.fire_timer += dt
            if b.fire_timer < stats["cooldown"]:
                continue
            target = self._turret_target(b, stats["range"])
            if target is None:
                continue
            b.fire_timer = 0.0
            b.last_target_pos = target.pos
            dmg = math.ceil(b.units / stats["damage_div"])
            self._damage_vehicle(target, dmg)
            if stats["splash"] > 0:  # rocket turret hits nearby enemies too
                for v in self.vehicles:
                    if (not v.dead and v is not target and
                            v.owner != b.owner and
                            _dist(v.pos, target.pos) <= stats["splash"]):
                        self._damage_vehicle(v, dmg)
            self.projectiles.append({
                "from": b.pos, "to": target.pos, "t": 0.0,
                "dur": max(0.12, _dist(b.pos, target.pos) / 900.0),
                "color": (C.NEUTRAL_COLOR if b.owner is None
                          else C.PLAYER_COLORS[b.owner % 4]),
                "rocket": tk == C.TurretKind.ROCKET,
            })

    def _turret_target(self, b: Building, rng: float):
        """Nearest valid target of a turret, or ``None``.

        Owned turrets shoot at enemy vehicles; neutral turrets shoot at
        *every* vehicle (rules.md sec. 12).  Ties break by vehicle id so
        behaviour stays deterministic.
        """
        best, best_d = None, rng
        for v in self.vehicles:
            if v.dead or (b.owner is not None and v.owner == b.owner):
                continue
            d = _dist(b.pos, v.pos)
            if d <= best_d or (d == best_d and best is not None
                               and v.id < best.id):
                best, best_d = v, d
        return best

    # ------------------------------------------------------------------
    # Healing towers (rules.md sec. 11, 12)
    # ------------------------------------------------------------------
    def _update_heal_towers(self, dt: float) -> None:
        for b in self.buildings:
            if b.kind != BuildingKind.HEAL_TOWER or b.owner is None:
                continue
            b.heal_timer += dt
            if b.heal_timer < C.HEAL_TOWER_INTERVAL or b.units < 1:
                continue
            b.heal_timer = 0.0
            rng = C.HEAL_TOWER_RANGE_PER_UNIT * b.units
            for v in self.vehicles:
                if not v.dead and v.owner == b.owner and \
                        _dist(b.pos, v.pos) <= rng:
                    self._heal_vehicle(v, C.HEAL_TOWER_AMOUNT)

    # ------------------------------------------------------------------
    # Buffer healing auras (rules.md sec. 5.4)
    # ------------------------------------------------------------------
    def _update_buffers(self, dt: float) -> None:
        for buf in self.vehicles:
            if buf.dead or buf.kind != VehicleKind.BUFFER:
                continue
            for v in self.vehicles:
                if v.dead or v is buf or v.owner != buf.owner:
                    continue
                if _dist(buf.pos, v.pos) <= C.BUFFER_HEAL_RADIUS:
                    self._heal_vehicle(v, C.BUFFER_HEAL_RATE * dt)

    # ------------------------------------------------------------------
    # Vehicles: combat, obstacles, movement, arrival (rules.md sec. 4, 9)
    # ------------------------------------------------------------------
    def _update_vehicles(self, dt: float) -> None:
        board = self.board
        for v in self.vehicles:
            if v.dead:
                continue
            self._flush_texts(v, dt)

            # --- resolve dead targets (sec. 9) --------------------------
            if v.combat_target is not None and v.combat_target.dead:
                opponent = v.combat_target.last_opponent
                v.combat_target = None
                # A joiner that destroyed its target keeps attacking the
                # remaining member of the original pair while it fights.
                if (opponent is not None and not opponent.dead and
                        opponent.combat_target is not None):
                    v.combat_target = opponent

            # --- active duel: stand still and shoot ----------------------
            if v.combat_target is not None:
                v.fire_timer += dt
                if v.fire_timer >= C.FIRE_INTERVAL:
                    v.fire_timer -= C.FIRE_INTERVAL
                    v.last_opponent = v.combat_target
                    self._damage_vehicle(
                        v.combat_target, math.ceil(v.units / 5.0))
                continue

            # --- detection (sec. 9) --------------------------------------
            enemy = self._nearest_enemy(v)
            if enemy is not None:
                v.combat_target = enemy
                v.last_opponent = enemy
                enemy.last_opponent = v
                if enemy.combat_target is None:
                    # The detected pair stops and fights each other; a
                    # vehicle already fighting does *not* retaliate against
                    # a joiner (sec. 9).
                    enemy.combat_target = v
                continue

            # --- ground obstacles (sec. 1, 4) ----------------------------
            tile = board.world_to_tile(v.x, v.y)
            obstacle = board.tiles[tile].obstacle if tile else None
            if v.kind != VehicleKind.HELICOPTER:
                if obstacle is not None and obstacle.kind == Obstacle.WALL:
                    v.wall_timer += dt
                    if v.wall_timer >= C.WALL_ATTACK_INTERVAL:
                        v.wall_timer -= C.WALL_ATTACK_INTERVAL
                        obstacle.hp -= C.WALL_ATTACK_DAMAGE
                        if obstacle.hp <= 0:
                            board.tiles[tile].obstacle = None
                    continue                    # stopped, shooting the wall
                if obstacle is not None and obstacle.kind == Obstacle.TRAP_FIRE:
                    self._damage_vehicle(v, C.FIRE_TRAP_DPS * dt)
                    if v.dead:
                        continue

            # --- movement (sec. 4, 5) ------------------------------------
            if not v.route or v.route_index >= len(v.route):
                continue                        # nothing to travel towards
            speed = C.VEHICLE_SPEED[v.kind]
            if (v.kind != VehicleKind.HELICOPTER and obstacle is not None
                    and obstacle.kind == Obstacle.TRAP_ICE):
                speed *= C.ICE_TRAP_SLOWDOWN
            step = speed * dt
            wp_tile = v.route[v.route_index]
            wx, wy = board.center_world(wp_tile)
            dx, dy = wx - v.x, wy - v.y
            d = math.hypot(dx, dy)
            if d <= step:
                v.x, v.y = wx, wy
                self._check_mine(wp_tile)
                v.route_index += 1
                if v.route_index >= len(v.route):
                    self._arrive(v)
                    continue
            else:
                v.x += dx / d * step
                v.y += dy / d * step
                self._check_mine(board.world_to_tile(v.x, v.y))
        self.vehicles = [v for v in self.vehicles if not v.dead]

    def _check_mine(self, tile) -> None:
        """Explode a mine when a ground vehicle reaches the tile centre.

        Helicopters ignore every obstacle, mines included (sec. 5.2).
        """
        if tile is None:
            return
        t = self.board.tiles[tile]
        if t.obstacle is None:
            return
        if t.obstacle.kind not in (Obstacle.MINE, Obstacle.MINE_WATER):
            return
        cx, cy = self.board.center_world(tile)
        for v in self.vehicles:
            if (not v.dead and v.kind != VehicleKind.HELICOPTER
                    and math.hypot(v.x - cx, v.y - cy)
                    <= C.MINE_TRIGGER_RADIUS):
                self._damage_vehicle(v, C.MINE_DAMAGE)
                t.obstacle = None      # a mine explodes only once (sec. 4)
                return

    def _nearest_enemy(self, v: Vehicle):
        """Nearest enemy within detection radius (rules.md sec. 9)."""
        best, best_d = None, C.DETECTION_RADIUS
        for e in self.vehicles:
            if e.dead or e.owner == v.owner:
                continue
            d = _dist(v.pos, e.pos)
            if d < best_d or (d == best_d and best is not None
                              and e.id < best.id):
                best, best_d = e, d
        return best

    def _arrive(self, v: Vehicle) -> None:
        """Resolve a vehicle reaching its destination building (sec. 4)."""
        v.dead = True
        b = self.building_at.get(v.dest_tile)
        if b is None:
            return
        p = v.units
        if b.owner == v.owner:
            b.units += p                       # friendly reinforcement
        elif p > b.units:
            b.owner = v.owner                  # captured
            b.units = p - b.units
        else:
            b.units -= p                       # attack repelled
            b.loss_acc += p                    # shows a floating -x

    # ------------------------------------------------------------------
    # Damage / healing / floating text
    # ------------------------------------------------------------------
    def _damage_vehicle(self, v: Vehicle, amount: float) -> None:
        if v.dead or amount <= 0:
            return
        v.units -= amount
        v.loss_acc += amount
        if v.units <= 0:
            v.units = 0.0
            v.dead = True                      # vehicle vanishes (sec. 4)

    def _heal_vehicle(self, v: Vehicle, amount: float) -> None:
        if v.dead or amount <= 0:
            return
        v.units += amount
        v.gain_acc += amount

    def _flush_texts(self, ent, dt: float) -> None:
        """Aggregate continuous losses/gains into floating numbers.

        A number is pushed as soon as at least one full unit has been
        accumulated, so damage shows on the very frame it happens; the
        periodic 1 s timer remains only as a fallback that flushes the
        leftover fraction of slow gains (e.g. healing).  Entries older
        than FLOAT_TEXT_LIFETIME are dropped.
        """
        ent.text_timer += dt
        for t in ent.texts:
            t[1] += dt
        if ent.texts:
            ent.texts = [t for t in ent.texts
                         if t[1] < C.FLOAT_TEXT_LIFETIME]
        flush = ent.loss_acc >= 1.0 or ent.gain_acc >= 1.0
        if ent.text_timer >= 1.0:
            ent.text_timer -= 1.0
            flush = True
        if flush:
            loss = round(ent.loss_acc)
            gain = round(ent.gain_acc)
            ent.loss_acc -= loss
            ent.gain_acc -= gain
            if loss > 0:
                ent.texts.append([-loss, 0.0])
            if gain > 0:
                ent.texts.append([gain, 0.0])

    def _update_projectiles(self, dt: float) -> None:
        """Age the purely visual turret shots."""
        for p in self.projectiles:
            p["t"] += dt
        self.projectiles = [p for p in self.projectiles
                            if p["t"] < p["dur"]]

    # ------------------------------------------------------------------
    # Win / elimination (rules.md sec. 2)
    # ------------------------------------------------------------------
    def _check_elimination(self) -> None:
        if self.over:
            return
        for p in self.players:
            if p.id in self.eliminated:
                continue
            has_building = any(b.owner == p.id for b in self.buildings)
            has_vehicle = any(v.owner == p.id and not v.dead
                              for v in self.vehicles)
            if not has_building and not has_vehicle:
                p.eliminated = True
                self.eliminated.add(p.id)
        enemies_alive = [pl.id for pl in self.players
                         if pl.id != self.human_id
                         and pl.id not in self.eliminated]
        if self.human_id in self.eliminated:
            self.over, self.winner = True, "ai"
        elif not enemies_alive:
            self.over, self.winner = True, "human"
