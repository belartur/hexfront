//! AI players (rules.md sections 13-14).
//!
//! Each AI player runs an independent decision loop (section 14.2) and, once
//! per interval, evaluates every (source, target) building pair with the
//! scoring formula from section 14.5:
//!
//! ```text
//! score = W1*chance + W2*value + W3*defence - W4*time
//!         - W5*danger - W6*source_risk
//! ```
//!
//! plus small security/evacuation adjustments and difficulty noise. Bonus
//! tiles (rules.md section 13) compete as extra targets under the same
//! formula: the effect value replaces the capture chance and the building
//! value, the round trip doubles time and danger, and the same threshold
//! decides. The behaviour is fully deterministic for a given level seed
//! (section 14.2).
//!
//! A decision is the most expensive thing the simulation does: it weighs every
//! building pair of the map, and on a big level that means thousands of route
//! searches plus a scan of all buildings per route tile. Two things keep it
//! cheap. First, the decisions of the individual AI players are spread evenly
//! over one interval ([`AiController::new`] takes the player index and the
//! player count), so the cost is paid in `player_count` slices instead of one
//! spike. Second, everything a decision derives from the *static* board is
//! derived once and reused: the routes ([`AiController::routes`]) and the
//! hostile turret threat ([`AiController::hostiles`]) never change while the
//! board stands, so only the unit counts and the vehicles are re-read on every
//! decision.

use std::collections::HashMap;
use std::rc::Rc;

use crate::board::Board;
use crate::constants::{self, AiDifficulty, TurretKind, VehicleKind};
use crate::entities::{
    BonusKind, Building, BuildingKind, is_base, is_turret, turret_kind_of, vehicle_kind_of,
};
use crate::game::Game;
use crate::hexgrid::Tile;
use crate::math::{dist2, sqr};
use crate::rng::Rng;

/// What one enemy turret contributes to the danger of a route, gathered once
/// per decision instead of per route tile.
struct Threat {
    /// Turret kind (section 10).
    kind: TurretKind,
    /// World position of the turret.
    pos: (f64, f64),
    /// Squared firing range, ready to compare against a squared distance.
    range_sq: f64,
    /// Firing range in j.
    range: f64,
    /// Units parked in the turret, i.e. its damage.
    units: f64,
}

/// Cached route between two buildings. Shared by reference, because a decision
/// weighs several pairs that route the same way and only ever reads the tiles.
type Route = Rc<Vec<Tile>>;

/// Reachable regions of the board, one labelling per vehicle kind.
type Regions = HashMap<VehicleKind, Option<Rc<Vec<u32>>>>;

/// One AI controller per computer player of `game`, seeded from the level
/// seed and given their position among the AI players.
///
/// The index and the count are what spread the decisions evenly over one
/// decision interval (rules.md section 14.2), so every way of starting a
/// game spaces them the same way.
pub fn controllers(game: &Game, difficulty: AiDifficulty, seed: u64) -> Vec<AiController> {
    let ai_players: Vec<usize> = game
        .players
        .iter()
        .filter(|p| !p.is_human)
        .map(|p| p.id)
        .collect();
    let count = ai_players.len();
    ai_players
        .iter()
        .enumerate()
        .map(|(index, id)| {
            AiController::new(*id, difficulty, seed.wrapping_add(*id as u64), index, count)
        })
        .collect()
}

/// Decision loop of a single AI player.
pub struct AiController {
    /// AI player id.
    pub player_id: usize,
    /// Difficulty preset.
    pub diff: AiDifficulty,
    rng: Rng,
    /// Decision phase timer.
    pub timer: f64,
    /// Tile -> time at which the inbound threat was first noticed.
    pub threat_seen: HashMap<Tile, f64>,
    /// Routes of the board, keyed by (source, target, vehicle kind).
    ///
    /// The road graph only depends on heights, ramps and bridges, none of
    /// which change while a match runs: a destroyed wall or an exploded mine
    /// never blocked a route in the first place, because `find_path` ignores
    /// obstacles (section 6). A route found once therefore stays valid for
    /// the whole match, and the cache turns every repeat visit of the same
    /// building pair into a hash lookup instead of another search. It lives
    /// as long as the controller, which is recreated for every match and for
    /// every editor playtest (where the edited board differs).
    routes: HashMap<(Tile, Tile, VehicleKind), Option<Route>>,
    /// Enemy and neutral turrets, gathered per decision.
    ///
    /// Their positions and ranges are static like the routes, but their unit
    /// counts grow and shrink, so this one is refreshed on every decision
    /// instead of being kept for the whole match.
    hostiles: Vec<Threat>,
    /// Reachable regions of the board per vehicle kind, built on the first
    /// decision that needs them and kept for the rest of the match.
    ///
    /// Same argument as for `routes`: they depend only on terrain, which does
    /// not change while a match runs. A decision then rejects an unreachable
    /// pair with two array reads instead of a search that would flood the
    /// source region first. `None` marks a board with a bridge, where the
    /// crossing modes break the symmetry the labelling relies on.
    regions: Regions,
}

impl AiController {
    /// Create a controller for `player_id` with `difficulty` and `seed`.
    ///
    /// `player_index` is the position of this player among the AI players and
    /// `player_count` how many of them there are. The decision phases are
    /// spread evenly over one interval that way (section 14.2): with three AI
    /// players and a two-second interval the controllers decide two thirds of
    /// a second apart, so a decision never lands on top of another player's
    /// and one interval of AI work is spread over `player_count` frames
    /// instead of spiking on a single one.
    pub fn new(
        player_id: usize,
        difficulty: AiDifficulty,
        seed: u64,
        player_index: usize,
        player_count: usize,
    ) -> Self {
        let phase = if player_count == 0 {
            0.0
        } else {
            difficulty.interval * player_index as f64 / player_count as f64
        };
        Self {
            player_id,
            diff: difficulty,
            rng: Rng::new(seed),
            // Stagger the decision phases of different AI players (14.2).
            timer: phase,
            threat_seen: HashMap::new(),
            routes: HashMap::new(),
            hostiles: Vec::new(),
            regions: HashMap::new(),
        }
    }
    /// Advance the decision loop; called every frame.
    pub fn update(&mut self, game: &mut Game, dt: f64) {
        if game.over {
            return;
        }
        self.timer += dt;
        if self.timer >= self.diff.interval {
            self.timer -= self.diff.interval;
            self.decide(game);
        }
    }
    /// Reachable regions of `board` for `kind`, built on first use and kept
    /// afterwards, or `None` when the board has a bridge and the shortcut
    /// does not apply (see [`Board::regions`]).
    ///
    /// The regions depend only on heights and ramps, which do not change while
    /// a match runs, so one build serves every later decision. Building is
    /// lazy rather than done for every kind up front: on a big board each
    /// labelling is a full sweep, and a level without hovercraft bases should
    /// never pay for one.
    fn regions(&mut self, board: &Board, kind: VehicleKind) -> Option<Rc<Vec<u32>>> {
        if let Some(regions) = self.regions.get(&kind) {
            return regions.clone();
        }
        let regions = board.regions(kind).map(Rc::new);
        self.regions.insert(kind, regions.clone());
        regions
    }
    /// Route `src -> dst` for a vehicle of `kind`, from the cache when the
    /// board has already been asked for this pair.
    ///
    /// The reachability check comes first: two fields in different regions
    /// have no route whatever the terrain between them looks like, and saying
    /// so costs one array read, while the search would have to flood the whole
    /// source region before it could answer the same.
    fn route(&mut self, board: &Board, src: Tile, dst: Tile, kind: VehicleKind) -> Option<Route> {
        if let Some(regions) = self.regions(board, kind)
            && !board.same_region(&regions, src, dst)
        {
            return None;
        }
        self.routes
            .entry((src, dst, kind))
            .or_insert_with(|| board.find_path(src, dst, kind).map(Rc::new))
            .clone()
    }
    /// Enemy and neutral turrets of the current decision (sections 14.3,
    /// 14.5): everything a route can be shot at from.
    fn gather_hostiles(&mut self, board: &Board, buildings: &[Building]) {
        self.hostiles.clear();
        for b in buildings.iter() {
            let kind = match turret_kind_of(b.kind) {
                Some(t) => t,
                None => continue,
            };
            if b.owner == Some(self.player_id) {
                continue;
            }
            let range = constants::turret_range(kind);
            self.hostiles.push(Threat {
                kind,
                pos: b.pos(board.side),
                range,
                range_sq: sqr(range),
                units: b.units,
            });
        }
    }
    /// Filter raw threats through the reaction delay (section 14.8).
    fn visible_threats(&mut self, game: &Game, raw: &HashMap<Tile, f64>) -> HashMap<Tile, f64> {
        let game_time = game.time;
        let delay = self.diff.reaction_delay;
        let mut visible: HashMap<Tile, f64> = HashMap::new();
        for (tile, units) in raw.iter() {
            let first_seen = *self.threat_seen.entry(*tile).or_insert(game_time);
            if game_time - first_seen >= delay {
                visible.insert(*tile, *units);
            }
        }
        visible
    }
    /// Danger of a route: hostile turret ranges + obstacles on it.
    fn route_danger(&self, board: &Board, path: &[Tile]) -> f64 {
        let mut danger = 0.0;
        for tile in path.iter() {
            let pos = board.center_world(*tile);
            let mut hit = false;
            for t in self.hostiles.iter() {
                if dist2(t.pos, pos) <= t.range_sq {
                    danger += 1.0;
                    hit = true;
                    break;
                }
            }
            if !hit
                && let Some(t) = board.tiles.get(tile)
                && t.obstacle.is_some()
            {
                danger += 0.5;
            }
        }
        danger / (path.len().max(1) as f64)
    }
    /// Conservative en-route damage estimate from hostile turrets.
    /// A turret whose range circle covers any route tile keeps firing
    /// while the vehicle crosses it, so the number of incoming shots is
    /// estimated from the covered distance (at most the full route
    /// length) divided by the turret's fire period. Used by the safety
    /// rules (sec. 14.6).
    fn route_damage(&self, board: &Board, path: &[Tile], speed: f64) -> f64 {
        let route_length = board.path_world_length(path, None);
        let mut damage = 0.0;
        // A turret counts once no matter how many route tiles it covers; the
        // flag is an index into `hostiles`, so the scan stays linear.
        let mut counted = vec![false; self.hostiles.len()];
        for tile in path.iter() {
            let pos = board.center_world(*tile);
            for (i, t) in self.hostiles.iter().enumerate() {
                if counted[i] || t.units < 1.0 {
                    continue;
                }
                if dist2(t.pos, pos) <= t.range_sq {
                    counted[i] = true;
                    let covered = (2.0 * t.range).min(route_length);
                    let shots = (covered / (speed * constants::turret_cooldown(t.kind))).ceil();
                    damage += shots * (t.units / constants::turret_damage_div(t.kind)).ceil();
                }
            }
        }
        damage
    }
    fn decide(&mut self, game: &mut Game) {
        let me = self.player_id;
        // Gather intelligence (sections 14.3, 14.4).
        let mut raw_enemy: HashMap<Tile, f64> = HashMap::new();
        let mut friendly: HashMap<Tile, f64> = HashMap::new();
        for v in game.vehicles.iter() {
            if v.dead || v.route.is_empty() {
                continue;
            }
            let dst = v.route[v.route.len() - 1];
            if v.owner == me {
                *friendly.entry(dst).or_insert(0.0) += v.units;
            } else {
                *raw_enemy.entry(dst).or_insert(0.0) += v.units;
            }
        }
        let visible = self.visible_threats(game, &raw_enemy);
        // Enemy turrets are re-read once per decision rather than per route
        // tile: the danger and damage scans below walk the route and compare
        // against this list instead of against every building on the map.
        self.gather_hostiles(&game.board, &game.buildings);
        // Candidate (source_idx, target tile).
        let mut best: Option<(f64, usize, Tile)> = None;
        let n = game.buildings.len();
        for si in 0..n {
            let src = &game.buildings[si];
            if src.owner != Some(me) || src.units <= 0.0 {
                continue;
            }
            let kind: VehicleKind = vehicle_kind_of(src.kind);
            // Do not strip turrets under threat without a good reason (14.5).
            if is_turret(src.kind) && visible.contains_key(&src.tile) {
                continue;
            }
            for di in 0..n {
                if si == di {
                    continue;
                }
                let dst = &game.buildings[di];
                let p = src.units;
                let b = dst.units;
                let own = dst.owner == Some(me);
                // Safety rules that need no route (14.6), applied before the
                // search: they reject the pair on unit counts alone, so a
                // hopeless attack never pays for a route search.
                if !own {
                    // `p <= b` alone already loses to the target garrison,
                    // and the damage a route would take is never negative, so
                    // the pair is hopeless whatever the route looks like.
                    if p <= b {
                        continue;
                    }
                } else if b + p > dst.capacity && (b + p - dst.capacity) > 0.5 * p {
                    // Overcrowding would kill more than half the convoy.
                    continue;
                }
                let path = match self.route(&game.board, src.tile, dst.tile, kind) {
                    Some(p) => p,
                    None => continue,
                };
                if !own {
                    let expected =
                        self.route_damage(&game.board, &path, constants::vehicle_speed(kind));
                    let mut extra = 0.0;
                    if let Some(dtk) = turret_kind_of(dst.kind)
                        && dst.units >= 1.0
                    {
                        extra += (dst.units / constants::turret_damage_div(dtk)).ceil();
                    }
                    if p <= b + expected + extra {
                        continue;
                    }
                }
                let d = self.diff;
                let mut score = 0.0;
                if own {
                    let threat = *visible.get(&dst.tile).unwrap_or(&0.0);
                    let backup = b + *friendly.get(&dst.tile).unwrap_or(&0.0) + 1.0;
                    score += d.w3 * (threat / (threat + backup));
                    if is_base(dst.kind) {
                        score += 0.15 * d.w2;
                    }
                    if (is_turret(dst.kind) || dst.kind == BuildingKind::HealTower)
                        && b < dst.capacity * 0.6
                    {
                        score += 0.3 * d.w2;
                    }
                } else {
                    let chance = ((p - b) / p.max(1.0)).clamp(0.0, 1.0);
                    score += d.w1 * (0.3 + 0.7 * chance);
                    let mut value = if is_base(dst.kind) {
                        3.0
                    } else if is_turret(dst.kind) || dst.kind == BuildingKind::HealTower {
                        2.0
                    } else {
                        1.5
                    };
                    if dst.owner.is_none() {
                        value += 1.0;
                    }
                    value *= 1.0 + 0.04 * game.board.neighbor_count(dst.tile) as f64;
                    score += d.w2 * value * 0.1;
                }
                let length = game.board.path_world_length(&path, Some(src.tile));
                score -= d.w4 * length / constants::vehicle_speed(kind) / 60.0;
                score -= d.w5 * self.route_danger(&game.board, &path);
                let src_threat = *visible.get(&src.tile).unwrap_or(&0.0);
                let src_backup = *friendly.get(&src.tile).unwrap_or(&0.0);
                if src_threat > p + src_backup {
                    score += d.w3 * 0.5;
                } else if src_threat > 0.0 {
                    score -= d.w6 * (src_threat / p.max(1.0)).min(1.0);
                }
                score += self.rng.gauss(0.0, self.diff.noise);
                if best.is_none() || score > best.unwrap().0 {
                    best = Some((score, si, dst.tile));
                }
            }
        }
        // Bonus targets (rules.md section 13): a dispatch to a bonus is the
        // same action as a dispatch to a building, only the target differs.
        // One decision performs at most one action, so a bonus is just one
        // more candidate next to the building pairs.
        for (si, src) in game.buildings.iter().enumerate() {
            if src.owner != Some(me) || src.units < 1.0 {
                continue;
            }
            // Never double-book a bonus: when one own vehicle is already on
            // its way there, no second vehicle is sent (rules.md 12).
            let kind = vehicle_kind_of(src.kind);
            for bonus in game.bonuses.iter().flatten() {
                if game
                    .vehicles
                    .iter()
                    .any(|v| !v.dead && v.owner == me && v.bonus_target == Some(bonus.tile))
                {
                    continue;
                }
                let Some(path) = self.route(&game.board, src.tile, bonus.tile, kind) else {
                    continue;
                };
                let Some(score) =
                    self.score_bonus(game, me, src, bonus, &path, kind, &visible, &friendly)
                else {
                    continue;
                };
                if best.is_none() || score > best.unwrap().0 {
                    best = Some((score, si, bonus.tile));
                }
            }
        }
        if let Some((score, si, dt2)) = best
            && score >= self.diff.threshold
        {
            let st = game.buildings[si].tile;
            if dt2 != st {
                game.try_send(me, st, dt2);
            }
        }
    }
    /// The first enemy vehicle racing for the same bonus, with the time it
    /// needs to get there.
    ///
    /// rules.md section 14.5: a bonus is not rejected just because somebody
    /// else is going for it -- the vehicle may still win the fight on the
    /// way, because combat never changes its destination. The caller needs
    /// the rival only to discount the value: full when the own vehicle
    /// arrives no later, and just the chance of winning the duel when the
    /// rival gets there first (full with a unit advantage, zero without).
    fn bonus_rival(&self, game: &Game, me: usize, bonus_tile: Tile) -> Option<(f64, f64)> {
        let mut best: Option<(f64, f64)> = None;
        for v in game.vehicles.iter() {
            if v.dead || v.owner == me || v.route.is_empty() {
                continue;
            }
            let heads_for_bonus =
                v.bonus_target == Some(bonus_tile) || v.route[v.route.len() - 1] == bonus_tile;
            if !heads_for_bonus {
                continue;
            }
            let left: Vec<Tile> = v.route[v.route_index.min(v.route.len())..].to_vec();
            let len = game.board.path_world_length(&left, None);
            let time = len / constants::vehicle_speed(v.kind);
            if best.is_none_or(|(bt, _)| time < bt) {
                best = Some((time, v.units));
            }
        }
        best
    }
    /// Score of one (source building, bonus) pair (rules.md section 13).
    ///
    /// The effect value replaces the capture chance and the building value:
    /// `+x` is worth `x / units`, `*x` the multiplier gain `x - 1` and a
    /// drone a fixed value independent of the unit count. The mission is a
    /// round trip along the same route, so the travel time, the danger and
    /// the expected damage count twice. The value shrinks with the fraction
    /// of units expected to survive the round trip (`+x` and `*x` only -- a
    /// drone recreates its own field when its carrier dies). A rival racing
    /// for the same field discounts the value by the chance of getting it:
    /// full when the own vehicle arrives no later, and only the chance of
    /// winning the fight when the rival is quicker. `None` means the AI does
    /// not send: the expected damage would eat the whole convoy, the rival is
    /// both quicker and at least as strong, or the source building is
    /// threatened and has no other force of its own defending it (a bonus
    /// mission leaves it empty twice as long).
    #[allow(clippy::too_many_arguments)]
    fn score_bonus(
        &mut self,
        game: &Game,
        me: usize,
        src: &Building,
        bonus: &crate::entities::Bonus,
        path: &Route,
        kind: VehicleKind,
        visible: &HashMap<Tile, f64>,
        friendly: &HashMap<Tile, f64>,
    ) -> Option<f64> {
        let p = src.units;
        if p < 1.0 {
            return None;
        }
        let length = game.board.path_world_length(path, Some(src.tile));
        let speed = constants::vehicle_speed(kind);
        let expected = self.route_damage(&game.board, path, speed);
        if expected >= p {
            return None;
        }
        let src_threat = *visible.get(&src.tile).unwrap_or(&0.0);
        let src_backup = *friendly.get(&src.tile).unwrap_or(&0.0);
        if src_threat > 0.0 && src_threat >= p + src_backup {
            return None;
        }
        let d = self.diff;
        let mut score = 0.0;
        match bonus.kind {
            BonusKind::Add(_) | BonusKind::Mul(_) => {
                let effect = match bonus.kind {
                    BonusKind::Add(x) => x as f64 / p.max(1.0),
                    BonusKind::Mul(x) => (x as f64 - 1.0).max(0.0),
                    BonusKind::Drone => 0.0,
                };
                let survived = ((p - 2.0 * expected) / p).clamp(0.0, 1.0);
                score += d.w1 * effect * survived;
            }
            BonusKind::Drone => score += d.w1 * 1.5 + d.w2 * 0.3,
        }
        score -= d.w4 * 2.0 * length / speed / 60.0;
        score -= d.w5 * 2.0 * self.route_danger(&game.board, path);
        if src_threat > 0.0 {
            score -= d.w6 * (src_threat / p.max(1.0)).min(1.0);
        }
        if let Some((rival_time, rival_units)) = self.bonus_rival(game, me, bonus.tile)
            && rival_time < length / speed
        {
            // The rival is first: only the chance of winning the duel counts.
            let win = ((p - rival_units) / p.max(1.0)).clamp(0.0, 1.0);
            if win <= 0.0 {
                return None;
            }
            score *= win;
        }
        score += self.rng.gauss(0.0, self.diff.noise);
        Some(score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::entities::{Building, Player};
    #[test]
    fn ai_sends_when_profitable() {
        let mut board = Board::new(12, 12);
        for t in board.tiles.clone().keys().copied().collect::<Vec<_>>() {
            board.tiles.get_mut(&t).unwrap().height = 1;
        }
        let players = vec![Player::new(0, true), Player::new(1, false)];
        let buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(1), 1, 1, 50.0),
            Building::new(BuildingKind::BaseTank, None, 10, 10, 5.0),
        ];
        let mut game = Game::new(board, players, buildings, Vec::new(), 0);
        let mut ai = AiController::new(1, crate::constants::AI_DIFFICULTIES[2], 42, 0, 1);
        ai.timer = 999.0;
        ai.update(&mut game, 0.0);
        assert!(!game.vehicles.is_empty());
    }
    #[test]
    fn decision_phases_are_spread_over_the_interval() {
        // rules.md section 14.2: the AI players decide one interval apart
        // from each other, shifted by index/count of the interval, so no two
        // of them weigh their options on the same frame.
        let d = crate::constants::AI_DIFFICULTIES[1];
        for count in 1..=4 {
            let timers: Vec<f64> = (0..count)
                .map(|i| AiController::new(i, d, 7, i, count).timer)
                .collect();
            // Consecutive phases are one interval/count apart, and the whole
            // group fits inside one interval so the order never wraps.
            for (i, t) in timers.iter().enumerate() {
                let want = d.interval * i as f64 / count as f64;
                assert!(
                    (t - want).abs() < 1e-9,
                    "{count} players: phase {i} is {t}, want {want}"
                );
                assert!(*t < d.interval, "{count} players: phase {i} is {t}");
            }
        }
    }
    #[test]
    fn staggered_players_never_decide_in_the_same_step() {
        // The phase offset has to stagger the decisions of a running match:
        // with two AI players no step may carry a decision of both, and each
        // player keeps deciding once per interval afterwards.
        let diff = crate::constants::AI_DIFFICULTIES[1];
        let mut ais: Vec<AiController> = (0..2)
            .map(|i| AiController::new(i + 1, diff, 11, i, 2))
            .collect();
        let mut decided_at: Vec<usize> = vec![usize::MAX; 2];
        let mut game = two_ai_players_game();
        let steps = (3.0 * diff.interval / constants::SIM_DT) as usize;
        for step in 0..steps {
            for (i, ai) in ais.iter_mut().enumerate() {
                ai.update(&mut game, constants::SIM_DT);
                // `update` decides exactly when its timer wraps back, which is
                // what the phase is meant to move apart.
                if ai.timer < constants::SIM_DT {
                    assert_ne!(
                        decided_at[i], step,
                        "player {i} decided twice in step {step}"
                    );
                    assert_ne!(
                        decided_at[1 - i],
                        step,
                        "players {i} and {} both decided in step {step}",
                        1 - i
                    );
                    decided_at[i] = step;
                }
            }
            game.update(constants::SIM_DT);
        }
        let decided = decided_at.iter().filter(|s| **s != usize::MAX).count();
        assert_eq!(decided, 2, "both AI players decide within three intervals");
    }
    #[test]
    fn a_route_is_asked_for_once_per_pair() {
        // The route cache is what keeps a decision cheap on a big map, so a
        // repeated query has to hand back the same route without a second
        // search. Comparing against the board proves it is the same path, not
        // just the same pointer.
        let mut board = Board::new(10, 10);
        for t in board.tiles.clone().keys().copied().collect::<Vec<_>>() {
            board.tiles.get_mut(&t).unwrap().height = 1;
        }
        let game = Game::new(board, vec![Player::new(0, true)], Vec::new(), Vec::new(), 0);
        let mut ai = AiController::new(1, crate::constants::AI_DIFFICULTIES[2], 5, 0, 1);
        let first = ai
            .route(&game.board, (0, 0), (9, 9), VehicleKind::Tank)
            .expect("a route exists");
        let second = ai
            .route(&game.board, (0, 0), (9, 9), VehicleKind::Tank)
            .expect("cached route exists");
        assert_eq!(&*first, &*second);
        assert_eq!(
            &*first,
            &game
                .board
                .find_path((0, 0), (9, 9), VehicleKind::Tank)
                .expect("a route exists")
        );
        // A different vehicle kind is a different road and its own entry; the
        // helicopter may well pick another equally short route (it walks
        // greedily, see `Board::find_path`), so compare it with the board.
        let heli = ai
            .route(&game.board, (0, 0), (9, 9), VehicleKind::Helicopter)
            .expect("a route exists");
        assert_eq!(
            &*heli,
            &game
                .board
                .find_path((0, 0), (9, 9), VehicleKind::Helicopter)
                .expect("a route exists"),
            "the cached helicopter route is the one the board returns"
        );
        assert_eq!(
            heli.len(),
            first.len(),
            "both kinds walk the same number of steps across open land"
        );
    }
    /// An AI weighs a bonus as a dispatch target and picks it when it beats
    /// every building (rules.md sections 13 and 14.5): a `*3` next to the base
    /// is worth more than any building move, so the vehicle drives there, and
    /// a second decision does not send a second vehicle after the same field.
    #[test]
    fn a_bonus_field_outscores_building_moves_and_is_not_doubled() {
        let mut board = Board::new(12, 12);
        for t in board.tiles.clone().keys().copied().collect::<Vec<_>>() {
            board.tiles.get_mut(&t).unwrap().height = 1;
        }
        let players = vec![Player::new(0, true), Player::new(1, false)];
        let buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(1), 2, 2, 40.0),
            Building::new(BuildingKind::BaseTank, Some(0), 9, 9, 40.0),
        ];
        let bonuses = vec![crate::entities::Bonus::new(
            (3, 2),
            crate::entities::BonusKind::Mul(3),
        )];
        let mut game = Game::new(board, players, buildings, bonuses, 0);
        game.sandbox = true;
        let diff = crate::constants::AI_DIFFICULTIES[2];
        let mut ai = AiController::new(1, diff, 7, 0, 1);
        // The first decision spends the base on the bonus.
        ai.timer = 999.0;
        ai.update(&mut game, 0.0);
        assert_eq!(game.vehicles.len(), 1, "the AI sends a vehicle");
        let v = &game.vehicles[0];
        assert_eq!(v.bonus_target, Some((3, 2)), "and it goes for the bonus");
        // While that vehicle is on its way no second one may be sent there.
        ai.timer = 999.0;
        ai.update(&mut game, 0.0);
        let chasing = game
            .vehicles
            .iter()
            .filter(|v| v.bonus_target == Some((3, 2)))
            .count();
        assert_eq!(chasing, 1, "no second vehicle after the same bonus");
        // Once the bonus is spent it is no longer a candidate: the base is
        // empty anyway, so no further dispatch happens.
        game.bonuses = vec![None];
        game.bonus_at.clear();
        game.buildings[0].units = 40.0;
        ai.timer = 999.0;
        ai.update(&mut game, 0.0);
        assert!(
            game.vehicles
                .iter()
                .filter(|v| v.bonus_target == Some((3, 2)))
                .count()
                <= 1,
            "a spent bonus is not a target any more"
        );
    }
    /// Flat 12x12 board with a base for each of two AI players plus a neutral
    /// one, so both controllers have something to weigh on every decision.
    /// The human player needs a building too: without one it is eliminated
    /// immediately (rules.md section 2) and the match ends before the second
    /// decision this test looks for.
    fn two_ai_players_game() -> Game {
        let mut board = Board::new(12, 12);
        for t in board.tiles.clone().keys().copied().collect::<Vec<_>>() {
            board.tiles.get_mut(&t).unwrap().height = 1;
        }
        let players = vec![
            Player::new(0, true),
            Player::new(1, false),
            Player::new(2, false),
        ];
        let buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(1), 1, 1, 40.0),
            Building::new(BuildingKind::BaseTank, Some(2), 10, 10, 40.0),
            Building::new(BuildingKind::BaseTank, Some(0), 5, 9, 40.0),
            Building::new(BuildingKind::BaseTank, None, 1, 9, 10.0),
        ];
        let mut game = Game::new(board, players, buildings, Vec::new(), 0);
        // Keep the match running for the whole test: the decisions of the two
        // AI players are what is measured, not who wins.
        game.sandbox = true;
        game
    }
}
