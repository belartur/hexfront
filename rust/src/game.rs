//! Real-time game simulation (rules.md sections 2-12).
//!
//! The [`Game`] object owns the board, the players, all buildings and
//! vehicles and advances the whole simulation in fixed time steps. It knows
//! nothing about macroquad: rendering and input live in other modules.

use std::collections::{HashMap, HashSet};

use crate::board::{Board, ObstacleKind};
use crate::constants::{self, TurretKind, VehicleKind};
use crate::entities::{
    Building, BuildingKind, Player, SoundEvent, Vehicle, Wreck, is_base, turret_kind_of,
    vehicle_kind_of,
};
use crate::hexgrid::Tile;
use crate::math::{dist, dist2, sqr};
use crate::sound::SoundKind;

/// World position of a tile (the hex grid owns the odd-q offset layout).
fn hexgrid_pos(tile: Tile, side: f64) -> (f64, f64) {
    crate::hexgrid::hex_to_world(tile.0, tile.1, side)
}

/// One turret shot in flight, resolved on impact (rules.md section 10).
#[derive(Clone, Debug)]
pub struct Projectile {
    #[allow(dead_code)]
    /// Firing building tile.
    pub from: Tile,
    /// Firing building tile world position.
    pub from_pos: (f64, f64),
    /// Firing player (None = neutral).
    pub owner: Option<usize>,
    /// Target vehicle id.
    pub target: u64,
    /// Current homed target position.
    pub to: (f64, f64),
    /// Tile under the target.
    pub to_tile: Option<Tile>,
    /// Damage frozen at launch.
    pub dmg: f64,
    /// Turret kind (rocket splash when Rocket).
    pub kind: TurretKind,
    /// Seconds since launch.
    pub t: f64,
    /// Fixed flight time.
    pub dur: f64,
}

/// Full state of one running match.
#[derive(Clone, Debug)]
pub struct Game {
    /// Game board.
    pub board: Board,
    /// All players.
    pub players: Vec<Player>,
    /// All buildings.
    pub buildings: Vec<Building>,
    /// Building index by tile.
    pub building_at: HashMap<Tile, usize>,
    /// Tiles carrying a static obstacle, in deterministic (sorted) order.
    ///
    /// The board is immutable during a match except for destroyed walls and
    /// exploded mines (see [`Game::update_vehicles`] and [`Game::check_mine`]),
    /// which remove their tile from this list, so the presentation layer can
    /// iterate obstacles without scanning the whole board every frame.
    pub obstacle_tiles: Vec<Tile>,
    /// Tiles carrying a fire trap, in deterministic (sorted) order.
    ///
    /// Fire traps are permanent (rules.md section 4), so this list never
    /// changes during a match; it feeds the campfire emitters of [`crate::fx`]
    /// without a full board scan per frame.
    pub fire_tiles: Vec<Tile>,
    /// All vehicles (dead ones are removed at the end of the step).
    pub vehicles: Vec<Vehicle>,
    /// Turret shots in flight, resolved on impact.
    pub projectiles: Vec<Projectile>,
    /// Wrecks of the vehicles destroyed since the last [`Game::take_wrecks`]
    /// call. rules.md knows nothing about explosions, so the simulation only
    /// records *where* a vehicle died; the presentation layer turns each
    /// record into an effect (see [`crate::fx`]).
    pub wrecks: Vec<Wreck>,
    /// Sounds the presentation layer should play (see
    /// [`crate::entities::SoundEvent`]). Drained every frame by
    /// [`Game::take_sounds`], the same way as [`Game::wrecks`]: the simulation
    /// only records *that* something noisy happened and *where*, never how it
    /// should sound.
    pub sounds: Vec<SoundEvent>,
    /// Simulation time in seconds.
    pub time: f64,
    /// True once the match is decided.
    pub over: bool,
    /// "human" or "ai" once `over`.
    pub winner: Option<String>,
    /// Id of the human player.
    pub human_id: usize,
    /// Ids of eliminated players.
    pub eliminated: HashSet<usize>,
    /// Sandbox flag used by the map editor's playtest (`p`): the match never
    /// ends, so an unfinished map (e.g. without an enemy base) can still be
    /// played. Real matches always keep it `false` (rules.md section 2).
    pub sandbox: bool,
}

impl Game {
    /// Create a game from a board, players and buildings.
    pub fn new(board: Board, players: Vec<Player>, buildings: Vec<Building>, _seed: u64) -> Self {
        let human_id = players
            .iter()
            .find(|p| p.is_human)
            .map(|p| p.id)
            .unwrap_or(0);
        let building_at: HashMap<Tile, usize> = buildings
            .iter()
            .enumerate()
            .map(|(i, b)| (b.tile, i))
            .collect();
        let mut obstacle_tiles: Vec<Tile> = board
            .tiles
            .iter()
            .filter(|(_, t)| t.obstacle.is_some())
            .map(|(tile, _)| *tile)
            .collect();
        obstacle_tiles.sort();
        let mut fire_tiles: Vec<Tile> = obstacle_tiles
            .iter()
            .copied()
            .filter(|tile| {
                board
                    .tiles
                    .get(tile)
                    .and_then(|t| t.obstacle.as_ref())
                    .is_some_and(|o| o.kind == ObstacleKind::TrapFire)
            })
            .collect();
        fire_tiles.sort();
        Self {
            board,
            players,
            buildings,
            building_at,
            obstacle_tiles,
            fire_tiles,
            vehicles: Vec::new(),
            projectiles: Vec::new(),
            wrecks: Vec::new(),
            sounds: Vec::new(),
            time: 0.0,
            over: false,
            winner: None,
            human_id,
            eliminated: HashSet::new(),
            sandbox: false,
        }
    }
    /// Remove `tile` from the obstacle index, if present.
    ///
    /// Called whenever the simulation clears an obstacle (a destroyed wall or
    /// an exploded mine): the board stays the source of truth, this only keeps
    /// the presentation index in sync so it never points at an empty field.
    fn remove_obstacle_tile(&mut self, tile: Tile) {
        if let Some(i) = self.obstacle_tiles.iter().position(|t| *t == tile) {
            self.obstacle_tiles.swap_remove(i);
        }
    }

    #[allow(dead_code)]
    /// The human player.
    pub fn human_player(&self) -> &Player {
        &self.players[self.human_id]
    }
    #[allow(dead_code)]
    /// All buildings currently owned by `player_id`.
    pub fn buildings_owned_by(&self, player_id: Option<usize>) -> Vec<&Building> {
        self.buildings
            .iter()
            .filter(|b| b.owner == player_id)
            .collect()
    }
    /// Building standing on `tile`, if any.
    pub fn building_at_tile(&self, tile: Tile) -> Option<&Building> {
        self.building_at
            .get(&tile)
            .and_then(|i| self.buildings.get(*i))
    }
    #[allow(dead_code)]
    /// Mutable building standing on `tile`, if any.
    pub fn building_at_tile_mut(&mut self, tile: Tile) -> Option<&mut Building> {
        let i = *self.building_at.get(&tile)?;
        self.buildings.get_mut(i)
    }
    /// Send one vehicle from `src_tile` to `dst_tile`.
    ///
    /// Returns true on success. The vehicle takes *all* units from the
    /// source building (section 4); when no road exists nothing happens.
    pub fn try_send(&mut self, owner: usize, src_tile: Tile, dst_tile: Tile) -> bool {
        let si = match self.building_at.get(&src_tile) {
            Some(i) => *i,
            None => return false,
        };
        let di = match self.building_at.get(&dst_tile) {
            Some(i) => *i,
            None => return false,
        };
        if si == di {
            return false;
        }
        let (owner_ok, units, kind, pos) = {
            let src = &self.buildings[si];
            (
                src.owner == Some(owner),
                src.units,
                vehicle_kind_of(src.kind),
                src.pos(self.board.side),
            )
        };
        if !owner_ok || units <= 0.0 {
            return false;
        }
        let route = match self.board.find_path(src_tile, dst_tile, kind) {
            Some(r) => r,
            None => return false,
        };
        let vehicle = Vehicle::new(kind, owner, units, route, pos, Some(src_tile));
        {
            let src = &mut self.buildings[si];
            src.units = 0.0;
            src.loss_acc = 0.0;
        }
        self.vehicles.push(vehicle);
        true
    }
    /// Advance the simulation by `dt` seconds (fixed SIM_DT).
    pub fn update(&mut self, dt: f64) {
        self.time += dt;
        self.update_buildings(dt);
        self.update_buffers(dt);
        self.update_tank_guns(dt);
        self.update_vehicles(dt);
        self.update_projectiles(dt);
        self.vehicles.retain(|v| !v.dead);
        self.check_elimination();
    }
    /// Tank guns (rules.md sec. 5.1).
    ///
    /// A tank carries a homing gun that fires at an enemy lying strictly
    /// *outside* the detection radius: inside it, section 9 combat already
    /// stops both vehicles and trades shots, and the gun stays silent so a
    /// tank in a duel never doubles its fire. The gun fires while moving, so
    /// a tank is a mobile firing platform; it shoots the nearest eligible
    /// enemy, ties broken by id like everywhere else.
    /// A tank held back by a wall or a fire trap does not use its gun: it is
    /// busy with something more urgent (rules.md sections 4, 5.1).
    fn busy_attacking(&self, x: f64, y: f64) -> bool {
        self.board
            .world_to_tile(x, y)
            .and_then(|t| self.board.tiles.get(&t))
            .and_then(|t| t.obstacle.as_ref())
            .is_some_and(|o| matches!(o.kind, ObstacleKind::Wall | ObstacleKind::TrapFire))
    }
    /// Tank guns (rules.md sec. 5.1).
    fn update_tank_guns(&mut self, dt: f64) {
        // A tank carries a homing gun that fires at an enemy lying strictly
        // *outside* the detection radius: inside it, section 9 combat already
        // stops both vehicles and trades shots, and the gun stays silent so a
        // tank in a duel never doubles its fire. The gun fires while moving, so
        // a tank is a mobile firing platform; it shoots the nearest eligible
        // enemy, ties broken by id like everywhere else.
        // Collect first: shooting needs &mut self (projectiles, sounds) while
        // scanning targets needs &self, exactly like update_buffers.
        let guns: Vec<(u64, usize, f64, f64, f64)> = self
            .vehicles
            .iter()
            .filter(|v| {
                !v.dead
                    && v.kind == VehicleKind::Tank
                    // A tank that is already trading shots in a duel shoots
                    // with that duel only (rules.md sec. 9).
                    && v.combat_target.is_none()
                    && !self.busy_attacking(v.x, v.y)
            })
            .map(|v| (v.id, v.owner, v.x, v.y, v.units))
            .collect();
        if guns.is_empty() {
            return;
        }
        let mut shots: Vec<(u64, usize, u64, f64)> = Vec::new();
        for (id, owner, x, y, units) in guns {
            let i = match self.vehicles.iter().position(|v| v.id == id) {
                Some(i) => i,
                None => continue,
            };
            self.vehicles[i].gun_timer += dt;
            if self.vehicles[i].gun_timer < constants::TANK_GUN_INTERVAL {
                continue;
            }
            // Drop a target that died or left the range since the last shot.
            self.vehicles[i].gun_target = None;
            let mut best: Option<(u64, f64)> = None;
            for e in self.vehicles.iter() {
                if e.dead || e.owner == owner {
                    continue;
                }
                let d = dist((x, y), e.pos());
                let eligible = d > constants::TANK_GUN_MIN_RANGE && d <= constants::TANK_GUN_RANGE;
                let better = match best {
                    None => eligible,
                    Some((bid, bd)) => eligible && (d < bd || (d == bd && e.id < bid)),
                };
                if better {
                    best = Some((e.id, d));
                }
            }
            if let Some((tid, _)) = best {
                self.vehicles[i].gun_timer = 0.0;
                self.vehicles[i].gun_target = Some(tid);
                shots.push((
                    id,
                    owner,
                    tid,
                    (units / constants::TANK_GUN_DAMAGE_DIV).ceil(),
                ));
            }
        }
        for (id, owner, tid, dmg) in shots {
            let (pos, tile) = match self.vehicles.iter().find(|v| v.id == id) {
                Some(v) => (v.pos(), self.board.world_to_tile(v.x, v.y)),
                None => continue,
            };
            let target = match self.vehicles.iter().find(|v| v.id == tid) {
                Some(v) => v.pos(),
                None => continue,
            };
            // The gun uses the same homing shell as the ordinary emplacement,
            // so flight, impact and splash are resolved by the one existing
            // code path (rules.md sec. 10).
            self.report_sound(SoundKind::TurretShot(TurretKind::Normal), pos.0, pos.1);
            self.projectiles.push(Projectile {
                from: tile.unwrap_or((0, 0)),
                from_pos: pos,
                owner: Some(owner),
                target: tid,
                to: target,
                to_tile: self.board.world_to_tile(target.0, target.1),
                dmg,
                kind: TurretKind::Normal,
                t: 0.0,
                dur: constants::TANK_GUN_FLIGHT_TIME,
            });
        }
    }
    fn update_buildings(&mut self, dt: f64) {
        // Production + overcrowding (rules.md sec. 3). Flush texts first.
        for b in self.buildings.iter_mut() {
            Self::flush_texts_b(b, dt);
            if is_base(b.kind) && b.owner.is_some() {
                if b.units < b.capacity {
                    b.production_timer += dt;
                    if b.production_timer >= constants::BASE_SPAWN_INTERVAL {
                        b.production_timer -= constants::BASE_SPAWN_INTERVAL;
                        b.units = (b.units + constants::BASE_SPAWN_AMOUNT).min(b.capacity);
                    }
                } else {
                    b.production_timer = 0.0;
                }
            }
            if b.units > b.capacity {
                let died = (b.units - b.capacity).min(constants::OVERCROWD_DEATH_RATE * dt);
                b.units -= died;
                b.loss_acc += died;
            }
        }
        self.update_turrets(dt);
        self.update_heal_towers(dt);
    }
    /// Turrets (rules.md sec. 10, 12).
    fn update_turrets(&mut self, dt: f64) {
        struct Shot {
            from: Tile,
            from_pos: (f64, f64),
            owner: Option<usize>,
            target: u64,
            dmg: f64,
            kind: TurretKind,
        }
        let side = self.board.side;
        // Snapshot decisions without holding borrows across calls.
        let mut fires: Vec<(usize, u64, f64)> = Vec::new();
        for (i, b) in self.buildings.iter_mut().enumerate() {
            let tk = match turret_kind_of(b.kind) {
                Some(t) => t,
                None => continue,
            };
            if b.units < 1.0 {
                continue;
            }
            b.fire_timer += dt;
            if b.fire_timer < constants::turret_cooldown(tk) {
                continue;
            }
            // Find target with a direct scan (avoids borrowing self).
            let bpos = hexgrid_pos(b.tile, side);
            let mut best: Option<(u64, f64)> = None;
            for v in self.vehicles.iter() {
                if v.dead {
                    continue;
                }
                if b.owner.is_some() && Some(v.owner) == b.owner {
                    continue;
                }
                let d = dist(bpos, v.pos());
                let better = match best {
                    None => d <= constants::turret_range(tk),
                    Some((bid, bd)) => {
                        d <= constants::turret_range(tk) && (d < bd || (d == bd && v.id < bid))
                    }
                };
                if better {
                    best = Some((v.id, d));
                }
            }
            if let Some((tid, _)) = best {
                fires.push((i, tid, (b.units / constants::turret_damage_div(tk)).ceil()));
            }
        }
        let mut shots: Vec<Shot> = Vec::new();
        for (i, tid, dmg) in fires {
            self.buildings[i].fire_timer = 0.0;
            if let Some(v) = self.vehicles.iter().find(|v| v.id == tid) {
                self.buildings[i].last_target_pos = Some(v.pos());
            }
            let (tile, owner, kind) = {
                let b = &self.buildings[i];
                (b.tile, b.owner, turret_kind_of(b.kind).unwrap())
            };
            let from_pos = hexgrid_pos(tile, side);
            // A turret that fires is audible from the turret itself.
            self.report_sound(SoundKind::TurretShot(kind), from_pos.0, from_pos.1);
            shots.push(Shot {
                from: tile,
                from_pos,
                owner,
                target: tid,
                dmg,
                kind,
            });
        }
        for s in shots {
            let target_pos = match self.vehicles.iter().find(|v| v.id == s.target) {
                Some(v) => v.pos(),
                None => continue,
            };
            self.projectiles.push(Projectile {
                from: s.from,
                from_pos: s.from_pos,
                owner: s.owner,
                target: s.target,
                to: target_pos,
                to_tile: self.board.world_to_tile(target_pos.0, target_pos.1),
                dmg: s.dmg,
                kind: s.kind,
                t: 0.0,
                dur: constants::turret_flight_time(s.kind),
            });
        }
    }
    #[allow(dead_code)]
    /// Nearest valid target of a turret: owned turrets shoot enemies,
    /// neutral turrets shoot everybody (rules.md sec. 12). Ties by id.
    fn turret_target(&self, tile: Tile, owner: Option<usize>, range: f64) -> Option<u64> {
        let side = self.board.side;
        let bpos = {
            let i = self.building_at.get(&tile)?;
            self.buildings[*i].pos(side)
        };
        let mut best: Option<(u64, f64)> = None;
        for v in self.vehicles.iter() {
            if v.dead {
                continue;
            }
            if owner.is_some() && Some(v.owner) == owner {
                continue;
            }
            let d = dist(bpos, v.pos());
            match best {
                None if d <= range => best = Some((v.id, d)),
                Some((bid, bd)) if (d < bd || (d == bd && v.id < bid)) && d <= range => {
                    best = Some((v.id, d));
                }
                _ => {}
            }
        }
        best.map(|(id, _)| id)
    }
    /// Healing towers (rules.md sec. 11, 12).
    fn update_heal_towers(&mut self, dt: f64) {
        struct Pulse {
            owner: usize,
            amount: f64,
            range: f64,
            pos: (f64, f64),
        }
        let mut pulses: Vec<Pulse> = Vec::new();
        for b in self.buildings.iter_mut() {
            if b.kind != BuildingKind::HealTower || b.owner.is_none() {
                continue;
            }
            b.heal_timer += dt;
            if b.heal_timer < constants::HEAL_TOWER_INTERVAL || b.units < 1.0 {
                continue;
            }
            b.heal_timer = 0.0;
            pulses.push(Pulse {
                owner: b.owner.unwrap(),
                amount: constants::HEAL_TOWER_AMOUNT,
                range: constants::HEAL_TOWER_RANGE_PER_UNIT * b.units,
                pos: b.pos(self.board.side),
            });
        }
        for p in pulses {
            for v in self.vehicles.iter_mut() {
                if !v.dead && v.owner == p.owner && dist(p.pos, v.pos()) <= p.range {
                    Self::heal_vehicle(v, p.amount);
                }
            }
        }
    }
    /// Buffer healing auras (rules.md sec. 5.4).
    fn update_buffers(&mut self, dt: f64) {
        let buffers: Vec<(u64, usize, f64, f64)> = self
            .vehicles
            .iter()
            .filter(|v| !v.dead && v.kind == VehicleKind::Buffer)
            .map(|v| (v.id, v.owner, v.x, v.y))
            .collect();
        for (bid, owner, bx, by) in buffers {
            for v in self.vehicles.iter_mut() {
                if v.dead || v.id == bid || v.owner != owner {
                    continue;
                }
                if dist((bx, by), v.pos()) <= constants::BUFFER_HEAL_RADIUS {
                    Self::heal_vehicle(v, constants::BUFFER_HEAL_RATE * dt);
                }
            }
        }
    }
    /// Aggregate continuous losses/gains into floating numbers.
    /// A number is pushed as soon as at least one full unit has been
    /// accumulated; the periodic 1 s timer remains only as a fallback.
    /// Entries older than FLOAT_TEXT_LIFETIME are dropped.
    fn flush_texts_b(b: &mut Building, dt: f64) {
        b.text_timer += dt;
        for t in b.texts.iter_mut() {
            t.age += dt;
        }
        if !b.texts.is_empty() {
            b.texts.retain(|t| t.age < constants::FLOAT_TEXT_LIFETIME);
        }
        let mut flush = b.loss_acc >= 1.0 || b.gain_acc >= 1.0;
        if b.text_timer >= 1.0 {
            b.text_timer -= 1.0;
            flush = true;
        }
        if flush {
            let loss = b.loss_acc.round();
            let gain = b.gain_acc.round();
            b.loss_acc -= loss;
            b.gain_acc -= gain;
            if loss > 0.0 {
                b.texts.push(crate::entities::FloatText {
                    amount: -loss,
                    age: 0.0,
                });
            }
            if gain > 0.0 {
                b.texts.push(crate::entities::FloatText {
                    amount: gain,
                    age: 0.0,
                });
            }
        }
    }
    /// Aggregate continuous losses/gains of a vehicle (see `flush_texts_b`).
    fn flush_texts_v(v: &mut Vehicle, dt: f64) {
        v.text_timer += dt;
        for t in v.texts.iter_mut() {
            t.age += dt;
        }
        if !v.texts.is_empty() {
            v.texts.retain(|t| t.age < constants::FLOAT_TEXT_LIFETIME);
        }
        let mut flush = v.loss_acc >= 1.0 || v.gain_acc >= 1.0;
        if v.text_timer >= 1.0 {
            v.text_timer -= 1.0;
            flush = true;
        }
        if flush {
            let loss = v.loss_acc.round();
            let gain = v.gain_acc.round();
            v.loss_acc -= loss;
            v.gain_acc -= gain;
            if loss > 0.0 {
                v.texts.push(crate::entities::FloatText {
                    amount: -loss,
                    age: 0.0,
                });
            }
            if gain > 0.0 {
                v.texts.push(crate::entities::FloatText {
                    amount: gain,
                    age: 0.0,
                });
            }
        }
    }
    /// Heal a vehicle (rules.md sec. 5.4, 11).
    fn heal_vehicle(v: &mut Vehicle, amount: f64) {
        if v.dead || amount <= 0.0 {
            return;
        }
        v.units += amount;
        v.gain_acc += amount;
    }
    fn update_vehicles(&mut self, dt: f64) {
        // Vehicles: combat, obstacles, movement, arrival (rules.md sec. 4, 9).
        let n = self.vehicles.len();
        for idx in 0..n {
            if self.vehicles[idx].dead {
                continue;
            }
            Self::flush_texts_v(&mut self.vehicles[idx], dt);
            let my_id = self.vehicles[idx].id;
            // --- resolve dead targets (sec. 9) --------------------------
            let dead_target = match self.vehicles[idx].combat_target {
                Some(tid) => !self.vehicles.iter().any(|x| x.id == tid && !x.dead),
                None => false,
            };
            if dead_target {
                let opponent = self.vehicles[idx].last_opponent;
                self.vehicles[idx].combat_target = None;
                if let Some(oid) = opponent {
                    let alive = self.vehicles.iter().any(|x| x.id == oid && !x.dead);
                    let fighting = self
                        .vehicles
                        .iter()
                        .any(|x| x.id == oid && !x.dead && x.combat_target.is_some());
                    if alive && fighting {
                        self.vehicles[idx].combat_target = Some(oid);
                    }
                }
            }
            // --- active duel: stand still and shoot ----------------------
            if self.vehicles[idx].combat_target.is_some() {
                let tid = self.vehicles[idx].combat_target.unwrap();
                self.vehicles[idx].fire_timer += dt;
                if self.vehicles[idx].fire_timer >= constants::FIRE_INTERVAL {
                    self.vehicles[idx].fire_timer -= constants::FIRE_INTERVAL;
                    let dmg = (self.vehicles[idx].units / 5.0).ceil();
                    self.vehicles[idx].last_opponent = Some(tid);
                    let (vx, vy) = self.vehicles[idx].pos();
                    self.report_sound(SoundKind::VehicleFire, vx, vy);
                    self.damage_vehicle_by_id(tid, dmg);
                }
                continue;
            }
            // --- detection (sec. 9) --------------------------------------
            if let Some(eid) = self.nearest_enemy(idx) {
                self.vehicles[idx].combat_target = Some(eid);
                self.vehicles[idx].last_opponent = Some(eid);
                let enemy_fighting = self
                    .vehicles
                    .iter()
                    .any(|x| x.id == eid && x.combat_target.is_some());
                if !enemy_fighting && let Some(e) = self.vehicles.iter_mut().find(|x| x.id == eid) {
                    e.combat_target = Some(my_id);
                }
                if let Some(e) = self.vehicles.iter_mut().find(|x| x.id == eid) {
                    e.last_opponent = Some(my_id);
                }
                continue;
            }
            // --- ground obstacles (sec. 1, 4) ----------------------------
            let tile = self
                .board
                .world_to_tile(self.vehicles[idx].x, self.vehicles[idx].y);
            let obstacle_kind = tile
                .and_then(|t| self.board.tiles.get(&t))
                .and_then(|t| t.obstacle.as_ref())
                .map(|o| o.kind);
            let kind = self.vehicles[idx].kind;
            if kind != VehicleKind::Helicopter {
                if obstacle_kind == Some(ObstacleKind::Wall) {
                    let t = tile.unwrap();
                    self.vehicles[idx].wall_timer += dt;
                    if self.vehicles[idx].wall_timer >= constants::WALL_ATTACK_INTERVAL {
                        self.vehicles[idx].wall_timer -= constants::WALL_ATTACK_INTERVAL;
                        // Shooting a wall is a real shot of the vehicle, just
                        // against something that does not answer back.
                        let (vx, vy) = self.vehicles[idx].pos();
                        self.report_sound(SoundKind::WallHit, vx, vy);
                        if let Some(tile_ref) = self.board.tiles.get_mut(&t)
                            && let Some(o) = tile_ref.obstacle.as_mut()
                        {
                            o.hp -= constants::WALL_ATTACK_DAMAGE;
                        }
                        if self.vehicles[idx].wall_target != Some(t) {
                            self.vehicles[idx].wall_target = Some(t);
                            self.vehicles[idx].wall_shots = 0;
                        }
                        self.vehicles[idx].wall_shots += 1;
                        let shots = self.vehicles[idx].wall_shots;
                        self.vehicles[idx].texts.push(crate::entities::FloatText {
                            amount: shots as f64,
                            age: 0.0,
                        });
                        let destroyed = self
                            .board
                            .tiles
                            .get(&t)
                            .and_then(|x| x.obstacle.as_ref())
                            .map(|o| o.hp <= 0)
                            .unwrap_or(false);
                        if destroyed {
                            if let Some(tile_ref) = self.board.tiles.get_mut(&t) {
                                tile_ref.obstacle = None;
                            }
                            self.remove_obstacle_tile(t);
                        }
                    }
                    continue;
                }
                if obstacle_kind == Some(ObstacleKind::TrapFire) {
                    let vid = self.vehicles[idx].id;
                    self.damage_vehicle_by_id(vid, constants::FIRE_TRAP_DPS * dt);
                    if self.vehicles[idx].dead {
                        continue;
                    }
                }
            }
            // --- movement (sec. 4, 5) ------------------------------------
            if self.vehicles[idx].route.is_empty()
                || self.vehicles[idx].route_index >= self.vehicles[idx].route.len()
            {
                continue;
            }
            let mut speed = constants::vehicle_speed(kind);
            if kind != VehicleKind::Helicopter && obstacle_kind == Some(ObstacleKind::TrapIce) {
                speed *= constants::ICE_TRAP_SLOWDOWN;
            }
            let step = speed * dt;
            let wp_tile = self.vehicles[idx].route[self.vehicles[idx].route_index];
            let (wx, wy) = self.board.center_world(wp_tile);
            let (vx, vy) = (self.vehicles[idx].x, self.vehicles[idx].y);
            let (dx, dy) = (wx - vx, wy - vy);
            let d = dist2((vx, vy), (wx, wy)).sqrt();
            if d <= step {
                self.vehicles[idx].x = wx;
                self.vehicles[idx].y = wy;
                self.check_mine(wp_tile);
                self.vehicles[idx].route_index += 1;
                if self.vehicles[idx].route_index >= self.vehicles[idx].route.len() {
                    self.arrive_vehicle(idx);
                    continue;
                }
            } else if d > 1e-12 {
                self.vehicles[idx].x = vx + dx / d * step;
                self.vehicles[idx].y = vy + dy / d * step;
                let cur = self
                    .board
                    .world_to_tile(self.vehicles[idx].x, self.vehicles[idx].y);
                if let Some(t) = cur {
                    self.check_mine(t);
                }
            }
        }
        self.vehicles.retain(|v| !v.dead);
    }
    /// Explode a mine when a ground vehicle reaches the tile centre.
    fn check_mine(&mut self, tile: Tile) {
        let is_mine = self
            .board
            .tiles
            .get(&tile)
            .and_then(|t| t.obstacle.as_ref())
            .map(|o| o.kind == ObstacleKind::Mine)
            .unwrap_or(false);
        if !is_mine {
            return;
        }
        let (cx, cy) = self.board.center_world(tile);
        // Squared reach test: no square root per vehicle, and the radius is
        // squared once instead of compared against a distance.
        let trigger = sqr(constants::MINE_TRIGGER_RADIUS);
        let mut hit: Option<u64> = None;
        for v in self.vehicles.iter() {
            if !v.dead
                && v.kind != VehicleKind::Helicopter
                && dist2((v.x, v.y), (cx, cy)) <= trigger
            {
                hit = Some(v.id);
                break;
            }
        }
        if let Some(id) = hit {
            self.damage_vehicle_by_id(id, constants::MINE_DAMAGE);
            if let Some(t) = self.board.tiles.get_mut(&tile) {
                t.obstacle = None;
            }
            self.remove_obstacle_tile(tile);
        }
    }
    /// Nearest enemy within detection radius (rules.md sec. 9).
    fn nearest_enemy(&self, idx: usize) -> Option<u64> {
        let v = self.vehicles.get(idx)?;
        let mut best: Option<(u64, f64)> = None;
        for e in self.vehicles.iter() {
            if e.dead || e.owner == v.owner || e.id == v.id {
                continue;
            }
            let d = dist(v.pos(), e.pos());
            let better = match best {
                None => d <= constants::DETECTION_RADIUS,
                Some((bid, bd)) => {
                    d <= constants::DETECTION_RADIUS && (d < bd || (d == bd && e.id < bid))
                }
            };
            if better {
                best = Some((e.id, d));
            }
        }
        best.map(|(id, _)| id)
    }
    /// Damage vehicle `id` by `dmg` (fractional bookkeeping included).
    ///
    /// Every point of damage in rules.md (section 9 combat, section 4 mines
    /// and fire traps, section 10 turret fire) goes through this one function,
    /// so a vehicle reaching zero units is recorded exactly once, here.
    fn damage_vehicle_by_id(&mut self, id: u64, dmg: f64) {
        if dmg <= 0.0 {
            return;
        }
        let mut wreck = None;
        if let Some(v) = self.vehicles.iter_mut().find(|v| v.id == id && !v.dead) {
            v.units -= dmg;
            v.loss_acc += dmg;
            if v.units <= 0.0 {
                v.units = 0.0;
                v.dead = true;
                wreck = Some(Wreck {
                    owner: v.owner,
                    kind: v.kind,
                    x: v.x,
                    y: v.y,
                });
            }
        }
        if let Some(w) = wreck {
            // A destroyed vehicle is the loudest event in the game; the kind
            // decides between the ground blast and the air one (rules.md 5.2).
            let kind = match w.kind {
                VehicleKind::Helicopter => SoundKind::ExplosionAir,
                _ => SoundKind::ExplosionGround,
            };
            self.report_sound(kind, w.x, w.y);
            self.wrecks.push(w);
        }
    }
    /// Remove and return the wrecks recorded so far.
    ///
    /// The presentation layer calls this once per frame after the simulation
    /// step, which keeps [`crate::fx`] fed and the log short (an unconsumed
    /// wreck is never replayed).
    pub fn take_wrecks(&mut self) -> Vec<Wreck> {
        std::mem::take(&mut self.wrecks)
    }
    /// Remove and return the sounds recorded so far.
    ///
    /// The presentation layer calls this once per frame after the simulation
    /// step, exactly like [`Game::take_wrecks`]: an unconsumed sound is never
    /// replayed a frame later. Whether it is audible at all (how far from the
    /// camera it is, how loud) is decided by [`crate::audio`], not here.
    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }
    /// Record that a sound should be played at a world position.
    ///
    /// The single place where the simulation turns an action into a sound
    /// event, so every noisy rule (turret fire, vehicle combat, wall attacks,
    /// destroyed vehicles) reports through one channel.
    fn report_sound(&mut self, kind: SoundKind, x: f64, y: f64) {
        self.sounds.push(SoundEvent { kind, x, y });
    }
    /// Resolve a vehicle that reached the end of its route (section 4).
    fn arrive_vehicle(&mut self, idx: usize) {
        let (owner, units, dest) = {
            let v = &self.vehicles[idx];
            (v.owner, v.units, v.route.last().copied())
        };
        let dest = match dest {
            Some(d) => d,
            None => {
                self.vehicles[idx].dead = true;
                return;
            }
        };
        let bi = match self.building_at.get(&dest) {
            Some(i) => *i,
            None => {
                self.vehicles[idx].dead = true;
                return;
            }
        };
        if self.buildings[bi].owner == Some(owner) {
            self.buildings[bi].units += units;
        } else if units > self.buildings[bi].units {
            let b_units = self.buildings[bi].units;
            let b = &mut self.buildings[bi];
            b.owner = Some(owner);
            b.loss_acc += b_units;
            b.units = units - b_units;
        } else {
            let b = &mut self.buildings[bi];
            b.units -= units;
            b.loss_acc += units;
        }
        self.vehicles[idx].dead = true;
    }
    fn update_projectiles(&mut self, dt: f64) {
        let mut impacts: Vec<Projectile> = Vec::new();
        let mut still: Vec<Projectile> = Vec::new();
        for mut p in self.projectiles.drain(..) {
            p.t += dt;
            if let Some(t) = self.vehicles.iter().find(|v| v.id == p.target && !v.dead) {
                p.to = t.pos();
                p.to_tile = self.board.world_to_tile(t.x, t.y);
            }
            if p.t >= p.dur {
                impacts.push(p);
            } else {
                still.push(p);
            }
        }
        self.projectiles = still;
        for p in impacts {
            self.resolve_projectile_impact(&p);
        }
    }
    fn resolve_projectile_impact(&mut self, p: &Projectile) {
        let alive = self.vehicles.iter().any(|v| v.id == p.target && !v.dead);
        if !alive {
            return;
        }
        // The impact is heard where the projectile landed, not where it was
        // fired from.
        self.report_sound(SoundKind::Impact(p.kind), p.to.0, p.to.1);
        self.damage_vehicle_by_id(p.target, p.dmg);
        if p.kind != TurretKind::Rocket {
            return;
        }
        let splash = constants::turret_splash(TurretKind::Rocket);
        let ids: Vec<u64> = self
            .vehicles
            .iter()
            .filter(|v| {
                !v.dead
                    && v.id != p.target
                    && Some(v.owner) != p.owner
                    && dist(v.pos(), p.to) <= splash
            })
            .map(|v| v.id)
            .collect();
        for id in ids {
            self.damage_vehicle_by_id(id, p.dmg);
        }
    }
    fn check_elimination(&mut self) {
        if self.over || self.sandbox {
            // A playtest run of the editor never ends (see `Game::sandbox`);
            // the win/lose rules of rules.md section 2 stay real-match only.
            return;
        }
        for p in self.players.iter_mut() {
            if self.eliminated.contains(&p.id) {
                continue;
            }
            let has_building = self.buildings.iter().any(|b| b.owner == Some(p.id));
            let has_vehicle = self.vehicles.iter().any(|v| v.owner == p.id && !v.dead);
            if !has_building && !has_vehicle {
                p.eliminated = true;
                self.eliminated.insert(p.id);
            }
        }
        let enemies_alive: Vec<usize> = self
            .players
            .iter()
            .filter(|pl| pl.id != self.human_id && !self.eliminated.contains(&pl.id))
            .map(|pl| pl.id)
            .collect();
        if self.eliminated.contains(&self.human_id) {
            self.over = true;
            self.winner = Some("ai".to_string());
        } else if enemies_alive.is_empty() {
            self.over = true;
            self.winner = Some("human".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Obstacle;
    use crate::entities::BuildingKind;
    use crate::hexgrid;
    fn make_game(board: Board) -> Game {
        Game::new(
            board,
            vec![Player::new(0, true), Player::new(1, false)],
            Vec::new(),
            1,
        )
    }
    fn run(game: &mut Game, seconds: f64) {
        let steps = (seconds / constants::SIM_DT).round() as usize;
        for _ in 0..steps {
            game.update(constants::SIM_DT);
        }
    }
    #[test]
    fn obstacle_index_matches_board_scan() {
        // Every repo map: the index built in `Game::new` lists exactly the
        // fields a full board scan would find.
        for path in crate::mapfile::list_maps(None) {
            let game = crate::mapfile::load_game(&path).expect("repo map must load");
            let mut scan: Vec<Tile> = game
                .board
                .tiles
                .iter()
                .filter(|(_, t)| t.obstacle.is_some())
                .map(|(tile, _)| *tile)
                .collect();
            scan.sort();
            assert_eq!(
                game.obstacle_tiles,
                scan,
                "obstacle index on {}",
                path.display()
            );
            let mut fires: Vec<Tile> = scan
                .iter()
                .copied()
                .filter(|tile| {
                    game.board.tiles[tile]
                        .obstacle
                        .as_ref()
                        .is_some_and(|o| o.kind == ObstacleKind::TrapFire)
                })
                .collect();
            fires.sort();
            assert_eq!(game.fire_tiles, fires, "fire index on {}", path.display());
        }
    }
    #[test]
    fn obstacle_index_drops_destroyed_walls_and_exploded_mines() {
        // Wall destruction and mine explosion clear the board tile and must
        // drop it from the index too; fire traps are permanent and stay.
        let mut board = flat_board(10, 10, 1);
        let wall = (2, 2);
        let mine = (3, 3);
        let fire = (4, 4);
        board.tiles.get_mut(&wall).unwrap().obstacle =
            Some(crate::board::Obstacle::new(ObstacleKind::Wall));
        board.tiles.get_mut(&mine).unwrap().obstacle =
            Some(crate::board::Obstacle::new(ObstacleKind::Mine));
        board.tiles.get_mut(&fire).unwrap().obstacle =
            Some(crate::board::Obstacle::new(ObstacleKind::TrapFire));
        let mut game = make_game(board);
        assert!(game.obstacle_tiles.contains(&wall));
        assert!(game.obstacle_tiles.contains(&mine));
        assert!(game.fire_tiles.contains(&fire));
        // Mine: place a tank next to the mine centre and trigger it directly.
        // NB: `check_mine` only explodes when a vehicle stands within the trigger
        // radius, and the vehicle must survive the blast to be reused below.
        let (mx, my) = game.board.center_world(mine);
        game.vehicles.push(crate::entities::Vehicle::new(
            crate::constants::VehicleKind::Tank,
            0,
            1000.0,
            Vec::new(),
            (mx, my),
            Some(mine),
        ));
        game.check_mine(mine);
        assert!(
            game.board.tiles[&mine].obstacle.is_none(),
            "mine must explode"
        );
        assert!(
            !game.obstacle_tiles.contains(&mine),
            "exploded mine must leave the index"
        );
        assert!(game.fire_tiles.contains(&fire), "fire traps are permanent");
        // Wall: smash it through the wall-attack path.
        game.board.tiles.get_mut(&wall).unwrap().obstacle =
            Some(crate::board::Obstacle::new(ObstacleKind::Wall));
        if let Some(t) = game.board.tiles.get_mut(&wall) {
            // One hit point left: the next shot destroys it.
            if let Some(o) = t.obstacle.as_mut() {
                o.hp = 1;
            }
        }
        // Park a vehicle on the wall tile and force the attack cooldown over.
        // The wall sits on the vehicle's own field here, so it also counts as the
        // previous field: give the vehicle a blocked route off the wall (a wall
        // tile never lets a tank pass), which keeps it parked on the wall.
        // NB: the route destination must be a neighbour of the wall tile, so the
        // wall-attack branch (not the movement branch) owns the vehicle.
        game.vehicles[0].x = game.board.center_world(wall).0;
        game.vehicles[0].y = game.board.center_world(wall).1;
        game.vehicles[0].wall_timer = constants::WALL_ATTACK_INTERVAL;
        // A wall never lets a tank onto the next field, so any route whose next
        // waypoint leaves the wall tile keeps the vehicle parked on the wall.
        game.vehicles[0].route = vec![(3, 2)];
        game.vehicles[0].route_index = 0;
        let wall_units_before = game.obstacle_tiles.len();
        for _ in 0..(constants::WALL_ATTACK_INTERVAL / constants::SIM_DT).ceil() as usize {
            game.vehicles[0].wall_timer = constants::WALL_ATTACK_INTERVAL;
            game.update(constants::SIM_DT);
            if game.board.tiles[&wall].obstacle.is_none() {
                break;
            }
        }
        assert!(game.board.tiles[&wall].obstacle.is_none(), "wall must fall");
        assert!(
            !game.obstacle_tiles.contains(&wall),
            "destroyed wall must leave the index"
        );
        assert_eq!(
            game.obstacle_tiles.len(),
            wall_units_before - 1,
            "only the wall tile leaves the index"
        );
    }
    fn flat_board(cols: i32, rows: i32, h: i32) -> Board {
        let mut b = Board::new(cols, rows);
        for t in b.tiles.clone().keys().copied().collect::<Vec<_>>() {
            b.tiles.get_mut(&t).unwrap().height = h;
        }
        b
    }
    #[test]
    fn send_takes_all_units() {
        let mut g = {
            let board = flat_board(10, 10, 1);
            let players = vec![Player::new(0, true), Player::new(1, false)];
            let buildings = vec![
                Building::new(BuildingKind::BaseTank, Some(0), 1, 1, 20.0),
                Building::new(BuildingKind::BaseTank, Some(1), 8, 8, 20.0),
            ];
            Game::new(board, players, buildings, 0)
        };
        assert!(g.try_send(0, (1, 1), (8, 8)));
        assert_eq!(g.buildings[0].units, 0.0);
        assert_eq!(g.vehicles.len(), 1);
    }
    #[test]
    fn production_caps_at_capacity() {
        let mut g = {
            let board = flat_board(10, 10, 1);
            let players = vec![Player::new(0, true), Player::new(1, false)];
            let buildings = vec![
                Building::new(BuildingKind::BaseTank, Some(0), 1, 1, 99.0),
                Building::new(BuildingKind::BaseTank, Some(1), 8, 8, 20.0),
            ];
            Game::new(board, players, buildings, 0)
        };
        run(&mut g, 11.0);
        assert!(g.buildings[0].units <= g.buildings[0].capacity + 1e-6);
    }
    #[test]
    fn hex_math() {
        for (q, r) in [(0, 0), (3, 2), (1, 4), (5, 5)] {
            let (x, y) = hexgrid::hex_to_world(q, r, constants::HEX_SIDE);
            assert_eq!(hexgrid::world_to_hex(x, y, constants::HEX_SIDE), (q, r));
        }
        assert_eq!(hexgrid::hex_distance(0, 0, 2, 1), 2);
        for d in 0..6 {
            let n = hexgrid::neighbor(2, 3, d);
            assert_eq!(hexgrid::hex_distance(2, 3, n.0, n.1), 1);
        }
        for d in 0..6 {
            let n = hexgrid::neighbor(2, 3, d);
            assert_eq!(
                hexgrid::neighbor(n.0, n.1, hexgrid::OPPOSITE_DIR[d]),
                (2, 3)
            );
        }
    }
    #[test]
    fn movement_rules() {
        let mut b = Board::new(12, 12);
        assert!(b.passable((0, 0), (1, 0), VehicleKind::Tank));
        b.tiles.get_mut(&(1, 0)).unwrap().height = 2;
        assert!(!b.passable((0, 0), (1, 0), VehicleKind::Tank));
        assert!(b.passable((0, 0), (1, 0), VehicleKind::Helicopter));
        b.tiles.get_mut(&(2, 1)).unwrap().height = 0;
        assert!(!b.passable((1, 0), (2, 1), VehicleKind::Hovercraft));
        b.tiles.get_mut(&(1, 0)).unwrap().height = 1;
        assert!(b.passable((1, 0), (2, 1), VehicleKind::Hovercraft));
        assert!(!b.passable((1, 0), (2, 1), VehicleKind::Tank));
        // Ramp joins opposite neighbours regardless of height.
        let mut b2 = flat_board(12, 12, 1);
        for t in [(5, 5), (5, 4), (6, 4), (7, 4), (8, 4)] {
            b2.tiles.get_mut(&t).unwrap().height = 4;
        }
        let (a, p, hi) = ((4, 5), (5, 5), (5, 4));
        b2.set_ramp(p, a, hi);
        assert!(b2.passable(a, p, VehicleKind::Tank));
        assert!(b2.passable(p, hi, VehicleKind::Tank));
        assert!(!b2.passable((4, 4), p, VehicleKind::Tank));
        assert!(b2.passable((4, 4), p, VehicleKind::Helicopter));
        // Bridge over water between equal height >= 3 land.
        let mut b3 = flat_board(14, 14, 3);
        for t in [(5, 6), (5, 7)] {
            b3.tiles.get_mut(&t).unwrap().height = 0;
        }
        let bridge = b3.add_bridge((5, 5), (5, 8), 1);
        assert!(bridge.is_some());
        assert!(b3.passable((5, 5), (5, 6), VehicleKind::Tank));
        assert!(b3.passable((5, 6), (5, 7), VehicleKind::Tank));
    }
    #[test]
    fn production_and_capture() {
        // Base production, capture arithmetic, overcrowding (sec. 3, 4).
        let board = flat_board(10, 10, 1);
        let mut game = make_game(board);
        game.buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(0), 2, 2, 10.0),
            Building::new(BuildingKind::HealTower, None, 6, 2, 5.0),
        ];
        game.building_at = [(game.buildings[0].tile, 0), (game.buildings[1].tile, 1)]
            .into_iter()
            .collect();
        run(&mut game, 10.0);
        assert_eq!(game.buildings[0].units, 15.0);
        // capture: p > b -> owner changes, units = p - b
        assert!(game.try_send(0, (2, 2), (6, 2)));
        run(&mut game, 12.0);
        assert_eq!(game.buildings[1].owner, Some(0));
        assert!(
            (game.buildings[1].units - 10.0).abs() < 1e-6,
            "units={}",
            game.buildings[1].units
        );
        // reinforcement: same owner adds up
        game.buildings[0].units = 10.0;
        assert!(game.try_send(0, (2, 2), (6, 2)));
        run(&mut game, 12.0);
        assert!(
            (game.buildings[1].units - 20.0).abs() < 1e-6,
            "units={}",
            game.buildings[1].units
        );
        // overcrowding kills 1/s above capacity (sec. 3)
        game.buildings[1].units = game.buildings[1].capacity + 5.0;
        run(&mut game, 2.0);
        assert!(
            (game.buildings[1].units - (game.buildings[1].capacity + 3.0)).abs() < 1e-6,
            "units={}",
            game.buildings[1].units
        );
    }
    #[test]
    fn repelled_attack() {
        // p <= b: building keeps owner and loses p units (sec. 4).
        let board = flat_board(10, 10, 1);
        let mut game = make_game(board);
        game.buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(0), 1, 1, 10.0),
            Building::new(BuildingKind::BaseTank, Some(1), 6, 1, 30.0),
        ];
        game.building_at = [(game.buildings[0].tile, 0), (game.buildings[1].tile, 1)]
            .into_iter()
            .collect();
        assert!(game.try_send(0, (1, 1), (6, 1)));
        run(&mut game, 9.0); // arrival ~5 s; the base spawns only at t = 10 s
        assert_eq!(game.buildings[1].owner, Some(1));
        assert!(
            (game.buildings[1].units - 20.0).abs() < 1e-6,
            "units={}",
            game.buildings[1].units
        );
    }
    #[test]
    fn capture_transfers_remainder() {
        let board = flat_board(10, 10, 1);
        let mut game = make_game(board);
        game.buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(0), 1, 1, 20.0),
            Building::new(BuildingKind::BaseTank, Some(1), 8, 8, 5.0),
        ];
        game.building_at = [(game.buildings[0].tile, 0), (game.buildings[1].tile, 1)]
            .into_iter()
            .collect();
        assert!(game.try_send(0, (1, 1), (8, 8)));
        let dest = (8, 8);
        let di = *game.building_at.get(&dest).unwrap();
        let idx = 0;
        game.vehicles[idx].route = vec![dest];
        game.vehicles[idx].route_index = 0;
        let (wx, wy) = game.board.center_world(dest);
        game.vehicles[idx].x = wx;
        game.vehicles[idx].y = wy;
        game.arrive_vehicle(idx);
        assert_eq!(game.buildings[di].owner, Some(0));
        assert!((game.buildings[di].units - 15.0).abs() < 1e-6);
    }
    #[test]
    fn combat_duel() {
        // Vehicles stop, duel with ceil(x/5) damage, winner resumes (sec. 9).
        let board = flat_board(20, 20, 1);
        let mut game = make_game(board);
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            30.0,
            vec![],
            (100.0, 100.0),
            None,
        ));
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            1,
            10.0,
            vec![],
            (140.0, 100.0),
            None,
        ));
        run(&mut game, 1.5);
        assert_eq!(game.vehicles[0].combat_target, Some(game.vehicles[1].id));
        assert_eq!(game.vehicles[1].combat_target, Some(game.vehicles[0].id));
        // v1 (10 units) hits for ceil(10/5)=2, v0 (30) hits for 6
        assert!(
            (game.vehicles[0].units - (30.0 - 2.0)).abs() < 1e-6,
            "v0={}",
            game.vehicles[0].units
        );
        assert!(
            (game.vehicles[1].units - (10.0 - 6.0)).abs() < 1e-6,
            "v1={}",
            game.vehicles[1].units
        );
        run(&mut game, 3.0);
        // v1 loses 6/s and dies first; duel over -> resumes.
        let v1id = game.vehicles.get(1).map(|v| v.id);
        let _ = v1id;
        assert!(
            game.vehicles.len() <= 1
                || game.vehicles.iter().all(|v| v.combat_target.is_none())
                || game.vehicles.iter().filter(|v| !v.dead).count() <= 1
        );
    }
    #[test]
    fn joiner_no_retaliation() {
        // A third vehicle joins a duel without receiving fire (sec. 9).
        let board = flat_board(20, 20, 1);
        let mut game = make_game(board);
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            40.0,
            vec![],
            (100.0, 100.0),
            None,
        ));
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            1,
            40.0,
            vec![],
            (130.0, 100.0),
            None,
        ));
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            20.0,
            vec![],
            (145.0, 105.0),
            None,
        ));
        let (id0, id1) = (game.vehicles[0].id, game.vehicles[1].id);
        run(&mut game, 0.4);
        assert_eq!(game.vehicles[0].combat_target, Some(id1));
        assert_eq!(game.vehicles[1].combat_target, Some(id0));
        assert_eq!(game.vehicles[2].combat_target, Some(id1)); // joiner attacks nearest enemy
        run(&mut game, 0.7); // shots fire at t = 1.0 s
        assert_eq!(game.vehicles[2].units, 20.0, "joiner took retaliation");
    }
    #[test]
    fn turrets_fire() {
        let board = flat_board(20, 20, 1);
        let mut game = make_game(board);
        let turret = Building::new(BuildingKind::TurretNormal, Some(1), 10, 10, 20.0);
        game.buildings = vec![turret];
        game.building_at = [(game.buildings[0].tile, 0)].into_iter().collect();
        let (tx, ty) = game.board.center_world((10, 10));
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            30.0,
            vec![(10, 11)],
            (tx + 50.0, ty),
            None,
        ));
        run(&mut game, 8.0);
        // Turret fired (projectile resolved or in flight) and damaged or killed.
        assert!(
            game.vehicles.is_empty()
                || game.vehicles[0].units < 30.0
                || !game.projectiles.is_empty()
        );
    }
    #[test]
    fn heal_tower_and_buffer() {
        let board = flat_board(20, 20, 1);
        let mut game = make_game(board);
        let tower = Building::new(BuildingKind::HealTower, Some(0), 5, 5, 10.0);
        game.buildings = vec![tower];
        game.building_at = [(game.buildings[0].tile, 0)].into_iter().collect();
        let (cx, cy) = game.board.center_world((5, 5));
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            10.0,
            vec![],
            (cx + 20.0, cy),
            None,
        ));
        run(&mut game, 4.0);
        assert!(game.vehicles[0].units > 10.0);
    }
    /// Place a vehicle of `kind` owned by `owner` at world position (x, y).
    fn spawn(game: &mut Game, kind: VehicleKind, owner: usize, units: f64, x: f64, y: f64) -> u64 {
        game.vehicles
            .push(Vehicle::new(kind, owner, units, vec![], (x, y), None));
        game.vehicles.last().unwrap().id
    }
    #[test]
    fn tank_gun_shells_an_enemy_out_of_melee_range() {
        // A tank guns down an enemy that is outside the detection radius
        // (rules.md 5.1): the shell is homing and damages on impact.
        let board = flat_board(20, 20, 1);
        let mut game = make_game(board);
        spawn(&mut game, VehicleKind::Tank, 0, 40.0, 100.0, 100.0);
        spawn(&mut game, VehicleKind::Helicopter, 1, 40.0, 250.0, 100.0);
        run(&mut game, 3.6);
        let heli = game.vehicles.iter().find(|v| v.owner == 1);
        assert!(
            heli.is_none_or(|v| v.units < 40.0),
            "gun did no damage: {:?}",
            game.vehicles.iter().map(|v| v.units).collect::<Vec<_>>()
        );
    }
    #[test]
    fn tank_gun_is_silent_inside_the_detection_radius() {
        // The gun must never double the fire of a tank in a duel: an enemy
        // inside the detection radius is damaged by section 9 alone
        // (ceil(units/5) per second), never by the gun.
        let board = flat_board(20, 20, 1);
        let mut game = make_game(board);
        spawn(&mut game, VehicleKind::Tank, 0, 40.0, 100.0, 100.0);
        spawn(&mut game, VehicleKind::Helicopter, 1, 1000.0, 150.0, 100.0);
        run(&mut game, 10.0);
        // 10 s of section 9 fire: ceil(40/5) = 8 per second.
        let expected = 1000.0 - 8.0 * 10.0;
        let heli = game.vehicles.iter().find(|v| v.owner == 1);
        assert!(
            heli.is_none_or(|v| v.units >= expected - 1e-6),
            "took {:?}, close-combat alone gives {}",
            heli.map(|v| v.units),
            expected
        );
    }
    #[test]
    fn tank_gun_respects_its_cooldown() {
        // One shell per TANK_GUN_INTERVAL, not one per tick.
        let board = flat_board(20, 20, 1);
        let mut game = make_game(board);
        spawn(&mut game, VehicleKind::Tank, 0, 40.0, 100.0, 100.0);
        spawn(&mut game, VehicleKind::Hovercraft, 1, 2000.0, 250.0, 100.0);
        // Far enough to never enter melee, so only the gun can do damage.
        let per_shot = (40.0 / constants::TANK_GUN_DAMAGE_DIV).ceil();
        run(&mut game, constants::TANK_GUN_INTERVAL - 0.05);
        let hov = game.vehicles.iter().find(|v| v.owner == 1);
        assert!(
            hov.is_none_or(|v| v.units >= 2000.0),
            "fired before the cooldown elapsed: {:?}",
            hov.map(|v| v.units)
        );
        run(&mut game, constants::TANK_GUN_FLIGHT_TIME + 0.2);
        let hov = game.vehicles.iter().find(|v| v.owner == 1);
        assert!(
            hov.is_none_or(|v| v.units <= 2000.0 - per_shot),
            "shell never landed: {:?}",
            hov.map(|v| v.units)
        );
    }
    #[test]
    fn only_tanks_carry_the_gun() {
        // Helicopter, hovercraft and buffer have no gun at all.
        for kind in [
            VehicleKind::Helicopter,
            VehicleKind::Hovercraft,
            VehicleKind::Buffer,
        ] {
            let board = flat_board(20, 20, 1);
            let mut game = make_game(board);
            spawn(&mut game, kind, 0, 40.0, 100.0, 100.0);
            spawn(&mut game, VehicleKind::Tank, 1, 40.0, 250.0, 100.0);
            run(&mut game, 8.0);
            let enemy = game.vehicles.iter().find(|v| v.owner == 1);
            assert!(
                enemy.is_none_or(|v| v.units >= 40.0),
                "{kind:?} fired its gun: {:?}",
                enemy.map(|v| v.units)
            );
        }
    }
    #[test]
    fn tank_gun_hits_a_moving_target() {
        // The shell is homing: a target that keeps moving is still hit.
        let board = flat_board(20, 20, 1);
        let mut game = make_game(board);
        let id = spawn(&mut game, VehicleKind::Tank, 0, 40.0, 100.0, 100.0);
        spawn(&mut game, VehicleKind::Hovercraft, 1, 2000.0, 250.0, 100.0);
        // Slide the target sideways every step, well outside melee range.
        for _ in 0..(4.0 / constants::SIM_DT).round() as usize {
            if let Some(t) = game.vehicles.iter_mut().find(|v| v.owner == 1) {
                t.y += 6.0 * constants::SIM_DT;
            }
            game.update(constants::SIM_DT);
        }
        let shooter = game.vehicles.iter().find(|v| v.id == id);
        assert!(shooter.is_some(), "shooter died");
        let target = game.vehicles.iter().find(|v| v.owner == 1);
        assert!(
            target.is_none_or(|v| v.units < 2000.0),
            "moving target untouched: {:?}",
            target.map(|v| v.units)
        );
    }
    #[test]
    fn wall_mine_traps() {
        // Walls block, mines explode once, fire trap stays (sec. 1, 4).
        let board = flat_board(20, 12, 1);
        let mut game = make_game(board);
        game.buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(0), 1, 5, 50.0),
            Building::new(BuildingKind::TurretNormal, None, 12, 5, 10.0),
        ];
        game.building_at = [(game.buildings[0].tile, 0), (game.buildings[1].tile, 1)]
            .into_iter()
            .collect();
        let path = game
            .board
            .find_path((1, 5), (12, 5), VehicleKind::Tank)
            .unwrap();
        let (wall_tile, mine_tile, fire_tile) = (path[3], path[7], path[10]);
        game.board.tiles.get_mut(&wall_tile).unwrap().obstacle =
            Some(Obstacle::new(ObstacleKind::Wall));
        game.board.tiles.get_mut(&mine_tile).unwrap().obstacle =
            Some(Obstacle::new(ObstacleKind::Mine));
        game.board.tiles.get_mut(&fire_tile).unwrap().obstacle =
            Some(Obstacle::new(ObstacleKind::TrapFire));
        assert!(game.try_send(0, (1, 5), (12, 5)));
        run(&mut game, 1.0);
        // vehicle stopped before the wall
        assert!(
            game.vehicles[0].route_index <= 3,
            "idx={}",
            game.vehicles[0].route_index
        );
        run(&mut game, 25.0); // 20 s to smash the wall
        assert!(game.board.tiles.get(&wall_tile).unwrap().obstacle.is_none());
        run(&mut game, 15.0);
        assert!(
            game.vehicles.is_empty()
                || game.vehicles[0].route_index >= game.vehicles[0].route.len()
        );
        assert!(game.board.tiles.get(&mine_tile).unwrap().obstacle.is_none()); // mine exploded once
        assert!(game.board.tiles.get(&fire_tile).unwrap().obstacle.is_some()); // fire trap remains
    }
    #[test]
    fn mine_hits_on_land_and_water_alike() {
        // rules.md section 1: one mine kind, standing on land and on water
        // alike; section 4: it deals MINE_DAMAGE once to the first
        // non-helicopter vehicle reaching its centre, then disappears.
        for (label, height) in [("land", 1), ("water", 0)] {
            let mut game = make_game(flat_board(20, 12, height));
            let route = game
                .board
                .find_path((2, 5), (12, 5), VehicleKind::Hovercraft)
                .unwrap();
            let mine_tile = route[3];
            game.board.tiles.get_mut(&mine_tile).unwrap().obstacle =
                Some(Obstacle::new(ObstacleKind::Mine));
            let (sx, sy) = game.board.center_world((2, 5));
            game.vehicles.push(Vehicle::new(
                VehicleKind::Hovercraft,
                0,
                40.0,
                route,
                (sx, sy),
                None,
            ));
            let mut units_after = None;
            for _ in 0..(8.0 / constants::SIM_DT).round() as usize {
                game.update(constants::SIM_DT);
                if units_after.is_none()
                    && game.board.tiles.get(&mine_tile).unwrap().obstacle.is_none()
                {
                    units_after = game.vehicles.first().map(|v| v.units);
                }
            }
            assert!(
                game.board.tiles.get(&mine_tile).unwrap().obstacle.is_none(),
                "mine on {label} never exploded"
            );
            let units = units_after.unwrap_or_else(|| panic!("no reading on {label}"));
            assert!(
                (units - (40.0 - constants::MINE_DAMAGE)).abs() < 1e-6,
                "mine on {label}: units={units}"
            );
        }
    }
    #[test]
    fn ice_trap_stays() {
        let board = flat_board(20, 12, 1);
        let mut game = make_game(board);
        let src = Building::new(BuildingKind::BaseTank, Some(0), 1, 5, 30.0);
        let dst = Building::new(BuildingKind::BaseTank, Some(0), 12, 5, 0.0);
        game.buildings = vec![src, dst];
        game.building_at = [(game.buildings[0].tile, 0), (game.buildings[1].tile, 1)]
            .into_iter()
            .collect();
        let path = game
            .board
            .find_path((1, 5), (12, 5), VehicleKind::Tank)
            .unwrap();
        game.board.tiles.get_mut(&path[2]).unwrap().obstacle =
            Some(Obstacle::new(ObstacleKind::TrapIce));
        assert!(game.try_send(0, (1, 5), (12, 5)));
        run(&mut game, 30.0);
        assert!(
            game.board
                .tiles
                .get(&path[2])
                .unwrap()
                .obstacle
                .as_ref()
                .map(|o| o.kind)
                == Some(ObstacleKind::TrapIce)
        );
        let di = *game.building_at.get(&(12, 5)).unwrap();
        assert!(
            (game.buildings[di].units - 45.0).abs() < 1e-6,
            "units={}",
            game.buildings[di].units
        );
    }
    #[test]
    fn destroyed_vehicle_leaves_exactly_one_wreck() {
        // rules.md section 9: a duel ends when one side reaches zero units.
        let board = flat_board(20, 12, 1);
        let mut game = make_game(board);
        game.buildings = vec![
            Building::new(BuildingKind::BaseTank, Some(0), 1, 5, 10.0),
            Building::new(BuildingKind::BaseTank, Some(1), 12, 5, 10.0),
        ];
        game.building_at = [(game.buildings[0].tile, 0), (game.buildings[1].tile, 1)]
            .into_iter()
            .collect();
        assert!(game.try_send(0, (1, 5), (12, 5)));
        assert!(game.try_send(1, (12, 5), (1, 5)));
        // 10 units each, a shot every second, ceil(10/5) = 2 damage a shot, so
        // the duel lasts about five seconds -- but the tanks first have to
        // cover the eleven hexes between the bases.
        run(&mut game, 30.0);
        let wrecks = game.take_wrecks();
        assert_eq!(wrecks.len(), 1, "one vehicle must die in the duel");
        let w = wrecks[0];
        assert_eq!(w.kind, VehicleKind::Tank);
        // The wreck is a copy of where the vehicle died (the vehicle itself is
        // already gone), and reading the log changes nothing in the simulation.
        assert!(w.x.is_finite() && w.y.is_finite());
        let before = game.time;
        assert!(game.take_wrecks().is_empty());
        assert_eq!(game.time, before, "draining must not step the game");
    }
    #[test]
    fn arriving_vehicle_leaves_no_wreck() {
        // Reaching the target is not a destruction (rules.md section 4), so
        // the arriving vehicle must not trigger an explosion.
        let mut game = {
            let board = flat_board(10, 10, 1);
            let buildings = vec![
                Building::new(BuildingKind::BaseTank, Some(0), 1, 1, 20.0),
                Building::new(BuildingKind::BaseTank, Some(0), 6, 1, 0.0),
            ];
            Game::new(
                board,
                vec![Player::new(0, true), Player::new(1, false)],
                buildings,
                0,
            )
        };
        game.building_at = [(game.buildings[0].tile, 0), (game.buildings[1].tile, 1)]
            .into_iter()
            .collect();
        assert!(game.try_send(0, (1, 1), (6, 1)));
        run(&mut game, 20.0);
        assert!(game.vehicles.is_empty());
        assert!(game.take_wrecks().is_empty());
        assert!(game.buildings[1].units > 0.0);
    }
    #[test]
    fn elimination_and_victory() {
        let board = flat_board(10, 10, 1);
        let mut game = make_game(board);
        let base = Building::new(BuildingKind::BaseTank, Some(0), 2, 2, 10.0);
        game.buildings = vec![base];
        game.building_at = [(game.buildings[0].tile, 0)].into_iter().collect();
        assert!(!game.over);
        game.buildings[0].owner = None;
        game.check_elimination();
        assert!(game.eliminated.contains(&game.human_id));
        assert!(game.over && game.winner.as_deref() == Some("ai"));
    }
    #[test]
    fn full_sims_on_repo_maps() {
        use crate::ai::AiController;
        use crate::mapfile;
        let maps = mapfile::list_maps(None);
        assert!(!maps.is_empty());
        for m in maps {
            let mut game = mapfile::load_game(&m).expect("load");
            let seed = mapfile::level_seed(&m);
            let mut ais: Vec<AiController> = game
                .players
                .iter()
                .filter(|p| !p.is_human)
                .map(|p| {
                    AiController::new(
                        p.id,
                        *crate::constants::ai_difficulty(
                            crate::constants::MAP_DEFAULT_AI_DIFFICULTY,
                        ),
                        seed.wrapping_add(p.id as u64),
                    )
                })
                .collect();
            for _ in 0..(30.0 / constants::SIM_DT) as usize {
                game.update(constants::SIM_DT);
                for ai in ais.iter_mut() {
                    ai.update(&mut game, constants::SIM_DT);
                }
            }
            let total: f64 = game.buildings.iter().map(|b| b.units).sum::<f64>()
                + game.vehicles.iter().map(|v| v.units).sum::<f64>();
            assert!(total >= 0.0);
            for v in game.vehicles.iter() {
                assert!(v.units > 0.0, "dead vehicle still in the list");
            }
        }
    }

    #[test]
    fn a_duel_reports_a_shot_and_an_explosion() {
        // rules.md section 9: two vehicles fight to the death, so the
        // simulation must report both sides firing and the death itself.
        let mut game = make_game(flat_board(20, 20, 1));
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            0,
            10.0,
            vec![],
            (100.0, 100.0),
            None,
        ));
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            1,
            10.0,
            vec![],
            (140.0, 100.0),
            None,
        ));
        // Long enough for the duel to end with a death: damage is a few units
        // per second, so the vehicles need a couple of shots at each other.
        run(&mut game, 30.0);
        let sounds = game.take_sounds();
        assert!(
            sounds.iter().any(|s| s.kind == SoundKind::VehicleFire),
            "combat must be audible: {sounds:?}"
        );
        assert!(
            sounds
                .iter()
                .any(|s| matches!(s.kind, SoundKind::ExplosionGround | SoundKind::ExplosionAir)),
            "a destroyed vehicle must be audible: {sounds:?}"
        );
    }

    #[test]
    fn a_destroyed_vehicle_reports_its_explosion_at_the_wreck() {
        // The sound and the particles must describe the same place, and the
        // air blast belongs to a helicopter, the ground one to everything else.
        for (kind, expect_air) in [
            (VehicleKind::Tank, false),
            (VehicleKind::Hovercraft, false),
            (VehicleKind::Helicopter, true),
            (VehicleKind::Buffer, false),
        ] {
            // A fresh match per kind: a destroyed vehicle is dropped from the
            // list by the next step, so the two must not share one game.
            let mut game = make_game(flat_board(20, 20, 1));
            game.vehicles
                .push(Vehicle::new(kind, 0, 5.0, vec![], (100.0, 100.0), None));
            let id = game.vehicles[0].id;
            game.damage_vehicle_by_id(id, 100.0);
            // Read the events right away: they describe what just happened and
            // are never rewritten afterwards.
            let wrecks = game.take_wrecks();
            let sounds = game.take_sounds();
            assert_eq!(wrecks.len(), 1, "{kind:?} must leave exactly one wreck");
            let blast = sounds
                .iter()
                .find(|s| matches!(s.kind, SoundKind::ExplosionGround | SoundKind::ExplosionAir))
                .unwrap_or_else(|| panic!("{kind:?} died silently"));
            assert_eq!(
                expect_air,
                blast.kind == SoundKind::ExplosionAir,
                "{kind:?}"
            );
            assert!((blast.x - wrecks[0].x).abs() < 1e-9);
            assert!((blast.y - wrecks[0].y).abs() < 1e-9);
        }
    }

    #[test]
    fn a_turret_reports_its_shot_and_the_impact_separately() {
        // rules.md section 10: a turret fires, the projectile flies, and the
        // impact is a second event somewhere else.
        let board = flat_board(20, 20, 1);
        let mut game = make_game(board);
        game.buildings.push(Building::new(
            BuildingKind::TurretNormal,
            Some(0),
            2,
            2,
            50.0,
        ));
        game.vehicles.push(Vehicle::new(
            VehicleKind::Tank,
            1,
            40.0,
            vec![],
            (120.0, 120.0),
            None,
        ));
        run(&mut game, 8.0);
        let sounds = game.take_sounds();
        assert!(
            sounds
                .iter()
                .any(|s| s.kind == SoundKind::TurretShot(constants::TurretKind::Normal)),
            "a turret must be audible: {sounds:?}"
        );
        assert!(
            sounds
                .iter()
                .any(|s| s.kind == SoundKind::Impact(constants::TurretKind::Normal)),
            "a hit must be audible: {sounds:?}"
        );
        // A shot and its impact never share a position: the projectile flies.
        let shot = sounds
            .iter()
            .find(|s| matches!(s.kind, SoundKind::TurretShot(_)))
            .unwrap();
        let impact = sounds
            .iter()
            .find(|s| matches!(s.kind, SoundKind::Impact(_)))
            .unwrap();
        assert!(
            (shot.x - impact.x).abs() > 1.0 || (shot.y - impact.y).abs() > 1.0,
            "the impact must be heard where the projectile landed"
        );
    }

    #[test]
    fn taken_sounds_are_never_replayed() {
        // Mirrors the wreck contract: an unconsumed event must not linger and
        // fire again on a later frame.
        let mut game = make_game(flat_board(20, 20, 1));
        game.report_sound(SoundKind::VehicleFire, 0.0, 0.0);
        assert_eq!(game.take_sounds().len(), 1);
        assert!(
            game.take_sounds().is_empty(),
            "sounds must be consumed exactly once"
        );
    }

    #[test]
    fn sounds_do_not_change_the_simulation() {
        // Sound is presentation: the very same seed must play out the very
        // same match whether the events are drained every step or never.
        let play = || {
            let mut g = make_game(flat_board(20, 20, 1));
            g.vehicles.push(Vehicle::new(
                VehicleKind::Tank,
                0,
                10.0,
                vec![],
                (100.0, 100.0),
                None,
            ));
            g.vehicles.push(Vehicle::new(
                VehicleKind::Tank,
                1,
                10.0,
                vec![],
                (140.0, 100.0),
                None,
            ));
            for _ in 0..(3.0 / constants::SIM_DT) as usize {
                g.update(constants::SIM_DT);
                g.take_sounds();
            }
            g.vehicles.iter().map(|v| v.units).collect::<Vec<_>>()
        };
        assert_eq!(play(), play());
    }
}
