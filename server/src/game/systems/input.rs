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

pub fn consume_inputs(
    arena: Res<Arena>,
    mut players: Query<(&mut Pose, &mut Inputs, &mut Intent, &Vitals, &mut Weapon)>,
) {
    for (mut pose, mut inputs, mut intent, vitals, mut weapon) in &mut players {
        *intent = Intent::default();
        inputs.budget = (inputs.budget + CMDS_PER_TICK).min(CMD_BUDGET_CAP);

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
            inputs.budget -= 1;
            inputs.last_seq = cmd.seq;
            if !vitals.alive {
                continue;
            }

            let yaw = if cmd.yaw.is_finite() {
                cmd.yaw
            } else {
                pose.yaw
            };
            let pitch = if cmd.pitch.is_finite() {
                cmd.pitch.clamp(-MAX_PITCH, MAX_PITCH)
            } else {
                pose.pitch
            };
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
            let stance = if cmd.crouch { CROUCH_SPEED_FACTOR } else { 1.0 };
            let speed = weapons::stats(weapon.kind).move_speed * stance;

            let fwd = cmd.forward as i32 - cmd.back as i32;
            let strafe = cmd.right as i32 - cmd.left as i32;
            if fwd != 0 || strafe != 0 {
                let (sin, cos) = yaw.sin_cos();
                let (fx, fz) = (-sin, -cos);
                let (rx, rz) = (cos, -sin);
                let mut dx = fx * fwd as f32 + rx * strafe as f32;
                let mut dz = fz * fwd as f32 + rz * strafe as f32;
                let len = (dx * dx + dz * dz).sqrt();
                dx *= speed * CMD_DT / len;
                dz *= speed * CMD_DT / len;
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
