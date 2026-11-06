//! Respawning: the dead fall, wait out the respawn time and come back on a spawn point.

use super::weapon::{forced_primary, issue_weapons};
use crate::game::components::*;
use crate::game::physics::step_vertical;
use crate::game::resources::*;
use crate::game::*;
use crate::pb;
use crate::pb::game_event::Event;
use bevy_ecs::prelude::*;

/// The spawn point on the player's side that is furthest from any living enemy.
pub fn choose_spawn(
    spawns: &[(f32, f32)],
    team: pb::Team,
    enemies: &[(f32, f32)],
    rng: &mut GameRng,
) -> (f32, f32) {
    let side = if team == pb::Team::Red { 1.0 } else { -1.0 };
    let points: Vec<(f32, f32)> = spawns.iter().map(|(x, z)| (x * side, z * side)).collect();
    furthest_spawn(&points, enemies, rng)
}

/// Of `points`, the one furthest from the nearest of `enemies`, or any when there are none.
/// A free-for-all chooses from all its points this way, with everyone else as an enemy.
pub fn furthest_spawn(
    points: &[(f32, f32)],
    enemies: &[(f32, f32)],
    rng: &mut GameRng,
) -> (f32, f32) {
    if enemies.is_empty() {
        return points[rng.index(points.len())];
    }
    let mut best = points[0];
    let mut best_score = f32::MIN;
    for &(x, z) in points {
        let nearest = enemies
            .iter()
            .map(|(ex, ez)| (ex - x).powi(2) + (ez - z).powi(2))
            .fold(f32::MAX, f32::min);
        // Jitter so players don't all reuse a single point.
        let score = nearest.sqrt() + rng.range(0.0, 8.0);
        if score > best_score {
            best_score = score;
            best = (x, z);
        }
    }
    best
}

/// Yaw that faces the middle of the map from `(x, z)`.
pub fn ffa_yaw(x: f32, z: f32) -> f32 {
    x.atan2(z)
}

/// Yaw that faces the opposing side of the map.
pub fn spawn_yaw(team: pb::Team) -> f32 {
    if team == pb::Team::Red {
        -std::f32::consts::FRAC_PI_2
    } else {
        std::f32::consts::FRAC_PI_2
    }
}

#[allow(clippy::type_complexity)]
pub fn respawn_players(
    arena: Res<Arena>,
    rules: Res<Rules>,
    mut rng: ResMut<GameRng>,
    mut log: ResMut<EventLog>,
    mut players: Query<(
        &Player,
        &mut Pose,
        &mut Vitals,
        &mut Weapon,
        &mut Loadout,
        &mut History,
        &mut Perks,
    )>,
) {
    let mut living: Vec<(pb::Team, f32, f32)> = players
        .iter()
        .filter(|(_, _, vitals, _, _, _, _)| vitals.alive)
        .map(|(player, pose, _, _, _, _, _)| (player.team, pose.x, pose.z))
        .collect();

    for (player, mut pose, mut vitals, mut weapon, mut loadout, mut history, mut perks) in
        &mut players
    {
        if vitals.alive {
            vitals.protect_left = (vitals.protect_left - TICK_DT).max(0.0);
            vitals.stun_left = (vitals.stun_left - TICK_DT).max(0.0);
            continue;
        }
        // A body killed in the air falls to the floor or the box top below it: two command steps a tick.
        let (mut y, mut vy) = (pose.y, pose.vy);
        for _ in 0..2 {
            step_vertical(&arena.map.boxes, pose.x, pose.z, &mut y, &mut vy, false);
        }
        (pose.y, pose.vy) = (y, vy);
        vitals.respawn_left -= TICK_DT;
        if vitals.respawn_left > 0.0 {
            continue;
        }
        // The player is dead, so not among the living: in a free-for-all, everyone there is an enemy.
        let enemies: Vec<(f32, f32)> = living
            .iter()
            .filter(|(team, _, _)| rules.hostile(*team, player.team))
            .map(|(_, x, z)| (*x, *z))
            .collect();
        let (x, z, yaw) = if rules.ffa {
            let (x, z) = furthest_spawn(&arena.ffa_spawns, &enemies, &mut rng);
            // Anyone else respawning this tick keeps away from here too, rather than sharing the point.
            living.push((player.team, x, z));
            (x, z, ffa_yaw(x, z))
        } else {
            let (x, z) = choose_spawn(&arena.spawns, player.team, &enemies, &mut rng);
            (x, z, spawn_yaw(player.team))
        };
        // Spawn points are on the floor.
        *pose = Pose {
            x,
            z,
            yaw,
            ..Default::default()
        };
        *vitals = Vitals {
            hp: augments::max_hp(rules.max_hp, &perks.active),
            alive: true,
            respawn_left: 0.0,
            protect_left: SPAWN_PROTECTION,
            stun_left: 0.0,
        };
        // Kept weapons: an augment's own, or one taken by Possession.
        let keeps_primary = forced_primary(&perks.active).is_some()
            || perks.has(pb::Augment::Possession)
            || perks.has(pb::Augment::Akimbo);
        if player.is_bot && !keeps_primary {
            loadout.0 = weapons::random_for_bot(&mut rng);
        }
        *weapon = issue_weapons(loadout.0, &perks.active, player.is_bot);
        perks.damaged_by.clear();
        perks.idle = 0.0;
        perks.regen = 0.0;
        perks.shield_hp = if perks.has(pb::Augment::Shield) {
            augments::SHIELD_HP
        } else {
            0
        };
        // Big Head mode: a player who reached the top level while down is crowned as they come back.
        if rules.big_head && perks.level >= big_head::MAX_LEVEL && !perks.king() {
            big_head::crown(&mut perks, &mut vitals, &mut weapon, rules.max_hp);
        }
        // Don't let a rewound shot hit where the player died.
        history.samples.clear();
        log.push(Event::Respawn(pb::RespawnEvent {
            player_id: player.id,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn dead_player_respawns_with_full_ammo() {
        let mut game = duel();
        let shot = line_up(&mut game, 0.03);
        let victim = game.entities[&VICTIM];
        game.world.get_mut::<Weapon>(victim).unwrap().magazine = 3;
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        assert!(!game.world.get::<Vitals>(victim).unwrap().alive);
        for _ in 0..(RESPAWN_TIME * TICK_RATE as f32) as u32 + 2 {
            game.tick();
        }
        let vitals = game.world.get::<Vitals>(victim).unwrap();
        assert!(vitals.alive);
        assert_eq!(vitals.hp, MAX_HP);
        assert_eq!(weapon(&game, VICTIM), (30, 120, false));
        assert!(
            game.world.get::<Pose>(victim).unwrap().x > 40.0,
            "blue spawns on the +x side"
        );
    }

    #[test]
    fn shield_is_only_restored_by_respawning() {
        let mut game = with_augment(Aug::Shield);
        let e = game.entities[&SHOOTER];
        assert_eq!(game.world.get::<Perks>(e).unwrap().shield_hp, 1200);
        game.world.get_mut::<Perks>(e).unwrap().shield_hp = 10;
        for _ in 0..(TICK_RATE * 10) {
            game.tick();
        }
        assert_eq!(
            game.world.get::<Perks>(e).unwrap().shield_hp,
            10,
            "no repair out of combat"
        );

        {
            let mut vitals = game.world.get_mut::<Vitals>(e).unwrap();
            vitals.alive = false;
            vitals.hp = 0;
            vitals.respawn_left = 0.01;
        }
        game.tick();
        assert!(game.world.get::<Vitals>(e).unwrap().alive);
        assert_eq!(
            game.world.get::<Perks>(e).unwrap().shield_hp,
            augments::SHIELD_HP
        );
    }

    #[test]
    fn vest_raises_max_hp_to_200_also_after_respawn() {
        let mut game = with_augment(Aug::Vest);
        assert_eq!(hp(&game, SHOOTER), 200);
        let e = game.entities[&SHOOTER];
        {
            let mut vitals = game.world.get_mut::<Vitals>(e).unwrap();
            vitals.alive = false;
            vitals.hp = 0;
            vitals.respawn_left = 0.01;
        }
        game.tick();
        assert_eq!(hp(&game, SHOOTER), 200);
    }

    #[test]
    fn free_for_all_players_start_on_points_of_their_own_facing_the_middle() {
        for map in [YARD, pb::MapId::Warehouse] {
            for seed in 1..=50 {
                let game = Match::with_rules(
                    &[],
                    map,
                    pb::GameMode::Normal,
                    pb::MatchType::FreeForAll,
                    [5, 5],
                    seed,
                );
                let mut taken = Vec::new();
                for &entity in game.entities.values() {
                    let point = assert_ffa_spawned(&game, entity);
                    assert!(
                        !taken.contains(&point),
                        "{map:?}, seed {seed}: two start at {point:?}"
                    );
                    taken.push(point);
                }
                assert_eq!(taken.len(), 2 * TEAM_SIZE);
            }
        }
    }

    #[test]
    fn free_for_all_players_respawning_on_the_same_tick_get_points_of_their_own() {
        let humans = [
            (SHOOTER, "a".into(), NO_TEAM, RIFLE),
            (VICTIM, "b".into(), NO_TEAM, RIFLE),
        ];
        for map in [YARD, pb::MapId::Warehouse] {
            for seed in 1..=50 {
                let mut game = Match::with_rules(
                    &humans,
                    map,
                    pb::GameMode::Normal,
                    pb::MatchType::FreeForAll,
                    [4, 4],
                    seed,
                );
                // Everyone but the shooter is due back on the next tick.
                for (&id, &entity) in &game.entities {
                    if id != SHOOTER {
                        let mut vitals = game.world.get_mut::<Vitals>(entity).unwrap();
                        vitals.alive = false;
                        vitals.hp = 0;
                        vitals.respawn_left = 0.01;
                    }
                }
                game.tick();
                let shooter = game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap();
                let mut taken = vec![(shooter.x, shooter.z)];
                for (&id, &entity) in &game.entities {
                    if id == SHOOTER {
                        continue;
                    }
                    assert!(game.world.get::<Vitals>(entity).unwrap().alive);
                    let point = assert_ffa_spawned(&game, entity);
                    assert!(
                        !taken.contains(&point),
                        "{map:?}, seed {seed}: two at {point:?}"
                    );
                    taken.push(point);
                }
            }
        }
    }
}
