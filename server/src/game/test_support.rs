//! Shared helpers for the game scenario tests: matches set up for a test, and ways to steer and read them.

pub use super::components::*;
pub use super::map::build_map;
pub use super::nav::NavGrid;
pub use super::physics::{blocks, step_move, step_vertical};
pub use super::resources::*;
pub use super::systems::grenade::GRENADE_RADIUS;
pub use super::*;
pub use crate::pb;
pub use pb::Augment as Aug;
pub use pb::WeaponKind as Kind;

pub const SHOOTER: u32 = 1;

pub const VICTIM: u32 = 2;

pub const RIFLE: pb::WeaponKind = pb::WeaponKind::Rifle;

pub const YARD: pb::MapId = pb::MapId::SupplyYard;

pub fn duel() -> Match {
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

pub fn place(game: &mut Match, id: u32, x: f32, z: f32, yaw: f32) {
    let entity = game.entities[&id];
    *game.world.get_mut::<Pose>(entity).unwrap() = Pose {
        x,
        z,
        yaw,
        ..Default::default()
    };
    game.world.get_mut::<Vitals>(entity).unwrap().protect_left = 0.0;
    game.world
        .get_mut::<History>(entity)
        .unwrap()
        .samples
        .clear();
}

pub fn cmd(seq: u32, tick: u32) -> pb::InputCmd {
    pb::InputCmd {
        seq,
        render_tick: tick,
        ..Default::default()
    }
}

pub fn weapon(game: &Match, id: u32) -> (u32, u32, bool) {
    let w = game.world.get::<Weapon>(game.entities[&id]).unwrap();
    (w.magazine, w.reserve, w.reloading())
}

pub fn tick_now(game: &Match) -> u32 {
    game.world.resource::<MatchState>().tick
}

// ---------------------------------------------------------------- jumping

/// A crate-sized box: 1.2 m on a side from x 0 to 1.2, centred on z 0.
pub fn crate_box() -> pb::MapBox {
    pb::MapBox {
        min_x: 0.0,
        max_x: 1.2,
        min_y: 0.0,
        max_y: 1.2,
        min_z: -0.6,
        max_z: 0.6,
        ..Default::default()
    }
}

/// Shooter at (-44, 0) looking along +X at a victim 6 m away with clear sight.
pub fn line_up(game: &mut Match, pitch: f32) -> pb::InputCmd {
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

/// One human on each team, plus the bots for each, as `[red, blue]`.
pub fn duel_with_bots(bots: [usize; 2], mode: pb::GameMode) -> Match {
    let humans = [
        (SHOOTER, "a".into(), pb::Team::Red, RIFLE),
        (VICTIM, "b".into(), pb::Team::Blue, RIFLE),
    ];
    Match::with_mode(&humans, YARD, mode, bots, 3)
}

pub fn shots_in_one_second(kind: pb::WeaponKind) -> u32 {
    let mut game = Match::new(
        &[(SHOOTER, "a".into(), pb::Team::Red, kind)],
        YARD,
        false,
        5,
    );
    let start = weapons::stats(kind).magazine;
    for seq in 0..TICK_RATE {
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                fire: true,
                ..cmd(seq + 1, tick_now(&game))
            }],
        );
        game.tick();
    }
    start - weapon(&game, SHOOTER).0
}

/// The shooter in `game` with the knife drawn, ready to attack, and the victim `dist` away along +x.
pub fn melee_setup(mut game: Match, dist: f32) -> (Match, pb::InputCmd) {
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(&mut game, SHOOTER, -46.0, 0.0, yaw);
    place(&mut game, VICTIM, -46.0 + dist, 0.0, 0.0);
    game.push_inputs(
        SHOOTER,
        vec![pb::InputCmd {
            slot: 3,
            yaw,
            ..cmd(1, 0)
        }],
    );
    for _ in 0..(DRAW_TIME * TICK_RATE as f32) as u32 + 2 {
        game.tick();
    }
    let swing = pb::InputCmd {
        fire: true,
        yaw,
        pitch: -0.3,
        ..cmd(2, tick_now(&game))
    };
    (game, swing)
}

pub fn crouch_cmd(seq: u32, tick: u32) -> pb::InputCmd {
    pb::InputCmd {
        crouch: true,
        ..cmd(seq, tick)
    }
}

/// Mean angle between the aim and where a machine gun's shots went.
pub fn mean_spread(crouch: bool) -> f32 {
    let mut game = duel();
    game.select_weapon(SHOOTER, pb::WeaponKind::MachineGun);
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(&mut game, SHOOTER, -46.0, 0.0, yaw);
    place(&mut game, VICTIM, -46.0, -8.0, 0.0);
    let aim = super::physics::view_dir(yaw, 0.0);
    let (mut total, mut count) = (0.0, 0);
    for seq in 1..=90 {
        let tick = tick_now(&game);
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                fire: true,
                crouch,
                yaw,
                ..cmd(seq, tick)
            }],
        );
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

/// Damage a level or raised shot from a standing or crouched rifleman does to a
/// crouched victim 4 m away.
pub fn shot_at_crouched(shooter_crouched: bool, aim_y: f32) -> i32 {
    let mut game = duel();
    let mut shot = line_up(&mut game, 0.0);
    game.push_inputs(VICTIM, vec![crouch_cmd(1, 0)]);
    game.tick();
    let eye = if shooter_crouched {
        CROUCH_EYE_HEIGHT
    } else {
        EYE_HEIGHT
    };
    shot.pitch = (aim_y - eye).atan2(4.0);
    shot.crouch = shooter_crouched;
    shot.render_tick = tick_now(&game);
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    MAX_HP - game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap().hp
}

/// Angle between the aim and each shot of a rifle magazine held on one spot.
pub fn sustained_fire_angles() -> Vec<f32> {
    let mut game = duel();
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(&mut game, SHOOTER, -46.0, 0.0, yaw);
    place(&mut game, VICTIM, -46.0, -8.0, 0.0);
    let aim = super::physics::view_dir(yaw, 0.0);
    let mut angles = Vec::new();
    for seq in 1..=88 {
        let tick = tick_now(&game);
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                fire: true,
                yaw,
                ..cmd(seq, tick)
            }],
        );
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

// ---------------------------------------------------------------- augment mode

pub fn augment_duel() -> Match {
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
pub fn with_augment(augment: Aug) -> Match {
    let mut game = augment_duel();
    game.apply_augment(SHOOTER, augment);
    game.apply_augment(VICTIM, Aug::QuickReload);
    game.world.resource_mut::<MatchState>().freeze_left = 0.0;
    game
}

pub fn draw_sidearm(game: &mut Match, id: u32) {
    let e = game.entities[&id];
    let mut weapon = game.world.get_mut::<Weapon>(e).unwrap();
    weapon.switch_to(2);
    weapon.cooldown = 0.0;
}

pub fn hp(game: &Match, id: u32) -> i32 {
    game.world.get::<Vitals>(game.entities[&id]).unwrap().hp
}

pub fn held(game: &Match, id: u32) -> Kind {
    game.world.get::<Weapon>(game.entities[&id]).unwrap().kind
}

/// Lines the shooter up and fires once at chest height, returning the tick's events.
pub fn shoot_once(game: &mut Match, seq: u32) -> Vec<pb::GameEvent> {
    let mut shot = line_up(game, -0.05);
    shot.seq = seq;
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    game.shared_snapshot().events
}

pub fn shot_events(events: &[pb::GameEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e.event, Some(pb::game_event::Event::Shot(_))))
        .count()
}

/// The victim is one hit from death and the shooter's next shot is ready.
pub fn kill_victim(game: &mut Match, seq: u32) {
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
    assert!(
        !game.world.get::<Vitals>(v).unwrap().alive,
        "shot {seq} should have killed"
    );
}

/// Every shot the shooter fires over a second of held trigger, as aim errors.
pub fn aim_errors(game: &mut Match) -> Vec<f32> {
    let yaw = -std::f32::consts::FRAC_PI_2;
    place(game, SHOOTER, -46.0, 0.0, yaw);
    place(game, VICTIM, -46.0, -8.0, 0.0);
    let mut errors = Vec::new();
    for seq in 1..=60 {
        let tick = tick_now(game);
        let e = game.entities[&SHOOTER];
        game.world.get_mut::<Weapon>(e).unwrap().cooldown = 0.0;
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                fire: true,
                forward: seq % 2 == 0,
                yaw,
                ..cmd(seq, tick)
            }],
        );
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

pub fn reload_seconds(augment: Aug) -> f32 {
    let mut game = with_augment(augment);
    let e = game.entities[&SHOOTER];
    game.world.get_mut::<Weapon>(e).unwrap().magazine = 10;
    game.push_inputs(
        SHOOTER,
        vec![pb::InputCmd {
            reload: true,
            ..cmd(1, 0)
        }],
    );
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

/// A shield bearer as the victim, facing `victim_yaw`, 4 m in front of the shooter.
pub fn shielded_target(victim_yaw: f32) -> Match {
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

// ---------------------------------------------------------------- free-for-all

pub const NO_TEAM: pb::Team = pb::Team::Unspecified;

/// A third free-for-all player, well out of the way unless placed.
pub const THIRD: u32 = 3;

/// `SHOOTER` and `VICTIM` in a free-for-all with `bots` bots.
pub fn ffa_with_bots(bots: usize, mode: pb::GameMode) -> Match {
    let humans = [
        (SHOOTER, "a".into(), NO_TEAM, RIFLE),
        (VICTIM, "b".into(), NO_TEAM, RIFLE),
    ];
    Match::with_rules(
        &humans,
        YARD,
        mode,
        pb::MatchType::FreeForAll,
        [bots, 0],
        42,
    )
}

pub fn ffa_duel() -> Match {
    ffa_with_bots(0, pb::GameMode::Normal)
}

/// A free-for-all of three past its waiting time, in which the shooter fires missiles.
pub fn ffa_missiles() -> Match {
    let humans = [
        (SHOOTER, "a".into(), NO_TEAM, RIFLE),
        (VICTIM, "b".into(), NO_TEAM, RIFLE),
        (THIRD, "c".into(), NO_TEAM, RIFLE),
    ];
    let mut game = Match::with_rules(
        &humans,
        YARD,
        pb::GameMode::Augment,
        pb::MatchType::FreeForAll,
        [0, 0],
        42,
    );
    game.apply_augment(SHOOTER, Aug::Missile);
    game.apply_augment(VICTIM, Aug::QuickReload);
    game.apply_augment(THIRD, Aug::QuickReload);
    game.world.resource_mut::<MatchState>().freeze_left = 0.0;
    place(&mut game, THIRD, 40.0, 28.0, 0.0);
    game
}

pub fn exploded(game: &mut Match) -> bool {
    game.shared_snapshot()
        .events
        .iter()
        .any(|e| matches!(e.event, Some(pb::game_event::Event::Explosion(_))))
}

/// Checks that a player stands on a free-for-all spawn point, facing the middle of the map.
pub fn assert_ffa_spawned(game: &Match, entity: Entity) -> (f32, f32) {
    let pose = *game.world.get::<Pose>(entity).unwrap();
    let point = (pose.x, pose.z);
    assert!(
        game.world.resource::<Arena>().ffa_spawns.contains(&point),
        "{point:?} is no spawn point"
    );
    let look = super::physics::view_dir(pose.yaw, 0.0);
    let to_middle = (-pose.x, -pose.z);
    let facing = (look[0] * to_middle.0 + look[2] * to_middle.1) / (to_middle.0.hypot(to_middle.1));
    assert!(
        facing > 0.999,
        "at {point:?} the yaw {} looks away from the middle",
        pose.yaw
    );
    point
}

// ---------------------------------------------------------------- grenades

pub const MATE: u32 = 3;

pub fn thrown(events: &[pb::GameEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e.event, Some(pb::game_event::Event::GrenadeThrown(_))))
        .count()
}

pub fn explosions(events: &[pb::GameEvent]) -> Vec<pb::ExplosionEvent> {
    events
        .iter()
        .filter_map(|e| match &e.event {
            Some(pb::game_event::Event::Explosion(blast)) => Some(*blast),
            _ => None,
        })
        .collect()
}

pub fn grenades_left(game: &mut Match, id: u32) -> u32 {
    let mut snapshot = game.shared_snapshot();
    game.fill_personal(id, &mut snapshot);
    snapshot.grenades_left
}

/// Takes out slot 4 and waits for it to be drawn. Returns the next command number.
pub fn draw_grenade(game: &mut Match, id: u32, seq: u32) -> u32 {
    game.push_inputs(
        id,
        vec![pb::InputCmd {
            slot: 4,
            ..cmd(seq, tick_now(game))
        }],
    );
    for _ in 0..=(DRAW_TIME * TICK_RATE as f32).ceil() as u32 {
        game.tick();
    }
    seq + 1
}

/// Throws what is in hand along (yaw, pitch); the tick's events.
pub fn throw(game: &mut Match, id: u32, seq: u32, yaw: f32, pitch: f32) -> Vec<pb::GameEvent> {
    game.push_inputs(
        id,
        vec![pb::InputCmd {
            fire: true,
            yaw,
            pitch,
            ..cmd(seq, tick_now(game))
        }],
    );
    game.tick();
    game.shared_snapshot().events
}

/// A grenade of `owner`'s lying still on the floor at (x, z), going off on the next tick but one.
pub fn drop_grenade(game: &mut Match, owner: u32, x: f32, z: f32) {
    let entity = game.entities[&owner];
    let team = game.world.get::<Player>(entity).unwrap().team;
    let mut grenades = game.world.resource_mut::<Grenades>();
    grenades.next_id += 1;
    let id = grenades.next_id;
    grenades.live.push(Grenade {
        id,
        owner: entity,
        owner_id: owner,
        team,
        pos: [x, GRENADE_RADIUS, z],
        vel: [0.0; 3],
        fuse_ticks: 1,
    });
}

/// The kills of the next two ticks.
pub fn kills_in_two_ticks(game: &mut Match) -> Vec<pb::KillEvent> {
    let mut kills = Vec::new();
    for _ in 0..2 {
        game.tick();
        for event in game.shared_snapshot().events {
            if let Some(pb::game_event::Event::Kill(kill)) = event.event {
                kills.push(kill);
            }
        }
    }
    kills
}

// ---------------------------------------------------------------- Big Head mode

pub fn big_head_duel() -> Match {
    duel_with_bots([0, 0], pb::GameMode::BigHead)
}

pub fn perks_of(game: &Match, id: u32) -> &Perks {
    game.world.get::<Perks>(game.entities[&id]).unwrap()
}

pub fn set_level(game: &mut Match, id: u32, level: i32) {
    let entity = game.entities[&id];
    game.world
        .get_mut::<Perks>(entity)
        .unwrap()
        .set_level(level);
}

/// Crowns `id` as a kill at the top level would.
pub fn make_king(game: &mut Match, id: u32) {
    let entity = game.entities[&id];
    let max_hp = game.world.resource::<Rules>().max_hp;
    let mut query = game.world.query::<(&mut Perks, &mut Vitals, &mut Weapon)>();
    let (mut perks, mut vitals, mut weapon) = query.get_mut(&mut game.world, entity).unwrap();
    perks.set_level(big_head::MAX_LEVEL);
    big_head::crown(&mut perks, &mut vitals, &mut weapon, max_hp);
}

/// The kills of the last tick.
pub fn kills_now(game: &mut Match) -> Vec<pb::KillEvent> {
    game.shared_snapshot()
        .events
        .into_iter()
        .filter_map(|event| match event.event {
            Some(pb::game_event::Event::Kill(kill)) => Some(kill),
            _ => None,
        })
        .collect()
}

/// The shooter, lined up as `line_up` does, shoots the victim in the head; the tick's kills.
pub fn headshot(game: &mut Match) -> Vec<pb::KillEvent> {
    let shot = line_up(game, 0.03);
    game.push_inputs(SHOOTER, vec![shot]);
    game.tick();
    kills_now(game)
}
