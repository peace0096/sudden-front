//! Server-authoritative game rules, run in order every tick.

use super::components::*;
use super::physics::{line_clear, ray_aabb, ray_world, step_move, step_vertical, sweep_ball, view_dir};
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
// A shield bearer's shield, in their own frame. The client's copy is in client/src/game/art/hitbox.ts.
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
        // Grenades are thrown, not reloaded, Infinite Ammo or not: a life has the ones it starts with.
        if weapon.kind == pb::WeaponKind::Grenade {
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

/// (zone, min, max) of a player's hitboxes, standing on `feet` (their height above the floor).
/// `pad` widens them, which keeps melee from demanding pixel-perfect aim.
fn hitboxes(x: f32, feet: f32, z: f32, pad: f32, crouching: bool, head_scale: f32) -> [(pb::HitZone, [f32; 3], [f32; 3]); 3] {
    let boxed = |hw: f32, y0: f32, y1: f32| {
        ([x - hw - pad, feet + y0, z - hw - pad], [x + hw + pad, feet + y1, z + hw + pad])
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

/// Distance along a ray to a player's shield, which stands in front of them on `feet`.
fn ray_shield(origin: [f32; 3], dir: [f32; 3], x: f32, feet: f32, z: f32, yaw: f32, crouching: bool) -> Option<f32> {
    // Into the player's frame, where they face -Z and stand at height 0.
    let (sin, cos) = yaw.sin_cos();
    let local = |dx: f32, dz: f32| (dx * cos - dz * sin, dx * sin + dz * cos);
    let (ox, oz) = local(origin[0] - x, origin[2] - z);
    let (dx, dz) = local(dir[0], dir[2]);
    let top = SHIELD_TOP - if crouching { CROUCH_DROP } else { 0.0 };
    ray_aabb(
        [ox, origin[1] - feet, oz],
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

/// Who set off a blast (fired the missile, threw the grenade), and with what.
#[derive(Clone, Copy)]
struct Blaster {
    owner: Entity,
    owner_id: u32,
    team: pb::Team,
    weapon: pb::WeaponKind,
}

/// Whether a missile in flight, or a blast, can touch or hurt `target`. Never its owner: a missile
/// starts out inside their hit range, nobody is hurt by their own blast, and in a free-for-all
/// their team is no protection.
fn blast_target(rules: &Rules, source: &Blaster, target: &TargetView) -> bool {
    target.hittable && rules.hostile(source.team, target.team) && target.entity != source.owner
}

/// Damages every enemy of the owner the blast at `center` reaches in the open: the weapon's damage
/// at the centre, falling off with distance to its splash radius.
fn blast(
    boxes: &[pb::MapBox],
    rules: &Rules,
    center: [f32; 3],
    source: &Blaster,
    targets: &[TargetView],
    hits: &mut Vec<Hit>,
) {
    let stats = weapons::stats(source.weapon);
    for target in targets.iter().filter(|s| blast_target(rules, source, s)) {
        let (x, feet, z, crouching, yaw) = (target.pose.x, target.pose.y, target.pose.z, target.pose.crouching, target.pose.yaw);
        let torso = [x, feet + if crouching { 0.7 } else { 1.2 }, z];
        let d = [torso[0] - center[0], torso[1] - center[1], torso[2] - center[2]];
        let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if distance >= stats.splash_radius || !line_clear(boxes, center, torso) {
            continue;
        }
        let falloff = 1.0 - SPLASH_FALLOFF * distance / stats.splash_radius;
        let to_torso = [d[0] / distance.max(1e-4), d[1] / distance.max(1e-4), d[2] / distance.max(1e-4)];
        let shield = target.shielded && ray_shield(center, to_torso, x, feet, z, yaw, crouching).is_some_and(|t| t < distance);
        hits.push(Hit {
            attacker: source.owner,
            attacker_id: source.owner_id,
            attacker_team: source.team,
            weapon: source.weapon,
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
    rules: &Rules,
    missiles: &mut Missiles,
    targets: &[TargetView],
    log: &mut EventLog,
    hits: &mut Vec<Hit>,
) {
    missiles.live.retain_mut(|missile| {
        let (pos, dir) = (missile.pos, missile.dir);
        let source = Blaster {
            owner: missile.owner,
            owner_id: missile.owner_id,
            team: missile.team,
            weapon: pb::WeaponKind::Missile,
        };
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
        for target in targets.iter().filter(|s| blast_target(rules, &source, s)) {
            let (x, feet, z) = (target.pose.x, target.pose.y, target.pose.z);
            let r = 0.28 + MISSILE_HIT_PAD;
            let top = feet + PLAYER_HEIGHT + MISSILE_HIT_PAD;
            if let Some(d) = ray_aabb(pos, dir, [x - r, feet, z - r], [x + r, top, z + r]) {
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
            blast(boxes, rules, missile.pos, &source, targets, hits);
            return false;
        }
        missile.life > 0.0
    });
}

/// Throws the grenade in hand along `aim`, lofted a little. The last one thrown, the weapon held
/// before it comes back, drawn by the time the throw's own `fire_interval` is up.
fn throw_grenade(
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
    let eye = if pose.crouching { CROUCH_EYE_HEIGHT } else { EYE_HEIGHT };
    // Within the thrower's own column, which no box reaches into, so it starts in the open.
    let (sin, cos) = aim.yaw.sin_cos();
    let pos = [pose.x - sin * THROW_REACH, pose.y + eye, pose.z - cos * THROW_REACH];
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
    log.push(Event::GrenadeThrown(pb::ThrowEvent { player_id: player.id }));
    if weapon.magazine == 0 {
        weapon.switch_to(weapon.slot_before_grenade());
    }
}

/// One tick of a grenade's flight: gravity, then the move, bouncing off whatever it touches. Off a
/// floor or box top slow enough it stays down and rolls, slowed by every touch, until it lies still.
pub fn roll_grenade(boxes: &[pb::MapBox], half_x: f32, half_z: f32, pos: &mut [f32; 3], vel: &mut [f32; 3]) {
    vel[1] -= GRENADE_GRAVITY * TICK_DT;
    let mut left = TICK_DT;
    for _ in 0..GRENADE_CONTACTS {
        let delta = vel.map(|v| v * left);
        let Some((t, axis, sign)) = sweep_ball(boxes, *pos, delta, GRENADE_RADIUS, half_x, half_z) else {
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
fn fly_grenades(
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
        roll_grenade(&map.boxes, map.half_x, map.half_z, &mut grenade.pos, &mut grenade.vel);
        true
    });
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn fire_weapons(
    arena: Res<Arena>,
    rules: Res<Rules>,
    mut state: ResMut<MatchState>,
    mut log: ResMut<EventLog>,
    mut rng: ResMut<GameRng>,
    mut missiles: ResMut<Missiles>,
    mut grenades: ResMut<Grenades>,
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
        // One throw at a time; like a shot, it ends spawn protection and Rest's quiet.
        if weapon.kind == pb::WeaponKind::Grenade {
            if weapon.cooldown <= COOLDOWN_SLACK && weapon.magazine > 0 {
                throw_grenade(&mut grenades, entity, player, pose, fire, &mut weapon, &mut log);
                vitals.protect_left = 0.0;
                perks.idle = 0.0;
            }
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
            let origin = [pose.x, pose.y + eye, pose.z];
            let rewind_tick = fire
                .render_tick
                .clamp(current_tick.saturating_sub(MAX_REWIND_TICKS), current_tick);
            let rewound = |target: &TargetView| {
                target.samples.at(rewind_tick).unwrap_or((
                    target.pose.x,
                    target.pose.y,
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
                    .filter(|s| s.hittable && rules.hostile(player.team, s.team) && s.entity != entity)
                {
                    let (x, feet, z, crouching, yaw) = rewound(target);
                    for (zone, min, max) in hitboxes(x, feet, z, pad, crouching, target.head_scale) {
                        if let Some(t) = ray_aabb(origin, dir, min, max) {
                            if t < best_t {
                                best_t = t;
                                hit_player = Some((target.entity, target.id, zone, false));
                            }
                        }
                    }
                    if target.shielded {
                        if let Some(t) = ray_shield(origin, dir, x, feet, z, yaw, crouching) {
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

    fly_missiles(&arena.map.boxes, &rules, &mut missiles, &world_view, &mut log, &mut hits);
    fly_grenades(&arena, &rules, &mut grenades, &world_view, &mut log, &mut hits);

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
        // Big Head mode: the King takes no harm. The hit stops at them (it was their body the shot
        // reached) and comes to nothing: no damage, no paralysis, nobody added to their attackers.
        if perks.king() {
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
        let victim_level = perks.level;
        let contributors = std::mem::take(&mut perks.damaged_by);
        let holders = contributors
            .iter()
            .filter(|&&e| players.get(e).is_ok_and(|p| p.8.has(pb::Augment::BigHead)))
            .count();
        if let Ok(mut victim) = players.get_mut(hit.victim) {
            let grown = victim.8.head_scale + augments::BIG_HEAD_STEP * holders as f32;
            victim.8.head_scale = grown.min(augments::BIG_HEAD_MAX);
        }
        // Big Head mode: a kill with the hammer is a cute shot, and one to the head of a player
        // whose head was smaller than normal and the smallest in the match (levels tied at the
        // bottom count) a dot shot. Then the victim's level drops, and their head shrinks with it.
        let special = if !rules.big_head {
            pb::SpecialKill::Unspecified
        } else if hit.weapon == pb::WeaponKind::Hammer {
            pb::SpecialKill::CuteShot
        } else if hit.zone == pb::HitZone::Head && victim_level < 0 && players.iter().all(|p| p.8.level >= victim_level) {
            pb::SpecialKill::DotShot
        } else {
            pb::SpecialKill::Unspecified
        };
        if rules.big_head {
            if let Ok(mut victim) = players.get_mut(hit.victim) {
                victim.8.set_level(big_head::after_death(victim_level));
            }
        }
        log.push(Event::Kill(pb::KillEvent {
            killer_id: hit.attacker_id,
            victim_id,
            headshot: hit.zone == pb::HitZone::Head,
            weapon: hit.weapon as i32,
            special: special as i32,
        }));
        if !rules.ffa {
            state.score_kill(hit.attacker_team);
        }
        // A free-for-all kill by a player who has left (with a missile in flight) counts for nobody.
        let Ok((_, _, _, _, mut weapon, mut vitals, mut stats, _, mut perks, mut loadout)) =
            players.get_mut(hit.attacker)
        else {
            continue;
        };
        stats.kills += 1;
        if rules.ffa && stats.kills >= state.kills_to_win && state.winner.is_none() {
            state.winner = Some(Winner::Player(hit.attacker_id));
        }
        // Big Head mode: a kill raises the level, from below 0 straight to big_head::COMEBACK_LEVEL.
        // Reaching the top crowns a living player at once, and one who is down (a grenade that went
        // off after they fell) as they come back. The King's own kills leave the level at the top.
        if rules.big_head && !perks.king() {
            let level = big_head::after_kill(perks.level);
            perks.set_level(level);
            if level == big_head::MAX_LEVEL && vitals.alive {
                big_head::crown(&mut perks, &mut vitals, &mut weapon, rules.max_hp);
            }
        }
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

/// Big Head mode: runs each King's reign down a tick. When it is over the player is back at
/// big_head::KING_END_LEVEL, the hammer gone, with a fresh set of their own weapons (full
/// ammunition, the primary in hand, as at a respawn).
pub fn update_kings(rules: Res<Rules>, mut players: Query<(&Player, &mut Perks, &mut Weapon, &Loadout)>) {
    if !rules.big_head {
        return;
    }
    for (player, mut perks, mut weapon, loadout) in &mut players {
        if !perks.king() {
            continue;
        }
        perks.king_ticks -= 1;
        if perks.king() {
            continue;
        }
        perks.set_level(big_head::KING_END_LEVEL);
        *weapon = issue_weapons(loadout.0, &perks.active, player.is_bot);
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
    let points: Vec<(f32, f32)> = spawns.iter().map(|(x, z)| (x * side, z * side)).collect();
    furthest_spawn(&points, enemies, rng)
}

/// Of `points`, the one furthest from the nearest of `enemies`, or any when there are none.
/// A free-for-all chooses from all its points this way, with everyone else as an enemy.
pub fn furthest_spawn(points: &[(f32, f32)], enemies: &[(f32, f32)], rng: &mut GameRng) -> (f32, f32) {
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

    for (player, mut pose, mut vitals, mut weapon, mut loadout, mut history, mut perks) in &mut players {
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
        perks.shield_hp = if perks.has(pb::Augment::Shield) { augments::SHIELD_HP } else { 0 };
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

pub fn record_history(state: Res<MatchState>, mut players: Query<(&Pose, &mut History)>) {
    for (pose, mut history) in &mut players {
        history.samples.push_back((state.tick, pose.x, pose.y, pose.z, pose.crouching, pose.yaw));
        while history.samples.len() > (MAX_REWIND_TICKS + 2) as usize {
            history.samples.pop_front();
        }
    }
}

pub fn advance_tick(mut state: ResMut<MatchState>) {
    state.tick += 1;
}
