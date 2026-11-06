//! Grenades: thrown, bounced and rolled until the fuse runs out, then a blast.

use super::blast::{blast, Blaster};
use super::fire::{Hit, TargetView};
use crate::game::components::*;
use crate::game::physics::{sweep_ball, view_dir};
use crate::game::resources::*;
use crate::game::*;
use crate::pb;
use crate::pb::game_event::Event;
use bevy_ecs::prelude::*;

/// A thrown grenade leaves the hand this fast (m/s), this much (radians) above the aim, which
/// lofts a level throw a little, from this far (m) in front of the eye.
pub const THROW_SPEED: f32 = 16.0;

pub const THROW_LIFT: f32 = 0.1;

const THROW_REACH: f32 = 0.2;

/// A grenade falls slower than a player (m/s²), which carries a lob about 21 m.
pub const GRENADE_GRAVITY: f32 = 12.0;

/// Seconds from leaving the hand to going off.
pub const GRENADE_FUSE: f32 = 3.0;

const FUSE_TICKS: u32 = (GRENADE_FUSE * TICK_RATE as f32) as u32;

/// The grenade is a ball this big (m) to the world.
pub const GRENADE_RADIUS: f32 = 0.06;

/// On touching a surface: the share of the speed into it the grenade bounces back with, and of the
/// speed along it kept.
const GRENADE_BOUNCE: f32 = 0.35;

const GRENADE_FRICTION: f32 = 0.65;

/// Bouncing up off a floor or box top slower than this (m/s), it stays down and rolls; slower than
/// GRENADE_REST all told, it lies still.
const GRENADE_SETTLE: f32 = 1.5;

const GRENADE_REST: f32 = 0.15;

/// The most surfaces a grenade touches in one tick; the rest of a busier tick is dropped.
const GRENADE_CONTACTS: usize = 4;

/// How far off a surface (m) a touching grenade is put back, so rounding never leaves it inside.
const GRENADE_STANDOFF: f32 = 1e-3;

/// Throws the grenade in hand along `aim`, lofted a little. The last one thrown, the weapon held
/// before it comes back, drawn by the time the throw's own `fire_interval` is up.
pub(super) fn throw_grenade(
    grenades: &mut Grenades,
    owner: Entity,
    player: &Player,
    pose: &Pose,
    aim: FireIntent,
    weapon: &mut Weapon,
    log: &mut EventLog,
) {
    weapon.magazine -= 1;
    weapon.cooldown += weapons::stats(pb::WeaponKind::Grenade).fire_interval;
    let eye = if pose.crouching {
        CROUCH_EYE_HEIGHT
    } else {
        EYE_HEIGHT
    };
    // Within the thrower's own column, which no box reaches into, so it starts in the open.
    let (sin, cos) = aim.yaw.sin_cos();
    let pos = [
        pose.x - sin * THROW_REACH,
        pose.y + eye,
        pose.z - cos * THROW_REACH,
    ];
    let dir = view_dir(aim.yaw, (aim.pitch + THROW_LIFT).min(MAX_PITCH));
    grenades.next_id += 1;
    grenades.live.push(Grenade {
        id: grenades.next_id,
        owner,
        owner_id: player.id,
        team: player.team,
        pos,
        vel: dir.map(|d| d * THROW_SPEED),
        fuse_ticks: FUSE_TICKS,
    });
    log.push(Event::GrenadeThrown(pb::ThrowEvent {
        player_id: player.id,
    }));
    if weapon.magazine == 0 {
        weapon.switch_to(weapon.slot_before_grenade());
    }
}

/// One tick of a grenade's flight: gravity, then the move, bouncing off whatever it touches. Off a
/// floor or box top slow enough it stays down and rolls, slowed by every touch, until it lies still.
pub fn roll_grenade(
    boxes: &[pb::MapBox],
    half_x: f32,
    half_z: f32,
    pos: &mut [f32; 3],
    vel: &mut [f32; 3],
) {
    vel[1] -= GRENADE_GRAVITY * TICK_DT;
    let mut left = TICK_DT;
    for _ in 0..GRENADE_CONTACTS {
        let delta = vel.map(|v| v * left);
        let Some((t, axis, sign)) = sweep_ball(boxes, *pos, delta, GRENADE_RADIUS, half_x, half_z)
        else {
            for a in 0..3 {
                pos[a] += delta[a];
            }
            break;
        };
        for a in 0..3 {
            pos[a] += delta[a] * t;
        }
        pos[axis] += sign * GRENADE_STANDOFF;
        left *= 1.0 - t;
        vel[axis] = -vel[axis] * GRENADE_BOUNCE;
        for a in (0..3).filter(|&a| a != axis) {
            vel[a] *= GRENADE_FRICTION;
        }
        if axis == 1 && sign > 0.0 && vel[1].abs() < GRENADE_SETTLE {
            vel[1] = 0.0;
            if vel.iter().map(|v| v * v).sum::<f32>() < GRENADE_REST * GRENADE_REST {
                *vel = [0.0; 3];
            }
        }
        if left <= 0.0 {
            break;
        }
    }
}

/// Moves each grenade a tick; one whose fuse has run out goes off where it is instead.
pub(super) fn fly_grenades(
    arena: &Arena,
    rules: &Rules,
    grenades: &mut Grenades,
    targets: &[TargetView],
    log: &mut EventLog,
    hits: &mut Vec<Hit>,
) {
    let map = &arena.map;
    grenades.live.retain_mut(|grenade| {
        if grenade.fuse_ticks == 0 {
            let [x, y, z] = grenade.pos;
            log.push(Event::Explosion(pb::ExplosionEvent { x, y, z }));
            let source = Blaster {
                owner: grenade.owner,
                owner_id: grenade.owner_id,
                team: grenade.team,
                weapon: pb::WeaponKind::Grenade,
            };
            blast(&map.boxes, rules, grenade.pos, &source, targets, hits);
            return false;
        }
        grenade.fuse_ticks -= 1;
        roll_grenade(
            &map.boxes,
            map.half_x,
            map.half_z,
            &mut grenade.pos,
            &mut grenade.vel,
        );
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn slot_4_holds_one_grenade_a_life_and_a_throw_goes_back_to_the_gun() {
        let mut game = duel();
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -46.0, 0.0, yaw);
        assert_eq!(grenades_left(&mut game, SHOOTER), 1);

        let seq = draw_grenade(&mut game, SHOOTER, 1);
        assert_eq!(held(&game, SHOOTER), Kind::Grenade);
        assert_eq!(weapon(&game, SHOOTER), (1, 0, false));
        // A grenade's stats come to the client like any weapon's, for its name and the throw's timing.
        assert!(weapons::all_stats()
            .iter()
            .any(|w| w.kind == Kind::Grenade as i32 && w.magazine == 1 && w.splash_radius > 0.0));

        let events = throw(&mut game, SHOOTER, seq, yaw, 0.2);
        assert_eq!(thrown(&events), 1);
        assert_eq!(game.shared_snapshot().grenades.len(), 1);
        assert_eq!(grenades_left(&mut game, SHOOTER), 0);
        // Straight back to the rifle and its magazine, which has to be drawn before it fires.
        assert_eq!(held(&game, SHOOTER), RIFLE);
        assert_eq!(weapon(&game, SHOOTER), (30, 120, false));
        let w = game.world.get::<Weapon>(game.entities[&SHOOTER]).unwrap();
        assert!(
            w.cooldown >= weapons::stats(Kind::Grenade).fire_interval - 0.05,
            "cooldown {}",
            w.cooldown
        );

        // With none left, slot 4 is nothing to switch to.
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                slot: 4,
                ..cmd(seq + 1, tick_now(&game))
            }],
        );
        game.tick();
        assert_eq!(held(&game, SHOOTER), RIFLE);
    }

    #[test]
    fn a_throw_goes_back_to_the_slot_held_before_the_grenade() {
        let mut game = duel();
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -46.0, 0.0, yaw);
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                slot: 3,
                ..cmd(1, tick_now(&game))
            }],
        );
        game.tick();
        assert_eq!(held(&game, SHOOTER), Kind::Knife);
        let seq = draw_grenade(&mut game, SHOOTER, 2);
        throw(&mut game, SHOOTER, seq, yaw, 0.2);
        assert_eq!(held(&game, SHOOTER), Kind::Knife);
    }

    #[test]
    fn a_grenade_goes_off_three_seconds_after_the_throw_where_it_lies() {
        let mut game = duel();
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -46.0, 0.0, yaw);
        place(&mut game, VICTIM, 40.0, 0.0, 0.0);
        let seq = draw_grenade(&mut game, SHOOTER, 1);
        throw(&mut game, SHOOTER, seq, yaw, 0.3);
        let thrown_at = tick_now(&game) - 1;
        let mut last = game.shared_snapshot().grenades[0];
        let went_off = loop {
            game.tick();
            let snapshot = game.shared_snapshot();
            if let Some(blast) = explosions(&snapshot.events).pop() {
                assert!(snapshot.grenades.is_empty());
                break (tick_now(&game) - 1, blast);
            }
            last = snapshot.grenades[0];
            assert!(tick_now(&game) < thrown_at + 200, "never went off");
        };
        let (tick, blast) = went_off;
        assert_eq!(
            tick - thrown_at,
            (GRENADE_FUSE * TICK_RATE as f32) as u32,
            "fuse"
        );
        // Thrown forward, it came to rest on the floor before going off, where it lay.
        assert!(last.x > -40.0, "landed at {}", last.x);
        assert_eq!(
            (last.vx, last.vy, last.vz),
            (0.0, 0.0, 0.0),
            "still moving: {last:?}"
        );
        assert!(
            (last.y - GRENADE_RADIUS).abs() < 0.01,
            "not on the floor: {}",
            last.y
        );
        assert_eq!((blast.x, blast.y, blast.z), (last.x, last.y, last.z));
    }

    #[test]
    fn a_grenade_bounces_off_a_wall_and_settles_on_the_floor_never_inside_anything() {
        let wall = pb::MapBox {
            min_x: 2.0,
            max_x: 3.0,
            min_y: 0.0,
            max_y: 3.0,
            min_z: -5.0,
            max_z: 5.0,
            ..Default::default()
        };
        let (mut pos, mut vel) = ([0.0f32, 1.4, 0.0], [12.0f32, 2.0, 1.0]);
        let mut back = false;
        for tick in 0..(4 * TICK_RATE) {
            roll_grenade(&[wall], 50.0, 50.0, &mut pos, &mut vel);
            assert!(
                pos[0] < wall.min_x - GRENADE_RADIUS + 0.01,
                "tick {tick}: in the wall at {pos:?}"
            );
            assert!(
                pos[1] > GRENADE_RADIUS - 0.01,
                "tick {tick}: under the floor at {pos:?}"
            );
            back |= vel[0] < 0.0;
        }
        assert!(back, "never bounced off the wall");
        assert!(pos[0] < 2.0, "{pos:?}");
        assert_eq!(vel, [0.0; 3], "still rolling");
        assert!((pos[1] - GRENADE_RADIUS).abs() < 0.01, "{pos:?}");
    }

    #[test]
    fn a_grenade_lobbed_over_the_perimeter_wall_stays_in_the_map() {
        let map = build_map(YARD).data;
        let (mut pos, mut vel) = ([47.0f32, 1.4, 0.0], [9.0f32, 15.0, 0.0]);
        let mut back = false;
        for tick in 0..(4 * TICK_RATE) {
            roll_grenade(&map.boxes, map.half_x, map.half_z, &mut pos, &mut vel);
            assert!(
                pos[0] < map.half_x - GRENADE_RADIUS + 0.01,
                "tick {tick}: out of the map at {pos:?}"
            );
            back |= vel[0] < 0.0;
        }
        assert!(back, "never turned back at the map's edge");
    }

    #[test]
    fn a_grenade_comes_back_with_each_life() {
        let mut game = duel();
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -46.0, 0.0, yaw);
        let seq = draw_grenade(&mut game, SHOOTER, 1);
        throw(&mut game, SHOOTER, seq, yaw, 0.2);
        assert_eq!(grenades_left(&mut game, SHOOTER), 0);
        {
            let mut vitals = game
                .world
                .get_mut::<Vitals>(game.entities[&SHOOTER])
                .unwrap();
            vitals.alive = false;
            vitals.respawn_left = TICK_DT;
        }
        game.tick();
        assert!(
            game.world
                .get::<Vitals>(game.entities[&SHOOTER])
                .unwrap()
                .alive
        );
        assert_eq!(grenades_left(&mut game, SHOOTER), 1);
    }
}
