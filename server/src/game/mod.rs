//! One match is one Bevy ECS `World` plus a chained `Schedule`.

pub mod augments;
mod bot;
mod components;
mod map;
mod nav;
mod physics;
mod systems;
pub mod weapons;
#[cfg(test)]
mod tests;

use crate::pb;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ExecutorKind;
use components::*;
use std::collections::HashMap;

pub const TICK_RATE: u32 = 30;
pub const TICK_DT: f32 = 1.0 / TICK_RATE as f32;
pub const CMD_DT: f32 = 1.0 / 60.0;

pub const KILLS_TO_WIN: u32 = 30;
/// The kill targets a host can choose from.
pub const KILL_TARGETS: [u32; 4] = [10, 20, 30, 50];
pub const RESPAWN_TIME: f32 = 3.0;
/// Also the window in which a weapon change applies immediately.
pub const SPAWN_PROTECTION: f32 = 3.0;
pub const MAX_HP: i32 = 100;
pub const DRAW_TIME: f32 = 0.35;

pub const PLAYER_HALF_WIDTH: f32 = 0.4;
pub const PLAYER_HEIGHT: f32 = 1.8;
/// Below the head hitbox: everyone is the same height on flat ground, so at head
/// level a crosshair held straight ahead would land on heads at every distance.
pub const EYE_HEIGHT: f32 = 1.42;
pub const MAX_PITCH: f32 = 1.45;
/// How far crouching lowers the head, eyes and upper hitboxes.
pub const CROUCH_DROP: f32 = 0.5;
pub const CROUCH_EYE_HEIGHT: f32 = EYE_HEIGHT - CROUCH_DROP;
pub const CROUCH_SPEED_FACTOR: f32 = 0.5;
pub const CROUCH_SPREAD_FACTOR: f32 = 0.5;

const TEAM_SIZE: usize = 5;
const FIRST_BOT_ID: u32 = 1_000_000;

pub struct Match {
    world: World,
    schedule: Schedule,
    entities: HashMap<u32, Entity>,
    fill_bots: bool,
    next_bot_id: u32,
}

impl Match {
    #[cfg(test)]
    pub fn new(
        humans: &[(u32, String, pb::Team, pb::WeaponKind)],
        map_id: pb::MapId,
        fill_bots: bool,
        seed: u64,
    ) -> Self {
        Self::with_mode(humans, map_id, pb::GameMode::Normal, fill_bots, seed)
    }

    pub fn set_kills_to_win(&mut self, kills: u32) {
        self.world.resource_mut::<MatchState>().kills_to_win = kills;
    }

    pub fn with_mode(
        humans: &[(u32, String, pb::Team, pb::WeaponKind)],
        map_id: pb::MapId,
        mode: pb::GameMode,
        fill_bots: bool,
        seed: u64,
    ) -> Self {
        let augmented = mode == pb::GameMode::Augment;
        let map::MapDef { data: map, spawns, lanes } = map::build_map(map_id);
        let nav = nav::NavGrid::new(&map.boxes, map.half_x, map.half_z);

        let mut world = World::new();
        world.insert_resource(Arena { map, nav, spawns, lanes });
        world.insert_resource(MatchState {
            kills_to_win: KILLS_TO_WIN,
            pick_left: if augmented { augments::AUGMENT_PICK } else { 0.0 },
            freeze_left: if augmented { augments::AUGMENT_WAIT } else { 0.0 },
            ..Default::default()
        });
        world.insert_resource(Rules {
            mode,
            max_hp: if augmented { augments::AUGMENT_MAX_HP } else { MAX_HP },
        });
        world.insert_resource(EventLog::default());
        world.insert_resource(Missiles::default());
        world.insert_resource(GameRng(seed | 1));

        let mut schedule = Schedule::default();
        schedule.set_executor_kind(ExecutorKind::SingleThreaded);
        schedule.add_systems(
            (
                bot::bot_think,
                systems::consume_inputs,
                systems::update_weapons,
                systems::fire_weapons,
                systems::update_perks,
                systems::respawn_players,
                systems::record_history,
                systems::advance_tick,
            )
                .chain(),
        );

        let mut game = Self {
            world,
            schedule,
            entities: HashMap::new(),
            fill_bots,
            next_bot_id: FIRST_BOT_ID,
        };

        for (id, name, team, weapon) in humans {
            game.spawn_player(*id, name.clone(), *team, false, *weapon);
            if augmented {
                game.deal_offer(*id);
            }
        }
        if fill_bots {
            for team in [pb::Team::Red, pb::Team::Blue] {
                let have = humans.iter().filter(|h| h.2 == team).count();
                for _ in have..TEAM_SIZE {
                    game.spawn_bot(team);
                }
            }
        }
        game
    }

    fn spawn_bot(&mut self, team: pb::Team) {
        let id = self.next_bot_id;
        self.next_bot_id += 1;
        let name = format!("BOT_{}", id - FIRST_BOT_ID + 1);
        let weapon = {
            let mut rng = self.world.resource_mut::<GameRng>();
            weapons::random_for_bot(&mut rng)
        };
        self.spawn_player(id, name, team, true, weapon);
        if self.world.resource::<Rules>().mode == pb::GameMode::Augment {
            // Bots choose at random from a hand dealt like anyone else's.
            self.deal_offer(id);
            self.pick_random(id);
        }
    }

    fn deal_offer(&mut self, id: u32) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        let mut seen = Vec::new();
        let offered = augments::deal(&mut self.world.resource_mut::<GameRng>(), &mut seen);
        if let Some(mut offer) = self.world.get_mut::<AugmentOffer>(entity) {
            offer.offered = offered;
            offer.seen = seen;
        }
    }

    fn pick_random(&mut self, id: u32) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        let offered = self
            .world
            .get::<AugmentOffer>(entity)
            .map(|offer| offer.offered.clone())
            .unwrap_or_default();
        if offered.is_empty() {
            return;
        }
        let index = self.world.resource_mut::<GameRng>().index(offered.len());
        self.apply_augment(id, offered[index]);
    }

    /// Swaps one card for one not seen yet, once per card.
    pub fn refresh_augments(&mut self, id: u32, slot: u32) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        let Some(offer) = self.world.get::<AugmentOffer>(entity) else {
            return;
        };
        let slot = slot as usize;
        if slot >= offer.offered.len() || offer.refreshed[slot] || self.world.resource::<MatchState>().pick_left <= 0.0 {
            return;
        }
        let mut seen = offer.seen.clone();
        let Some(card) = augments::deal_one(&mut self.world.resource_mut::<GameRng>(), &mut seen) else {
            return;
        };
        if let Some(mut offer) = self.world.get_mut::<AugmentOffer>(entity) {
            offer.offered[slot] = card;
            offer.seen = seen;
            offer.refreshed[slot] = true;
        }
    }

    /// Takes one of the cards on offer; anything else is ignored.
    pub fn pick_augment(&mut self, id: u32, augment: pb::Augment) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        let on_offer = self
            .world
            .get::<AugmentOffer>(entity)
            .is_some_and(|offer| offer.offered.contains(&augment));
        if on_offer && self.world.resource::<MatchState>().pick_left > 0.0 {
            self.apply_augment(id, augment);
        }
    }

    fn apply_augment(&mut self, id: u32, augment: pb::Augment) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        let active = if augment == pb::Augment::DoubleTime {
            augments::roll_double(&mut self.world.resource_mut::<GameRng>())
        } else {
            vec![augment]
        };
        let forced = systems::forced_primary(&active);
        let max_hp = augments::max_hp(self.world.resource::<Rules>().max_hp, &active);
        let mut player = self.world.entity_mut(entity);
        let is_bot = player.get::<Player>().unwrap().is_bot;
        if let Some(mut offer) = player.get_mut::<AugmentOffer>() {
            offer.offered.clear();
        }
        let primary = {
            let mut loadout = player.get_mut::<Loadout>().unwrap();
            if let Some(kind) = forced {
                loadout.0 = kind;
            }
            if is_bot && active.contains(&pb::Augment::Akimbo) {
                loadout.0 = pb::WeaponKind::Smg;
            }
            loadout.0
        };
        *player.get_mut::<Weapon>().unwrap() = systems::issue_weapons(primary, &active, is_bot);
        player.get_mut::<Vitals>().unwrap().hp = max_hp;
        let mut perks = player.get_mut::<Perks>().unwrap();
        perks.augment = augment;
        perks.shield_hp = if active.contains(&pb::Augment::Shield) { augments::SHIELD_HP } else { 0 };
        perks.head_scale = if active.contains(&pb::Augment::SmallHead) { 0.5 } else { 1.0 };
        perks.active = active;
    }

    /// Only input for aiming counts while frozen; nobody moves or fires.
    fn freeze_tick(&mut self) {
        let mut query = self.world.query::<(&mut Pose, &mut Inputs)>();
        for (mut pose, mut inputs) in query.iter_mut(&mut self.world) {
            while let Some(cmd) = inputs.queue.pop_front() {
                inputs.last_seq = cmd.seq;
                if cmd.yaw.is_finite() {
                    pose.yaw = cmd.yaw;
                }
                if cmd.pitch.is_finite() {
                    pose.pitch = cmd.pitch.clamp(-MAX_PITCH, MAX_PITCH);
                }
            }
        }
        let undecided = self
            .world
            .query::<&AugmentOffer>()
            .iter(&self.world)
            .any(|offer| !offer.offered.is_empty());
        let mut state = self.world.resource_mut::<MatchState>();
        state.tick += 1;
        state.frozen_ticks += 1;
        if state.pick_left <= 0.0 {
            state.freeze_left = (state.freeze_left - TICK_DT).max(0.0);
            return;
        }
        state.pick_left = (state.pick_left - TICK_DT).max(0.0);
        if !undecided {
            state.pick_left = 0.0;
        }
        if state.pick_left > 0.0 {
            return;
        }
        // Time's up: whoever hasn't chosen gets a card at random.
        let ids: Vec<u32> = self.entities.keys().copied().collect();
        for id in ids {
            self.pick_random(id);
        }
    }

    fn spawn_player(&mut self, id: u32, name: String, team: pb::Team, is_bot: bool, weapon: pb::WeaponKind) {
        let spawns = self.world.resource::<Arena>().spawns.clone();
        let (x, z) = {
            let mut rng = self.world.resource_mut::<GameRng>();
            systems::choose_spawn(&spawns, team, &[], &mut rng)
        };
        let mut entity = self.world.spawn((
            Player {
                id,
                name,
                team,
                is_bot,
            },
            Pose {
                x,
                z,
                yaw: systems::spawn_yaw(team),
                pitch: 0.0,
                crouching: false,
            },
            Vitals {
                hp: self.world.resource::<Rules>().max_hp,
                alive: true,
                respawn_left: 0.0,
                protect_left: SPAWN_PROTECTION,
                stun_left: 0.0,
            },
            Weapon::new(weapon),
            Loadout(weapon),
            Perks {
                head_scale: 1.0,
                ..Default::default()
            },
            AugmentOffer::default(),
            Inputs::default(),
            Intent::default(),
            Stats::default(),
            History::default(),
        ));
        if is_bot {
            entity.insert(BotBrain::default());
        }
        let entity = entity.id();
        self.entities.insert(id, entity);
    }

    pub fn push_inputs(&mut self, id: u32, cmds: Vec<pb::InputCmd>) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        if let Some(mut inputs) = self.world.get_mut::<Inputs>(entity) {
            inputs.queue.extend(cmds);
        }
    }

    /// Takes effect now while still under spawn protection (which firing
    /// cancels), otherwise at the next spawn.
    pub fn select_weapon(&mut self, id: u32, kind: pb::WeaponKind) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        let active = self.world.get::<Perks>(entity).map(|p| p.active.clone()).unwrap_or_default();
        // The Missile Launcher augment fills slot 1 itself.
        if !weapons::PRIMARIES.contains(&kind) || systems::forced_primary(&active).is_some() {
            return;
        }
        if let Some(mut loadout) = self.world.get_mut::<Loadout>(entity) {
            loadout.0 = kind;
        }
        let swap_now = self
            .world
            .get::<Vitals>(entity)
            .is_some_and(|v| v.alive && v.protect_left > 0.0);
        if swap_now {
            if let Some(mut weapon) = self.world.get_mut::<Weapon>(entity) {
                *weapon = Weapon::equipped(kind, &active);
            }
        }
    }

    /// A human left mid-match: a bot takes the slot if the room uses bots.
    pub fn remove_player(&mut self, id: u32) -> Vec<pb::RosterEntry> {
        if let Some(entity) = self.entities.remove(&id) {
            let team = self.world.get::<Player>(entity).map(|p| p.team);
            self.world.despawn(entity);
            if let (true, Some(team)) = (self.fill_bots, team) {
                self.spawn_bot(team);
            }
        }
        self.roster()
    }

    pub fn tick(&mut self) {
        if self.winner().is_some() {
            return;
        }
        self.world.resource_mut::<EventLog>().0.clear();
        if self.world.resource::<MatchState>().freeze_left > 0.0 {
            self.freeze_tick();
            return;
        }
        self.schedule.run(&mut self.world);
    }

    pub fn winner(&self) -> Option<pb::Team> {
        self.world.resource::<MatchState>().winner
    }

    pub fn roster(&mut self) -> Vec<pb::RosterEntry> {
        let mut query = self.world.query::<&Player>();
        let mut roster: Vec<pb::RosterEntry> = query
            .iter(&self.world)
            .map(|p| pb::RosterEntry {
                id: p.id,
                name: p.name.clone(),
                team: p.team as i32,
                is_bot: p.is_bot,
            })
            .collect();
        roster.sort_by_key(|r| r.id);
        roster
    }

    pub fn start_message(&self, your_id: u32) -> pb::MatchStart {
        let mut roster: Vec<pb::RosterEntry> = self
            .entities
            .values()
            .filter_map(|e| self.world.get::<Player>(*e))
            .map(|p| pb::RosterEntry {
                id: p.id,
                name: p.name.clone(),
                team: p.team as i32,
                is_bot: p.is_bot,
            })
            .collect();
        roster.sort_by_key(|r| r.id);
        pb::MatchStart {
            your_id,
            config: Some(pb::GameConfig {
                tick_rate: TICK_RATE,
                kills_to_win: self.world.resource::<MatchState>().kills_to_win,
                player_half_width: PLAYER_HALF_WIDTH,
                player_height: PLAYER_HEIGHT,
                eye_height: EYE_HEIGHT,
                respawn_time: RESPAWN_TIME,
                weapons: weapons::all_stats(),
                draw_time: DRAW_TIME,
                crouch_eye_height: CROUCH_EYE_HEIGHT,
                crouch_speed_factor: CROUCH_SPEED_FACTOR,
                crouch_spread_factor: CROUCH_SPREAD_FACTOR,
                max_hp: self.world.resource::<Rules>().max_hp,
                mode: self.world.resource::<Rules>().mode as i32,
                start_wait: if self.world.resource::<Rules>().mode == pb::GameMode::Augment {
                    augments::AUGMENT_WAIT
                } else {
                    0.0
                },
            }),
            map: Some(self.world.resource::<Arena>().map.clone()),
            roster,
        }
    }

    /// Everything in a snapshot that's the same for every recipient.
    pub fn shared_snapshot(&mut self) -> pb::Snapshot {
        let mut query = self
            .world
            .query::<(&Player, &Pose, &Vitals, &Intent, &Weapon, &Stats, &Perks)>();
        let mut players: Vec<pb::PlayerState> = query
            .iter(&self.world)
            .map(|(player, pose, vitals, intent, weapon, stats, perks)| pb::PlayerState {
                id: player.id,
                x: pose.x,
                z: pose.z,
                yaw: pose.yaw,
                pitch: pose.pitch,
                hp: vitals.hp,
                alive: vitals.alive,
                moving: intent.moving,
                crouching: pose.crouching,
                reloading: weapon.reloading(),
                protected: vitals.protect_left > 0.0,
                respawn_in: vitals.respawn_left.max(0.0),
                kills: stats.kills,
                deaths: stats.deaths,
                weapon: weapon.kind as i32,
                augment: perks.augment as i32,
                shield_hp: perks.shield_hp,
                head_scale: perks.head_scale,
                active: perks.active.iter().map(|a| *a as i32).collect(),
                stun_left: vitals.stun_left,
            })
            .collect();
        players.sort_by_key(|p| p.id);
        let state = self.world.resource::<MatchState>();
        pb::Snapshot {
            tick: state.tick,
            red_score: state.red_score,
            blue_score: state.blue_score,
            freeze_left: state.freeze_left,
            pick_left: state.pick_left,
            clock: state.tick.saturating_sub(state.frozen_ticks) as f32 * TICK_DT,
            players,
            missiles: self
                .world
                .resource::<Missiles>()
                .live
                .iter()
                .map(|m| pb::Missile {
                    id: m.id,
                    x: m.pos[0],
                    y: m.pos[1],
                    z: m.pos[2],
                    vx: m.dir[0] * systems::MISSILE_SPEED,
                    vy: m.dir[1] * systems::MISSILE_SPEED,
                    vz: m.dir[2] * systems::MISSILE_SPEED,
                })
                .collect(),
            events: self.world.resource::<EventLog>().0.clone(),
            ..Default::default()
        }
    }

    pub fn fill_personal(&self, id: u32, snapshot: &mut pb::Snapshot) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        if let Some(weapon) = self.world.get::<Weapon>(entity) {
            snapshot.magazine = weapon.magazine;
            snapshot.reserve = weapon.reserve;
            snapshot.reload_remaining = weapon.reload_left;
            snapshot.primary_weapon = weapon.primary as i32;
            snapshot.sidearm_weapon = weapon.sidearm as i32;
        }
        if let Some(loadout) = self.world.get::<Loadout>(entity) {
            snapshot.selected_weapon = loadout.0 as i32;
        }
        if let Some(offer) = self.world.get::<AugmentOffer>(entity) {
            snapshot.offered = offer.offered.iter().map(|a| *a as i32).collect();
            snapshot.refreshable = offer.offered.iter().zip(offer.refreshed).map(|(_, used)| !used).collect();
        }
        if let Some(inputs) = self.world.get::<Inputs>(entity) {
            snapshot.last_input_seq = inputs.last_seq;
        }
    }

    pub fn end_message(&mut self, winner: pb::Team, return_in: f32) -> pb::MatchEnd {
        let (red_score, blue_score) = {
            let state = self.world.resource::<MatchState>();
            (state.red_score, state.blue_score)
        };
        let mut query = self.world.query::<(&Player, &Stats)>();
        let mut scoreboard: Vec<pb::ScoreEntry> = query
            .iter(&self.world)
            .map(|(player, stats)| pb::ScoreEntry {
                id: player.id,
                name: player.name.clone(),
                team: player.team as i32,
                kills: stats.kills,
                deaths: stats.deaths,
                is_bot: player.is_bot,
            })
            .collect();
        scoreboard.sort_by_key(|s| (std::cmp::Reverse(s.kills), s.deaths));
        pb::MatchEnd {
            winner: winner as i32,
            red_score,
            blue_score,
            scoreboard,
            return_in,
        }
    }
}
