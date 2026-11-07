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
mod test_support;
pub mod weapons;

use crate::pb;
use bevy_ecs::prelude::*;
use components::*;
use resources::*;
use std::collections::HashMap;

pub const TICK_RATE: u32 = 30;
pub const TICK_DT: f32 = 1.0 / TICK_RATE as f32;

/// A timer in seconds after one more tick: down by TICK_DT, stopping at zero.
pub fn count_down(seconds: f32) -> f32 {
    (seconds - TICK_DT).max(0.0)
}

/// One frozen tick of the augment pick and the wait after it, as `(pick_left, freeze_left,
/// time_up)`. The pick counts down first, and ends early once nobody is `undecided`; `time_up`
/// when it has just ended, and whoever hasn't chosen is dealt a card. Then the wait counts down.
fn freeze_step(pick_left: f32, freeze_left: f32, undecided: bool) -> (f32, f32, bool) {
    if pick_left <= 0.0 {
        return (pick_left, count_down(freeze_left), false);
    }
    let pick_left = if undecided {
        count_down(pick_left)
    } else {
        0.0
    };
    (pick_left, freeze_left, pick_left <= 0.0)
}
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

/// How big a match's ECS world is, for the server's metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EcsStats {
    pub entities: usize,
    pub archetypes: usize,
    pub tables: usize,
    /// Component types registered.
    pub components: usize,
}

impl EcsStats {
    /// Two worlds' numbers together.
    pub fn plus(self, other: Self) -> Self {
        Self {
            entities: self.entities.saturating_add(other.entities),
            archetypes: self.archetypes.saturating_add(other.archetypes),
            tables: self.tables.saturating_add(other.tables),
            components: self.components.saturating_add(other.components),
        }
    }
}

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
            let count = bots[0].saturating_add(bots[1]);
            let seated = humans.len().saturating_add(count);
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
            let seated = humans
                .iter()
                .filter(|h| h.2 == team)
                .count()
                .saturating_add(count);
            debug_assert!(seated <= TEAM_SIZE, "{team:?} would seat {seated} players");
            for _ in 0..count {
                game.spawn_bot(team);
            }
        }
        game
    }

    fn spawn_bot(&mut self, team: pb::Team) {
        let id = self.next_bot_id;
        self.next_bot_id = self.next_bot_id.wrapping_add(1);
        let name = format!("BOT_{}", id.saturating_sub(FIRST_BOT_ID).saturating_add(1));
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
                (pose.yaw, pose.pitch) =
                    systems::input::view_angles(cmd.yaw, cmd.pitch, pose.yaw, pose.pitch);
            }
        }
        let undecided = self
            .world
            .query::<&AugmentOffer>()
            .iter(&self.world)
            .any(|offer| !offer.offered.is_empty());
        let mut state = self.world.resource_mut::<MatchState>();
        state.tick = state.tick.saturating_add(1);
        state.frozen_ticks = state.frozen_ticks.saturating_add(1);
        let (pick_left, freeze_left, time_up) =
            freeze_step(state.pick_left, state.freeze_left, undecided);
        (state.pick_left, state.freeze_left) = (pick_left, freeze_left);
        if !time_up {
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

    /// The size of this match's ECS world.
    pub fn ecs_stats(&self) -> EcsStats {
        EcsStats {
            entities: self.world.entity_count() as usize,
            archetypes: self.world.archetypes().len(),
            tables: self.world.storages().tables.len(),
            components: self.world.components().len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn ecs_stats_add_up_field_by_field() {
        let one = EcsStats {
            entities: 1,
            archetypes: 2,
            tables: 3,
            components: 4,
        };
        assert_eq!(
            one.plus(one),
            EcsStats {
                entities: 2,
                archetypes: 4,
                tables: 6,
                components: 8,
            }
        );
        let full = EcsStats {
            entities: usize::MAX,
            ..one
        };
        assert_eq!(full.plus(one).entities, usize::MAX, "saturates");
    }

    #[test]
    fn a_match_world_holds_an_entity_for_each_player_and_more_than_one_archetype() {
        let game = Match::new(&[], YARD, true, 1);
        let stats = game.ecs_stats();
        assert!(stats.entities >= 2 * TEAM_SIZE, "{stats:?}");
        assert!(stats.archetypes > 1 && stats.tables > 1, "{stats:?}");
        assert!(stats.components > 0, "{stats:?}");
    }

    #[test]
    fn a_timer_counts_down_a_tick_at_a_time_and_stops_at_zero() {
        assert_eq!(count_down(1.0), 1.0 - TICK_DT);
        assert_eq!(count_down(TICK_DT / 2.0), 0.0);
        assert_eq!(count_down(0.0), 0.0);
    }

    #[test]
    fn the_pick_counts_down_and_its_end_deals_the_undecided_a_card() {
        assert_eq!(freeze_step(1.0, 5.0, true), (1.0 - TICK_DT, 5.0, false));
        assert_eq!(freeze_step(TICK_DT, 5.0, true), (0.0, 5.0, true));
    }

    #[test]
    fn the_pick_ends_early_once_everyone_has_chosen() {
        assert_eq!(freeze_step(10.0, 5.0, false), (0.0, 5.0, true));
    }

    #[test]
    fn the_wait_counts_down_only_after_the_pick() {
        assert_eq!(freeze_step(0.0, 5.0, false), (0.0, 5.0 - TICK_DT, false));
        assert_eq!(freeze_step(0.0, 0.0, false), (0.0, 0.0, false));
    }

    #[test]
    fn a_chosen_kill_target_ends_the_match_there_and_reaches_the_client() {
        let mut game = duel();
        game.set_kills_to_win(10);
        assert_eq!(game.start_message(SHOOTER).config.unwrap().kills_to_win, 10);
        game.world.resource_mut::<MatchState>().red_score = 9;
        let shot = line_up(&mut game, 0.03);
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        assert_eq!(game.winner(), Some(Winner::Team(pb::Team::Red)));
        assert_eq!(game.end_message(8.0).red_score, 10);
    }

    #[test]
    fn first_team_to_30_kills_wins_and_the_match_freezes() {
        let mut game = duel();
        game.world.resource_mut::<MatchState>().red_score = KILLS_TO_WIN - 1;
        let shot = line_up(&mut game, 0.03);
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        assert_eq!(game.winner(), Some(Winner::Team(pb::Team::Red)));
        let end = game.end_message(8.0);
        assert_eq!((end.winner, end.winner_id), (pb::Team::Red as i32, 0));
        assert_eq!(end.red_score, KILLS_TO_WIN);
        let frozen = tick_now(&game);
        game.tick();
        assert_eq!(tick_now(&game), frozen);
    }

    #[test]
    fn fill_bots_makes_it_five_versus_five() {
        let mut game = Match::new(
            &[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)],
            YARD,
            true,
            3,
        );
        let roster = game.roster();
        assert_eq!(roster.len(), 10);
        assert_eq!(
            roster
                .iter()
                .filter(|r| r.team == pb::Team::Red as i32)
                .count(),
            5
        );
        assert_eq!(roster.iter().filter(|r| r.is_bot).count(), 9);
    }

    #[test]
    fn each_team_gets_as_many_bots_as_it_is_given() {
        // One human on red, then two bots for red and three for blue.
        let humans = [(SHOOTER, "a".into(), pb::Team::Red, RIFLE)];
        let mut game = Match::with_mode(&humans, YARD, pb::GameMode::Normal, [2, 3], 3);
        let roster = game.roster();
        let on = |team: pb::Team, bots: bool| {
            roster
                .iter()
                .filter(|r| r.team == team as i32 && r.is_bot == bots)
                .count()
        };
        assert_eq!((on(pb::Team::Red, false), on(pb::Team::Red, true)), (1, 2));
        assert_eq!(
            (on(pb::Team::Blue, false), on(pb::Team::Blue, true)),
            (0, 3)
        );
        assert_eq!(roster.len(), 6);
    }

    #[test]
    fn a_leaving_player_is_not_replaced() {
        let mut game = Match::new(
            &[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)],
            YARD,
            true,
            3,
        );
        let roster = game.remove_player(SHOOTER);
        // The seat stays empty: nine bots are left, four of them on the leaver's team.
        assert_eq!(roster.len(), 9);
        assert!(roster.iter().all(|r| r.is_bot));
        assert_eq!(
            roster
                .iter()
                .filter(|r| r.team == pb::Team::Red as i32)
                .count(),
            4
        );
    }

    #[test]
    fn a_team_left_with_nobody_forfeits_the_match() {
        // Blue is only its human, so with them gone red wins at once, with no kill scored.
        let mut game = duel_with_bots([2, 0], pb::GameMode::Normal);
        assert_eq!(game.winner(), None);
        game.remove_player(VICTIM);
        assert_eq!(game.winner(), Some(Winner::Team(pb::Team::Red)));
        let end = game.end_message(8.0);
        assert_eq!(
            (end.winner, end.red_score, end.blue_score),
            (pb::Team::Red as i32, 0, 0)
        );
        // It is over: the clock no longer runs.
        let frozen = tick_now(&game);
        game.tick();
        assert_eq!(tick_now(&game), frozen);
    }

    #[test]
    fn a_forfeit_ends_a_match_that_is_still_choosing_augments() {
        let mut game = duel_with_bots([0, 0], pb::GameMode::Augment);
        assert!(
            game.world.resource::<MatchState>().freeze_left > 0.0,
            "still frozen before play"
        );
        game.remove_player(SHOOTER);
        assert_eq!(game.winner(), Some(Winner::Team(pb::Team::Blue)));
    }

    #[test]
    fn a_team_that_still_has_a_bot_does_not_forfeit() {
        let mut game = duel_with_bots([1, 1], pb::GameMode::Normal);
        game.remove_player(VICTIM);
        assert_eq!(game.winner(), None);
        // The bots carry on; the empty seat does not stop or crash the match.
        for _ in 0..(10 * TICK_RATE) {
            game.tick();
        }
        assert_eq!(game.roster().len(), 3);
        assert_eq!(game.winner(), None);
    }

    #[test]
    fn a_forfeit_does_not_overwrite_a_result_already_in() {
        let mut game = duel_with_bots([0, 0], pb::GameMode::Normal);
        game.world.resource_mut::<MatchState>().winner = Some(Winner::Team(pb::Team::Red));
        game.remove_player(SHOOTER);
        assert_eq!(game.winner(), Some(Winner::Team(pb::Team::Red)));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "would seat 6 players")]
    fn a_team_cannot_be_given_more_than_five_players() {
        let humans = [(SHOOTER, "a".into(), pb::Team::Red, RIFLE)];
        Match::with_mode(&humans, YARD, pb::GameMode::Normal, [5, 0], 3);
    }

    #[test]
    fn weapon_change_is_immediate_under_protection_otherwise_next_spawn() {
        let mut game = duel();
        game.select_weapon(SHOOTER, pb::WeaponKind::Smg);
        assert_eq!(weapon(&game, SHOOTER), (25, 125, false));

        game.world
            .get_mut::<Vitals>(game.entities[&SHOOTER])
            .unwrap()
            .protect_left = 0.0;
        game.select_weapon(SHOOTER, pb::WeaponKind::MachineGun);
        assert_eq!(
            weapon(&game, SHOOTER).0,
            25,
            "keeps the gun in hand mid-fight"
        );

        let entity = game.entities[&SHOOTER];
        {
            let mut v = game.world.get_mut::<Vitals>(entity).unwrap();
            v.alive = false;
            v.respawn_left = 0.01;
        }
        game.tick();
        assert_eq!(weapon(&game, SHOOTER), (100, 200, false));
    }

    #[test]
    fn normal_mode_is_unchanged_by_augments() {
        let mut game = duel();
        assert_eq!(hp(&game, SHOOTER), MAX_HP);
        let snapshot = game.shared_snapshot();
        assert_eq!(snapshot.freeze_left, 0.0);
        let mut personal = snapshot;
        game.fill_personal(SHOOTER, &mut personal);
        assert!(personal.offered.is_empty());
        game.pick_augment(SHOOTER, Aug::Missile);
        assert_eq!(held(&game, SHOOTER), RIFLE);
    }

    #[test]
    fn augment_mode_starts_with_150_hp_and_a_5_second_wait() {
        let mut game = augment_duel();
        assert_eq!(hp(&game, SHOOTER), 150);
        for id in [SHOOTER, VICTIM] {
            let card = game
                .world
                .get::<AugmentOffer>(game.entities[&id])
                .unwrap()
                .offered[0];
            game.pick_augment(id, card);
        }
        let start = game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap().x;
        let total = (augments::AUGMENT_WAIT * TICK_RATE as f32).round() as u32;
        for seq in 1..total {
            let yaw = 0.3;
            game.push_inputs(
                SHOOTER,
                vec![pb::InputCmd {
                    forward: true,
                    fire: true,
                    yaw,
                    ..cmd(seq, 0)
                }],
            );
            game.tick();
        }
        let e = game.entities[&SHOOTER];
        assert_eq!(
            game.world.get::<Pose>(e).unwrap().x,
            start,
            "no moving while waiting"
        );
        assert_eq!(
            game.world.get::<Pose>(e).unwrap().yaw,
            0.3,
            "aiming still works"
        );
        assert_eq!(weapon(&game, SHOOTER).0, 30, "no firing while waiting");
        let mut personal = game.shared_snapshot();
        game.fill_personal(SHOOTER, &mut personal);
        assert_eq!(
            personal.last_input_seq,
            total - 1,
            "inputs are still acknowledged"
        );
        assert!(personal.freeze_left > 0.0);
        // Ending the choice takes a tick, and rounding can leave the wait one more.
        game.push_inputs(SHOOTER, vec![]);
        game.tick();
        game.tick();
        game.tick();
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                forward: true,
                yaw: 1.5,
                ..cmd(total, 0)
            }],
        );
        game.tick();
        assert_ne!(
            game.world.get::<Pose>(e).unwrap().x,
            start,
            "moves once the wait is over"
        );
    }

    #[test]
    fn each_card_can_be_swapped_once_for_one_not_seen_yet() {
        for seed in 1..200 {
            let mut game = Match::with_mode(
                &[
                    (SHOOTER, "a".into(), pb::Team::Red, RIFLE),
                    (VICTIM, "b".into(), pb::Team::Blue, RIFLE),
                ],
                YARD,
                pb::GameMode::Augment,
                [0, 0],
                seed,
            );
            let e = game.entities[&SHOOTER];
            let mut seen = game.world.get::<AugmentOffer>(e).unwrap().offered.clone();
            assert_eq!(seen.len(), 3);
            assert!(
                seen[0] != seen[1] && seen[1] != seen[2] && seen[0] != seen[2],
                "{seen:?}"
            );

            for slot in 0..3 {
                let before = game.world.get::<AugmentOffer>(e).unwrap().offered.clone();
                game.refresh_augments(SHOOTER, slot);
                let after = game.world.get::<AugmentOffer>(e).unwrap().offered.clone();
                for i in 0..3 {
                    if i == slot as usize {
                        assert!(!seen.contains(&after[i]), "{seen:?} {after:?}");
                    } else {
                        assert_eq!(after[i], before[i], "only the chosen card changes");
                    }
                }
                seen.push(after[slot as usize]);
                game.refresh_augments(SHOOTER, slot);
                assert_eq!(
                    game.world.get::<AugmentOffer>(e).unwrap().offered,
                    after,
                    "once per card"
                );
            }
            let before = game.world.get::<AugmentOffer>(e).unwrap().offered.clone();
            game.refresh_augments(SHOOTER, 3);
            assert_eq!(game.world.get::<AugmentOffer>(e).unwrap().offered, before);
        }
    }

    #[test]
    fn choosing_takes_15_seconds_and_the_5_second_wait_comes_after() {
        let ticks = |seconds: f32| (seconds * TICK_RATE as f32).round() as u32;
        let mut game = augment_duel();
        let e = game.entities[&SHOOTER];
        for _ in 0..ticks(augments::AUGMENT_PICK) - 1 {
            game.tick();
        }
        assert_eq!(
            game.world.get::<Perks>(e).unwrap().augment,
            Aug::Unspecified,
            "still choosing"
        );
        assert_eq!(
            game.world.resource::<MatchState>().freeze_left,
            augments::AUGMENT_WAIT,
            "the wait hasn't begun"
        );
        let card = game.world.get::<AugmentOffer>(e).unwrap().offered[1];
        game.pick_augment(SHOOTER, card);
        game.tick();
        assert_eq!(
            game.world.get::<Perks>(e).unwrap().augment,
            card,
            "a late choice stands"
        );
        assert_ne!(
            game.world
                .get::<Perks>(game.entities[&VICTIM])
                .unwrap()
                .augment,
            Aug::Unspecified,
            "time chose"
        );
        assert!(game.world.resource::<MatchState>().freeze_left > 0.0);
        for _ in 0..ticks(augments::AUGMENT_WAIT) + 1 {
            game.tick();
        }
        let snapshot = game.shared_snapshot();
        assert_eq!(snapshot.freeze_left, 0.0);
        assert!(snapshot.clock < 0.1, "the clock starts with the fight");
        game.tick();
        assert!(game.shared_snapshot().clock > 0.0);
    }

    #[test]
    fn everyone_choosing_early_skips_to_the_wait() {
        let mut game = augment_duel();
        for id in [SHOOTER, VICTIM] {
            let card = game
                .world
                .get::<AugmentOffer>(game.entities[&id])
                .unwrap()
                .offered[0];
            game.pick_augment(id, card);
        }
        game.tick();
        assert_eq!(game.world.resource::<MatchState>().pick_left, 0.0);
        game.tick();
        assert!(game.world.resource::<MatchState>().freeze_left < augments::AUGMENT_WAIT);
    }

    #[test]
    fn double_time_applies_both_augments() {
        let mut game = with_augment(Aug::DoubleTime);
        let e = game.entities[&SHOOTER];
        assert_eq!(game.world.get::<Perks>(e).unwrap().active.len(), 2);
        // Both effects really work: a vest and a magnum at once.
        game.world.get_mut::<Perks>(e).unwrap().active = vec![Aug::Vest, Aug::Magnum];
        {
            let mut vitals = game.world.get_mut::<Vitals>(e).unwrap();
            vitals.alive = false;
            vitals.hp = 0;
            vitals.respawn_left = 0.01;
        }
        game.tick();
        assert_eq!(hp(&game, SHOOTER), 200);
        draw_sidearm(&mut game, SHOOTER);
        assert_eq!(held(&game, SHOOTER), Kind::Magnum);
    }

    #[test]
    fn only_a_card_on_offer_can_be_picked_and_time_picks_for_you() {
        let mut game = augment_duel();
        let e = game.entities[&SHOOTER];
        let offered = game.world.get::<AugmentOffer>(e).unwrap().offered.clone();
        let absent = [Aug::InfiniteAmmo, Aug::SoulDrain, Aug::Missile, Aug::Rest]
            .into_iter()
            .find(|a| !offered.contains(a))
            .unwrap();
        game.pick_augment(SHOOTER, absent);
        assert_eq!(
            game.world.get::<Perks>(e).unwrap().augment,
            Aug::Unspecified
        );

        game.pick_augment(SHOOTER, offered[1]);
        assert_eq!(game.world.get::<Perks>(e).unwrap().augment, offered[1]);

        for _ in
            0..((augments::AUGMENT_PICK + augments::AUGMENT_WAIT) * TICK_RATE as f32) as u32 + 2
        {
            game.tick();
        }
        let v = game.entities[&VICTIM];
        let victim_pick = game.world.get::<Perks>(v).unwrap().augment;
        assert_ne!(
            victim_pick,
            Aug::Unspecified,
            "the timer chose for the idle player"
        );
        assert_eq!(
            game.world.get::<Perks>(e).unwrap().augment,
            offered[1],
            "a choice stands"
        );
    }

    #[test]
    fn bots_choose_at_random() {
        let mut chosen = std::collections::HashSet::new();
        for seed in 1..40 {
            // The human on red, and each team filled to five with bots.
            let mut game = Match::with_mode(
                &[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)],
                YARD,
                pb::GameMode::Augment,
                [4, 5],
                seed,
            );
            let mut query = game.world.query::<(&Player, &Perks)>();
            for (player, perks) in query.iter(&game.world) {
                if player.is_bot {
                    assert_ne!(perks.augment, Aug::Unspecified);
                    chosen.insert(perks.augment as i32);
                }
            }
        }
        assert!(chosen.len() >= 12, "only saw {chosen:?}");
    }

    #[test]
    fn a_weapon_pick_cant_smuggle_in_augment_weapons() {
        let mut game = duel();
        game.select_weapon(SHOOTER, Kind::Missile);
        assert_eq!(held(&game, SHOOTER), RIFLE);
        let mut rpg = with_augment(Aug::Missile);
        rpg.select_weapon(SHOOTER, Kind::Sniper);
        assert_eq!(held(&rpg, SHOOTER), Kind::Missile);
    }

    #[test]
    fn full_augment_matches_run_clean() {
        for seed in 1..=8 {
            // The human on red, and each team filled to five with bots.
            let mut game = Match::with_mode(
                &[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)],
                YARD,
                pb::GameMode::Augment,
                [4, 5],
                seed,
            );
            for _ in 0..(TICK_RATE * 300) {
                game.tick();
                let mut query = game.world.query::<(&Vitals, &Weapon, &Perks)>();
                for (vitals, weapon, perks) in query.iter(&game.world) {
                    assert!(vitals.hp <= augments::VEST_MAX_HP && vitals.hp >= 0);
                    assert!(perks.shield_hp >= 0 && perks.shield_hp <= augments::SHIELD_HP);
                    assert!(weapon.magazine <= weapons::stats(weapon.kind).magazine.max(1));
                }
                if game.winner().is_some() {
                    break;
                }
            }
            assert!(game.winner().is_some(), "seed {seed} never finished");
        }
    }

    #[test]
    fn a_free_for_all_left_with_one_player_is_theirs() {
        let mut game = ffa_duel();
        game.remove_player(VICTIM);
        assert_eq!(game.winner(), Some(Winner::Player(SHOOTER)));
        let end = game.end_message(8.0);
        assert_eq!(
            (end.winner, end.winner_id, end.red_score, end.blue_score),
            (NO_TEAM as i32, SHOOTER, 0, 0)
        );
    }

    #[test]
    fn a_free_for_all_with_a_bot_left_goes_on() {
        let mut game = ffa_with_bots(1, pb::GameMode::Normal);
        game.remove_player(VICTIM);
        assert_eq!(game.winner(), None, "the shooter and the bot play on");
        for _ in 0..(5 * TICK_RATE) {
            game.tick();
        }
        assert_eq!(game.roster().len(), 2);
        assert_eq!(game.winner(), None);
    }

    #[test]
    fn a_free_for_all_forfeit_ends_a_match_that_is_still_choosing_augments() {
        let mut game = ffa_with_bots(0, pb::GameMode::Augment);
        assert!(
            game.world.resource::<MatchState>().pick_left > 0.0,
            "still choosing"
        );
        game.remove_player(SHOOTER);
        assert_eq!(game.winner(), Some(Winner::Player(VICTIM)));
    }
}
