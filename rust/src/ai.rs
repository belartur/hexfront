//! AI players (rules.md section 13).
//!
//! Each AI player runs an independent decision loop (section 13.2) and, once
//! per interval, evaluates every (source, target) building pair with the
//! scoring formula from section 13.5:
//!
//! ```text
//! score = W1*chance + W2*value + W3*defence - W4*time
//!         - W5*danger - W6*source_risk
//! ```
//!
//! plus small security/evacuation adjustments and difficulty noise. The
//! behaviour is fully deterministic for a given level seed (section 13.2).

use std::collections::HashMap;

use crate::constants::{self, AiDifficulty, VehicleKind};
use crate::entities::{BuildingKind, is_base, is_turret, turret_kind_of, vehicle_kind_of};
use crate::game::Game;
use crate::hexgrid::Tile;
use crate::rng::Rng;

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
}

impl AiController {
    /// Create a controller for `player_id` with `difficulty` and `seed`.
    pub fn new(player_id: usize, difficulty: AiDifficulty, seed: u64) -> Self {
        Self {
            player_id,
            diff: difficulty,
            rng: Rng::new(seed),
            // Stagger the decision phases of different AI players (13.2).
            timer: 0.7 * player_id as f64,
            threat_seen: HashMap::new(),
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
    #[allow(dead_code)]
    /// Map destination tile -> total units of `owner`'s vehicles.
    fn inbound_units(&self, game: &Game, owner: usize) -> HashMap<Tile, f64> {
        let mut result: HashMap<Tile, f64> = HashMap::new();
        for v in game.vehicles.iter() {
            if v.dead || v.owner != owner || v.route.is_empty() {
                continue;
            }
            let dst = v.route[v.route.len() - 1];
            *result.entry(dst).or_insert(0.0) += v.units;
        }
        result
    }
    /// Filter raw threats through the reaction delay (section 13.8).
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
    fn route_danger(&self, game: &Game, path: &[Tile]) -> f64 {
        let board = &game.board;
        let mut danger = 0.0;
        for tile in path.iter() {
            let pos = board.center_world(*tile);
            let mut hit = false;
            for b in game.buildings.iter() {
                let tk = match turret_kind_of(b.kind) {
                    Some(t) => t,
                    None => continue,
                };
                if b.owner == Some(self.player_id) {
                    continue;
                }
                let bp = b.pos(board.side);
                if ((bp.0 - pos.0).powi(2) + (bp.1 - pos.1).powi(2)).sqrt()
                    <= constants::turret_range(tk)
                {
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
    /// rules (sec. 13.6).
    fn route_damage(&self, game: &Game, path: &[Tile], speed: f64) -> f64 {
        let board = &game.board;
        let route_length = board.path_world_length(path, None);
        let mut damage = 0.0;
        let mut counted: std::collections::HashSet<Tile> = std::collections::HashSet::new();
        for tile in path.iter() {
            let pos = board.center_world(*tile);
            for b in game.buildings.iter() {
                let tk = match turret_kind_of(b.kind) {
                    Some(t) => t,
                    None => continue,
                };
                if b.owner == Some(self.player_id) || b.units < 1.0 || counted.contains(&b.tile) {
                    continue;
                }
                let bp = b.pos(board.side);
                if ((bp.0 - pos.0).powi(2) + (bp.1 - pos.1).powi(2)).sqrt()
                    <= constants::turret_range(tk)
                {
                    counted.insert(b.tile);
                    let covered = (2.0 * constants::turret_range(tk)).min(route_length);
                    let shots = (covered / (speed * constants::turret_cooldown(tk))).ceil();
                    damage += shots * (b.units / constants::turret_damage_div(tk)).ceil();
                }
            }
        }
        damage
    }
    fn decide(&mut self, game: &mut Game) {
        let me = self.player_id;
        // Gather intelligence (sections 13.3, 13.4).
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
        // Cache routes: the board is static during one decision, so each
        // (source, target, kind) pair is routed at most once.
        let mut route_cache: HashMap<(usize, usize), Option<Vec<Tile>>> = HashMap::new();
        // Candidate (source_idx, target_idx, path).
        let mut best: Option<(f64, usize, usize)> = None;
        let n = game.buildings.len();
        for si in 0..n {
            let src = &game.buildings[si];
            if src.owner != Some(me) || src.units <= 0.0 {
                continue;
            }
            let kind: VehicleKind = vehicle_kind_of(src.kind);
            // Do not strip turrets under threat without a good reason (13.5).
            if is_turret(src.kind) && visible.contains_key(&src.tile) {
                continue;
            }
            for di in 0..n {
                if si == di {
                    continue;
                }
                let path = match route_cache
                    .entry((si, di))
                    .or_insert_with(|| {
                        game.board
                            .find_path(src.tile, game.buildings[di].tile, kind)
                    })
                    .clone()
                {
                    Some(p) => p,
                    None => continue,
                };
                let dst = &game.buildings[di];
                let p = src.units;
                let b = dst.units;
                let own = dst.owner == Some(me);
                if !own {
                    let expected = self.route_damage(game, &path, constants::vehicle_speed(kind));
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
                if own && b + p > dst.capacity && (b + p - dst.capacity) > 0.5 * p {
                    continue;
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
                    value *= 1.0 + 0.04 * game.board.neighbors(dst.tile).len() as f64;
                    score += d.w2 * value * 0.1;
                }
                let length = game.board.path_world_length(&path, Some(src.tile));
                score -= d.w4 * length / constants::vehicle_speed(kind) / 60.0;
                score -= d.w5 * self.route_danger(game, &path);
                let src_threat = *visible.get(&src.tile).unwrap_or(&0.0);
                let src_backup = *friendly.get(&src.tile).unwrap_or(&0.0);
                if src_threat > p + src_backup {
                    score += d.w3 * 0.5;
                } else if src_threat > 0.0 {
                    score -= d.w6 * (src_threat / p.max(1.0)).min(1.0);
                }
                score += self.rng.gauss(0.0, self.diff.noise);
                if best.is_none() || score > best.unwrap().0 {
                    best = Some((score, si, di));
                }
            }
        }
        if let Some((score, si, di)) = best
            && score >= self.diff.threshold
        {
            let (st, dt2) = (game.buildings[si].tile, game.buildings[di].tile);
            game.try_send(me, st, dt2);
        }
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
        let mut game = Game::new(board, players, buildings, 0);
        let mut ai = AiController::new(1, crate::constants::AI_DIFFICULTIES[2], 42);
        ai.timer = 999.0;
        ai.update(&mut game, 0.0);
        assert!(!game.vehicles.is_empty());
    }
}
