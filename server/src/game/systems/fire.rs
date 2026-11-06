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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn a_player_in_the_air_is_hit_where_they_are() {
        // The victim 4 m away hangs 0.6 m up: a shot at that height plus a standing head's middle
        // (1.63 m) is a headshot, one at a standing head misses over their shoulder.
        let shoot = |raised: f32, aim_height: f32| -> i32 {
            let mut game = duel();
            let yaw = -std::f32::consts::FRAC_PI_2;
            place(&mut game, SHOOTER, -46.0, 0.0, yaw);
            place(&mut game, VICTIM, -42.0, 0.0, 0.0);
            game.world
                .get_mut::<Pose>(game.entities[&VICTIM])
                .unwrap()
                .y = raised;
            game.tick(); // History holds the raised victim.
            let shot = pb::InputCmd {
                fire: true,
                yaw,
                pitch: (aim_height - EYE_HEIGHT).atan2(4.0),
                ..cmd(1, tick_now(&game))
            };
            game.push_inputs(SHOOTER, vec![shot]);
            game.tick();
            100 - hp(&game, VICTIM)
        };
        assert_eq!(shoot(0.6, 0.6 + 1.63), weapons::stats(RIFLE).damage_head);
        assert_eq!(shoot(0.6, 0.6 + 0.4), weapons::stats(RIFLE).damage_legs);
        assert_eq!(shoot(0.0, 0.6 + 1.63), 0, "over a standing player's head");
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
        let shot = game
            .shared_snapshot()
            .events
            .iter()
            .any(|e| matches!(e.event, Some(pb::game_event::Event::Shot(_))));
        assert!(!shot);
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
    fn sniper_body_shot_kills_in_one_hit() {
        let mut game = duel();
        game.select_weapon(SHOOTER, pb::WeaponKind::Sniper);
        let shot = line_up(&mut game, -0.1);
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        assert!(
            !game
                .world
                .get::<Vitals>(game.entities[&VICTIM])
                .unwrap()
                .alive
        );
    }

    #[test]
    fn knife_hits_in_reach_for_high_damage_without_ammo() {
        let (mut game, swing) = melee_setup(duel(), 1.5);
        game.push_inputs(SHOOTER, vec![swing]);
        game.tick();
        let victim = game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap();
        assert_eq!(
            victim.hp,
            MAX_HP - weapons::stats(pb::WeaponKind::Knife).damage_body
        );
        assert_eq!(weapon(&game, SHOOTER).0, 0);
    }

    #[test]
    fn knife_cannot_reach_far_targets() {
        let (mut game, swing) = melee_setup(duel(), 4.0);
        game.push_inputs(SHOOTER, vec![swing]);
        game.tick();
        assert_eq!(
            game.world.get::<Vitals>(game.entities[&VICTIM]).unwrap().hp,
            MAX_HP
        );
        let melee_shot = game.shared_snapshot().events.iter().any(|e| {
        matches!(&e.event, Some(pb::game_event::Event::Shot(s)) if s.melee && s.hit_player_id == 0)
    });
        assert!(
            melee_shot,
            "the swing still happens and is reported as melee"
        );
    }

    #[test]
    fn knife_never_triggers_a_reload_or_empty_state() {
        let (mut game, _) = melee_setup(duel(), 1.5);
        for _ in 0..10 {
            game.tick();
        }
        assert!(!weapon(&game, SHOOTER).2);
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
        assert_eq!(
            damage_at((1.63f32 - EYE_HEIGHT).atan2(4.0)),
            rifle.damage_head
        );
    }

    #[test]
    fn crouching_tightens_real_shots() {
        let (standing, crouched) = (mean_spread(false), mean_spread(true));
        assert!(
            crouched < standing * 0.7,
            "standing {standing}, crouched {crouched}"
        );
    }

    #[test]
    fn crouched_hitboxes_drop_with_the_player() {
        let rifle = weapons::stats(RIFLE);
        assert_eq!(
            shot_at_crouched(false, EYE_HEIGHT),
            0,
            "passes over a crouched player"
        );
        assert_eq!(shot_at_crouched(false, 1.13), rifle.damage_head);
        assert_eq!(shot_at_crouched(false, 0.7), rifle.damage_body);
        assert_eq!(shot_at_crouched(false, 0.2), rifle.damage_legs);
        assert_eq!(
            shot_at_crouched(true, CROUCH_EYE_HEIGHT),
            rifle.damage_body,
            "level from a crouch"
        );
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
    fn sustained_fire_really_spreads_shots() {
        let angles = sustained_fire_angles();
        assert!(angles.len() >= 25, "fired {}", angles.len());
        let mean = |a: &[f32]| a.iter().sum::<f32>() / a.len() as f32;
        let (early, late) = (mean(&angles[..3]), mean(&angles[12..]));
        assert!(late > early * 3.0, "early {early}, late {late}");
        assert!(late > weapons::stats(RIFLE).bloom_max * 0.4, "late {late}");
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
        assert!(
            aim_errors(&mut rifle).iter().any(|e| *e > 1e-3),
            "a rifle still spreads"
        );
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
                game.push_inputs(
                    SHOOTER,
                    vec![pb::InputCmd {
                        fire: trigger,
                        yaw,
                        ..cmd(seq, tick_now(&game))
                    }],
                );
                game.tick();
                fired += shot_events(&game.shared_snapshot().events);
            }
            fired
        };
        assert_eq!(
            shots_over(Aug::PistolUpgrade, 15, false),
            3,
            "one pull, one burst"
        );
        assert_eq!(
            shots_over(Aug::InfiniteAmmo, 15, false),
            1,
            "an ordinary pistol fires once"
        );
        let (fast, slow) = (
            shots_over(Aug::PistolUpgrade, 30, true),
            shots_over(Aug::InfiniteAmmo, 30, true),
        );
        assert!(fast >= slow * 2, "upgraded {fast}, ordinary {slow}");
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
        assert!(
            dealt >= weapons::stats(RIFLE).damage_body * 2,
            "dealt {dealt} at 4 m"
        );
    }

    #[test]
    fn shield_soaks_frontal_fire_but_not_shots_from_behind() {
        // The shooter is on the -X side, firing along +X.
        let facing_shooter = std::f32::consts::FRAC_PI_2;
        let mut game = shielded_target(facing_shooter);
        game.tick();
        let v = game.entities[&VICTIM];
        let body = weapons::stats(RIFLE).damage_body;
        assert_eq!(
            game.world.get::<Perks>(v).unwrap().shield_hp,
            augments::SHIELD_HP - body
        );
        assert_eq!(hp(&game, VICTIM), 150);

        let mut game = shielded_target(-facing_shooter);
        game.tick();
        let v = game.entities[&VICTIM];
        assert_eq!(
            game.world.get::<Perks>(v).unwrap().shield_hp,
            augments::SHIELD_HP,
            "behind the shield"
        );
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
        assert_eq!(
            game.world.get::<Perks>(v).unwrap().shield_hp,
            augments::SHIELD_HP
        );
        assert_eq!(hp(&game, VICTIM), 150 - weapons::stats(RIFLE).damage_body);
    }

    #[test]
    fn magnum_is_a_hard_hitting_sidearm() {
        let mut game = with_augment(Aug::Magnum);
        draw_sidearm(&mut game, SHOOTER);
        assert_eq!(held(&game, SHOOTER), Kind::Magnum);
        assert_eq!(weapon(&game, SHOOTER), (6, 30, false));
        shoot_once(&mut game, 1);
        assert_eq!(
            150 - hp(&game, VICTIM),
            weapons::stats(Kind::Magnum).damage_body
        );
        let magnum = weapons::stats(Kind::Magnum);
        assert!(magnum.move_speed > weapons::stats(RIFLE).move_speed);
        assert!(magnum.fire_interval > weapons::stats(Kind::Pistol).fire_interval);
    }

    #[test]
    fn big_head_grows_the_victims_head_additively_for_every_contributor() {
        let mut game = with_augment(Aug::BigHead);
        let v = game.entities[&VICTIM];
        let scale = |game: &Match| game.world.get::<Perks>(v).unwrap().head_scale;
        assert_eq!(scale(&game), 1.0);
        kill_victim(&mut game, 1);
        assert!((scale(&game) - (1.0 + augments::BIG_HEAD_STEP)).abs() < 1e-4);
        kill_victim(&mut game, 2);
        assert!(
            (scale(&game) - (1.0 + 2.0 * augments::BIG_HEAD_STEP)).abs() < 1e-4,
            "adds up over deaths"
        );
        // It stops growing at the cap, however many more deaths follow.
        for seq in 3..20 {
            kill_victim(&mut game, seq);
        }
        assert_eq!(scale(&game), augments::BIG_HEAD_MAX);

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
        assert_eq!(
            game.world.get::<Perks>(v).unwrap().head_scale,
            1.0,
            "the holder didn't help"
        );
        let helper = game.entities[&77];
        game.world
            .get_mut::<Perks>(v)
            .unwrap()
            .damaged_by
            .push(helper);
        kill_victim(&mut game, 2);
        assert!(
            (game.world.get::<Perks>(v).unwrap().head_scale - (1.0 + augments::BIG_HEAD_STEP))
                .abs()
                < 1e-4
        );
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
        game.world
            .get_mut::<Weapon>(v)
            .unwrap()
            .set_primary(Kind::Sniper);
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
            game.world
                .get_mut::<Weapon>(v)
                .unwrap()
                .set_primary(Kind::AkimboSmg);
            kill_victim(&mut game, 2 + i);
            assert_eq!(hp(&game, SHOOTER), 150, "no decay");
        }
        assert_eq!(
            held(&game, SHOOTER),
            Kind::Smg,
            "a taken akimbo is a plain SMG"
        );
    }

    #[test]
    fn no_recoil_keeps_sustained_fire_tight() {
        let mut game = with_augment(Aug::NoRecoil);
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -46.0, 0.0, yaw);
        place(&mut game, VICTIM, -46.0, -8.0, 0.0);
        for seq in 1..=45 {
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
        }
        let e = game.entities[&SHOOTER];
        assert!(weapon(&game, SHOOTER).0 < 25, "it did fire");
        assert_eq!(game.world.get::<Weapon>(e).unwrap().bloom, 0.0);
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
            game.push_inputs(
                VICTIM,
                vec![pb::InputCmd {
                    forward: true,
                    fire: true,
                    slot: 2,
                    yaw: 1.0,
                    ..cmd(seq, tick_now(&game))
                }],
            );
            game.tick();
        }
        let pose = *game.world.get::<Pose>(v).unwrap();
        assert_eq!(pose.x, start, "can't move");
        assert_eq!(pose.yaw, 1.0, "can look around");
        assert_eq!(weapon(&game, VICTIM).0, magazine, "can't fire");
        assert_eq!(held(&game, VICTIM), RIFLE, "can't switch");

        for _ in 0..(TICK_RATE + 5) {
            seq += 1;
            game.push_inputs(
                VICTIM,
                vec![pb::InputCmd {
                    forward: true,
                    yaw: 1.0,
                    ..cmd(seq, tick_now(&game))
                }],
            );
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
    fn in_a_free_for_all_anyone_can_shoot_anyone_and_no_team_scores() {
        let mut game = ffa_duel();
        let shot = line_up(&mut game, 0.03);
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        assert!(
            !game
                .world
                .get::<Vitals>(game.entities[&VICTIM])
                .unwrap()
                .alive
        );
        let snapshot = game.shared_snapshot();
        assert_eq!((snapshot.red_score, snapshot.blue_score), (0, 0));
        let kills: Vec<(u32, u32)> = snapshot.players.iter().map(|p| (p.id, p.kills)).collect();
        assert_eq!(kills, [(SHOOTER, 1), (VICTIM, 0)]);
        assert_eq!(game.winner(), None);
    }

    #[test]
    fn in_a_free_for_all_the_knife_cuts_anyone() {
        let (mut game, swing) = melee_setup(ffa_duel(), 1.5);
        game.push_inputs(SHOOTER, vec![swing]);
        game.tick();
        assert_eq!(
            hp(&game, VICTIM),
            MAX_HP - weapons::stats(Kind::Knife).damage_body
        );
    }

    #[test]
    fn the_first_player_to_the_target_wins_a_free_for_all() {
        let mut game = ffa_duel();
        game.set_kills_to_win(10);
        game.world
            .get_mut::<Stats>(game.entities[&SHOOTER])
            .unwrap()
            .kills = 9;
        let shot = line_up(&mut game, 0.03);
        assert_eq!(game.winner(), None);
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        assert_eq!(game.winner(), Some(Winner::Player(SHOOTER)));
        let end = game.end_message(8.0);
        assert_eq!((end.winner, end.winner_id), (NO_TEAM as i32, SHOOTER));
        assert_eq!((end.red_score, end.blue_score), (0, 0));
        assert_eq!(
            (end.scoreboard[0].id, end.scoreboard[0].kills),
            (SHOOTER, 10)
        );
        assert!(end.scoreboard.iter().all(|s| s.team == NO_TEAM as i32));
        // It is over: the clock no longer runs.
        let frozen = tick_now(&game);
        game.tick();
        assert_eq!(tick_now(&game), frozen);
    }
}
