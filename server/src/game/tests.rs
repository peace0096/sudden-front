use super::components::*;
use super::map::build_map;
use super::nav::NavGrid;
use super::physics::{blocks, step_move};
use pb::Augment as Aug;
use pb::WeaponKind as Kind;
use super::*;
use crate::pb;

const SHOOTER: u32 = 1;
const VICTIM: u32 = 2;
const RIFLE: pb::WeaponKind = pb::WeaponKind::Rifle;
const YARD: pb::MapId = pb::MapId::SupplyYard;

fn duel() -> Match {
    Match::new(
        &[
            (SHOOTER, "a".into(), pb::Team::Red, RIFLE),
            (VICTIM, "b".into(), pb::Team::Blue, RIFLE),
        ],
        YARD,
        false,
        42,
    )
}

fn place(game: &mut Match, id: u32, x: f32, z: f32, yaw: f32) {
    let entity = game.entities[&id];
    *game.world.get_mut::<Pose>(entity).unwrap() = Pose {
        x,
        z,
        yaw,
        pitch: 0.0,
        crouching: false,
    };
    game.world.get_mut::<Vitals>(entity).unwrap().protect_left = 0.0;
    game.world.get_mut::<History>(entity).unwrap().samples.clear();
}

fn cmd(seq: u32, tick: u32) -> pb::InputCmd {
    pb::InputCmd {
        seq,
        render_tick: tick,
        ..Default::default()
    }
}

fn weapon(game: &Match, id: u32) -> (u32, u32, bool) {
    let w = game.world.get::<Weapon>(game.entities[&id]).unwrap();
    (w.magazine, w.reserve, w.reloading())
}

fn tick_now(game: &Match) -> u32 {
    game.world.resource::<MatchState>().tick
}

#[test]
fn spawns_with_30_mag_and_120_reserve() {
    assert_eq!(weapon(&duel(), SHOOTER), (30, 120, false));
}

#[test]
fn every_map_is_fully_connected_and_spawns_are_clear() {
    for id in [pb::MapId::SupplyYard, pb::MapId::Warehouse] {
        let def = build_map(id);
        let map = &def.data;
        let nav = NavGrid::new(&map.boxes, map.half_x, map.half_z);
        let total = nav.open_cells();
        let spawn = def.spawns[0];
        assert_eq!(nav.reachable_from(spawn.0, spawn.1), total, "{id:?}: isolated area");
        assert_eq!(nav.reachable_from(-spawn.0, -spawn.1), total, "{id:?}: isolated area");
        for (x, z) in def.spawns.iter().chain(def.lanes.iter()) {
            for side in [1.0, -1.0] {
                let (x, z) = (x * side, z * side);
                let inside = map.boxes.iter().any(|b| {
                    blocks(b)
                        && x + PLAYER_HALF_WIDTH > b.min_x
                        && x - PLAYER_HALF_WIDTH < b.max_x
                        && z + PLAYER_HALF_WIDTH > b.min_z
                        && z - PLAYER_HALF_WIDTH < b.max_z
                });
                assert!(!inside, "{id:?}: point ({x},{z}) is inside a collider");
            }
        }
    }
}

#[test]
fn walls_stop_movement() {
    let map = build_map(YARD).data;
    let (mut x, mut z) = (-20.0, -3.0);
    for _ in 0..200 {
        step_move(&map.boxes, &mut x, &mut z, 0.0, -0.1);
    }
    // The loading dock's front edge is at z = -5.4.
    assert!((z - (-5.4 + PLAYER_HALF_WIDTH)).abs() < 1e-3, "z = {z}");
}

#[test]
fn speed_hack_inputs_are_rate_limited() {
    let mut game = duel();
    place(&mut game, SHOOTER, -40.0, 0.0, -std::f32::consts::FRAC_PI_2);
    let flood: Vec<pb::InputCmd> = (1..=30)
        .map(|seq| pb::InputCmd {
            forward: true,
            yaw: -std::f32::consts::FRAC_PI_2,
            ..cmd(seq, 0)
        })
        .collect();
    game.push_inputs(SHOOTER, flood);
    game.tick();
    let pose = *game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap();
    // At most the budget cap's worth of 1/60 s steps in one tick.
    assert!(pose.x - -40.0 <= weapons::stats(RIFLE).move_speed * CMD_DT * 4.0 + 1e-3, "moved {}", pose.x + 40.0);
}

#[test]
fn nan_view_angles_do_not_corrupt_state() {
    let mut game = duel();
    game.push_inputs(
        SHOOTER,
        vec![pb::InputCmd {
            forward: true,
            yaw: f32::NAN,
            pitch: f32::INFINITY,
            ..cmd(1, 0)
        }],
    );
    game.tick();
    let pose = *game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap();
    assert!(pose.x.is_finite() && pose.z.is_finite() && pose.yaw.is_finite() && pose.pitch.is_finite());
}

#[test]
fn firing_consumes_ammo_at_fire_rate_and_reload_refills_from_reserve() {
    let mut game = duel();
    place(&mut game, SHOOTER, -45.0, 28.0, std::f32::consts::PI);
    // Hold fire for one second: 600 rpm means 10 shots at most.
    for seq in 0..TICK_RATE {
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                fire: true,
                ..cmd(seq * 2 + 1, tick_now(&game))
            }],
        );
        game.tick();
    }
    let (mag, reserve, _) = weapon(&game, SHOOTER);
    assert_eq!(mag, 20);
    assert_eq!(reserve, 120);

    game.push_inputs(
        SHOOTER,
        vec![pb::InputCmd {
            reload: true,
            ..cmd(100, 0)
        }],
    );
    game.tick();
    assert!(weapon(&game, SHOOTER).2, "reload should have started");
    // Can't fire during the reload.
    game.push_inputs(
        SHOOTER,
        vec![pb::InputCmd {
            fire: true,
            ..cmd(101, 0)
        }],
    );
    game.tick();
    assert_eq!(weapon(&game, SHOOTER).0, 20);

    for _ in 0..(weapons::stats(RIFLE).reload_time * TICK_RATE as f32) as u32 + 2 {
        game.tick();
    }
    assert_eq!(weapon(&game, SHOOTER), (30, 110, false));
}

#[test]
fn cannot_fire_on_empty_magazine() {
    let mut game = duel();
    game.world
        .get_mut::<Weapon>(game.entities[&SHOOTER])
        .unwrap()
        .magazine = 0;
    game.push_inputs(
        SHOOTER,
        vec![pb::InputCmd {
            fire: true,
            ..cmd(1, 0)
        }],
    );
    game.tick();
    assert_eq!(weapon(&game, SHOOTER).0, 0);
    let shot = game.shared_snapshot().events.iter().any(|e| matches!(e.event, Some(pb::game_event::Event::Shot(_))));
    assert!(!shot);
}

/// Shooter at (-44, 0) looking along +X at a victim 6 m away with clear sight.
fn line_up(game: &mut Match, pitch: f32) -> pb::InputCmd {
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(game, SHOOTER, -46.0, 0.0, yaw);
    place(game, VICTIM, -42.0, 0.0, 0.0);
    game.tick(); // Record the placed positions in history.
    pb::InputCmd {
        fire: true,
        yaw,
        pitch,
        ..cmd(1, tick_now(game))
    }
}

#[test]
fn headshot_kills_and_scores_for_the_team() {
    let mut game = duel();
    let shot = line_up(&mut game, 0.03);
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    let snapshot = game.shared_snapshot();
    assert_eq!(snapshot.red_score, 1);
    let kill = snapshot.events.iter().find_map(|e| match &e.event {
        Some(pb::game_event::Event::Kill(k)) => Some(k.clone()),
        _ => None,
    });
    let kill = kill.expect("kill event");
    assert!(kill.headshot);
    assert_eq!(kill.victim_id, VICTIM);
    let victim = game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap();
    assert!(!victim.alive);
}

#[test]
fn body_shot_does_partial_damage() {
    let mut game = duel();
    let shot = line_up(&mut game, -0.1);
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    let victim = game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap();
    assert_eq!(victim.hp, MAX_HP - weapons::stats(RIFLE).damage_body);
    assert!(victim.alive);
}

#[test]
fn walls_block_bullets() {
    let mut game = duel();
    // Either side of the walk-through warehouse's solid north wall.
    let yaw = std::f32::consts::PI;
    place(&mut game, SHOOTER, -31.0, 5.0, yaw);
    place(&mut game, VICTIM, -31.0, 8.0, 0.0);
    game.tick();
    game.push_inputs(
        SHOOTER,
        vec![pb::InputCmd {
            fire: true,
            yaw,
            pitch: -0.1,
            ..cmd(1, tick_now(&game))
        }],
    );
    game.tick();
    let victim = game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap();
    assert_eq!(victim.hp, MAX_HP);
}

#[test]
fn no_friendly_fire() {
    let mut game = Match::new(
        &[
            (SHOOTER, "a".into(), pb::Team::Red, RIFLE),
            (VICTIM, "b".into(), pb::Team::Red, RIFLE),
        ],
        YARD,
        false,
        9,
    );
    let shot = line_up(&mut game, -0.1);
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    let mate = game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap();
    assert_eq!(mate.hp, MAX_HP);
}

#[test]
fn lag_compensation_hits_where_the_client_saw_the_target() {
    let mut game = duel();
    let shot = line_up(&mut game, -0.1);
    let seen_at = shot.render_tick;
    // Tick along so the sighting is in the past, then move the victim away.
    game.tick();
    game.tick();
    let entity = game.entities[&VICTIM];
    game.world.get_mut::<Pose>(entity).unwrap().z = 10.0;
    game.push_inputs(
        SHOOTER,
        vec![pb::InputCmd {
            render_tick: seen_at,
            ..shot
        }],
    );
    game.tick();
    let victim = game.world.get::<Vitals>(entity).unwrap();
    assert_eq!(victim.hp, MAX_HP - weapons::stats(RIFLE).damage_body);
}

#[test]
fn spawn_protection_blocks_damage_until_it_expires() {
    let mut game = duel();
    let shot = line_up(&mut game, -0.1);
    let entity = game.entities[&VICTIM];
    game.world.get_mut::<Vitals>(entity).unwrap().protect_left = 1.0;
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    assert_eq!(game.world.get::<Vitals>(entity).unwrap().hp, MAX_HP);
}

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
    assert!(game.world.get::<Pose>(victim).unwrap().x > 40.0, "blue spawns on the +x side");
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
    assert_eq!(game.winner(), Some(pb::Team::Red));
    assert_eq!(game.end_message(pb::Team::Red, 8.0).red_score, 10);
}

#[test]
fn first_team_to_30_kills_wins_and_the_match_freezes() {
    let mut game = duel();
    game.world.resource_mut::<MatchState>().red_score = KILLS_TO_WIN - 1;
    let shot = line_up(&mut game, 0.03);
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    assert_eq!(game.winner(), Some(pb::Team::Red));
    let end = game.end_message(pb::Team::Red, 8.0);
    assert_eq!(end.red_score, KILLS_TO_WIN);
    let frozen = tick_now(&game);
    game.tick();
    assert_eq!(tick_now(&game), frozen);
}

#[test]
fn fill_bots_makes_it_five_versus_five() {
    let mut game = Match::new(&[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)], YARD, true, 3);
    let roster = game.roster();
    assert_eq!(roster.len(), 10);
    assert_eq!(roster.iter().filter(|r| r.team == pb::Team::Red as i32).count(), 5);
    assert_eq!(roster.iter().filter(|r| r.is_bot).count(), 9);
}

#[test]
fn each_team_gets_as_many_bots_as_it_is_given() {
    // One human on red, then two bots for red and three for blue.
    let humans = [(SHOOTER, "a".into(), pb::Team::Red, RIFLE)];
    let mut game = Match::with_mode(&humans, YARD, pb::GameMode::Normal, [2, 3], 3);
    let roster = game.roster();
    let on = |team: pb::Team, bots: bool| roster.iter().filter(|r| r.team == team as i32 && r.is_bot == bots).count();
    assert_eq!((on(pb::Team::Red, false), on(pb::Team::Red, true)), (1, 2));
    assert_eq!((on(pb::Team::Blue, false), on(pb::Team::Blue, true)), (0, 3));
    assert_eq!(roster.len(), 6);
}

#[test]
fn a_leaving_player_is_not_replaced() {
    let mut game = Match::new(&[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)], YARD, true, 3);
    let roster = game.remove_player(SHOOTER);
    // The seat stays empty: nine bots are left, four of them on the leaver's team.
    assert_eq!(roster.len(), 9);
    assert!(roster.iter().all(|r| r.is_bot));
    assert_eq!(roster.iter().filter(|r| r.team == pb::Team::Red as i32).count(), 4);
}

/// One human on each team, plus the bots for each, as `[red, blue]`.
fn duel_with_bots(bots: [usize; 2], mode: pb::GameMode) -> Match {
    let humans = [
        (SHOOTER, "a".into(), pb::Team::Red, RIFLE),
        (VICTIM, "b".into(), pb::Team::Blue, RIFLE),
    ];
    Match::with_mode(&humans, YARD, mode, bots, 3)
}

#[test]
fn a_team_left_with_nobody_forfeits_the_match() {
    // Blue is only its human, so with them gone red wins at once, with no kill scored.
    let mut game = duel_with_bots([2, 0], pb::GameMode::Normal);
    assert_eq!(game.winner(), None);
    game.remove_player(VICTIM);
    assert_eq!(game.winner(), Some(pb::Team::Red));
    let end = game.end_message(pb::Team::Red, 8.0);
    assert_eq!((end.winner, end.red_score, end.blue_score), (pb::Team::Red as i32, 0, 0));
    // It is over: the clock no longer runs.
    let frozen = tick_now(&game);
    game.tick();
    assert_eq!(tick_now(&game), frozen);
}

#[test]
fn a_forfeit_ends_a_match_that_is_still_choosing_augments() {
    let mut game = duel_with_bots([0, 0], pb::GameMode::Augment);
    assert!(game.world.resource::<MatchState>().freeze_left > 0.0, "still frozen before play");
    game.remove_player(SHOOTER);
    assert_eq!(game.winner(), Some(pb::Team::Blue));
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
    game.world.resource_mut::<MatchState>().winner = Some(pb::Team::Red);
    game.remove_player(SHOOTER);
    assert_eq!(game.winner(), Some(pb::Team::Red));
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "would seat 6 players")]
fn a_team_cannot_be_given_more_than_five_players() {
    let humans = [(SHOOTER, "a".into(), pb::Team::Red, RIFLE)];
    Match::with_mode(&humans, YARD, pb::GameMode::Normal, [5, 0], 3);
}

#[test]
fn bot_match_plays_to_completion_with_a_30_kill_winner() {
    let mut game = Match::new(&[], YARD, true, 777);
    // 20 simulated minutes is far beyond any plausible match length.
    for _ in 0..(20 * 60 * TICK_RATE) {
        game.tick();
        if game.winner().is_some() {
            break;
        }
    }
    let winner = game.winner().expect("bots never finished: AI is stuck");
    let snapshot = game.shared_snapshot();
    let (winning, losing) = if winner == pb::Team::Red {
        (snapshot.red_score, snapshot.blue_score)
    } else {
        (snapshot.blue_score, snapshot.red_score)
    };
    assert_eq!(winning, KILLS_TO_WIN);
    assert!(losing < KILLS_TO_WIN);
    println!(
        "match took {:.1}s, {}-{}",
        snapshot.tick as f32 / TICK_RATE as f32,
        snapshot.red_score,
        snapshot.blue_score
    );
}

#[test]
fn empty_magazine_reloads_automatically() {
    let mut game = duel();
    game.world.get_mut::<Weapon>(game.entities[&SHOOTER]).unwrap().magazine = 0;
    game.tick();
    assert!(weapon(&game, SHOOTER).2, "reload should start with no R press");
    for _ in 0..(weapons::stats(RIFLE).reload_time * TICK_RATE as f32) as u32 + 2 {
        game.tick();
    }
    assert_eq!(weapon(&game, SHOOTER), (30, 90, false));
}

#[test]
fn no_auto_reload_without_reserve() {
    let mut game = duel();
    let entity = game.entities[&SHOOTER];
    {
        let mut w = game.world.get_mut::<Weapon>(entity).unwrap();
        w.magazine = 0;
        w.reserve = 0;
    }
    game.tick();
    assert_eq!(weapon(&game, SHOOTER), (0, 0, false));
}

#[test]
fn weapon_table_matches_the_spec() {
    use pb::WeaponKind::*;
    let (rifle, sniper, mg, smg) = (weapons::stats(Rifle), weapons::stats(Sniper), weapons::stats(MachineGun), weapons::stats(Smg));
    assert_eq!((rifle.magazine, rifle.reserve), (30, 120));
    assert_eq!((sniper.magazine, sniper.reserve), (5, 20));
    assert_eq!((mg.magazine, mg.reserve), (100, 200));
    assert_eq!((smg.magazine, smg.reserve), (25, 125));
    // Damage: sniper > machine gun > rifle > SMG.
    assert!(sniper.damage_body > mg.damage_body && mg.damage_body > rifle.damage_body && rifle.damage_body > smg.damage_body);
    // Speed: SMG > rifle > sniper > machine gun.
    assert!(smg.move_speed > rifle.move_speed && rifle.move_speed > sniper.move_speed && sniper.move_speed > mg.move_speed);
    // Fire rate: machine gun > SMG > rifle > bolt action.
    assert!(mg.fire_interval < smg.fire_interval && smg.fire_interval < rifle.fire_interval && rifle.fire_interval < sniper.fire_interval);
    // Hip accuracy: SMG > rifle > sniper > machine gun, and the scope beats all.
    assert!(smg.spread_still < rifle.spread_still && rifle.spread_still < sniper.spread_still && sniper.spread_still < mg.spread_still);
    assert!(sniper.scoped_spread_still < smg.spread_still);
}

fn shots_in_one_second(kind: pb::WeaponKind) -> u32 {
    let mut game = Match::new(&[(SHOOTER, "a".into(), pb::Team::Red, kind)], YARD, false, 5);
    let start = weapons::stats(kind).magazine;
    for seq in 0..TICK_RATE {
        game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, ..cmd(seq + 1, tick_now(&game)) }]);
        game.tick();
    }
    start - weapon(&game, SHOOTER).0
}

#[test]
fn fire_rates_are_distinct_at_a_30hz_tick() {
    assert_eq!(shots_in_one_second(pb::WeaponKind::Sniper), 1);
    assert_eq!(shots_in_one_second(pb::WeaponKind::Rifle), 10);
    // 800 and 1000 rpm land between whole counts within one second.
    let smg = shots_in_one_second(pb::WeaponKind::Smg);
    let mg = shots_in_one_second(pb::WeaponKind::MachineGun);
    assert!((13..=14).contains(&smg), "smg fired {smg}");
    assert!((16..=17).contains(&mg), "mg fired {mg}");
}

#[test]
fn sniper_body_shot_kills_in_one_hit() {
    let mut game = duel();
    game.select_weapon(SHOOTER, pb::WeaponKind::Sniper);
    let shot = line_up(&mut game, -0.1);
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    assert!(!game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap().alive);
}

#[test]
fn scope_needs_a_moment_and_only_counts_for_scoped_weapons() {
    use pb::WeaponKind::*;
    let sniper = weapons::stats(Sniper);
    assert!(systems::shot_cone(sniper, true, false, 0.0) < systems::shot_cone(sniper, false, false, 0.0) / 10.0);
    let rifle = weapons::stats(Rifle);
    assert_eq!(systems::shot_cone(rifle, true, false, 0.0), systems::shot_cone(rifle, false, false, 0.0));

    let mut game = duel();
    game.select_weapon(SHOOTER, Sniper);
    let entity = game.entities[&SHOOTER];
    for seq in 0..3 {
        game.push_inputs(SHOOTER, vec![pb::InputCmd { aim: true, ..cmd(seq + 1, 0) }]);
        game.tick();
    }
    let early = game.world.get::<Weapon>(entity).unwrap().aim_time;
    assert!(early < systems::SCOPE_READY_TIME);
    for seq in 3..12 {
        game.push_inputs(SHOOTER, vec![pb::InputCmd { aim: true, ..cmd(seq + 1, 0) }]);
        game.tick();
    }
    assert!(game.world.get::<Weapon>(entity).unwrap().aim_time >= systems::SCOPE_READY_TIME);
}

#[test]
fn weapon_change_is_immediate_under_protection_otherwise_next_spawn() {
    let mut game = duel();
    game.select_weapon(SHOOTER, pb::WeaponKind::Smg);
    assert_eq!(weapon(&game, SHOOTER), (25, 125, false));

    game.world.get_mut::<Vitals>(game.entities[&SHOOTER]).unwrap().protect_left = 0.0;
    game.select_weapon(SHOOTER, pb::WeaponKind::MachineGun);
    assert_eq!(weapon(&game, SHOOTER).0, 25, "keeps the gun in hand mid-fight");

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
fn movement_speed_depends_on_the_weapon() {
    use pb::WeaponKind::*;
    let walked = |kind| {
        let mut game = Match::new(&[(SHOOTER, "a".into(), pb::Team::Red, kind)], YARD, false, 5);
        let yaw = std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, 40.0, -30.0, yaw);
        let before = game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap().x;
        for seq in 0..20 {
            game.push_inputs(SHOOTER, vec![pb::InputCmd { forward: true, yaw, ..cmd(seq + 1, 0) }]);
            game.tick();
        }
        (game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap().x - before).abs()
    };
    assert!(walked(Smg) > walked(Rifle));
    assert!(walked(Rifle) > walked(Sniper));
    assert!(walked(Sniper) > walked(MachineGun));
}

#[test]
fn warehouse_bot_match_finishes() {
    let mut game = Match::new(&[], pb::MapId::Warehouse, true, 91);
    for _ in 0..(20 * 60 * TICK_RATE) {
        game.tick();
        if game.winner().is_some() {
            break;
        }
    }
    assert!(game.winner().is_some(), "bots never finished on the warehouse map");
    let s = game.shared_snapshot();
    println!("warehouse match took {:.1}s, {}-{}", s.tick as f32 / TICK_RATE as f32, s.red_score, s.blue_score);
}

#[test]
fn sliding_along_any_wall_never_teleports() {
    // Rounding can leave a player at `wall + radius` looking a hair inside the
    // wall. Moving sideways must not treat that as a collision.
    for id in [pb::MapId::SupplyYard, pb::MapId::Warehouse] {
        let map = build_map(id).data;
        for kind in weapons::ALL {
            let step = weapons::stats(kind).move_speed * CMD_DT;
            for b in map.boxes.iter().filter(|b| blocks(b)) {
                // Each of the four faces: (start x, start z, along x, along z, into x, into z).
                let r = PLAYER_HALF_WIDTH;
                let faces = [
                    (b.min_x, b.max_z + r, step, 0.0, 0.0, -step),
                    (b.max_x, b.min_z - r, -step, 0.0, 0.0, step),
                    (b.min_x - r, b.min_z, 0.0, step, step, 0.0),
                    (b.max_x + r, b.max_z, 0.0, -step, -step, 0.0),
                ];
                for (mut x, mut z, along_x, along_z, into_x, into_z) in faces {
                    for _ in 0..60 {
                        let (old_x, old_z) = (x, z);
                        step_move(&map.boxes, &mut x, &mut z, along_x + into_x, along_z + into_z);
                        let moved = (x - old_x).abs().max((z - old_z).abs());
                        assert!(moved <= step * 1.5, "{id:?}: jumped {moved} from ({old_x},{old_z}) to ({x},{z})");
                    }
                }
            }
        }
    }
}

#[test]
fn slots_keep_their_own_ammo() {
    let mut game = duel();
    let e = game.entities[&SHOOTER];
    game.world.get_mut::<Weapon>(e).unwrap().magazine = 17;
    game.push_inputs(SHOOTER, vec![pb::InputCmd { slot: 2, ..cmd(1, 0) }]);
    game.tick();
    assert_eq!(weapon(&game, SHOOTER), (12, 36, false));
    assert_eq!(game.world.get::<Weapon>(e).unwrap().kind, pb::WeaponKind::Pistol);

    game.push_inputs(SHOOTER, vec![pb::InputCmd { slot: 3, ..cmd(2, 0) }]);
    game.tick();
    assert_eq!(game.world.get::<Weapon>(e).unwrap().kind, pb::WeaponKind::Knife);

    game.push_inputs(SHOOTER, vec![pb::InputCmd { slot: 1, ..cmd(3, 0) }]);
    game.tick();
    assert_eq!(weapon(&game, SHOOTER), (17, 120, false));
    assert_eq!(game.world.get::<Weapon>(e).unwrap().kind, RIFLE);
}

#[test]
fn switching_abandons_a_reload_and_takes_time_to_draw() {
    let mut game = duel();
    let e = game.entities[&SHOOTER];
    game.world.get_mut::<Weapon>(e).unwrap().magazine = 10;
    game.push_inputs(SHOOTER, vec![pb::InputCmd { reload: true, ..cmd(1, 0) }]);
    game.tick();
    assert!(weapon(&game, SHOOTER).2);

    game.push_inputs(SHOOTER, vec![pb::InputCmd { slot: 2, fire: true, ..cmd(2, 0) }]);
    game.tick();
    assert_eq!(weapon(&game, SHOOTER), (12, 36, false), "no shot during the draw");

    for _ in 0..(DRAW_TIME * TICK_RATE as f32) as u32 + 1 {
        game.tick();
    }
    game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, ..cmd(3, 0) }]);
    game.tick();
    assert_eq!(weapon(&game, SHOOTER).0, 11);

    game.push_inputs(SHOOTER, vec![pb::InputCmd { slot: 1, ..cmd(4, 0) }]);
    game.tick();
    assert_eq!(weapon(&game, SHOOTER), (10, 120, false), "the reload did not complete");
}

#[test]
fn switching_cannot_skip_a_bolt_cycle() {
    let mut game = duel();
    game.select_weapon(SHOOTER, pb::WeaponKind::Sniper);
    game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, ..cmd(1, 0) }]);
    game.tick();
    let e = game.entities[&SHOOTER];
    let before = game.world.get::<Weapon>(e).unwrap().cooldown;
    assert!(before > DRAW_TIME);
    game.push_inputs(SHOOTER, vec![pb::InputCmd { slot: 2, ..cmd(2, 0) }]);
    game.push_inputs(SHOOTER, vec![pb::InputCmd { slot: 1, ..cmd(3, 0) }]);
    game.tick();
    assert!(game.world.get::<Weapon>(e).unwrap().cooldown > DRAW_TIME);
}

#[test]
fn sidearm_stats_match_the_spec() {
    use pb::WeaponKind::*;
    let (rifle, pistol, knife, smg) = (weapons::stats(Rifle), weapons::stats(Pistol), weapons::stats(Knife), weapons::stats(Smg));
    assert_eq!((pistol.magazine, pistol.reserve), (12, 36));
    assert!(pistol.damage_body < rifle.damage_body);
    assert!(pistol.fire_interval > rifle.fire_interval, "low fire rate");
    assert!(pistol.move_speed > rifle.move_speed, "slightly high speed");
    assert!(pistol.spread_still < smg.spread_still, "high accuracy");
    assert!(knife.damage_body > rifle.damage_body, "high damage");
    assert!(knife.move_speed > pistol.move_speed && knife.move_speed < smg.move_speed);
    assert!(knife.melee_range > 0.0);
}

/// Shooter drawn to `slot`, ready to attack, with the victim `dist` away along +x.
fn melee_setup(dist: f32) -> (Match, pb::InputCmd) {
    let mut game = duel();
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(&mut game, SHOOTER, -46.0, 0.0, yaw);
    place(&mut game, VICTIM, -46.0 + dist, 0.0, 0.0);
    game.push_inputs(SHOOTER, vec![pb::InputCmd { slot: 3, yaw, ..cmd(1, 0) }]);
    for _ in 0..(DRAW_TIME * TICK_RATE as f32) as u32 + 2 {
        game.tick();
    }
    let swing = pb::InputCmd { fire: true, yaw, pitch: -0.3, ..cmd(2, tick_now(&game)) };
    (game, swing)
}

#[test]
fn knife_hits_in_reach_for_high_damage_without_ammo() {
    let (mut game, swing) = melee_setup(1.5);
    game.push_inputs(SHOOTER, vec![swing]);
    game.tick();
    let victim = game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap();
    assert_eq!(victim.hp, MAX_HP - weapons::stats(pb::WeaponKind::Knife).damage_body);
    assert_eq!(weapon(&game, SHOOTER).0, 0);
}

#[test]
fn knife_cannot_reach_far_targets() {
    let (mut game, swing) = melee_setup(4.0);
    game.push_inputs(SHOOTER, vec![swing]);
    game.tick();
    assert_eq!(game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap().hp, MAX_HP);
    let melee_shot = game.shared_snapshot().events.iter().any(|e| {
        matches!(&e.event, Some(pb::game_event::Event::Shot(s)) if s.melee && s.hit_player_id == 0)
    });
    assert!(melee_shot, "the swing still happens and is reported as melee");
}

#[test]
fn knife_never_triggers_a_reload_or_empty_state() {
    let (mut game, _) = melee_setup(1.5);
    for _ in 0..10 {
        game.tick();
    }
    assert!(!weapon(&game, SHOOTER).2);
}

#[test]
fn slot_speed_applies_from_the_command_that_switches() {
    let walk = |slot: u32| {
        let mut game = duel();
        let yaw = std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, 40.0, -30.0, yaw);
        game.push_inputs(SHOOTER, vec![pb::InputCmd { forward: true, slot, yaw, ..cmd(1, 0) }]);
        game.tick();
        40.0 - game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap().x
    };
    let expected = weapons::stats(pb::WeaponKind::Knife).move_speed * CMD_DT;
    assert!((walk(3) - expected).abs() < 1e-3);
    assert!(walk(3) > walk(1));
}

#[test]
fn a_level_crosshair_hits_the_body_and_a_head_needs_raised_aim() {
    let damage_at = |pitch: f32| {
        let mut game = duel();
        let shot = line_up(&mut game, pitch);
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        MAX_HP - game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap().hp
    };
    let rifle = weapons::stats(RIFLE);
    assert_eq!(damage_at(0.0), rifle.damage_body);
    // The target stands 4 m away; aim at the middle of the head.
    assert_eq!(damage_at((1.63f32 - EYE_HEIGHT).atan2(4.0)), rifle.damage_head);
}

fn crouch_cmd(seq: u32, tick: u32) -> pb::InputCmd {
    pb::InputCmd { crouch: true, ..cmd(seq, tick) }
}

#[test]
fn crouching_halves_speed_and_persists_between_commands() {
    let mut game = duel();
    let yaw = std::f32::consts::FRAC_PI_2;
    place(&mut game, SHOOTER, 40.0, -30.0, yaw);
    game.push_inputs(SHOOTER, vec![pb::InputCmd { forward: true, yaw, ..crouch_cmd(1, 0) }]);
    game.tick();
    let e = game.entities[&SHOOTER];
    let moved = 40.0 - game.world.get::<Pose>(e).unwrap().x;
    assert!((moved - weapons::stats(RIFLE).move_speed * CROUCH_SPEED_FACTOR * CMD_DT).abs() < 1e-3);
    game.tick();
    assert!(game.world.get::<Pose>(e).unwrap().crouching, "no new command, so the stance holds");
    game.push_inputs(SHOOTER, vec![cmd(2, 0)]);
    game.tick();
    assert!(!game.world.get::<Pose>(e).unwrap().crouching);
}

/// Mean angle between the aim and where a machine gun's shots went.
fn mean_spread(crouch: bool) -> f32 {
    let mut game = duel();
    game.select_weapon(SHOOTER, pb::WeaponKind::MachineGun);
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(&mut game, SHOOTER, -46.0, 0.0, yaw);
    place(&mut game, VICTIM, -46.0, -8.0, 0.0);
    let aim = super::physics::view_dir(yaw, 0.0);
    let (mut total, mut count) = (0.0, 0);
    for seq in 1..=90 {
        let tick = tick_now(&game);
        game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, crouch, yaw, ..cmd(seq, tick) }]);
        game.tick();
        for event in game.shared_snapshot().events {
            if let Some(pb::game_event::Event::Shot(s)) = event.event {
                let d = [s.to_x - s.from_x, s.to_y - s.from_y, s.to_z - s.from_z];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let dot = (d[0] * aim[0] + d[1] * aim[1] + d[2] * aim[2]) / len;
                total += dot.clamp(-1.0, 1.0).acos();
                count += 1;
            }
        }
    }
    assert!(count > 30, "fired {count}");
    total / count as f32
}

#[test]
fn crouching_tightens_real_shots() {
    let (standing, crouched) = (mean_spread(false), mean_spread(true));
    assert!(crouched < standing * 0.7, "standing {standing}, crouched {crouched}");
}

/// Damage a level or raised shot from a standing or crouched rifleman does to a
/// crouched victim 4 m away.
fn shot_at_crouched(shooter_crouched: bool, aim_y: f32) -> i32 {
    let mut game = duel();
    let mut shot = line_up(&mut game, 0.0);
    game.push_inputs(VICTIM, vec![crouch_cmd(1, 0)]);
    game.tick();
    let eye = if shooter_crouched { CROUCH_EYE_HEIGHT } else { EYE_HEIGHT };
    shot.pitch = (aim_y - eye).atan2(4.0);
    shot.crouch = shooter_crouched;
    shot.render_tick = tick_now(&game);
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    MAX_HP - game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap().hp
}

#[test]
fn crouched_hitboxes_drop_with_the_player() {
    let rifle = weapons::stats(RIFLE);
    assert_eq!(shot_at_crouched(false, EYE_HEIGHT), 0, "passes over a crouched player");
    assert_eq!(shot_at_crouched(false, 1.13), rifle.damage_head);
    assert_eq!(shot_at_crouched(false, 0.7), rifle.damage_body);
    assert_eq!(shot_at_crouched(false, 0.2), rifle.damage_legs);
    assert_eq!(shot_at_crouched(true, CROUCH_EYE_HEIGHT), rifle.damage_body, "level from a crouch");
}

#[test]
fn lag_compensation_rewinds_the_stance() {
    let mut game = duel();
    let mut shot = line_up(&mut game, 0.0);
    // History labels the sample just recorded one tick before the current tick.
    let seen_at = tick_now(&game) - 1;
    game.push_inputs(VICTIM, vec![crouch_cmd(1, 0)]);
    game.tick();
    // The shooter was still rendering the victim standing when firing.
    shot.render_tick = seen_at;
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    let victim = game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap();
    assert_eq!(victim.hp, MAX_HP - weapons::stats(RIFLE).damage_body);
}

#[test]
fn bots_can_kill_a_crouching_player() {
    // Bots rarely happen upon a player idling at spawn, so one match proves little.
    let mut deaths = 0;
    for seed in 1..=12 {
        let mut game = Match::new(&[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)], YARD, true, seed);
        for seq in 1..=(TICK_RATE * 180) {
            game.push_inputs(SHOOTER, vec![crouch_cmd(seq, tick_now(&game))]);
            game.tick();
        }
        deaths += game.world.get::<Stats>(game.entities[&SHOOTER]).unwrap().deaths;
    }
    assert!(deaths > 0, "bots never hit a crouched target");
}

/// Angle between the aim and each shot of a rifle magazine held on one spot.
fn sustained_fire_angles() -> Vec<f32> {
    let mut game = duel();
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(&mut game, SHOOTER, -46.0, 0.0, yaw);
    place(&mut game, VICTIM, -46.0, -8.0, 0.0);
    let aim = super::physics::view_dir(yaw, 0.0);
    let mut angles = Vec::new();
    for seq in 1..=88 {
        let tick = tick_now(&game);
        game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, yaw, ..cmd(seq, tick) }]);
        game.tick();
        for event in game.shared_snapshot().events {
            if let Some(pb::game_event::Event::Shot(s)) = event.event {
                let d = [s.to_x - s.from_x, s.to_y - s.from_y, s.to_z - s.from_z];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let dot = (d[0] * aim[0] + d[1] * aim[1] + d[2] * aim[2]) / len;
                angles.push(dot.clamp(-1.0, 1.0).acos());
            }
        }
    }
    angles
}

#[test]
fn sustained_fire_really_spreads_shots() {
    let angles = sustained_fire_angles();
    assert!(angles.len() >= 25, "fired {}", angles.len());
    let mean = |a: &[f32]| a.iter().sum::<f32>() / a.len() as f32;
    let (early, late) = (mean(&angles[..3]), mean(&angles[12..]));
    assert!(late > early * 3.0, "early {early}, late {late}");
    assert!(late > weapons::stats(RIFLE).bloom_max * 0.4, "late {late}");
}

#[test]
fn bloom_recovers_once_the_trigger_is_released() {
    let mut game = duel();
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(&mut game, SHOOTER, -46.0, 0.0, yaw);
    place(&mut game, VICTIM, -46.0, -8.0, 0.0);
    for seq in 1..=30 {
        let tick = tick_now(&game);
        game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, yaw, ..cmd(seq, tick) }]);
        game.tick();
    }
    let e = game.entities[&SHOOTER];
    assert!(game.world.get::<Weapon>(e).unwrap().bloom > 0.02);
    for _ in 0..TICK_RATE {
        game.tick();
    }
    assert_eq!(game.world.get::<Weapon>(e).unwrap().bloom, 0.0);
}

// ---------------------------------------------------------------- augment mode

fn augment_duel() -> Match {
    Match::with_mode(
        &[
            (SHOOTER, "a".into(), pb::Team::Red, RIFLE),
            (VICTIM, "b".into(), pb::Team::Blue, RIFLE),
        ],
        YARD,
        pb::GameMode::Augment,
        [0, 0],
        42,
    )
}

/// A duel past its waiting time with the shooter holding `augment`.
fn with_augment(augment: Aug) -> Match {
    let mut game = augment_duel();
    game.apply_augment(SHOOTER, augment);
    game.apply_augment(VICTIM, Aug::QuickReload);
    game.world.resource_mut::<MatchState>().freeze_left = 0.0;
    game
}

fn draw_sidearm(game: &mut Match, id: u32) {
    let e = game.entities[&id];
    let mut weapon = game.world.get_mut::<Weapon>(e).unwrap();
    weapon.switch_to(2);
    weapon.cooldown = 0.0;
}

fn hp(game: &Match, id: u32) -> i32 {
    game.world.get::<Vitals>(game.entities[&id]).unwrap().hp
}

fn held(game: &Match, id: u32) -> Kind {
    game.world.get::<Weapon>(game.entities[&id]).unwrap().kind
}

/// Lines the shooter up and fires once at chest height, returning the tick's events.
fn shoot_once(game: &mut Match, seq: u32) -> Vec<pb::GameEvent> {
    let mut shot = line_up(game, -0.05);
    shot.seq = seq;
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    game.shared_snapshot().events
}

fn shot_events(events: &[pb::GameEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e.event, Some(pb::game_event::Event::Shot(_))))
        .count()
}

/// The victim is one hit from death and the shooter's next shot is ready.
fn kill_victim(game: &mut Match, seq: u32) {
    let (e, v) = (game.entities[&SHOOTER], game.entities[&VICTIM]);
    {
        let mut vitals = game.world.get_mut::<Vitals>(v).unwrap();
        vitals.alive = true;
        vitals.hp = 1;
        vitals.respawn_left = 0.0;
    }
    let mut w = game.world.get_mut::<Weapon>(e).unwrap();
    w.cooldown = 0.0;
    w.reload_left = 0.0;
    w.magazine = w.magazine.max(5);
    shoot_once(game, seq);
    assert!(!game.world.get::<Vitals>(v).unwrap().alive, "shot {seq} should have killed");
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
        let card = game.world.get::<AugmentOffer>(game.entities[&id]).unwrap().offered[0];
        game.pick_augment(id, card);
    }
    let start = game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap().x;
    let total = (augments::AUGMENT_WAIT * TICK_RATE as f32).round() as u32;
    for seq in 1..total {
        let yaw = 0.3;
        game.push_inputs(SHOOTER, vec![pb::InputCmd { forward: true, fire: true, yaw, ..cmd(seq, 0) }]);
        game.tick();
    }
    let e = game.entities[&SHOOTER];
    assert_eq!(game.world.get::<Pose>(e).unwrap().x, start, "no moving while waiting");
    assert_eq!(game.world.get::<Pose>(e).unwrap().yaw, 0.3, "aiming still works");
    assert_eq!(weapon(&game, SHOOTER).0, 30, "no firing while waiting");
    let mut personal = game.shared_snapshot();
    game.fill_personal(SHOOTER, &mut personal);
    assert_eq!(personal.last_input_seq, total - 1, "inputs are still acknowledged");
    assert!(personal.freeze_left > 0.0);
    // Ending the choice takes a tick, and rounding can leave the wait one more.
    game.push_inputs(SHOOTER, vec![]);
    game.tick();
    game.tick();
    game.tick();
    game.push_inputs(SHOOTER, vec![pb::InputCmd { forward: true, yaw: 1.5, ..cmd(total, 0) }]);
    game.tick();
    assert_ne!(game.world.get::<Pose>(e).unwrap().x, start, "moves once the wait is over");
}

#[test]
fn grades_roll_at_40_35_25() {
    let mut rng = GameRng(12345);
    let mut counts = [0u32; 3];
    let total = 100_000;
    for _ in 0..total {
        counts[augments::roll_grade(&mut rng) as usize - 1] += 1;
    }
    for (count, expected) in counts.iter().zip([0.4, 0.35, 0.25]) {
        let share = *count as f64 / total as f64;
        assert!((share - expected).abs() < 0.01, "{counts:?}");
    }
}

#[test]
fn each_card_can_be_swapped_once_for_one_not_seen_yet() {
    for seed in 1..200 {
        let mut game = Match::with_mode(
            &[(SHOOTER, "a".into(), pb::Team::Red, RIFLE), (VICTIM, "b".into(), pb::Team::Blue, RIFLE)],
            YARD,
            pb::GameMode::Augment,
            [0, 0],
            seed,
        );
        let e = game.entities[&SHOOTER];
        let mut seen = game.world.get::<AugmentOffer>(e).unwrap().offered.clone();
        assert_eq!(seen.len(), 3);
        assert!(seen[0] != seen[1] && seen[1] != seen[2] && seen[0] != seen[2], "{seen:?}");

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
            assert_eq!(game.world.get::<AugmentOffer>(e).unwrap().offered, after, "once per card");
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
    assert_eq!(game.world.get::<Perks>(e).unwrap().augment, Aug::Unspecified, "still choosing");
    assert_eq!(game.world.resource::<MatchState>().freeze_left, augments::AUGMENT_WAIT, "the wait hasn't begun");
    let card = game.world.get::<AugmentOffer>(e).unwrap().offered[1];
    game.pick_augment(SHOOTER, card);
    game.tick();
    assert_eq!(game.world.get::<Perks>(e).unwrap().augment, card, "a late choice stands");
    assert_ne!(game.world.get::<Perks>(game.entities[&VICTIM]).unwrap().augment, Aug::Unspecified, "time chose");
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
        let card = game.world.get::<AugmentOffer>(game.entities[&id]).unwrap().offered[0];
        game.pick_augment(id, card);
    }
    game.tick();
    assert_eq!(game.world.resource::<MatchState>().pick_left, 0.0);
    game.tick();
    assert!(game.world.resource::<MatchState>().freeze_left < augments::AUGMENT_WAIT);
}

#[test]
fn double_time_draws_two_that_dont_clash_at_the_usual_odds() {
    let slot_one = [Aug::Missile, Aug::Akimbo, Aug::Possession];
    let slot_two = [Aug::Shotgun, Aug::Magnum, Aug::PistolUpgrade, Aug::Shield, Aug::Taser];
    let mut rng = GameRng(99);
    let mut prism = 0;
    let total = 20_000;
    for _ in 0..total {
        let active = augments::roll_double(&mut rng);
        assert_eq!(active.len(), 2);
        assert_ne!(active[0], active[1]);
        assert!(!active.contains(&Aug::DoubleTime));
        for group in [&slot_one[..], &slot_two[..]] {
            assert!(active.iter().filter(|a| group.contains(a)).count() <= 1, "{active:?}");
        }
        if [Aug::Wallhack, Aug::NoRecoil, Aug::Borrow, Aug::Missile, Aug::Akimbo].contains(&active[0]) {
            prism += 1;
        }
    }
    let share = prism as f64 / total as f64;
    assert!((share - 0.25).abs() < 0.02, "first draw prism share {share}");
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

/// Every shot the shooter fires over a second of held trigger, as aim errors.
fn aim_errors(game: &mut Match) -> Vec<f32> {
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(game, SHOOTER, -46.0, 0.0, yaw);
    place(game, VICTIM, -46.0, -8.0, 0.0);
    let mut errors = Vec::new();
    for seq in 1..=60 {
        let tick = tick_now(game);
        let e = game.entities[&SHOOTER];
        game.world.get_mut::<Weapon>(e).unwrap().cooldown = 0.0;
        game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, forward: seq % 2 == 0, yaw, ..cmd(seq, tick) }]);
        game.tick();
        for event in game.shared_snapshot().events {
            if let Some(pb::game_event::Event::Shot(s)) = event.event {
                let (dx, dz) = (s.to_x - s.from_x, s.to_z - s.from_z);
                let pitch = (s.to_y - s.from_y).atan2((dx * dx + dz * dz).sqrt());
                errors.push(pitch.abs());
            }
        }
    }
    errors
}

#[test]
fn magnum_and_no_recoil_have_no_spread_even_on_the_move() {
    let mut magnum = with_augment(Aug::Magnum);
    draw_sidearm(&mut magnum, SHOOTER);
    let errors = aim_errors(&mut magnum);
    assert!(!errors.is_empty());
    assert!(errors.iter().all(|e| *e < 1e-4), "{errors:?}");

    let mut steady = with_augment(Aug::NoRecoil);
    let errors = aim_errors(&mut steady);
    assert!(!errors.is_empty());
    assert!(errors.iter().all(|e| *e < 1e-4), "{errors:?}");

    let mut rifle = with_augment(Aug::SoulDrain);
    assert!(aim_errors(&mut rifle).iter().any(|e| *e > 1e-3), "a rifle still spreads");
}

#[test]
fn augment_mode_hits_harder_with_the_sniper_and_smg_only() {
    let hit = |kind, mode| weapons::damage(kind, mode, pb::HitZone::Body);
    let (normal, augment) = (pb::GameMode::Normal, pb::GameMode::Augment);
    assert_eq!(hit(Kind::Sniper, normal), 100);
    assert_eq!(hit(Kind::Sniper, augment), 150, "still one body shot at 150 HP");
    assert_eq!(hit(Kind::Smg, normal), 24);
    assert_eq!(hit(Kind::Smg, augment), 28);
    assert_eq!(hit(Kind::AkimboSmg, augment), 28);
    assert_eq!(hit(RIFLE, augment), hit(RIFLE, normal));
    assert!(hit(Kind::Magnum, augment) > 100 && hit(Kind::Magnum, augment) < 150, "a body survives a clean magnum shot");
    assert!(weapons::damage(Kind::Magnum, augment, pb::HitZone::Head) > 200);
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
    assert_eq!(game.world.get::<Perks>(e).unwrap().augment, Aug::Unspecified);

    game.pick_augment(SHOOTER, offered[1]);
    assert_eq!(game.world.get::<Perks>(e).unwrap().augment, offered[1]);

    for _ in 0..((augments::AUGMENT_PICK + augments::AUGMENT_WAIT) * TICK_RATE as f32) as u32 + 2 {
        game.tick();
    }
    let v = game.entities[&VICTIM];
    let victim_pick = game.world.get::<Perks>(v).unwrap().augment;
    assert_ne!(victim_pick, Aug::Unspecified, "the timer chose for the idle player");
    assert_eq!(game.world.get::<Perks>(e).unwrap().augment, offered[1], "a choice stands");
}

#[test]
fn bots_choose_at_random() {
    let mut chosen = std::collections::HashSet::new();
    for seed in 1..40 {
        // The human on red, and each team filled to five with bots.
        let mut game = Match::with_mode(&[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)], YARD, pb::GameMode::Augment, [4, 5], seed);
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
fn infinite_ammo_reloads_without_spending_reserve() {
    let mut game = with_augment(Aug::InfiniteAmmo);
    let e = game.entities[&SHOOTER];
    {
        let mut w = game.world.get_mut::<Weapon>(e).unwrap();
        w.magazine = 0;
        w.reserve = 0;
    }
    for _ in 0..(weapons::stats(RIFLE).reload_time * TICK_RATE as f32) as u32 + 3 {
        game.tick();
    }
    assert_eq!(weapon(&game, SHOOTER), (30, 0, false));
}

#[test]
fn soul_drain_heals_50_up_to_the_cap() {
    let mut game = with_augment(Aug::SoulDrain);
    let e = game.entities[&SHOOTER];
    game.world.get_mut::<Vitals>(e).unwrap().hp = 100;
    kill_victim(&mut game, 1);
    assert_eq!(hp(&game, SHOOTER), 150);
    game.world.get_mut::<Vitals>(e).unwrap().hp = 60;
    kill_victim(&mut game, 3);
    assert_eq!(hp(&game, SHOOTER), 110);
    game.world.get_mut::<Vitals>(e).unwrap().hp = 145;
    kill_victim(&mut game, 2);
    assert_eq!(hp(&game, SHOOTER), 150);
}

#[test]
fn upgraded_pistol_bursts_three_rounds_per_trigger_and_fires_faster() {
    let shots_over = |augment: Aug, ticks: u32, hold: bool| {
        let mut game = with_augment(augment);
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -46.0, 0.0, yaw);
        place(&mut game, VICTIM, -46.0, -8.0, 0.0);
        let e = game.entities[&SHOOTER];
        {
            let mut w = game.world.get_mut::<Weapon>(e).unwrap();
            w.switch_to(2);
            w.cooldown = 0.0;
            w.reserve = 200;
            w.magazine = 12;
        }
        let mut fired = 0;
        for seq in 1..=ticks {
            let trigger = hold || seq == 1;
            game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: trigger, yaw, ..cmd(seq, tick_now(&game)) }]);
            game.tick();
            fired += shot_events(&game.shared_snapshot().events);
        }
        fired
    };
    assert_eq!(shots_over(Aug::PistolUpgrade, 15, false), 3, "one pull, one burst");
    assert_eq!(shots_over(Aug::InfiniteAmmo, 15, false), 1, "an ordinary pistol fires once");
    let (fast, slow) = (shots_over(Aug::PistolUpgrade, 30, true), shots_over(Aug::InfiniteAmmo, 30, true));
    assert!(fast >= slow * 2, "upgraded {fast}, ordinary {slow}");
}

fn reload_seconds(augment: Aug) -> f32 {
    let mut game = with_augment(augment);
    let e = game.entities[&SHOOTER];
    game.world.get_mut::<Weapon>(e).unwrap().magazine = 10;
    game.push_inputs(SHOOTER, vec![pb::InputCmd { reload: true, ..cmd(1, 0) }]);
    let mut ticks = 0;
    loop {
        game.tick();
        if weapon(&game, SHOOTER).0 == 30 {
            return ticks as f32 * TICK_DT;
        }
        ticks += 1;
        assert!(ticks < 600);
    }
}

#[test]
fn reload_augments_change_the_time_it_takes() {
    let base = reload_seconds(Aug::SoulDrain);
    assert!((base - 2.2).abs() < 0.1, "{base}");
    assert!((reload_seconds(Aug::QuickReload) - base / 2.0).abs() < 0.1);
    assert!((reload_seconds(Aug::NoRecoil) - base * 2.0).abs() < 0.1);
    assert_eq!(reload_seconds(Aug::Borrow), 0.0, "no reload motion at all");
}

#[test]
fn borrow_still_spends_reserve() {
    let mut game = with_augment(Aug::Borrow);
    let e = game.entities[&SHOOTER];
    game.world.get_mut::<Weapon>(e).unwrap().magazine = 10;
    game.push_inputs(SHOOTER, vec![pb::InputCmd { reload: true, ..cmd(1, 0) }]);
    game.tick();
    assert_eq!(weapon(&game, SHOOTER), (30, 100, false));
}

#[test]
fn shotgun_fires_six_pellets_for_one_round_and_hits_hard_up_close() {
    let mut game = with_augment(Aug::Shotgun);
    assert_eq!(held(&game, SHOOTER), RIFLE, "it's the sidearm now");
    draw_sidearm(&mut game, SHOOTER);
    assert_eq!(held(&game, SHOOTER), Kind::Shotgun);
    assert_eq!(weapon(&game, SHOOTER), (6, 36, false));
    let events = shoot_once(&mut game, 1);
    assert_eq!(shot_events(&events), 6);
    assert_eq!(weapon(&game, SHOOTER).0, 5);
    let dealt = 150 - hp(&game, VICTIM);
    assert!(dealt >= weapons::stats(RIFLE).damage_body * 2, "dealt {dealt} at 4 m");
}

#[test]
fn shotgun_loads_a_shell_at_a_time_and_can_fire_mid_reload() {
    let mut game = with_augment(Aug::Shotgun);
    let e = game.entities[&SHOOTER];
    draw_sidearm(&mut game, SHOOTER);
    game.world.get_mut::<Weapon>(e).unwrap().magazine = 2;
    game.push_inputs(SHOOTER, vec![pb::InputCmd { reload: true, ..cmd(1, 0) }]);
    let shell_ticks = (weapons::stats(Kind::Shotgun).reload_time * TICK_RATE as f32) as u32 + 2;
    for _ in 0..shell_ticks {
        game.tick();
    }
    let (magazine, reserve, reloading) = weapon(&game, SHOOTER);
    assert_eq!((magazine, reserve), (3, 35), "one shell in");
    assert!(reloading, "and on to the next");

    let yaw = -std::f32::consts::FRAC_PI_2;
    game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, yaw, ..cmd(2, tick_now(&game)) }]);
    game.tick();
    let (magazine, _, reloading) = weapon(&game, SHOOTER);
    assert_eq!(magazine, 2, "fired during the reload");
    assert!(!reloading, "firing stops the reload");
}

#[test]
fn rest_heals_only_when_crouched_and_out_of_combat() {
    let mut game = with_augment(Aug::Rest);
    let e = game.entities[&SHOOTER];
    game.world.get_mut::<Vitals>(e).unwrap().hp = 50;
    game.world.get_mut::<Vitals>(e).unwrap().protect_left = 0.0;
    let crouch = |game: &mut Match, seq: u32, down: bool| {
        game.push_inputs(SHOOTER, vec![pb::InputCmd { crouch: down, ..cmd(seq, 0) }]);
        game.tick();
    };
    // Standing never heals.
    for seq in 1..=(TICK_RATE * 5) {
        crouch(&mut game, seq, false);
    }
    assert_eq!(hp(&game, SHOOTER), 50);

    // Fresh from a fight: four seconds must pass first.
    game.world.get_mut::<Perks>(e).unwrap().idle = 0.0;
    for seq in 1..=(TICK_RATE * 3) {
        crouch(&mut game, 1000 + seq, true);
    }
    assert_eq!(hp(&game, SHOOTER), 50, "still in combat");
    for seq in 1..=(TICK_RATE * 3) {
        crouch(&mut game, 2000 + seq, true);
    }
    let healed = hp(&game, SHOOTER);
    assert!(healed > 50 && healed < 150, "gradual, got {healed}");
    for seq in 1..=(TICK_RATE * 10) {
        crouch(&mut game, 3000 + seq, true);
    }
    assert_eq!(hp(&game, SHOOTER), 150, "stops at the maximum");
}

/// A shield bearer as the victim, facing `victim_yaw`, 4 m in front of the shooter.
fn shielded_target(victim_yaw: f32) -> Match {
    let mut game = augment_duel();
    game.apply_augment(SHOOTER, Aug::SoulDrain);
    game.apply_augment(VICTIM, Aug::Shield);
    draw_sidearm(&mut game, VICTIM);
    game.world.resource_mut::<MatchState>().freeze_left = 0.0;
    let shot = line_up(&mut game, -0.05);
    let v = game.entities[&VICTIM];
    game.world.get_mut::<Pose>(v).unwrap().yaw = victim_yaw;
    game.world.get_mut::<History>(v).unwrap().samples.clear();
    let mut shot = shot;
    shot.render_tick = tick_now(&game);
    game.push_inputs(SHOOTER, vec![shot]);
    game
}

#[test]
fn shield_soaks_frontal_fire_but_not_shots_from_behind() {
    // The shooter is on the -X side, firing along +X.
    let facing_shooter = std::f32::consts::FRAC_PI_2;
    let mut game = shielded_target(facing_shooter);
    game.tick();
    let v = game.entities[&VICTIM];
    let body = weapons::stats(RIFLE).damage_body;
    assert_eq!(game.world.get::<Perks>(v).unwrap().shield_hp, augments::SHIELD_HP - body);
    assert_eq!(hp(&game, VICTIM), 150);

    let mut game = shielded_target(-facing_shooter);
    game.tick();
    let v = game.entities[&VICTIM];
    assert_eq!(game.world.get::<Perks>(v).unwrap().shield_hp, augments::SHIELD_HP, "behind the shield");
    assert_eq!(hp(&game, VICTIM), 150 - body);
}

#[test]
fn shield_only_works_with_the_pistol_in_hand() {
    // Same frontal shot as above, but the bearer holds the primary.
    let facing_shooter = std::f32::consts::FRAC_PI_2;
    let mut game = shielded_target(facing_shooter);
    let v = game.entities[&VICTIM];
    game.world.get_mut::<Weapon>(v).unwrap().switch_to(1);
    assert_eq!(held(&game, VICTIM), RIFLE, "every weapon is usable");
    game.tick();
    assert_eq!(game.world.get::<Perks>(v).unwrap().shield_hp, augments::SHIELD_HP);
    assert_eq!(hp(&game, VICTIM), 150 - weapons::stats(RIFLE).damage_body);
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
    assert_eq!(game.world.get::<Perks>(e).unwrap().shield_hp, 10, "no repair out of combat");

    {
        let mut vitals = game.world.get_mut::<Vitals>(e).unwrap();
        vitals.alive = false;
        vitals.hp = 0;
        vitals.respawn_left = 0.01;
    }
    game.tick();
    assert!(game.world.get::<Vitals>(e).unwrap().alive);
    assert_eq!(game.world.get::<Perks>(e).unwrap().shield_hp, augments::SHIELD_HP);
}

#[test]
fn upgraded_pistol_has_more_ammo_and_damage() {
    let mut game = with_augment(Aug::PistolUpgrade);
    draw_sidearm(&mut game, SHOOTER);
    assert_eq!(held(&game, SHOOTER), Kind::PistolPlus);
    assert_eq!(weapon(&game, SHOOTER), (12, 120, false));
    assert!(weapons::stats(Kind::PistolPlus).damage_body > weapons::stats(Kind::Pistol).damage_body);
}

#[test]
fn magnum_is_a_hard_hitting_sidearm() {
    let mut game = with_augment(Aug::Magnum);
    draw_sidearm(&mut game, SHOOTER);
    assert_eq!(held(&game, SHOOTER), Kind::Magnum);
    assert_eq!(weapon(&game, SHOOTER), (6, 30, false));
    shoot_once(&mut game, 1);
    assert_eq!(150 - hp(&game, VICTIM), weapons::stats(Kind::Magnum).damage_body);
    let magnum = weapons::stats(Kind::Magnum);
    assert!(magnum.move_speed > weapons::stats(RIFLE).move_speed);
    assert!(magnum.fire_interval > weapons::stats(Kind::Pistol).fire_interval);
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
fn akimbo_doubles_magazine_and_fire_rate_only_for_the_smg() {
    let mut rifle = with_augment(Aug::Akimbo);
    assert_eq!(held(&rifle, SHOOTER), RIFLE, "needs an SMG");
    rifle.select_weapon(SHOOTER, Kind::Smg);
    assert_eq!(held(&rifle, SHOOTER), Kind::AkimboSmg);
    assert_eq!(weapon(&rifle, SHOOTER).0, weapons::stats(Kind::Smg).magazine * 2);

    let shots = |augment: Aug| {
        let mut game = with_augment(augment);
        game.select_weapon(SHOOTER, Kind::Smg);
        let e = game.entities[&SHOOTER];
        game.world.get_mut::<Weapon>(e).unwrap().cooldown = 0.0;
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -46.0, 0.0, yaw);
        place(&mut game, VICTIM, -46.0, -8.0, 0.0);
        let mut fired = 0;
        for seq in 1..=30 {
            game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, yaw, ..cmd(seq, tick_now(&game)) }]);
            game.tick();
            fired += shot_events(&game.shared_snapshot().events);
        }
        fired
    };
    let (double, single) = (shots(Aug::Akimbo), shots(Aug::SoulDrain));
    assert!(double >= single * 2 - 1, "akimbo {double}, single {single}");
}

#[test]
fn big_head_grows_the_victims_head_additively_for_every_contributor() {
    let mut game = with_augment(Aug::BigHead);
    let v = game.entities[&VICTIM];
    let scale = |game: &Match| game.world.get::<Perks>(v).unwrap().head_scale;
    assert_eq!(scale(&game), 1.0);
    kill_victim(&mut game, 1);
    assert!((scale(&game) - 1.1).abs() < 1e-4);
    kill_victim(&mut game, 2);
    assert!((scale(&game) - 1.2).abs() < 1e-4, "adds up over deaths");

    // An assist counts: the killer has no such augment, a helper who hurt the victim does.
    let mut game = Match::with_mode(
        &[
            (SHOOTER, "a".into(), pb::Team::Red, RIFLE),
            (VICTIM, "b".into(), pb::Team::Blue, RIFLE),
            (77, "c".into(), pb::Team::Red, RIFLE),
        ],
        YARD,
        pb::GameMode::Augment,
        [0, 0],
        42,
    );
    game.apply_augment(SHOOTER, Aug::SoulDrain);
    game.apply_augment(VICTIM, Aug::QuickReload);
    game.apply_augment(77, Aug::BigHead);
    game.world.resource_mut::<MatchState>().freeze_left = 0.0;
    let v = game.entities[&VICTIM];
    kill_victim(&mut game, 1);
    assert_eq!(game.world.get::<Perks>(v).unwrap().head_scale, 1.0, "the holder didn't help");
    let helper = game.entities[&77];
    game.world.get_mut::<Perks>(v).unwrap().damaged_by.push(helper);
    kill_victim(&mut game, 2);
    assert!((game.world.get::<Perks>(v).unwrap().head_scale - 1.1).abs() < 1e-4);
}

#[test]
fn a_bigger_head_is_easier_to_hit() {
    let damage = |head_scale: f32| {
        let mut game = with_augment(Aug::SoulDrain);
        let v = game.entities[&VICTIM];
        game.world.get_mut::<Perks>(v).unwrap().head_scale = head_scale;
        let shot = line_up(&mut game, (1.9f32 - EYE_HEIGHT).atan2(4.0));
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        150 - hp(&game, VICTIM)
    };
    assert_eq!(damage(1.0), 0);
    assert_eq!(damage(2.0), weapons::stats(RIFLE).damage_head);
}

#[test]
fn a_small_head_is_harder_to_hit() {
    // At 4 m, aimed at 1.70: inside a normal head, above a small one.
    let damage = |augment: Aug| {
        let mut game = augment_duel();
        game.apply_augment(SHOOTER, Aug::SoulDrain);
        game.apply_augment(VICTIM, augment);
        game.world.resource_mut::<MatchState>().freeze_left = 0.0;
        let shot = line_up(&mut game, (1.7f32 - EYE_HEIGHT).atan2(4.0));
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        150 - hp(&game, VICTIM)
    };
    assert_eq!(damage(Aug::Rest), weapons::stats(RIFLE).damage_head);
    assert_eq!(damage(Aug::SmallHead), 0);
}

#[test]
fn possession_always_resets_hp_to_150_and_takes_the_victims_primary() {
    let mut game = with_augment(Aug::Possession);
    let (e, v) = (game.entities[&SHOOTER], game.entities[&VICTIM]);
    game.world.get_mut::<Weapon>(v).unwrap().set_primary(Kind::Sniper);
    game.world.get_mut::<Weapon>(v).unwrap().cooldown = 0.0;
    game.world.get_mut::<Weapon>(v).unwrap().switch_to(2);

    game.world.get_mut::<Vitals>(e).unwrap().hp = 20;
    kill_victim(&mut game, 1);
    assert_eq!(hp(&game, SHOOTER), 150);
    assert_eq!(held(&game, SHOOTER), Kind::Sniper);
    assert_eq!(weapon(&game, SHOOTER), (5, 20, false));
    assert_eq!(game.world.get::<Loadout>(e).unwrap().0, Kind::Sniper);

    for i in 0..10 {
        game.world.get_mut::<Vitals>(e).unwrap().hp = 20;
        game.world.get_mut::<Weapon>(v).unwrap().set_primary(Kind::AkimboSmg);
        kill_victim(&mut game, 2 + i);
        assert_eq!(hp(&game, SHOOTER), 150, "no decay");
    }
    assert_eq!(held(&game, SHOOTER), Kind::Smg, "a taken akimbo is a plain SMG");
}

#[test]
fn no_recoil_keeps_sustained_fire_tight() {
    let mut game = with_augment(Aug::NoRecoil);
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(&mut game, SHOOTER, -46.0, 0.0, yaw);
    place(&mut game, VICTIM, -46.0, -8.0, 0.0);
    for seq in 1..=45 {
        let tick = tick_now(&game);
        game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, yaw, ..cmd(seq, tick) }]);
        game.tick();
    }
    let e = game.entities[&SHOOTER];
    assert!(weapon(&game, SHOOTER).0 < 25, "it did fire");
    assert_eq!(game.world.get::<Weapon>(e).unwrap().bloom, 0.0);
}

#[test]
fn missiles_fly_then_blast_without_hurting_the_shooter() {
    let mut game = with_augment(Aug::Missile);
    assert_eq!(held(&game, SHOOTER), Kind::Missile);
    assert_eq!(weapon(&game, SHOOTER), (9, 36, false));

    let events = shoot_once(&mut game, 1);
    assert_eq!(weapon(&game, SHOOTER).0, 8);
    assert!(events.iter().any(|e| matches!(&e.event, Some(pb::game_event::Event::Shot(s)) if s.explosive)));
    assert_eq!(hp(&game, VICTIM), 150, "still in the air");
    assert_eq!(game.shared_snapshot().missiles.len(), 1);

    let mut exploded = false;
    for _ in 0..4 {
        game.tick();
        exploded |= game
            .shared_snapshot()
            .events
            .iter()
            .any(|e| matches!(e.event, Some(pb::game_event::Event::Explosion(_))));
    }
    assert!(exploded);
    let dealt = 150 - hp(&game, VICTIM);
    assert!(dealt > 60 && dealt <= 150, "dealt {dealt}");
    assert_eq!(hp(&game, SHOOTER), 150, "no damage to yourself");
    assert!(game.shared_snapshot().missiles.is_empty());
}

#[test]
fn a_missile_has_a_wide_hit_range() {
    // Aimed 0.9 m wide of the body at 4 m: a bullet misses, a missile doesn't.
    let fire_wide = |augment: Aug| {
        let mut game = with_augment(augment);
        let mut shot = line_up(&mut game, -0.05);
        shot.yaw += (0.9f32 / 4.0).atan();
        game.push_inputs(SHOOTER, vec![shot]);
        for _ in 0..6 {
            game.tick();
        }
        150 - hp(&game, VICTIM)
    };
    assert!(fire_wide(Aug::Missile) > 0);
    assert_eq!(fire_wide(Aug::SoulDrain), 0);
}

#[test]
fn a_missile_blast_catches_people_beside_it_but_not_behind_walls() {
    let yaw = std::f32::consts::PI;
    let dealt = |z: f32| {
        let mut game = with_augment(Aug::Missile);
        place(&mut game, SHOOTER, -31.0, 4.0, yaw);
        place(&mut game, VICTIM, -29.0, z, 0.0);
        game.tick();
        game.push_inputs(SHOOTER, vec![pb::InputCmd { fire: true, yaw, ..cmd(1, tick_now(&game)) }]);
        for _ in 0..30 {
            game.tick();
        }
        150 - hp(&game, VICTIM)
    };
    assert!(dealt(5.0) > 0);
    assert_eq!(dealt(8.5), 0, "behind the wall");
}

#[test]
fn taser_paralyses_for_5_seconds_without_hurting() {
    let mut game = with_augment(Aug::Taser);
    assert_eq!(held(&game, SHOOTER), RIFLE);
    draw_sidearm(&mut game, SHOOTER);
    assert_eq!(held(&game, SHOOTER), Kind::Taser);
    assert_eq!(weapon(&game, SHOOTER), (1, 6, false));
    shoot_once(&mut game, 1);
    let v = game.entities[&VICTIM];
    assert_eq!(hp(&game, VICTIM), 150, "non-lethal");
    assert!((game.world.get::<Vitals>(v).unwrap().stun_left - 5.0).abs() < 0.1);

    let start = game.world.get::<Pose>(v).unwrap().x;
    let magazine = weapon(&game, VICTIM).0;
    let mut seq = 1;
    for _ in 0..(4 * TICK_RATE) {
        seq += 1;
        game.push_inputs(VICTIM, vec![pb::InputCmd { forward: true, fire: true, slot: 2, yaw: 1.0, ..cmd(seq, tick_now(&game)) }]);
        game.tick();
    }
    let pose = *game.world.get::<Pose>(v).unwrap();
    assert_eq!(pose.x, start, "can't move");
    assert_eq!(pose.yaw, 1.0, "can look around");
    assert_eq!(weapon(&game, VICTIM).0, magazine, "can't fire");
    assert_eq!(held(&game, VICTIM), RIFLE, "can't switch");

    for _ in 0..(TICK_RATE + 5) {
        seq += 1;
        game.push_inputs(VICTIM, vec![pb::InputCmd { forward: true, yaw: 1.0, ..cmd(seq, tick_now(&game)) }]);
        game.tick();
    }
    assert_eq!(game.world.get::<Vitals>(v).unwrap().stun_left, 0.0);
    assert_ne!(game.world.get::<Pose>(v).unwrap().x, start, "moves again");
}

#[test]
fn a_shield_stops_a_taser() {
    let mut game = shielded_target(std::f32::consts::FRAC_PI_2);
    let e = game.entities[&SHOOTER];
    let mut weapon = Weapon::equipped(RIFLE, &[Aug::Taser]);
    weapon.switch_to(2);
    weapon.cooldown = 0.0;
    *game.world.get_mut::<Weapon>(e).unwrap() = weapon;
    game.tick();
    let v = game.entities[&VICTIM];
    assert_eq!(game.world.get::<Vitals>(v).unwrap().stun_left, 0.0);
}

#[test]
fn bots_fire_their_taser_once_then_use_their_gun() {
    assert_eq!(systems::issue_weapons(RIFLE, &[Aug::Taser], true).kind, Kind::Taser);
    let mut game = Match::new(&[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)], YARD, true, 3);
    let mut query = game.world.query_filtered::<Entity, With<BotBrain>>();
    let bot = query.iter(&game.world).next().unwrap();
    let mut weapon = systems::issue_weapons(RIFLE, &[Aug::Taser], true);
    weapon.magazine = 0;
    *game.world.get_mut::<Weapon>(bot).unwrap() = weapon;
    game.tick();
    assert_eq!(game.world.get::<Weapon>(bot).unwrap().kind, RIFLE);
}

#[test]
fn full_augment_matches_run_clean() {
    for seed in 1..=8 {
        // The human on red, and each team filled to five with bots.
        let mut game = Match::with_mode(&[(SHOOTER, "a".into(), pb::Team::Red, RIFLE)], YARD, pb::GameMode::Augment, [4, 5], seed);
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
