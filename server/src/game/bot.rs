//! Bots feed ordinary `InputCmd`s into the same queue as humans, so they obey
//! exactly the same movement, ammo and fire-rate rules.

use super::components::*;
use super::physics::line_clear;
use super::*;
use crate::pb;
use bevy_ecs::prelude::*;
use std::f32::consts::{PI, TAU};

const SIGHT_RANGE: f32 = 46.0;
const SIGHT_HALF_ANGLE: f32 = 1.4;
const REACTION_MIN: f32 = 0.45;
const REACTION_MAX: f32 = 0.95;
const TURN_RATE: f32 = 3.2;
const AIM_CHEST_Y: f32 = 1.2;
const AIM_CHEST_Y_CROUCHED: f32 = AIM_CHEST_Y - CROUCH_DROP;
const INITIAL_AIM_ERROR: f32 = 0.085;
const AIM_SETTLE_RATE: f32 = 0.55;
const AIM_WOBBLE: f32 = 0.02;
/// Now and then a bot lobs its grenade at an enemy it has watched for a while (s) in the middle
/// distance (m): with this chance a tick once the rest holds, about once in two seconds.
const GRENADE_RANGE: std::ops::Range<f32> = 8.0..22.0;
const GRENADE_AFTER: f32 = 1.0;
const GRENADE_CHANCE: f32 = 0.015;
/// A lob lands this share of the way to the enemy's feet and rolls on toward them.
const GRENADE_SHORT: f32 = 0.75;

fn wrap_angle(mut a: f32) -> f32 {
    while a > PI {
        a -= TAU;
    }
    while a < -PI {
        a += TAU;
    }
    a
}

fn turn_towards(current: f32, target: f32, max_step: f32) -> f32 {
    current + wrap_angle(target - current).clamp(-max_step, max_step)
}

fn yaw_to(dx: f32, dz: f32) -> f32 {
    (-dx).atan2(-dz)
}

/// The aim's pitch that lands a thrown grenade `distance` metres away and `rise` metres higher
/// (lower if negative) on the low arc, or `None` beyond its reach. Along the throw's line, at
/// distance x with T the tangent of the angle it leaves at, it is x·T − k(1 + T²) up, where
/// k = g·x²/(2v²).
fn lob_pitch(distance: f32, rise: f32) -> Option<f32> {
    let (speed, gravity) = (systems::THROW_SPEED, systems::GRENADE_GRAVITY);
    let k = gravity * distance * distance / (2.0 * speed * speed);
    let room = distance * distance - 4.0 * k * (rise + k);
    if k <= 0.0 || room < 0.0 {
        return None;
    }
    let tangent = (distance - room.sqrt()) / (2.0 * k);
    Some(tangent.atan() - systems::THROW_LIFT)
}

struct Seen {
    entity: Entity,
    team: pb::Team,
    x: f32,
    z: f32,
    aim_y: f32,
    /// Feet height, where a grenade is aimed.
    feet: f32,
}

#[allow(clippy::type_complexity)]
pub fn bot_think(
    arena: Res<Arena>,
    rules: Res<Rules>,
    state: Res<MatchState>,
    mut rng: ResMut<GameRng>,
    others: Query<(Entity, &Player, &Pose, &Vitals)>,
    mut bots: Query<(Entity, &mut BotBrain, &mut Inputs, &mut Weapon, &Pose, &Player, &Vitals)>,
) {
    let living: Vec<Seen> = others
        .iter()
        .filter(|(_, _, _, vitals)| vitals.alive && vitals.protect_left <= 0.0)
        .map(|(entity, player, pose, _)| Seen {
            entity,
            team: player.team,
            x: pose.x,
            z: pose.z,
            // A player in the air or on a box top carries their chest up with them.
            aim_y: pose.y + if pose.crouching { AIM_CHEST_Y_CROUCHED } else { AIM_CHEST_Y },
            feet: pose.y,
        })
        .collect();

    for (entity, mut brain, mut inputs, mut weapon, pose, player, vitals) in &mut bots {
        if !vitals.alive {
            brain.path.clear();
            brain.target = None;
            brain.tracked_for = 0.0;
            brain.throwing = false;
            continue;
        }
        // Thrown: the server has already put the grenade away.
        if weapon.grenades() == 0 {
            brain.throwing = false;
        }
        // A taser is good for one shot at the start of a life; then the real gun.
        if weapon.kind == crate::pb::WeaponKind::Taser && weapon.magazine == 0 {
            weapon.switch_to(1);
        }
        let stats = weapons::stats(weapon.kind);
        // Bots have endless spare magazines; only the reload pause matters.
        if weapon.reserve == 0 {
            weapon.reserve = stats.reserve;
        }

        let eye = [pose.x, pose.y + EYE_HEIGHT, pose.z];
        let mut best: Option<(&Seen, f32)> = None;
        for other in living.iter().filter(|o| rules.hostile(player.team, o.team) && o.entity != entity) {
            let (dx, dz) = (other.x - pose.x, other.z - pose.z);
            let dist = (dx * dx + dz * dz).sqrt();
            if dist > SIGHT_RANGE {
                continue;
            }
            let tracked = brain.target == Some(other.entity);
            let in_fov = wrap_angle(yaw_to(dx, dz) - pose.yaw).abs() < SIGHT_HALF_ANGLE;
            if !(tracked || in_fov || dist < 4.0) {
                continue;
            }
            if !line_clear(&arena.map.boxes, eye, [other.x, other.aim_y, other.z]) {
                continue;
            }
            if best.is_none_or(|(_, d)| dist < d) {
                best = Some((other, dist));
            }
        }

        let mut cmd = pb::InputCmd {
            yaw: pose.yaw,
            pitch: pose.pitch,
            render_tick: state.tick,
            ..Default::default()
        };

        if let Some((enemy, dist)) = best {
            if brain.target != Some(enemy.entity) {
                brain.target = Some(enemy.entity);
                brain.tracked_for = 0.0;
                brain.aim_error_yaw = rng.range(-INITIAL_AIM_ERROR, INITIAL_AIM_ERROR);
                brain.aim_error_pitch = rng.range(-INITIAL_AIM_ERROR, INITIAL_AIM_ERROR) * 0.6;
                brain.burst_left = 0;
                brain.burst_pause = rng.range(REACTION_MIN, REACTION_MAX);
            }
            brain.tracked_for += TICK_DT;
            brain.path.clear();

            let settle = (1.0 - AIM_SETTLE_RATE * TICK_DT).max(0.0);
            brain.aim_error_yaw *= settle;
            brain.aim_error_pitch *= settle;

            let wobble = AIM_WOBBLE * (state.tick as f32 * 0.17 + entity.index_u32() as f32).sin();
            let want_yaw = yaw_to(enemy.x - pose.x, enemy.z - pose.z) + brain.aim_error_yaw + wobble;
            let want_pitch = (enemy.aim_y - eye[1]).atan2(dist) + brain.aim_error_pitch;
            cmd.yaw = turn_towards(pose.yaw, want_yaw, TURN_RATE * TICK_DT);
            cmd.pitch = want_pitch.clamp(-MAX_PITCH, MAX_PITCH);

            // Side-step while shooting.
            brain.strafe_timer -= TICK_DT;
            if brain.strafe_timer <= 0.0 {
                brain.strafe_timer = rng.range(0.5, 1.4);
                brain.strafe_left = rng.next_f32() < 0.5;
            }
            cmd.left = brain.strafe_left;
            cmd.right = !brain.strafe_left;
            if dist > 28.0 {
                cmd.forward = true;
            }

            cmd.aim = stats.has_scope;
            let on_target = wrap_angle(want_yaw - cmd.yaw).abs() < 0.1;
            let lob = lob_pitch(dist * GRENADE_SHORT, enemy.feet - eye[1]);
            if !brain.throwing
                && weapon.grenades() > 0
                && brain.tracked_for > GRENADE_AFTER
                && GRENADE_RANGE.contains(&dist)
                && lob.is_some()
                && rng.next_f32() < GRENADE_CHANCE
            {
                brain.throwing = true;
            }
            if brain.throwing {
                // Out with the grenade, the same way a player takes it: slot 4. Once it is in
                // hand the trigger throws it as soon as it is drawn, and the server puts it away.
                if weapon.kind != pb::WeaponKind::Grenade {
                    cmd.slot = 4;
                } else if let Some(pitch) = lob {
                    cmd.pitch = pitch.clamp(-MAX_PITCH, MAX_PITCH);
                    cmd.fire = on_target;
                } else {
                    cmd.slot = weapon.slot_before_grenade();
                    brain.throwing = false;
                }
            } else if brain.burst_pause > 0.0 {
                brain.burst_pause -= TICK_DT;
            } else if on_target && weapon.magazine > 0 {
                if brain.burst_left == 0 {
                    // A bolt-action fires single aimed shots.
                    brain.burst_left = if stats.has_scope { 1 } else { 4 + rng.index(6) as u32 };
                }
                cmd.fire = true;
                if weapon.cooldown <= 0.0 && !weapon.reloading() {
                    brain.burst_left -= 1;
                    if brain.burst_left == 0 {
                        brain.burst_pause = rng.range(0.35, 0.8);
                    }
                }
            }
        } else {
            brain.target = None;
            // Nobody left to throw at: the grenade goes back.
            if brain.throwing {
                brain.throwing = false;
                if weapon.kind == pb::WeaponKind::Grenade {
                    cmd.slot = weapon.slot_before_grenade();
                }
            }
            if weapon.magazine < stats.magazine / 2 {
                cmd.reload = true;
            }

            // Patrol: head for a lane or towards a random enemy.
            brain.repath_in -= TICK_DT;
            let finished = brain.path_index >= brain.path.len();
            if finished || brain.repath_in <= 0.0 {
                let enemies: Vec<&Seen> = living
                    .iter()
                    .filter(|o| rules.hostile(player.team, o.team) && o.entity != entity)
                    .collect();
                let goal = if !enemies.is_empty() && rng.next_f32() < 0.45 {
                    let enemy = enemies[rng.index(enemies.len())];
                    (enemy.x, enemy.z)
                } else {
                    arena.lanes[rng.index(arena.lanes.len())]
                };
                brain.path = arena.nav.path((pose.x, pose.z), goal);
                brain.path_index = 0;
                brain.repath_in = rng.range(5.0, 10.0);
            }

            while let Some(&(wx, wz)) = brain.path.get(brain.path_index) {
                if (wx - pose.x).powi(2) + (wz - pose.z).powi(2) < 0.6 * 0.6 {
                    brain.path_index += 1;
                } else {
                    break;
                }
            }
            if let Some(&(wx, wz)) = brain.path.get(brain.path_index) {
                let want = yaw_to(wx - pose.x, wz - pose.z);
                cmd.yaw = turn_towards(pose.yaw, want, TURN_RATE * 1.4 * TICK_DT);
                cmd.pitch = pose.pitch * 0.9;
                cmd.forward = wrap_angle(want - cmd.yaw).abs() < 0.9;
            }

            // Anything that stops progress forces a new route.
            let moved = (pose.x - brain.last_x).abs() + (pose.z - brain.last_z).abs();
            brain.stuck_time = if cmd.forward && moved < 0.01 {
                brain.stuck_time + TICK_DT
            } else {
                0.0
            };
            if brain.stuck_time > 0.6 {
                brain.path.clear();
                brain.stuck_time = 0.0;
            }
        }
        brain.last_x = pose.x;
        brain.last_z = pose.z;

        inputs.queue.clear();
        for _ in 0..2 {
            inputs.queue.push_back(cmd);
        }
    }
}
