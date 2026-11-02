//! Server-authoritative game rules, run in order every tick.

use super::components::*;
use super::physics::{line_clear, ray_aabb, ray_world, step_move, view_dir};
use super::weapons::{self, Stats as WeaponStats};
use super::*;
use crate::pb;
use crate::pb::game_event::Event;
use bevy_ecs::prelude::*;

/// Commands a player may consume per tick (two at 60 Hz input / 30 Hz tick),
/// with a little slack for network jitter. Anything faster is a speed hack.
const CMDS_PER_TICK: u32 = 2;
const CMD_BUDGET_CAP: u32 = 8;
const MAX_QUEUED_CMDS: usize = 32;
/// How far back a shot may rewind targets.
const MAX_REWIND_TICKS: u32 = 15;
const SHOT_RANGE: f32 = 200.0;
const BLOOM_DECAY: f32 = 0.09;
/// Bloom only recovers this long after the next shot was due, so it builds up
/// over sustained fire however slowly a weapon adds to it.
const BLOOM_HOLD_EXTRA: f32 = 0.1;
/// How long the scope must be held before its accuracy applies.
pub const SCOPE_READY_TIME: f32 = 0.2;
/// Safety bound on shots per tick for the fastest weapons.
const MAX_SHOTS_PER_TICK: u32 = 3;
const MELEE_HITBOX_PAD: f32 = 0.25;
/// Rounding must not delay a shot by a tick: by then the trigger's next command
/// carries the kick of the client's shot, and the bullet would go there.
const COOLDOWN_SLACK: f32 = 0.002;
/// Share of a blast's damage lost by the edge of its radius.
const SPLASH_FALLOFF: f32 = 0.7;
pub const MISSILE_SPEED: f32 = 60.0;
const MISSILE_LIFE: f32 = 3.0;
/// How far from a player's body a missile still counts as hitting them.
const MISSILE_HIT_PAD: f32 = 0.8;
// A shield bearer's shield, in their own frame. Its visuals are in character.ts.
const SHIELD_HALF_WIDTH: f32 = 0.45;
const SHIELD_NEAR: f32 = 0.55;
const SHIELD_FAR: f32 = 0.65;
const SHIELD_TOP: f32 = 1.75;

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

            let yaw = if cmd.yaw.is_finite() { cmd.yaw } else { pose.yaw };
            let pitch = if cmd.pitch.is_finite() {
                cmd.pitch.clamp(-MAX_PITCH, MAX_PITCH)
            } else {
                pose.pitch
            };
            pose.yaw = yaw;
            pose.pitch = pitch;
            // Paralysis leaves only the view.
            if vitals.stun_left > 0.0 {
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
                step_move(&arena.map.boxes, &mut x, &mut z, dx, dz);
                pose.x = x;
                pose.z = z;
                intent.moving = true;
            }
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

fn load_rounds(weapon: &mut Weapon, wanted: u32, infinite: bool) {
    let moved = if infinite { wanted } else { wanted.min(weapon.reserve) };
    weapon.magazine += moved;
    if !infinite {
        weapon.reserve -= moved;
    }
}

pub fn update_weapons(
    mut log: ResMut<EventLog>,
    mut players: Query<(&Player, &mut Weapon, &Intent, &Vitals, &Perks)>,
) {
    for (player, mut weapon, intent, vitals, perks) in &mut players {
        let stats = weapons::stats(weapon.kind);
        // While the trigger is held the cooldown may go slightly negative, which
        // carries the remainder into the next shot. That is what lets fire
        // intervals that aren't a multiple of the tick length average correctly.
        if weapon.cooldown > 0.0 {
            weapon.cooldown -= TICK_DT;
        }
        if intent.fire.is_none() && weapon.burst_left == 0 {
            weapon.cooldown = weapon.cooldown.max(0.0);
        }
        if weapon.bloom_hold > 0.0 {
            weapon.bloom_hold -= TICK_DT;
        } else {
            weapon.bloom = (weapon.bloom - BLOOM_DECAY * TICK_DT).max(0.0);
        }
        if !vitals.alive || vitals.stun_left > 0.0 {
            continue;
        }

        let infinite = perks.has(pb::Augment::InfiniteAmmo);
        let reload_time = weapons::reload_time(stats, &perks.active);
        if weapon.reloading() {
            weapon.reload_left -= TICK_DT;
            if weapon.reload_left <= 0.0 {
                weapon.reload_left = 0.0;
                if stats.shell_reload {
                    load_rounds(&mut weapon, 1, infinite);
                    if weapon.magazine < stats.magazine && (infinite || weapon.reserve > 0) {
                        weapon.reload_left = reload_time;
                        log.push(Event::Reload(pb::ReloadEvent {
                            player_id: player.id,
                        }));
                    }
                } else {
                    let wanted = stats.magazine - weapon.magazine;
                    load_rounds(&mut weapon, wanted, infinite);
                }
            }
        } else {
            // An empty magazine reloads by itself; R reloads early.
            let wants_reload = intent.reload || weapon.magazine == 0;
            if wants_reload && weapon.magazine < stats.magazine && (infinite || weapon.reserve > 0) {
                log.push(Event::Reload(pb::ReloadEvent {
                    player_id: player.id,
                }));
                if reload_time <= 0.0 {
                    let wanted = stats.magazine - weapon.magazine;
                    load_rounds(&mut weapon, wanted, infinite);
                } else {
                    weapon.reload_left = reload_time;
                    weapon.cooldown = weapon.cooldown.max(0.0);
                }
            }
        }

        // The scope drops out while the bolt cycles and during a reload.
        if stats.has_scope && intent.aim && !weapon.reloading() && weapon.cooldown <= 0.0 {
            weapon.aim_time += TICK_DT;
        } else {
            weapon.aim_time = 0.0;
        }
    }
}

/// (zone, min, max) of a standing player's hitboxes relative to their feet.
/// `pad` widens them, which keeps melee from demanding pixel-perfect aim.
fn hitboxes(x: f32, z: f32, pad: f32, crouching: bool, head_scale: f32) -> [(pb::HitZone, [f32; 3], [f32; 3]); 3] {
    let boxed = |hw: f32, y0: f32, y1: f32| {
        ([x - hw - pad, y0, z - hw - pad], [x + hw + pad, y1, z + hw + pad])
    };
    let drop = if crouching { CROUCH_DROP } else { 0.0 };
    // A resized head still sits on the same neck.
    let head = boxed(0.125 * head_scale, 1.5 - drop, 1.5 + 0.26 * head_scale - drop);
    let body = boxed(0.28, 0.85 - drop, 1.5 - drop);
    let legs = boxed(0.22, 0.0, 0.85 - drop);
    [
        (pb::HitZone::Head, head.0, head.1),
        (pb::HitZone::Body, body.0, body.1),
        (pb::HitZone::Legs, legs.0, legs.1),
    ]
}

/// Distance along a ray to a player's shield, which stands in front of them.
fn ray_shield(origin: [f32; 3], dir: [f32; 3], x: f32, z: f32, yaw: f32, crouching: bool) -> Option<f32> {
    // Into the player's frame, where they face -Z.
    let (sin, cos) = yaw.sin_cos();
    let local = |dx: f32, dz: f32| (dx * cos - dz * sin, dx * sin + dz * cos);
    let (ox, oz) = local(origin[0] - x, origin[2] - z);
    let (dx, dz) = local(dir[0], dir[2]);
    let top = SHIELD_TOP - if crouching { CROUCH_DROP } else { 0.0 };
    ray_aabb(
        [ox, origin[1], oz],
        [dx, dir[1], dz],
        [-SHIELD_HALF_WIDTH, 0.0, -SHIELD_FAR],
        [SHIELD_HALF_WIDTH, top, -SHIELD_NEAR],
    )
}

/// The slot 1 weapon an augment comes with, which the player can't change.
pub fn forced_primary(active: &[pb::Augment]) -> Option<pb::WeaponKind> {
    active.contains(&pb::Augment::Missile).then_some(pb::WeaponKind::Missile)
}

/// A spawning player's weapons. Bots never change slot, so those whose augment
/// lives in slot 2 start with it in hand.
pub fn issue_weapons(primary: pb::WeaponKind, active: &[pb::Augment], is_bot: bool) -> Weapon {
    let mut weapon = Weapon::equipped(primary, active);
    if is_bot && (weapon.sidearm != pb::WeaponKind::Pistol || active.contains(&pb::Augment::Shield)) {
        weapon.switch_to(2);
        weapon.cooldown = 0.0;
    }
    weapon
}

/// Half-angle in radians of the random cone a shot leaves in.
pub fn shot_cone(stats: &WeaponStats, scoped: bool, moving: bool, bloom: f32) -> f32 {
    if scoped && stats.has_scope {
        stats.scoped_spread_still + if moving { stats.scoped_spread_moving } else { 0.0 }
    } else {
        stats.spread_still + if moving { stats.spread_moving } else { 0.0 } + bloom
    }
}

struct Hit {
    attacker: Entity,
    attacker_id: u32,
    attacker_team: pb::Team,
    weapon: pb::WeaponKind,
    victim: Entity,
    zone: pb::HitZone,
    amount: i32,
    shield: bool,
    stun: f32,
}

struct TargetView {
    entity: Entity,
    id: u32,
    team: pb::Team,
    pose: Pose,
    samples: History,
    hittable: bool,
    head_scale: f32,
    shielded: bool,
}

/// Damages every enemy of `owner` the blast at `center` reaches in the open.
fn blast(boxes: &[pb::MapBox], center: [f32; 3], missile: &Missile, targets: &[TargetView], hits: &mut Vec<Hit>) {
    let stats = weapons::stats(pb::WeaponKind::Missile);
    for target in targets.iter().filter(|s| s.hittable && s.team != missile.team) {
        let (x, z, crouching, yaw) = (target.pose.x, target.pose.z, target.pose.crouching, target.pose.yaw);
        let torso = [x, if crouching { 0.7 } else { 1.2 }, z];
        let d = [torso[0] - center[0], torso[1] - center[1], torso[2] - center[2]];
        let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if distance >= stats.splash_radius || !line_clear(boxes, center, torso) {
            continue;
        }
        let falloff = 1.0 - SPLASH_FALLOFF * distance / stats.splash_radius;
        let to_torso = [d[0] / distance.max(1e-4), d[1] / distance.max(1e-4), d[2] / distance.max(1e-4)];
        let shield = target.shielded && ray_shield(center, to_torso, x, z, yaw, crouching).is_some_and(|t| t < distance);
        hits.push(Hit {
            attacker: missile.owner,
            attacker_id: missile.owner_id,
            attacker_team: missile.team,
            weapon: pb::WeaponKind::Missile,
            victim: target.entity,
            zone: pb::HitZone::Body,
            amount: (stats.damage_body as f32 * falloff) as i32,
            shield,
            stun: 0.0,
        });
    }
}

/// Flies each missile one tick; whatever it touches sets it off.
fn fly_missiles(
    boxes: &[pb::MapBox],
    missiles: &mut Missiles,
    targets: &[TargetView],
    log: &mut EventLog,
    hits: &mut Vec<Hit>,
) {
    missiles.live.retain_mut(|missile| {
        let (pos, dir) = (missile.pos, missile.dir);
        let step = MISSILE_SPEED * TICK_DT;
        let mut t = step;
        let mut contact = false;
        if let Some(wall) = ray_world(boxes, pos, dir, step) {
            t = wall;
            contact = true;
        }
        if dir[1] < 0.0 && pos[1] / -dir[1] < t {
            t = pos[1] / -dir[1];
            contact = true;
        }
        for target in targets.iter().filter(|s| s.hittable && s.team != missile.team) {
            let (x, z) = (target.pose.x, target.pose.z);
            let r = 0.28 + MISSILE_HIT_PAD;
            let top = PLAYER_HEIGHT + MISSILE_HIT_PAD;
            if let Some(d) = ray_aabb(pos, dir, [x - r, 0.0, z - r], [x + r, top, z + r]) {
                if d < t {
                    t = d;
                    contact = true;
                }
            }
        }
        // Just short of the surface, so the blast isn't on its far side.
        let t = if contact { (t - 0.05).max(0.0) } else { t };
        missile.pos = [pos[0] + dir[0] * t, pos[1] + dir[1] * t, pos[2] + dir[2] * t];
        missile.life -= TICK_DT;
        if contact {
            let [x, y, z] = missile.pos;
            log.push(Event::Explosion(pb::ExplosionEvent { x, y, z }));
            blast(boxes, missile.pos, missile, targets, hits);
            return false;
        }
        missile.life > 0.0
    });
}

#[allow(clippy::type_complexity)]
pub fn fire_weapons(
    arena: Res<Arena>,
    rules: Res<Rules>,
    mut state: ResMut<MatchState>,
    mut log: ResMut<EventLog>,
    mut rng: ResMut<GameRng>,
    mut missiles: ResMut<Missiles>,
    mut players: Query<(
        Entity,
        &Player,
        &Pose,
        &Intent,
        &mut Weapon,
        &mut Vitals,
        &mut Stats,
        &History,
        &mut Perks,
        &mut Loadout,
    )>,
) {
    let current_tick = state.tick;

    // Copied up front because shooters mutate their weapons while reading targets.
    let world_view: Vec<TargetView> = players
        .iter()
        .map(|(entity, player, pose, _, weapon, vitals, _, history, perks, _)| TargetView {
            entity,
            id: player.id,
            team: player.team,
            pose: *pose,
            samples: History {
                samples: history.samples.clone(),
            },
            hittable: vitals.alive && vitals.protect_left <= 0.0,
            head_scale: perks.head_scale,
            // The shield comes out with the pistol.
            shielded: perks.shield_hp > 0 && weapon.kind == pb::WeaponKind::Pistol,
        })
        .collect();

    let mut hits: Vec<Hit> = Vec::new();

    for (entity, player, pose, intent, mut weapon, mut vitals, _, _, mut perks, _) in &mut players {
        // A burst finishes by itself once started.
        let fire = intent.fire.or(if weapon.burst_left > 0 { weapon.burst_fire } else { None });
        let Some(fire) = fire else {
            continue;
        };
        if vitals.stun_left > 0.0 {
            weapon.burst_left = 0;
            continue;
        }
        if !vitals.alive {
            continue;
        }
        let stats = weapons::stats(weapon.kind);
        // A shell-by-shell reload gives way to the trigger once there's a round to fire.
        let can_interrupt = stats.shell_reload && weapon.magazine > 0;
        if weapon.reloading() && !can_interrupt {
            continue;
        }
        let scoped = intent.aim && weapon.aim_time >= SCOPE_READY_TIME;

        let melee = stats.melee_range > 0.0;
        let range = if melee { stats.melee_range } else { SHOT_RANGE };
        let pad = if melee { MELEE_HITBOX_PAD } else { 0.0 };
        let bursts = weapon.kind == pb::WeaponKind::PistolPlus;
        let no_recoil = perks.has(pb::Augment::NoRecoil);

        let mut shots = 0;
        while weapon.cooldown <= COOLDOWN_SLACK && (melee || weapon.magazine > 0) && shots < MAX_SHOTS_PER_TICK {
            shots += 1;
            weapon.reload_left = 0.0;
            if !melee {
                weapon.magazine -= 1;
            }
            if bursts {
                if weapon.burst_left == 0 {
                    weapon.burst_left = weapons::BURST_ROUNDS - 1;
                    weapon.burst_fire = Some(fire);
                } else {
                    weapon.burst_left -= 1;
                }
                let burst_on = weapon.burst_left > 0 && weapon.magazine > 0;
                weapon.cooldown += if burst_on { weapons::BURST_GAP } else { weapons::BURST_RECOVERY };
            } else {
                weapon.cooldown += stats.fire_interval;
            }
            if weapon.magazine == 0 {
                weapon.burst_left = 0;
            }
            vitals.protect_left = 0.0;
            perks.idle = 0.0;

            let stance = if pose.crouching { CROUCH_SPREAD_FACTOR } else { 1.0 };
            // A shotgun's pattern is what it is, not recoil.
            let cone = if no_recoil && stats.pellets == 0 {
                0.0
            } else {
                shot_cone(stats, scoped, intent.moving, weapon.bloom) * stance
            };
            if !no_recoil {
                weapon.bloom = (weapon.bloom + stats.bloom_per_shot).min(stats.bloom_max);
                weapon.bloom_hold = stats.fire_interval + BLOOM_HOLD_EXTRA;
            }

            let eye = if pose.crouching { CROUCH_EYE_HEIGHT } else { EYE_HEIGHT };
            let origin = [pose.x, eye, pose.z];
            let rewind_tick = fire
                .render_tick
                .clamp(current_tick.saturating_sub(MAX_REWIND_TICKS), current_tick);
            let rewound = |target: &TargetView| {
                target.samples.at(rewind_tick).unwrap_or((
                    target.pose.x,
                    target.pose.z,
                    target.pose.crouching,
                    target.pose.yaw,
                ))
            };

            for pellet in 0..stats.pellets.max(1) {
                // Random cone around the aim direction.
                let angle = rng.range(0.0, std::f32::consts::TAU);
                let radius = cone * rng.next_f32().sqrt();
                let dir = view_dir(fire.yaw + radius * angle.cos(), fire.pitch + radius * angle.sin());

                if stats.splash_radius > 0.0 {
                    missiles.next_id += 1;
                    let id = missiles.next_id;
                    missiles.live.push(Missile {
                        id,
                        owner: entity,
                        owner_id: player.id,
                        team: player.team,
                        pos: origin,
                        dir,
                        life: MISSILE_LIFE,
                    });
                    log.push(Event::Shot(pb::ShotEvent {
                        shooter_id: player.id,
                        from_x: origin[0],
                        from_y: origin[1],
                        from_z: origin[2],
                        to_x: origin[0],
                        to_y: origin[1],
                        to_z: origin[2],
                        explosive: true,
                        ..Default::default()
                    }));
                    continue;
                }

                let world_t = ray_world(&arena.map.boxes, origin, dir, range);
                let mut best_t = world_t.unwrap_or(range);
                let mut hit_player = None;

                // Targets are rewound to what this client was rendering when it fired.
                for target in world_view
                    .iter()
                    .filter(|s| s.hittable && s.team != player.team && s.entity != entity)
                {
                    let (x, z, crouching, yaw) = rewound(target);
                    for (zone, min, max) in hitboxes(x, z, pad, crouching, target.head_scale) {
                        if let Some(t) = ray_aabb(origin, dir, min, max) {
                            if t < best_t {
                                best_t = t;
                                hit_player = Some((target.entity, target.id, zone, false));
                            }
                        }
                    }
                    if target.shielded {
                        if let Some(t) = ray_shield(origin, dir, x, z, yaw, crouching) {
                            if t < best_t {
                                best_t = t;
                                hit_player = Some((target.entity, target.id, pb::HitZone::Body, true));
                            }
                        }
                    }
                }

                log.push(Event::Shot(pb::ShotEvent {
                    shooter_id: player.id,
                    from_x: origin[0],
                    from_y: origin[1],
                    from_z: origin[2],
                    to_x: origin[0] + dir[0] * best_t,
                    to_y: origin[1] + dir[1] * best_t,
                    to_z: origin[2] + dir[2] * best_t,
                    hit_player_id: hit_player.map(|h| h.1).unwrap_or(0),
                    hit_world: hit_player.is_none() && world_t.is_some(),
                    melee,
                    pellet: pellet > 0,
                    explosive: false,
                }));

                if let Some((victim, _, zone, shield)) = hit_player {
                    hits.push(Hit {
                        attacker: entity,
                        attacker_id: player.id,
                        attacker_team: player.team,
                        weapon: weapon.kind,
                        victim,
                        zone,
                        amount: weapons::damage(weapon.kind, rules.mode, zone),
                        shield,
                        stun: stats.stun,
                    });
                }
            }
        }
    }

    fly_missiles(&arena.map.boxes, &mut missiles, &world_view, &mut log, &mut hits);

    for hit in hits {
        if state.winner.is_some() {
            break;
        }
        let Ok((_, victim_player, _, _, victim_weapon, mut vitals, mut stats, _, mut perks, _)) =
            players.get_mut(hit.victim)
        else {
            continue;
        };
        if !vitals.alive {
            continue;
        }
        perks.idle = 0.0;
        if !perks.damaged_by.contains(&hit.attacker) {
            perks.damaged_by.push(hit.attacker);
        }
        let victim_id = victim_player.id;
        // Once earlier pellets break it, what's left reaches the player.
        if hit.shield && perks.shield_hp > 0 {
            perks.shield_hp = (perks.shield_hp - hit.amount).max(0);
            log.push(Event::Damage(pb::DamageEvent {
                attacker_id: hit.attacker_id,
                victim_id,
                amount: hit.amount,
                zone: hit.zone as i32,
                shield: true,
            }));
            continue;
        }
        if hit.stun > 0.0 {
            vitals.stun_left = hit.stun;
        }
        vitals.hp -= hit.amount;
        log.push(Event::Damage(pb::DamageEvent {
            attacker_id: hit.attacker_id,
            victim_id,
            amount: hit.amount,
            zone: hit.zone as i32,
            shield: false,
        }));
        if vitals.hp > 0 {
            continue;
        }
        vitals.hp = 0;
        vitals.alive = false;
        vitals.stun_left = 0.0;
        vitals.respawn_left = RESPAWN_TIME;
        stats.deaths += 1;
        perks.shield_hp = 0;
        let victim_primary = victim_weapon.primary;
        let contributors = std::mem::take(&mut perks.damaged_by);
        let holders = contributors
            .iter()
            .filter(|&&e| players.get(e).is_ok_and(|p| p.8.has(pb::Augment::BigHead)))
            .count();
        if let Ok(mut victim) = players.get_mut(hit.victim) {
            victim.8.head_scale += augments::BIG_HEAD_STEP * holders as f32;
        }
        log.push(Event::Kill(pb::KillEvent {
            killer_id: hit.attacker_id,
            victim_id,
            headshot: hit.zone == pb::HitZone::Head,
            weapon: hit.weapon as i32,
        }));
        state.score_kill(hit.attacker_team);
        let Ok((_, _, _, _, mut weapon, mut vitals, mut stats, _, perks, mut loadout)) =
            players.get_mut(hit.attacker)
        else {
            continue;
        };
        stats.kills += 1;
        if !vitals.alive {
            continue;
        }
        if perks.has(pb::Augment::SoulDrain) {
            let cap = augments::max_hp(rules.max_hp, &perks.active);
            vitals.hp = (vitals.hp + augments::SOUL_DRAIN_HEAL).min(cap);
        }
        if perks.has(pb::Augment::Possession) {
            let taken = weapons::base_primary(victim_primary);
            vitals.hp = augments::POSSESSION_HP;
            loadout.0 = taken;
            weapon.set_primary(taken);
        }
    }
}

/// Rest: heals a player crouching out of combat.
pub fn update_perks(rules: Res<Rules>, mut players: Query<(&Pose, &mut Vitals, &mut Perks)>) {
    for (pose, mut vitals, mut perks) in &mut players {
        if !perks.has(pb::Augment::Rest) || !vitals.alive {
            continue;
        }
        perks.idle += TICK_DT;
        if perks.idle < augments::REST_DELAY {
            continue;
        }
        let cap = augments::max_hp(rules.max_hp, &perks.active);
        if !pose.crouching || vitals.hp >= cap {
            continue;
        }
        perks.regen += augments::REST_HEAL_PER_SECOND * TICK_DT;
        let whole = perks.regen as i32;
        perks.regen -= whole as f32;
        vitals.hp = (vitals.hp + whole).min(cap);
    }
}

/// The spawn point on the player's side that is furthest from any living enemy.
pub fn choose_spawn(
    spawns: &[(f32, f32)],
    team: pb::Team,
    enemies: &[(f32, f32)],
    rng: &mut GameRng,
) -> (f32, f32) {
    let side = if team == pb::Team::Red { 1.0 } else { -1.0 };
    if enemies.is_empty() {
        let (x, z) = spawns[rng.index(spawns.len())];
        return (x * side, z * side);
    }
    let mut best = (spawns[0].0 * side, spawns[0].1 * side);
    let mut best_score = f32::MIN;
    for (x, z) in spawns.iter().map(|(x, z)| (x * side, z * side)) {
        let nearest = enemies
            .iter()
            .map(|(ex, ez)| (ex - x).powi(2) + (ez - z).powi(2))
            .fold(f32::MAX, f32::min);
        // Jitter so one team doesn't reuse a single point.
        let score = nearest.sqrt() + rng.range(0.0, 8.0);
        if score > best_score {
            best_score = score;
            best = (x, z);
        }
    }
    best
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
    let living: Vec<(pb::Team, f32, f32)> = players
        .iter()
        .filter(|(_, _, vitals, _, _, _, _)| vitals.alive)
        .map(|(player, pose, _, _, _, _, _)| (player.team, pose.x, pose.z))
        .collect();

    for (player, mut pose, mut vitals, mut weapon, mut loadout, mut history, mut perks) in &mut players {
        if vitals.alive {
            vitals.protect_left = (vitals.protect_left - TICK_DT).max(0.0);
            vitals.stun_left = (vitals.stun_left - TICK_DT).max(0.0);
            continue;
        }
        vitals.respawn_left -= TICK_DT;
        if vitals.respawn_left > 0.0 {
            continue;
        }
        let enemies: Vec<(f32, f32)> = living
            .iter()
            .filter(|(team, _, _)| *team != player.team)
            .map(|(_, x, z)| (*x, *z))
            .collect();
        let (x, z) = choose_spawn(&arena.spawns, player.team, &enemies, &mut rng);
        *pose = Pose {
            x,
            z,
            yaw: spawn_yaw(player.team),
            pitch: 0.0,
            crouching: false,
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
        perks.shield_hp = if perks.has(pb::Augment::Shield) { augments::SHIELD_HP } else { 0 };
        // Don't let a rewound shot hit where the player died.
        history.samples.clear();
        log.push(Event::Respawn(pb::RespawnEvent {
            player_id: player.id,
        }));
    }
}

pub fn record_history(state: Res<MatchState>, mut players: Query<(&Pose, &mut History)>) {
    for (pose, mut history) in &mut players {
        history.samples.push_back((state.tick, pose.x, pose.z, pose.crouching, pose.yaw));
        while history.samples.len() > (MAX_REWIND_TICKS + 2) as usize {
            history.samples.pop_front();
        }
    }
}

pub fn advance_tick(mut state: ResMut<MatchState>) {
    state.tick += 1;
}
