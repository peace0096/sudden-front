//! Player input: each tick spends a budget of queued commands on view, movement and intent.

use crate::game::components::*;
use crate::game::physics::{step_move, step_vertical};
use crate::game::resources::*;
use crate::game::*;
use bevy_ecs::prelude::*;

/// Commands a player may consume per tick (two at 60 Hz input / 30 Hz tick),
/// with a little slack for network jitter. Anything faster is a speed hack.
const CMDS_PER_TICK: u32 = 2;

const CMD_BUDGET_CAP: u32 = 8;

const MAX_QUEUED_CMDS: usize = 32;

/// The budget after a tick's refill: two more commands, never more than the cap.
fn refill(budget: u32) -> u32 {
    budget.saturating_add(CMDS_PER_TICK).min(CMD_BUDGET_CAP)
}

/// The view a command asks for, `(yaw, pitch)`. An angle that isn't a finite number keeps the
/// current one, and pitch is clamped to what a neck allows.
fn view_angles(cmd_yaw: f32, cmd_pitch: f32, yaw: f32, pitch: f32) -> (f32, f32) {
    let yaw = if cmd_yaw.is_finite() { cmd_yaw } else { yaw };
    let pitch = if cmd_pitch.is_finite() {
        cmd_pitch.clamp(-MAX_PITCH, MAX_PITCH)
    } else {
        pitch
    };
    (yaw, pitch)
}

/// -1, 0 or 1 along an axis whose keys are `plus` and `minus`; both held cancel out.
fn axis(plus: bool, minus: bool) -> i32 {
    match (plus, minus) {
        (true, false) => 1,
        (false, true) => -1,
        _ => 0,
    }
}

/// Walking speed (m/s) with `kind` in hand, crouched or not.
fn walk_speed(kind: pb::WeaponKind, crouching: bool) -> f32 {
    let stance = if crouching { CROUCH_SPEED_FACTOR } else { 1.0 };
    weapons::stats(kind).move_speed * stance
}

/// The step `(dx, dz)` one command walks facing `yaw`, `forward` and `right` each -1, 0 or 1, at
/// `speed`: a diagonal is no faster than a straight line. `None` when no key moves the player.
fn walk_step(yaw: f32, forward: i32, right: i32, speed: f32) -> Option<(f32, f32)> {
    if forward == 0 && right == 0 {
        return None;
    }
    let (sin, cos) = yaw.sin_cos();
    let (fx, fz) = (-sin, -cos);
    let (rx, rz) = (cos, -sin);
    let dx = fx * forward as f32 + rx * right as f32;
    let dz = fz * forward as f32 + rz * right as f32;
    let len = (dx * dx + dz * dz).sqrt();
    let scale = speed * CMD_DT / len;
    Some((dx * scale, dz * scale))
}

pub fn consume_inputs(
    arena: Res<Arena>,
    mut players: Query<(&mut Pose, &mut Inputs, &mut Intent, &Vitals, &mut Weapon)>,
) {
    for (mut pose, mut inputs, mut intent, vitals, mut weapon) in &mut players {
        *intent = Intent::default();
        inputs.budget = refill(inputs.budget);

        // Drop a backlog rather than let it become permanent input latency.
        while inputs.queue.len() > MAX_QUEUED_CMDS {
            if let Some(cmd) = inputs.queue.pop_front() {
                inputs.last_seq = cmd.seq;
            }
        }

        while inputs.budget > 0 {
            let Some(cmd) = inputs.queue.pop_front() else {
                break;
            };
            inputs.budget = inputs.budget.saturating_sub(1);
            inputs.last_seq = cmd.seq;
            if !vitals.alive {
                continue;
            }

            let (yaw, pitch) = view_angles(cmd.yaw, cmd.pitch, pose.yaw, pose.pitch);
            pose.yaw = yaw;
            pose.pitch = pitch;
            let (mut y, mut vy) = (pose.y, pose.vy);
            // Paralysis leaves only the view, and gravity.
            if vitals.stun_left > 0.0 {
                step_vertical(&arena.map.boxes, pose.x, pose.z, &mut y, &mut vy, false);
                (pose.y, pose.vy) = (y, vy);
                continue;
            }

            // Applied per command, because the client predicts each one at the
            // speed of the weapon it had selected by then.
            weapon.switch_to(cmd.slot);
            pose.crouching = cmd.crouch;
            let speed = walk_speed(weapon.kind, cmd.crouch);
            let (forward, right) = (axis(cmd.forward, cmd.back), axis(cmd.right, cmd.left));
            if let Some((dx, dz)) = walk_step(yaw, forward, right, speed) {
                let (mut x, mut z) = (pose.x, pose.z);
                step_move(&arena.map.boxes, &mut x, &mut z, pose.y, dx, dz);
                pose.x = x;
                pose.z = z;
                intent.moving = true;
            }
            // Up or down after the step, at its new place: over a box top it lands there.
            step_vertical(&arena.map.boxes, pose.x, pose.z, &mut y, &mut vy, cmd.jump);
            (pose.y, pose.vy) = (y, vy);
            // The tick's first shot aims where that command aimed; the client's
            // recoil from it only reaches the commands after it.
            if cmd.fire && intent.fire.is_none() {
                intent.fire = Some(FireIntent {
                    yaw,
                    pitch,
                    render_tick: cmd.render_tick,
                });
            }
            intent.reload |= cmd.reload;
            intent.aim = cmd.aim;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn the_command_budget_refills_by_two_a_tick_up_to_the_cap() {
        assert_eq!(refill(0), CMDS_PER_TICK);
        assert_eq!(refill(CMD_BUDGET_CAP), CMD_BUDGET_CAP);
        assert_eq!(refill(u32::MAX), CMD_BUDGET_CAP);
    }

    #[test]
    fn a_command_with_broken_angles_keeps_the_current_view() {
        assert_eq!(view_angles(f32::NAN, f32::INFINITY, 0.5, -0.2), (0.5, -0.2));
    }

    #[test]
    fn pitch_is_clamped_to_what_a_neck_allows() {
        assert_eq!(view_angles(1.0, 3.0, 0.0, 0.0), (1.0, MAX_PITCH));
        assert_eq!(view_angles(1.0, -3.0, 0.0, 0.0), (1.0, -MAX_PITCH));
    }

    #[test]
    fn opposite_keys_cancel_out_on_an_axis() {
        assert_eq!(axis(true, false), 1);
        assert_eq!(axis(false, true), -1);
        assert_eq!(axis(true, true), 0);
        assert_eq!(axis(false, false), 0);
    }

    #[test]
    fn crouching_walks_at_the_crouch_share_of_the_weapons_speed() {
        let speed = weapons::stats(Kind::Rifle).move_speed;
        assert_eq!(walk_speed(Kind::Rifle, false), speed);
        assert_eq!(walk_speed(Kind::Rifle, true), speed * CROUCH_SPEED_FACTOR);
    }

    #[test]
    fn no_movement_key_means_no_step() {
        assert_eq!(walk_step(0.3, 0, 0, 6.0), None);
    }

    #[test]
    fn forward_at_yaw_zero_is_towards_negative_z_and_right_towards_positive_x() {
        let (dx, dz) = walk_step(0.0, 1, 0, 6.0).unwrap();
        assert!(dx.abs() < 1e-6 && (dz + 6.0 * CMD_DT).abs() < 1e-6);
        let (dx, dz) = walk_step(0.0, 0, 1, 6.0).unwrap();
        assert!((dx - 6.0 * CMD_DT).abs() < 1e-6 && dz.abs() < 1e-6);
    }

    #[test]
    fn a_diagonal_step_is_no_longer_than_a_straight_one() {
        let (dx, dz) = walk_step(0.7, 1, -1, 6.0).unwrap();
        assert!(((dx * dx + dz * dz).sqrt() - 6.0 * CMD_DT).abs() < 1e-6);
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
        assert!(
            pose.x - -40.0 <= weapons::stats(RIFLE).move_speed * CMD_DT * 4.0 + 1e-3,
            "moved {}",
            pose.x + 40.0
        );
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
        assert!(
            pose.x.is_finite()
                && pose.z.is_finite()
                && pose.yaw.is_finite()
                && pose.pitch.is_finite()
        );
    }

    #[test]
    fn movement_speed_depends_on_the_weapon() {
        use pb::WeaponKind::*;
        let walked = |kind| {
            let mut game = Match::new(
                &[(SHOOTER, "a".into(), pb::Team::Red, kind)],
                YARD,
                false,
                5,
            );
            let yaw = std::f32::consts::FRAC_PI_2;
            place(&mut game, SHOOTER, 40.0, -30.0, yaw);
            let before = game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap().x;
            for seq in 0..20 {
                game.push_inputs(
                    SHOOTER,
                    vec![pb::InputCmd {
                        forward: true,
                        yaw,
                        ..cmd(seq + 1, 0)
                    }],
                );
                game.tick();
            }
            (game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap().x - before).abs()
        };
        assert!(walked(Smg) > walked(Rifle));
        assert!(walked(Rifle) > walked(Sniper));
        assert!(walked(Sniper) > walked(MachineGun));
    }

    #[test]
    fn slot_speed_applies_from_the_command_that_switches() {
        let walk = |slot: u32| {
            let mut game = duel();
            let yaw = std::f32::consts::FRAC_PI_2;
            place(&mut game, SHOOTER, 40.0, -30.0, yaw);
            game.push_inputs(
                SHOOTER,
                vec![pb::InputCmd {
                    forward: true,
                    slot,
                    yaw,
                    ..cmd(1, 0)
                }],
            );
            game.tick();
            40.0 - game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap().x
        };
        let expected = weapons::stats(pb::WeaponKind::Knife).move_speed * CMD_DT;
        assert!((walk(3) - expected).abs() < 1e-3);
        assert!(walk(3) > walk(1));
    }

    #[test]
    fn crouching_halves_speed_and_persists_between_commands() {
        let mut game = duel();
        let yaw = std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, 40.0, -30.0, yaw);
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                forward: true,
                yaw,
                ..crouch_cmd(1, 0)
            }],
        );
        game.tick();
        let e = game.entities[&SHOOTER];
        let moved = 40.0 - game.world.get::<Pose>(e).unwrap().x;
        assert!(
            (moved - weapons::stats(RIFLE).move_speed * CROUCH_SPEED_FACTOR * CMD_DT).abs() < 1e-3
        );
        game.tick();
        assert!(
            game.world.get::<Pose>(e).unwrap().crouching,
            "no new command, so the stance holds"
        );
        game.push_inputs(SHOOTER, vec![cmd(2, 0)]);
        game.tick();
        assert!(!game.world.get::<Pose>(e).unwrap().crouching);
    }
}
