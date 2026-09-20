"""AI players (rules.md section 13).

Each AI player runs an independent decision loop (sec. 13.2) and, once
per interval, evaluates every (source, target) building pair with the
scoring formula from sec. 13.5::

    score = W1*chance + W2*value + W3*defence - W4*time
            - W5*danger - W6*source_risk

plus small security/evacuation adjustments and difficulty noise.  The
behaviour is fully deterministic for a given level seed (sec. 13.2).
"""

import math
import random

from . import constants as C
from .constants import AIDifficulty, AI_DIFFICULTIES
from .entities import (BuildingKind, is_base, is_turret, turret_kind_of,
                       vehicle_kind_of)


__all__ = ["AIController", "AIDifficulty", "AI_DIFFICULTIES"]


class AIController:
    """Decision loop of a single AI player."""

    def __init__(self, game, player_id: int, difficulty: AIDifficulty,
                 seed: int):
        self.game = game
        self.player_id = player_id
        self.diff = difficulty
        self.rng = random.Random(seed)
        #: Stagger the decision phases of different AI players (sec. 13.2).
        self.timer = 0.7 * player_id
        #: tile -> time at which the inbound threat was first noticed.
        self.threat_seen = {}

    # ------------------------------------------------------------------
    def update(self, dt: float) -> None:
        """Advance the decision loop; called every frame."""
        if self.game.over:
            return
        self.timer += dt
        if self.timer >= self.diff.interval:
            self.timer -= self.diff.interval
            self._decide()

    # ------------------------------------------------------------------
    # Information gathering (sec. 13.3, 13.4)
    # ------------------------------------------------------------------
    def _inbound_units(self, owner: int) -> dict:
        """Map destination tile -> total units of ``owner``'s vehicles."""
        result = {}
        for v in self.game.vehicles:
            if v.dead or v.owner != owner or not v.route:
                continue
            result[v.route[-1]] = result.get(v.route[-1], 0.0) + v.units
        return result

    def _visible_threats(self, raw: dict) -> dict:
        """Filter raw threats through the reaction delay (sec. 13.8)."""
        game_time = self.game.time
        visible = {}
        for tile, units in raw.items():
            first_seen = self.threat_seen.setdefault(tile, game_time)
            if game_time - first_seen >= self.diff.reaction_delay:
                visible[tile] = units
        return visible

    # ------------------------------------------------------------------
    # Scoring (sec. 13.5)
    # ------------------------------------------------------------------
    def _route_danger(self, path: list) -> float:
        """Danger of a route: hostile turret ranges + obstacles on it."""
        game = self.game
        board = game.board
        danger = 0.0
        for tile in path:
            pos = board.center_world(tile)
            hit = False
            for b in game.buildings:
                tk = turret_kind_of(b.kind)
                if tk is None or b.owner == self.player_id:
                    continue
                if math.hypot(b.pos[0] - pos[0],
                              b.pos[1] - pos[1]) <= C.TURRET_STATS[tk]["range"]:
                    danger += 1.0
                    hit = True
                    break
            if not hit:
                t = board.tiles[tile]
                if t.obstacle is not None:
                    danger += 0.5
        return danger / max(1, len(path))

    def _route_damage(self, path: list, speed: float) -> float:
        """Conservative en-route damage estimate from hostile turrets.

        A turret whose range circle covers any route tile keeps firing
        while the vehicle crosses it, so the number of incoming shots is
        estimated from the covered distance (at most the full route
        length) divided by the turret's fire period.  Used by the safety
        rules (sec. 13.6).
        """
        board = self.game.board
        route_length = board.path_world_length(path)
        damage = 0.0
        counted = set()
        for tile in path:
            pos = board.center_world(tile)
            for b in self.game.buildings:
                tk = turret_kind_of(b.kind)
                if (tk is None or b.owner == self.player_id
                        or b.units < 1 or b.tile in counted):
                    continue
                stats = C.TURRET_STATS[tk]
                if math.hypot(b.pos[0] - pos[0],
                              b.pos[1] - pos[1]) <= stats["range"]:
                    counted.add(b.tile)
                    covered = min(2.0 * stats["range"], route_length)
                    shots = math.ceil(covered / (speed * stats["cooldown"]))
                    damage += shots * math.ceil(b.units /
                                                stats["damage_div"])
        return damage

    def _decide(self) -> None:
        game = self.game
        me = self.player_id
        # Gather enemy vehicles of *every* other player (sec. 13.4).
        threats = {}
        for v in game.vehicles:
            if v.dead or v.owner == me or not v.route:
                continue
            threats[v.route[-1]] = threats.get(v.route[-1], 0.0) + v.units
        visible = self._visible_threats(threats)
        friendly = self._inbound_units(me)

        best = None  # (score, src, dst)
        for src in game.buildings:
            if src.owner != me or src.units <= 0:
                continue
            kind = vehicle_kind_of(src.kind)
            # Do not strip turrets / healing towers that are under threat
            # without a good reason (sec. 13.5).
            if is_turret(src.kind) and src.tile in visible:
                continue
            for dst in game.buildings:
                if dst is src:
                    continue
                path = game.board.find_path(src.tile, dst.tile, kind)
                if path is None:
                    continue
                p = src.units
                b = dst.units
                own = dst.owner == me

                # Safety rules (sec. 13.6): a hostile target must be
                # outmatched *and* the attack must survive the expected
                # en-route fire of hostile and neutral turrets.  The
                # estimate uses a 2x safety margin, plus one full close-
                # range shot when the target itself is a turret.
                if not own:
                    expected = self._route_damage(path, C.VEHICLE_SPEED[kind])
                    dtk = turret_kind_of(dst.kind)
                    if dtk is not None and dst.units >= 1:
                        stats = C.TURRET_STATS[dtk]
                        expected += math.ceil(dst.units /
                                              stats["damage_div"])
                    if p <= b + expected:
                        continue
                if own and b + p > dst.capacity and \
                        (b + p - dst.capacity) > 0.5 * p:
                    continue

                d = self.diff
                score = 0.0
                if own:
                    # Reinforcement of a threatened building (sec. 13.5).
                    threat = visible.get(dst.tile, 0.0)
                    backup = b + friendly.get(dst.tile, 0.0) + 1.0
                    score += d.w3 * (threat / (threat + backup))
                    if is_base(dst.kind):
                        score += 0.15 * d.w2      # concentration point
                    if is_turret(dst.kind) or \
                            dst.kind == BuildingKind.HEAL_TOWER:
                        if b < dst.capacity * 0.6:
                            score += 0.3 * d.w2   # keep defences stocked
                else:
                    # Chance of taking the target over (sec. 4, 13.5).
                    chance = min(1.0, (p - b) / max(p, 1.0))
                    score += d.w1 * (0.3 + 0.7 * chance)
                    value = 3.0 if is_base(dst.kind) else (
                        2.0 if is_turret(dst.kind) or
                        dst.kind == BuildingKind.HEAL_TOWER else 1.5)
                    if dst.owner is None:
                        value += 1.0              # free real estate
                    # Central buildings are worth more (sec. 13.5).
                    value *= 1.0 + 0.04 * len(game.board.neighbors(dst.tile))
                    score += d.w2 * value * 0.1

                # Travel time from route length and vehicle speed (sec. 5).
                length = game.board.path_world_length(path, src.tile)
                score -= d.w4 * length / C.VEHICLE_SPEED[kind] / 60.0

                # Mines, traps, walls and hostile turret ranges (sec. 13.5).
                score -= d.w5 * self._route_danger(path)

                # Source risk / evacuation (sec. 13.5).
                src_threat = visible.get(src.tile, 0.0)
                if src_threat > p + friendly.get(src.tile, 0.0):
                    score += d.w3 * 0.5           # evacuate a doomed base
                elif src_threat > 0:
                    score -= d.w6 * min(1.0, src_threat / max(p, 1.0))

                # Difficulty noise (sec. 13.8).
                score += self.rng.gauss(0.0, self.diff.noise)

                if best is None or score > best[0]:
                    best = (score, src, dst)

        if best is not None and best[0] >= self.diff.threshold:
            _, src, dst = best
            game.try_send(me, src.tile, dst.tile)
