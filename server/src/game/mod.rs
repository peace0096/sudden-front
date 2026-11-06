//! One match is one Bevy ECS `World` plus a chained `Schedule`.

pub mod augments;
mod big_head;
mod components;
mod map;
mod nav;
mod physics;
mod resources;
mod snapshot;
mod systems;
#[cfg(test)]
mod tests;
pub mod weapons;

use crate::pb;
use bevy_ecs::prelude::*;
use components::*;
use resources::*;
use std::collections::HashMap;

pub const TICK_RATE: u32 = 30;
pub const TICK_DT: f32 = 1.0 / TICK_RATE as f32;
pub const CMD_DT: f32 = 1.0 / 60.0;

pub const KILLS_TO_WIN: u32 = 30;
/// A free-for-all's default target: one player takes longer than a team to reach a total.
pub const KILLS_TO_WIN_FFA: u32 = 20;
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
/// A jump's take-off speed (m/s up) and the gravity that ends it (m/s²): stepped at CMD_DT, it
/// lifts the feet about 1.34 m, over a crate (1.2 m) or a barrel (1.1 m), and lasts about 0.75 s.
pub const JUMP_SPEED: f32 = 7.5;
pub const GRAVITY: f32 = 20.0;
pub const CROUCH_SPREAD_FACTOR: f32 = 0.5;

/// Seats per team.
pub const TEAM_SIZE: usize = 5;
const FIRST_BOT_ID: u32 = 1_000_000;

pub struct Match {
    world: World,
    schedule: Schedule,
    entities: HashMap<u32, Entity>,
    next_bot_id: u32,
}

impl Match {
    /// A normal-mode match; `fill` tops each team up to five with bots.
    #[cfg(test)]
    pub fn new(
        humans: &[(u32, String, pb::Team, pb::WeaponKind)],
        map_id: pb::MapId,
        fill: bool,
        seed: u64,
    ) -> Self {
        let bots = [pb::Team::Red, pb::Team::Blue].map(|team| {
            let have = humans.iter().filter(|h| h.2 == team).count();
            if fill {
                TEAM_SIZE.saturating_sub(have)
            } else {
                0
            }
        });
        Self::with_mode(humans, map_id, pb::GameMode::Normal, bots, seed)
    }

    pub fn set_kills_to_win(&mut self, kills: u32) {
        self.world.resource_mut::<MatchState>().kills_to_win = kills;
    }

    /// A team deathmatch; `bots` is how many bots each team gets, `[red, blue]`.
    #[cfg(test)]
    pub fn with_mode(
        humans: &[(u32, String, pb::Team, pb::WeaponKind)],
        map_id: pb::MapId,
        mode: pb::GameMode,
        bots: [usize; 2],
        seed: u64,
    ) -> Self {
        Self::with_rules(
            humans,
            map_id,
            mode,
            pb::MatchType::TeamDeathmatch,
            bots,
            seed,
        )
    }

    /// `bots` is how many bots each team gets, `[red, blue]`. A free-for-all gets their sum;
    /// nobody in it has a team, so its humans come with `Unspecified` as well.
    pub fn with_rules(
        humans: &[(u32, String, pb::Team, pb::WeaponKind)],
        map_id: pb::MapId,
        mode: pb::GameMode,
        match_type: pb::MatchType,
        bots: [usize; 2],
        seed: u64,
    ) -> Self {
        let augmented = mode == pb::GameMode::Augment;
        let ffa = match_type == pb::MatchType::FreeForAll;
        let map::MapDef {
            data: map,
            spawns,
            ffa_spawns,
            lanes,
        } = map::build_map(map_id);
        let nav = nav::NavGrid::new(&map.boxes, map.half_x, map.half_z);

        let mut world = World::new();
        world.insert_resource(Arena {
            map,
            nav,
            spawns,
            ffa_spawns,
            lanes,
        });
        world.insert_resource(MatchState {
            kills_to_win: if ffa { KILLS_TO_WIN_FFA } else { KILLS_TO_WIN },
            pick_left: if augmented {
                augments::AUGMENT_PICK
            } else {
                0.0
            },
            freeze_left: if augmented {
                augments::AUGMENT_WAIT
            } else {
                0.0
            },
            ..Default::default()
        });
        world.insert_resource(Rules {
            mode,
            max_hp: if augmented {
                augments::AUGMENT_MAX_HP
            } else {
                MAX_HP
            },
            ffa,
            big_head: mode == pb::GameMode::BigHead,
        });
        world.insert_resource(EventLog::default());
        world.insert_resource(Missiles::default());
        world.insert_resource(Grenades::default());
        world.insert_resource(GameRng(seed | 1));

        let schedule = systems::schedule();

        let mut game = Self {
            world,
            schedule,
            entities: HashMap::new(),
            next_bot_id: FIRST_BOT_ID,
        };

        debug_assert!(
            !ffa || humans.iter().all(|h| h.2 == pb::Team::Unspecified),
            "a free-for-all has no teams"
        );
        for (id, name, team, weapon) in humans {
            game.spawn_player(*id, name.clone(), *team, false, *weapon);
            if augmented {
                game.deal_offer(*id);
            }
        }
        if ffa {
            let count = bots[0] + bots[1];
            let seated = humans.len() + count;
            debug_assert!(
                seated <= 2 * TEAM_SIZE,
                "a free-for-all would seat {seated} players"
            );
            for _ in 0..count {
                game.spawn_bot(pb::Team::Unspecified);
            }
            return game;
        }
        for (team, count) in [pb::Team::Red, pb::Team::Blue].into_iter().zip(bots) {
            let seated = humans.iter().filter(|h| h.2 == team).count() + count;
            debug_assert!(seated <= TEAM_SIZE, "{team:?} would seat {seated} players");
            for _ in 0..count {
                game.spawn_bot(team);
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
        if slot >= offer.offered.len()
            || offer.refreshed[slot]
            || self.world.resource::<MatchState>().pick_left <= 0.0
        {
            return;
        }
        let mut seen = offer.seen.clone();
        let Some(card) = augments::deal_one(&mut self.world.resource_mut::<GameRng>(), &mut seen)
        else {
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
        let forced = systems::weapon::forced_primary(&active);
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
        *player.get_mut::<Weapon>().unwrap() =
            systems::weapon::issue_weapons(primary, &active, is_bot);
        player.get_mut::<Vitals>().unwrap().hp = max_hp;
        let mut perks = player.get_mut::<Perks>().unwrap();
        perks.augment = augment;
        perks.shield_hp = if active.contains(&pb::Augment::Shield) {
            augments::SHIELD_HP
        } else {
            0
        };
        perks.head_scale = if active.contains(&pb::Augment::SmallHead) {
            0.5
        } else {
            1.0
        };
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

    fn spawn_player(
        &mut self,
        id: u32,
        name: String,
        team: pb::Team,
        is_bot: bool,
        weapon: pb::WeaponKind,
    ) {
        let (x, z, yaw) = if self.world.resource::<Rules>().ffa {
            // Away from everyone already in, so that the players spread over the map.
            let others: Vec<(f32, f32)> = self
                .world
                .query::<&Pose>()
                .iter(&self.world)
                .map(|p| (p.x, p.z))
                .collect();
            let points = self.world.resource::<Arena>().ffa_spawns.clone();
            let (x, z) = systems::respawn::furthest_spawn(
                &points,
                &others,
                &mut self.world.resource_mut::<GameRng>(),
            );
            (x, z, systems::respawn::ffa_yaw(x, z))
        } else {
            let spawns = self.world.resource::<Arena>().spawns.clone();
            let (x, z) = {
                let mut rng = self.world.resource_mut::<GameRng>();
                systems::respawn::choose_spawn(&spawns, team, &[], &mut rng)
            };
            (x, z, systems::respawn::spawn_yaw(team))
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
                yaw,
                ..Default::default()
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
    /// cancels), otherwise at the next spawn. A King keeps the hammer: the choice waits for the
    /// weapons the reign's end hands out.
    pub fn select_weapon(&mut self, id: u32, kind: pb::WeaponKind) {
        let Some(&entity) = self.entities.get(&id) else {
            return;
        };
        let king = self.world.get::<Perks>(entity).is_some_and(|p| p.king());
        let active = self
            .world
            .get::<Perks>(entity)
            .map(|p| p.active.clone())
            .unwrap_or_default();
        // The Missile Launcher augment fills slot 1 itself.
        if !weapons::PRIMARIES.contains(&kind) || systems::weapon::forced_primary(&active).is_some()
        {
            return;
        }
        if let Some(mut loadout) = self.world.get_mut::<Loadout>(entity) {
            loadout.0 = kind;
        }
        let swap_now = !king
            && self
                .world
                .get::<Vitals>(entity)
                .is_some_and(|v| v.alive && v.protect_left > 0.0);
        if swap_now {
            if let Some(mut weapon) = self.world.get_mut::<Weapon>(entity) {
                *weapon = Weapon::equipped(kind, &active);
            }
        }
    }

    /// A human joins while the match runs. They come in as a respawn would: at the spawn point
    /// furthest from every living enemy, under spawn protection. In augment mode they are dealt a
    /// hand to choose from while the cards are still being chosen; after that one of it is taken
    /// at random at once, as for anyone who didn't choose in time. Their kills and deaths start at
    /// zero and the scores stay as they are. Returns the roster with them in it.
    pub fn add_player(
        &mut self,
        id: u32,
        name: String,
        team: pb::Team,
        weapon: pb::WeaponKind,
    ) -> Vec<pb::RosterEntry> {
        // They aren't in yet, so in a free-for-all everyone living counts.
        let enemies: Vec<(f32, f32)> = {
            let mut living = self.world.query::<(&Player, &Pose, &Vitals)>();
            let rules = self.world.resource::<Rules>();
            living
                .iter(&self.world)
                .filter(|(player, _, vitals)| vitals.alive && rules.hostile(player.team, team))
                .map(|(_, pose, _)| (pose.x, pose.z))
                .collect()
        };
        let (x, z, yaw) = if self.world.resource::<Rules>().ffa {
            let points = self.world.resource::<Arena>().ffa_spawns.clone();
            let (x, z) = systems::respawn::furthest_spawn(
                &points,
                &enemies,
                &mut self.world.resource_mut::<GameRng>(),
            );
            (x, z, systems::respawn::ffa_yaw(x, z))
        } else {
            let spawns = self.world.resource::<Arena>().spawns.clone();
            let (x, z) = systems::respawn::choose_spawn(
                &spawns,
                team,
                &enemies,
                &mut self.world.resource_mut::<GameRng>(),
            );
            (x, z, systems::respawn::spawn_yaw(team))
        };
        self.spawn_player(id, name, team, false, weapon);
        // spawn_player places players for the start of a match; this one goes where it is safe now.
        if let Some(mut pose) = self
            .entities
            .get(&id)
            .and_then(|&entity| self.world.get_mut::<Pose>(entity))
        {
            (pose.x, pose.z, pose.yaw) = (x, z, yaw);
        }
        if self.world.resource::<Rules>().mode == pb::GameMode::Augment {
            self.deal_offer(id);
            if self.world.resource::<MatchState>().pick_left <= 0.0 {
                self.pick_random(id);
            }
        }
        self.roster()
    }

    /// A human left mid-match. No bot takes the seat: the match goes on a player short, unless
    /// that leaves a team with nobody, or a free-for-all with one player, which forfeits.
    pub fn remove_player(&mut self, id: u32) -> Vec<pb::RosterEntry> {
        if let Some(entity) = self.entities.remove(&id) {
            self.world.despawn(entity);
            self.forfeit_an_empty_team();
        }
        self.roster()
    }

    /// The moment a team has nobody left, the other team wins. In a free-for-all each player is
    /// a team of their own, so the last one left wins. A result that is already in stays.
    fn forfeit_an_empty_team(&mut self) {
        let ffa = self.world.resource::<Rules>().ffa;
        let mut query = self.world.query::<&Player>();
        let winner = if ffa {
            let ids: Vec<u32> = query.iter(&self.world).map(|p| p.id).collect();
            match ids[..] {
                [id] => Some(Winner::Player(id)),
                _ => None,
            }
        } else {
            let teams: Vec<pb::Team> = query.iter(&self.world).map(|p| p.team).collect();
            match (
                teams.contains(&pb::Team::Red),
                teams.contains(&pb::Team::Blue),
            ) {
                (true, false) => Some(Winner::Team(pb::Team::Red)),
                (false, true) => Some(Winner::Team(pb::Team::Blue)),
                _ => None,
            }
        };
        let mut state = self.world.resource_mut::<MatchState>();
        if state.winner.is_none() {
            state.winner = winner;
        }
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

    pub fn winner(&self) -> Option<Winner> {
        self.world.resource::<MatchState>().winner
    }
}
