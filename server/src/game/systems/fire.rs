//! Firing: shots and melee swings hit the first thing on their line; hits then land as damage and kills.

use super::grenade::{fly_grenades, throw_grenade};
use super::history::MAX_REWIND_TICKS;
use super::missile::{fly_missiles, MISSILE_LIFE};
use crate::game::components::*;
use crate::game::physics::{ray_aabb, ray_world, view_dir};
use crate::game::resources::*;
use crate::game::weapons::{self, Stats as WeaponStats};
use crate::game::*;
use crate::pb;
use crate::pb::game_event::Event;
use bevy_ecs::prelude::*;

const SHOT_RANGE: f32 = 200.0;

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

// A shield bearer's shield, in their own frame. The client's copy is in client/src/game/art/hitbox.ts.
const SHIELD_HALF_WIDTH: f32 = 0.45;

const SHIELD_NEAR: f32 = 0.55;

const SHIELD_FAR: f32 = 0.65;

const SHIELD_TOP: f32 = 1.75;

/// (zone, min, max) of a player's hitboxes, standing on `feet` (their height above the floor).
/// `pad` widens them, which keeps melee from demanding pixel-perfect aim.
fn hitboxes(
    x: f32,
    feet: f32,
    z: f32,
    pad: f32,
    crouching: bool,
    head_scale: f32,
) -> [(pb::HitZone, [f32; 3], [f32; 3]); 3] {
    let boxed = |hw: f32, y0: f32, y1: f32| {
        (
            [x - hw - pad, feet + y0, z - hw - pad],
            [x + hw + pad, feet + y1, z + hw + pad],
        )
    };
    let drop = if crouching { CROUCH_DROP } else { 0.0 };
    // A resized head still sits on the same neck.
    let head = boxed(
        0.125 * head_scale,
        1.5 - drop,
        1.5 + 0.26 * head_scale - drop,
    );
    let body = boxed(0.28, 0.85 - drop, 1.5 - drop);
    let legs = boxed(0.22, 0.0, 0.85 - drop);
    [
        (pb::HitZone::Head, head.0, head.1),
        (pb::HitZone::Body, body.0, body.1),
        (pb::HitZone::Legs, legs.0, legs.1),
    ]
}

/// Distance along a ray to a player's shield, which stands in front of them on `feet`.
pub(super) fn ray_shield(
    origin: [f32; 3],
    dir: [f32; 3],
    x: f32,
    feet: f32,
    z: f32,
    yaw: f32,
    crouching: bool,
) -> Option<f32> {
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

/// Half-angle in radians of the random cone a shot leaves in.
pub fn shot_cone(stats: &WeaponStats, scoped: bool, moving: bool, bloom: f32) -> f32 {
    if scoped && stats.has_scope {
        stats.scoped_spread_still
            + if moving {
                stats.scoped_spread_moving
            } else {
                0.0
            }
    } else {
        stats.spread_still + if moving { stats.spread_moving } else { 0.0 } + bloom
    }
}

pub(super) struct Hit {
    pub(super) attacker: Entity,
    pub(super) attacker_id: u32,
    pub(super) attacker_team: pb::Team,
    pub(super) weapon: pb::WeaponKind,
    pub(super) victim: Entity,
    pub(super) zone: pb::HitZone,
    pub(super) amount: i32,
    pub(super) shield: bool,
    pub(super) stun: f32,
}

pub(super) struct TargetView {
    pub(super) entity: Entity,
    pub(super) id: u32,
    pub(super) team: pb::Team,
    pub(super) pose: Pose,
    pub(super) samples: History,
    pub(super) hittable: bool,
    pub(super) head_scale: f32,
    pub(super) shielded: bool,
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
        .map(
            |(entity, player, pose, _, weapon, vitals, _, history, perks, _)| TargetView {
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
            },
        )
        .collect();

    let mut hits: Vec<Hit> = Vec::new();

    for (entity, player, pose, intent, mut weapon, mut vitals, _, _, mut perks, _) in &mut players {
        // A burst finishes by itself once started.
        let fire = intent.fire.or(if weapon.burst_left > 0 {
            weapon.burst_fire
        } else {
            None
        });
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
                throw_grenade(
                    &mut grenades,
                    entity,
                    player,
                    pose,
                    fire,
                    &mut weapon,
                    &mut log,
                );
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
        while weapon.cooldown <= COOLDOWN_SLACK
            && (melee || weapon.magazine > 0)
            && shots < MAX_SHOTS_PER_TICK
        {
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
                weapon.cooldown += if burst_on {
                    weapons::BURST_GAP
                } else {
                    weapons::BURST_RECOVERY
                };
            } else {
                weapon.cooldown += stats.fire_interval;
            }
            if weapon.magazine == 0 {
                weapon.burst_left = 0;
            }
            vitals.protect_left = 0.0;
            perks.idle = 0.0;

            let stance = if pose.crouching {
                CROUCH_SPREAD_FACTOR
            } else {
                1.0
            };
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

            let eye = if pose.crouching {
                CROUCH_EYE_HEIGHT
            } else {
                EYE_HEIGHT
            };
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
                let dir = view_dir(
                    fire.yaw + radius * angle.cos(),
                    fire.pitch + radius * angle.sin(),
                );

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
                for target in world_view.iter().filter(|s| {
                    s.hittable && rules.hostile(player.team, s.team) && s.entity != entity
                }) {
                    let (x, feet, z, crouching, yaw) = rewound(target);
                    for (zone, min, max) in hitboxes(x, feet, z, pad, crouching, target.head_scale)
                    {
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
                                hit_player =
                                    Some((target.entity, target.id, pb::HitZone::Body, true));
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

    fly_missiles(
        &arena.map.boxes,
        &rules,
        &mut missiles,
        &world_view,
        &mut log,
        &mut hits,
    );
    fly_grenades(
        &arena,
        &rules,
        &mut grenades,
        &world_view,
        &mut log,
        &mut hits,
    );

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
        } else if hit.zone == pb::HitZone::Head
            && victim_level < 0
            && players.iter().all(|p| p.8.level >= victim_level)
        {
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
