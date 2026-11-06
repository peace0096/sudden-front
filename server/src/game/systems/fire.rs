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

/// Every player, as the fire system reads and writes them.
type Shooters<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Player,
        &'static Pose,
        &'static Intent,
        &'static mut Weapon,
        &'static mut Vitals,
        &'static mut Stats,
        &'static History,
        &'static mut Perks,
        &'static mut Loadout,
    ),
>;

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

/// The cone a shot actually leaves in: [`shot_cone`], narrowed by crouching. No Recoil leaves
/// none, except a shotgun's pattern, which is what it is and not recoil.
fn spread(
    stats: &WeaponStats,
    scoped: bool,
    moving: bool,
    bloom: f32,
    crouching: bool,
    no_recoil: bool,
) -> f32 {
    if no_recoil && stats.pellets == 0 {
        return 0.0;
    }
    let stance = if crouching { CROUCH_SPREAD_FACTOR } else { 1.0 };
    shot_cone(stats, scoped, moving, bloom) * stance
}

/// Where the weapon fires this tick: where the trigger aims, or, once it is let go, where a
/// started burst keeps on firing.
fn trigger(fire: Option<FireIntent>, weapon: &Weapon) -> Option<FireIntent> {
    fire.or(if weapon.burst_left > 0 {
        weapon.burst_fire
    } else {
        None
    })
}

/// Whether a reload keeps the weapon from firing. A shell-by-shell reload gives way to the trigger
/// once there is a round to fire.
fn reload_blocks(weapon: &Weapon, stats: &WeaponStats) -> bool {
    weapon.reloading() && !(stats.shell_reload && weapon.magazine > 0)
}

/// Chambers a shot: a round leaves the magazine (a melee swing spends none), a reload under way is
/// abandoned, and the cooldown grows by the fire interval. Upgraded Pistol fires bursts instead:
/// a trigger pull starts one, its rounds come a short gap apart, and a longer recovery follows the
/// last. An empty magazine ends a burst.
fn chamber(weapon: &mut Weapon, stats: &WeaponStats, melee: bool, bursts: bool, fire: FireIntent) {
    weapon.reload_left = 0.0;
    if !melee {
        weapon.magazine = weapon.magazine.saturating_sub(1);
    }
    if bursts {
        if weapon.burst_left == 0 {
            weapon.burst_left = weapons::BURST_ROUNDS - 1;
            weapon.burst_fire = Some(fire);
        } else {
            weapon.burst_left = weapon.burst_left.saturating_sub(1);
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
}

/// A shot's kick: the bloom grows, up to the weapon's cap, and holds a little past the next shot.
fn kick(weapon: &mut Weapon, stats: &WeaponStats) {
    weapon.bloom = (weapon.bloom + stats.bloom_per_shot).min(stats.bloom_max);
    weapon.bloom_hold = stats.fire_interval + BLOOM_HOLD_EXTRA;
}

/// The tick a shot rewinds its targets to: the one the shooter's client was rendering, but no
/// further back than MAX_REWIND_TICKS and never ahead of now.
fn rewind_tick(render_tick: u32, current_tick: u32) -> u32 {
    render_tick.clamp(current_tick.saturating_sub(MAX_REWIND_TICKS), current_tick)
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

impl TargetView {
    /// Where the target stood at `tick`, as (x, feet, z, crouching, yaw): from its history, or
    /// where it is now when the history doesn't reach back that far.
    fn at(&self, tick: u32) -> (f32, f32, f32, bool, f32) {
        self.samples.at(tick).unwrap_or((
            self.pose.x,
            self.pose.y,
            self.pose.z,
            self.pose.crouching,
            self.pose.yaw,
        ))
    }
}

/// What a ray from `origin` along `dir` strikes first within `reach`, each target as it stood at
/// `tick`: `(distance, struck)`, where `struck` is the target and the zone, and `true` when it was
/// their shield. `pad` widens the hitboxes, for melee.
fn first_struck<'a>(
    origin: [f32; 3],
    dir: [f32; 3],
    reach: f32,
    targets: impl Iterator<Item = &'a TargetView>,
    tick: u32,
    pad: f32,
) -> (f32, Option<(&'a TargetView, pb::HitZone, bool)>) {
    let mut best_t = reach;
    let mut struck = None;
    for target in targets {
        let (x, feet, z, crouching, yaw) = target.at(tick);
        for (zone, min, max) in hitboxes(x, feet, z, pad, crouching, target.head_scale) {
            if let Some(t) = ray_aabb(origin, dir, min, max) {
                if t < best_t {
                    best_t = t;
                    struck = Some((target, zone, false));
                }
            }
        }
        if target.shielded {
            if let Some(t) = ray_shield(origin, dir, x, feet, z, yaw, crouching) {
                if t < best_t {
                    best_t = t;
                    struck = Some((target, pb::HitZone::Body, true));
                }
            }
        }
    }
    (best_t, struck)
}

/// What a hit did to its victim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// Their shield took it.
    Shield,
    Wounded,
    Killed,
}

/// Lands `hit` on its victim. A shield soaks it while the shield lasts; otherwise it hurts, may
/// paralyse, and kills at 0 HP. Any of those marks the attacker as one who hurt the victim, and
/// breaks Rest's quiet. `None` when the hit comes to nothing: the victim is already down, or is the
/// King (Big Head mode), whom nothing harms; the shot still stops at their body.
fn take_hit(vitals: &mut Vitals, perks: &mut Perks, hit: &Hit) -> Option<Outcome> {
    if !vitals.alive || perks.king() {
        return None;
    }
    perks.idle = 0.0;
    if !perks.damaged_by.contains(&hit.attacker) {
        perks.damaged_by.push(hit.attacker);
    }
    // Once earlier pellets break it, what's left reaches the player.
    if hit.shield && perks.shield_hp > 0 {
        perks.shield_hp = perks.shield_hp.saturating_sub(hit.amount).max(0);
        return Some(Outcome::Shield);
    }
    if hit.stun > 0.0 {
        vitals.stun_left = hit.stun;
    }
    vitals.hp = vitals.hp.saturating_sub(hit.amount);
    if vitals.hp > 0 {
        return Some(Outcome::Wounded);
    }
    vitals.hp = 0;
    vitals.alive = false;
    vitals.stun_left = 0.0;
    vitals.respawn_left = RESPAWN_TIME;
    perks.shield_hp = 0;
    Some(Outcome::Killed)
}

/// A victim's head after the kill: a step bigger for each of their attackers who held Big Head, up
/// to the cap.
fn grown_head(scale: f32, holders: usize) -> f32 {
    (scale + augments::BIG_HEAD_STEP * holders as f32).min(augments::BIG_HEAD_MAX)
}

/// Big Head mode's special kills. Any kill with the hammer is a cute shot. A headshot on a player
/// whose head is smaller than normal (below level 0) and the smallest in the match is a dot shot:
/// `lowest` is the lowest level anyone has, and a tie at the bottom counts.
fn special_kill(
    big_head: bool,
    weapon: pb::WeaponKind,
    zone: pb::HitZone,
    victim_level: i32,
    lowest: i32,
) -> pb::SpecialKill {
    if !big_head {
        pb::SpecialKill::Unspecified
    } else if weapon == pb::WeaponKind::Hammer {
        pb::SpecialKill::CuteShot
    } else if zone == pb::HitZone::Head && victim_level < 0 && lowest >= victim_level {
        pb::SpecialKill::DotShot
    } else {
        pb::SpecialKill::Unspecified
    }
}

/// What a kill earns its killer. Big Head mode: a level, from below 0 straight to
/// big_head::COMEBACK_LEVEL. Reaching the top crowns a living player at once (one who is down, after
/// a grenade that went off when they fell, is crowned as they come back), and the King's own kills
/// leave the level at the top. And while they live, Soul Drain heals them, and Possession hands them
/// the victim's primary and fresh health.
fn reward_killer(
    rules: &Rules,
    perks: &mut Perks,
    vitals: &mut Vitals,
    weapon: &mut Weapon,
    loadout: &mut Loadout,
    victim_primary: pb::WeaponKind,
) {
    if rules.big_head && !perks.king() {
        let level = big_head::after_kill(perks.level);
        perks.set_level(level);
        if level == big_head::MAX_LEVEL && vitals.alive {
            big_head::crown(perks, vitals, weapon, rules.max_hp);
        }
    }
    if !vitals.alive {
        return;
    }
    if perks.has(pb::Augment::SoulDrain) {
        let cap = augments::max_hp(rules.max_hp, &perks.active);
        vitals.hp = vitals.hp.saturating_add(augments::SOUL_DRAIN_HEAL).min(cap);
    }
    if perks.has(pb::Augment::Possession) {
        let taken = weapons::base_primary(victim_primary);
        vitals.hp = augments::POSSESSION_HP;
        loadout.0 = taken;
        weapon.set_primary(taken);
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system takes each resource it uses as a parameter"
)]
pub fn fire_weapons(
    arena: Res<Arena>,
    rules: Res<Rules>,
    mut state: ResMut<MatchState>,
    mut log: ResMut<EventLog>,
    mut rng: ResMut<GameRng>,
    mut missiles: ResMut<Missiles>,
    mut grenades: ResMut<Grenades>,
    mut players: Shooters,
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
        let Some(fire) = trigger(intent.fire, &weapon) else {
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
        if reload_blocks(&weapon, stats) {
            continue;
        }
        let scoped = intent.aim && weapon.aim_time >= SCOPE_READY_TIME;
        let melee = stats.melee_range > 0.0;
        let range = if melee { stats.melee_range } else { SHOT_RANGE };
        let pad = if melee { MELEE_HITBOX_PAD } else { 0.0 };
        let bursts = weapon.kind == pb::WeaponKind::PistolPlus;
        let no_recoil = perks.has(pb::Augment::NoRecoil);
        let origin = pose.eye();
        // Targets are rewound to what this client was rendering when it fired.
        let rewind = rewind_tick(fire.render_tick, current_tick);

        let mut shots: u32 = 0;
        while weapon.cooldown <= COOLDOWN_SLACK
            && (melee || weapon.magazine > 0)
            && shots < MAX_SHOTS_PER_TICK
        {
            shots = shots.saturating_add(1);
            chamber(&mut weapon, stats, melee, bursts, fire);
            vitals.protect_left = 0.0;
            perks.idle = 0.0;
            let cone = spread(
                stats,
                scoped,
                intent.moving,
                weapon.bloom,
                pose.crouching,
                no_recoil,
            );
            if !no_recoil {
                kick(&mut weapon, stats);
            }

            for pellet in 0..stats.pellets.max(1) {
                // Random cone around the aim direction.
                let angle = rng.range(0.0, std::f32::consts::TAU);
                let radius = cone * rng.next_f32().sqrt();
                let dir = view_dir(
                    fire.yaw + radius * angle.cos(),
                    fire.pitch + radius * angle.sin(),
                );

                if stats.splash_radius > 0.0 {
                    missiles.next_id = missiles.next_id.wrapping_add(1);
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
                let targets = world_view.iter().filter(|s| {
                    s.hittable && rules.hostile(player.team, s.team) && s.entity != entity
                });
                let (best_t, struck) =
                    first_struck(origin, dir, world_t.unwrap_or(range), targets, rewind, pad);

                log.push(Event::Shot(pb::ShotEvent {
                    shooter_id: player.id,
                    from_x: origin[0],
                    from_y: origin[1],
                    from_z: origin[2],
                    to_x: origin[0] + dir[0] * best_t,
                    to_y: origin[1] + dir[1] * best_t,
                    to_z: origin[2] + dir[2] * best_t,
                    hit_player_id: struck.map(|s| s.0.id).unwrap_or(0),
                    hit_world: struck.is_none() && world_t.is_some(),
                    melee,
                    pellet: pellet > 0,
                    explosive: false,
                }));

                if let Some((target, zone, shield)) = struck {
                    hits.push(Hit {
                        attacker: entity,
                        attacker_id: player.id,
                        attacker_team: player.team,
                        weapon: weapon.kind,
                        victim: target.entity,
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
    land_hits(hits, &mut players, &mut state, &rules, &mut log);
}

/// Lands the tick's hits in order, until the match is won: damage, then for a kill the victim's
/// death, the score, and what the kill earns its killer.
fn land_hits(
    hits: Vec<Hit>,
    players: &mut Shooters,
    state: &mut MatchState,
    rules: &Rules,
    log: &mut EventLog,
) {
    for hit in hits {
        if state.winner.is_some() {
            break;
        }
        let Ok((_, victim_player, _, _, victim_weapon, mut vitals, mut stats, _, mut perks, _)) =
            players.get_mut(hit.victim)
        else {
            continue;
        };
        let Some(outcome) = take_hit(&mut vitals, &mut perks, &hit) else {
            continue;
        };
        let victim_id = victim_player.id;
        log.push(Event::Damage(pb::DamageEvent {
            attacker_id: hit.attacker_id,
            victim_id,
            amount: hit.amount,
            zone: hit.zone as i32,
            shield: outcome == Outcome::Shield,
        }));
        if outcome != Outcome::Killed {
            continue;
        }
        stats.deaths = stats.deaths.saturating_add(1);
        let victim_primary = victim_weapon.primary;
        let victim_level = perks.level;
        let contributors = std::mem::take(&mut perks.damaged_by);
        let holders = contributors
            .iter()
            .filter(|&&e| players.get(e).is_ok_and(|p| p.8.has(pb::Augment::BigHead)))
            .count();
        let lowest = players
            .iter()
            .map(|p| p.8.level)
            .min()
            .unwrap_or(victim_level);
        if let Ok(mut victim) = players.get_mut(hit.victim) {
            victim.8.head_scale = grown_head(victim.8.head_scale, holders);
            // Big Head mode: the victim's level drops, and their head shrinks with it.
            if rules.big_head {
                victim.8.set_level(big_head::after_death(victim_level));
            }
        }
        let special = special_kill(rules.big_head, hit.weapon, hit.zone, victim_level, lowest);
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
        stats.kills = stats.kills.saturating_add(1);
        if rules.ffa && stats.kills >= state.kills_to_win && state.winner.is_none() {
            state.winner = Some(Winner::Player(hit.attacker_id));
        }
        reward_killer(
            rules,
            &mut perks,
            &mut vitals,
            &mut weapon,
            &mut loadout,
            victim_primary,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    fn entities<const N: usize>() -> [Entity; N] {
        let mut world = World::new();
        std::array::from_fn(|_| world.spawn_empty().id())
    }

    fn standing(x: f32, z: f32) -> Pose {
        Pose {
            x,
            z,
            ..Default::default()
        }
    }

    fn target(entity: Entity, pose: Pose, shielded: bool) -> TargetView {
        TargetView {
            entity,
            id: 7,
            team: pb::Team::Blue,
            pose,
            samples: History::default(),
            hittable: true,
            head_scale: 1.0,
            shielded,
        }
    }

    fn alive(hp: i32) -> Vitals {
        Vitals {
            hp,
            alive: true,
            respawn_left: 0.0,
            protect_left: 0.0,
            stun_left: 0.0,
        }
    }

    fn hit(attacker: Entity, victim: Entity, amount: i32) -> Hit {
        Hit {
            attacker,
            attacker_id: 1,
            attacker_team: pb::Team::Red,
            weapon: Kind::Rifle,
            victim,
            zone: pb::HitZone::Body,
            amount,
            shield: false,
            stun: 0.0,
        }
    }

    fn aim(render_tick: u32) -> FireIntent {
        FireIntent {
            yaw: 0.0,
            pitch: 0.0,
            render_tick,
        }
    }

    fn rules(big_head: bool) -> Rules {
        Rules {
            mode: pb::GameMode::Normal,
            max_hp: MAX_HP,
            ffa: false,
            big_head,
        }
    }

    #[test]
    fn the_head_sits_on_the_body_on_the_legs_and_crouching_lowers_both() {
        let [head, body, legs] = hitboxes(0.0, 0.0, 0.0, 0.0, false, 1.0);
        assert_eq!(
            (head.0, body.0, legs.0),
            (pb::HitZone::Head, pb::HitZone::Body, pb::HitZone::Legs)
        );
        assert_eq!(legs.1[1], 0.0);
        assert_eq!(legs.2[1], body.1[1]);
        assert_eq!(body.2[1], head.1[1]);
        let [low_head, low_body, _] = hitboxes(0.0, 0.0, 0.0, 0.0, true, 1.0);
        assert!((head.1[1] - low_head.1[1] - CROUCH_DROP).abs() < 1e-6);
        assert!((body.2[1] - low_body.2[1] - CROUCH_DROP).abs() < 1e-6);
    }

    #[test]
    fn a_bigger_head_grows_up_and_out_from_the_same_neck() {
        let [normal, ..] = hitboxes(0.0, 0.0, 0.0, 0.0, false, 1.0);
        let [big, ..] = hitboxes(0.0, 0.0, 0.0, 0.0, false, 2.0);
        assert_eq!(big.1[1], normal.1[1]);
        assert!(big.2[1] > normal.2[1] && big.2[0] > normal.2[0]);
    }

    #[test]
    fn melee_padding_widens_every_hitbox() {
        let plain = hitboxes(0.0, 0.0, 0.0, 0.0, false, 1.0);
        let padded = hitboxes(0.0, 0.0, 0.0, MELEE_HITBOX_PAD, false, 1.0);
        for (a, b) in plain.iter().zip(&padded) {
            assert!((b.2[0] - a.2[0] - MELEE_HITBOX_PAD).abs() < 1e-6);
        }
    }

    #[test]
    fn a_shot_from_in_front_meets_the_shield_and_one_above_it_passes() {
        // The bearer stands at the origin facing -Z; the shot comes at them from in front.
        let towards = [0.0, 0.0, 1.0];
        let t = ray_shield([0.0, 1.2, -5.0], towards, 0.0, 0.0, 0.0, 0.0, false).unwrap();
        assert!((t - (5.0 - SHIELD_FAR)).abs() < 1e-4);
        let over = [0.0, SHIELD_TOP + 0.1, -5.0];
        assert_eq!(ray_shield(over, towards, 0.0, 0.0, 0.0, 0.0, false), None);
    }

    #[test]
    fn a_scope_tightens_only_a_scoped_weapon_and_bloom_widens_only_unscoped_shots() {
        let sniper = weapons::stats(Kind::Sniper);
        assert!(sniper.has_scope);
        assert_eq!(
            shot_cone(sniper, true, false, 0.5),
            sniper.scoped_spread_still
        );
        assert_eq!(
            shot_cone(sniper, true, true, 0.0),
            sniper.scoped_spread_still + sniper.scoped_spread_moving
        );
        assert_eq!(
            shot_cone(sniper, false, false, 0.5),
            sniper.spread_still + 0.5
        );
        let rifle = weapons::stats(Kind::Rifle);
        assert!(!rifle.has_scope);
        assert_eq!(shot_cone(rifle, true, false, 0.0), rifle.spread_still);
    }

    #[test]
    fn crouching_narrows_the_cone_and_no_recoil_removes_it_except_a_shotguns_pattern() {
        let rifle = weapons::stats(Kind::Rifle);
        let standing = spread(rifle, false, true, 0.0, false, false);
        assert_eq!(
            spread(rifle, false, true, 0.0, true, false),
            standing * CROUCH_SPREAD_FACTOR
        );
        assert_eq!(spread(rifle, false, true, 0.0, false, true), 0.0);
        let shotgun = weapons::stats(Kind::Shotgun);
        assert!(shotgun.pellets > 0);
        assert!(spread(shotgun, false, false, 0.0, false, true) > 0.0);
    }

    #[test]
    fn a_burst_keeps_firing_where_it_started_after_the_trigger_is_let_go() {
        let mut weapon = Weapon::new(Kind::Rifle);
        assert!(trigger(None, &weapon).is_none());
        assert_eq!(
            trigger(Some(aim(3)), &weapon).map(|f| f.render_tick),
            Some(3)
        );
        weapon.burst_left = 1;
        weapon.burst_fire = Some(aim(5));
        assert_eq!(trigger(None, &weapon).map(|f| f.render_tick), Some(5));
        assert_eq!(
            trigger(Some(aim(6)), &weapon).map(|f| f.render_tick),
            Some(6)
        );
    }

    #[test]
    fn a_reload_holds_the_trigger_unless_a_shell_by_shell_reload_has_a_round_in() {
        let mut weapon = Weapon::new(Kind::Rifle);
        let rifle = weapons::stats(Kind::Rifle);
        assert!(!reload_blocks(&weapon, rifle));
        weapon.reload_left = 1.0;
        assert!(reload_blocks(&weapon, rifle));
        let shotgun = weapons::stats(Kind::Shotgun);
        weapon.magazine = 1;
        assert!(!reload_blocks(&weapon, shotgun));
        weapon.magazine = 0;
        assert!(reload_blocks(&weapon, shotgun));
    }

    #[test]
    fn a_shot_spends_a_round_drops_the_reload_and_waits_a_fire_interval() {
        let rifle = weapons::stats(Kind::Rifle);
        let mut weapon = Weapon::new(Kind::Rifle);
        let magazine = weapon.magazine;
        weapon.reload_left = 1.0;
        chamber(&mut weapon, rifle, false, false, aim(0));
        assert_eq!(weapon.magazine, magazine.saturating_sub(1));
        assert!(!weapon.reloading());
        assert_eq!(weapon.cooldown, rifle.fire_interval);
    }

    #[test]
    fn a_melee_swing_spends_no_round() {
        let mut weapon = Weapon::new(Kind::Rifle);
        let magazine = weapon.magazine;
        chamber(
            &mut weapon,
            weapons::stats(Kind::Knife),
            true,
            false,
            aim(0),
        );
        assert_eq!(weapon.magazine, magazine);
    }

    #[test]
    fn a_burst_fires_its_rounds_a_short_gap_apart_and_then_recovers() {
        let stats = weapons::stats(Kind::PistolPlus);
        let mut weapon = Weapon::new(Kind::Rifle);
        chamber(&mut weapon, stats, false, true, aim(0));
        assert_eq!(weapon.burst_left, weapons::BURST_ROUNDS - 1);
        assert_eq!(weapon.cooldown, weapons::BURST_GAP);
        for _ in 1..weapons::BURST_ROUNDS {
            weapon.cooldown = 0.0;
            chamber(&mut weapon, stats, false, true, aim(0));
        }
        assert_eq!(weapon.burst_left, 0);
        assert_eq!(weapon.cooldown, weapons::BURST_RECOVERY);
    }

    #[test]
    fn an_empty_magazine_ends_a_burst() {
        let stats = weapons::stats(Kind::PistolPlus);
        let mut weapon = Weapon::new(Kind::Rifle);
        weapon.magazine = 1;
        chamber(&mut weapon, stats, false, true, aim(0));
        assert_eq!((weapon.magazine, weapon.burst_left), (0, 0));
        assert_eq!(weapon.cooldown, weapons::BURST_RECOVERY);
    }

    #[test]
    fn bloom_grows_with_each_shot_up_to_the_weapons_cap() {
        let rifle = weapons::stats(Kind::Rifle);
        let mut weapon = Weapon::new(Kind::Rifle);
        kick(&mut weapon, rifle);
        assert_eq!(weapon.bloom, rifle.bloom_per_shot.min(rifle.bloom_max));
        assert_eq!(weapon.bloom_hold, rifle.fire_interval + BLOOM_HOLD_EXTRA);
        for _ in 0..1000 {
            kick(&mut weapon, rifle);
        }
        assert_eq!(weapon.bloom, rifle.bloom_max);
    }

    #[test]
    fn a_shot_rewinds_to_what_its_client_saw_but_not_past_the_window_or_ahead_of_now() {
        assert_eq!(rewind_tick(95, 100), 95);
        assert_eq!(
            rewind_tick(10, 100),
            100_u32.saturating_sub(MAX_REWIND_TICKS)
        );
        assert_eq!(rewind_tick(150, 100), 100);
        assert_eq!(rewind_tick(0, 5), 0);
    }

    #[test]
    fn a_ray_strikes_the_nearest_body_part_in_its_way_within_reach() {
        let [a, b] = entities();
        let near = target(a, standing(0.0, -5.0), false);
        let far = target(b, standing(0.0, -10.0), false);
        let eye = [0.0, 1.6, 0.0];
        let ahead = [0.0, 0.0, -1.0];
        let (t, struck) = first_struck(eye, ahead, 100.0, [&far, &near].into_iter(), 0, 0.0);
        let (who, zone, shield) = struck.unwrap();
        assert_eq!((who.entity, zone, shield), (a, pb::HitZone::Head, false));
        assert!((t - (5.0 - 0.125)).abs() < 1e-4);
        let chest = [0.0, 1.2, 0.0];
        let (_, struck) = first_struck(chest, ahead, 100.0, [&near].into_iter(), 0, 0.0);
        assert_eq!(struck.map(|s| s.1), Some(pb::HitZone::Body));
        let (t, struck) = first_struck(eye, ahead, 3.0, [&near].into_iter(), 0, 0.0);
        assert!(struck.is_none());
        assert_eq!(t, 3.0);
    }

    #[test]
    fn a_shield_in_front_takes_the_shot_before_the_body_behind_it() {
        let [a] = entities();
        // Five metres down -Z, facing back towards the shooter at the origin.
        let mut pose = standing(0.0, -5.0);
        pose.yaw = std::f32::consts::PI;
        let bearer = target(a, pose, true);
        let chest = [0.0, 1.2, 0.0];
        let (_, struck) = first_struck(
            chest,
            [0.0, 0.0, -1.0],
            100.0,
            [&bearer].into_iter(),
            0,
            0.0,
        );
        let (_, zone, shield) = struck.unwrap();
        assert_eq!((zone, shield), (pb::HitZone::Body, true));
    }

    #[test]
    fn a_target_is_struck_where_its_history_puts_it_at_the_rewound_tick() {
        let [a] = entities();
        let mut view = target(a, standing(5.0, -5.0), false);
        view.samples
            .samples
            .push_back((10, 0.0, 0.0, -5.0, false, 0.0));
        let chest = [0.0, 1.2, 0.0];
        let ahead = [0.0, 0.0, -1.0];
        let (_, then) = first_struck(chest, ahead, 100.0, [&view].into_iter(), 10, 0.0);
        assert!(then.is_some(), "it stood in the line of fire at tick 10");
        let (_, before) = first_struck(chest, ahead, 100.0, [&view].into_iter(), 9, 0.0);
        assert!(
            before.is_none(),
            "before its history starts it is where it is now"
        );
    }

    #[test]
    fn a_hit_wounds_then_kills_at_zero_hp_and_the_dead_take_no_more() {
        let [attacker, victim] = entities();
        let mut vitals = alive(100);
        let mut perks = Perks::default();
        let first = take_hit(&mut vitals, &mut perks, &hit(attacker, victim, 40));
        assert_eq!(first, Some(Outcome::Wounded));
        assert_eq!(vitals.hp, 60);
        let second = take_hit(&mut vitals, &mut perks, &hit(attacker, victim, 80));
        assert_eq!(second, Some(Outcome::Killed));
        assert_eq!(
            (vitals.hp, vitals.alive, vitals.respawn_left),
            (0, false, RESPAWN_TIME)
        );
        assert_eq!(perks.damaged_by, vec![attacker]);
        assert_eq!(
            take_hit(&mut vitals, &mut perks, &hit(attacker, victim, 10)),
            None
        );
    }

    #[test]
    fn a_shield_soaks_hits_until_it_breaks() {
        let [attacker, victim] = entities();
        let mut vitals = alive(100);
        let mut perks = Perks {
            shield_hp: 50,
            ..Default::default()
        };
        let blocked = Hit {
            shield: true,
            ..hit(attacker, victim, 30)
        };
        let outcome = take_hit(&mut vitals, &mut perks, &blocked);
        assert_eq!(outcome, Some(Outcome::Shield));
        assert_eq!((perks.shield_hp, vitals.hp), (20, 100));
        take_hit(&mut vitals, &mut perks, &blocked);
        assert_eq!(perks.shield_hp, 0);
        let outcome = take_hit(&mut vitals, &mut perks, &blocked);
        assert_eq!(outcome, Some(Outcome::Wounded));
        assert_eq!(vitals.hp, 70);
    }

    #[test]
    fn the_king_takes_no_harm_and_a_taser_paralyses_without_hurting() {
        let [attacker, victim] = entities();
        let mut vitals = alive(100);
        let mut king = Perks {
            king_ticks: 10,
            ..Default::default()
        };
        assert_eq!(
            take_hit(&mut vitals, &mut king, &hit(attacker, victim, 500)),
            None
        );
        assert!(king.damaged_by.is_empty());
        let mut perks = Perks::default();
        let zap = Hit {
            stun: 5.0,
            ..hit(attacker, victim, 0)
        };
        let outcome = take_hit(&mut vitals, &mut perks, &zap);
        assert_eq!(outcome, Some(Outcome::Wounded));
        assert_eq!((vitals.hp, vitals.stun_left), (100, 5.0));
    }

    #[test]
    fn each_big_head_holder_grows_the_victims_head_a_step_up_to_the_cap() {
        assert_eq!(grown_head(1.0, 0), 1.0);
        assert_eq!(grown_head(1.0, 2), 1.0 + 2.0 * augments::BIG_HEAD_STEP);
        assert_eq!(
            grown_head(augments::BIG_HEAD_MAX, 1),
            augments::BIG_HEAD_MAX
        );
    }

    #[test]
    fn big_head_mode_names_hammer_kills_and_headshots_on_the_smallest_head() {
        use pb::{HitZone, SpecialKill};
        let special = |weapon, zone, level, lowest| special_kill(true, weapon, zone, level, lowest);
        assert_eq!(
            special_kill(false, Kind::Hammer, HitZone::Head, -3, -3),
            SpecialKill::Unspecified
        );
        assert_eq!(
            special(Kind::Hammer, HitZone::Legs, 2, -3),
            SpecialKill::CuteShot
        );
        assert_eq!(
            special(Kind::Rifle, HitZone::Head, -3, -3),
            SpecialKill::DotShot
        );
        assert_eq!(
            special(Kind::Rifle, HitZone::Head, -2, -3),
            SpecialKill::Unspecified,
            "someone's head is smaller"
        );
        assert_eq!(
            special(Kind::Rifle, HitZone::Head, 0, 0),
            SpecialKill::Unspecified,
            "a normal head"
        );
        assert_eq!(
            special(Kind::Rifle, HitZone::Body, -3, -3),
            SpecialKill::Unspecified
        );
    }

    #[test]
    fn soul_drain_heals_a_living_killer_up_to_their_cap() {
        let mut perks = Perks {
            active: vec![Aug::SoulDrain],
            ..Default::default()
        };
        let mut vitals = alive(MAX_HP.saturating_sub(10));
        let mut weapon = Weapon::new(Kind::Rifle);
        let mut loadout = Loadout(Kind::Rifle);
        reward_killer(
            &rules(false),
            &mut perks,
            &mut vitals,
            &mut weapon,
            &mut loadout,
            Kind::Sniper,
        );
        assert_eq!(vitals.hp, augments::max_hp(MAX_HP, &perks.active));
    }

    #[test]
    fn possession_hands_the_killer_the_victims_primary_and_fresh_health() {
        let mut perks = Perks {
            active: vec![Aug::Possession],
            ..Default::default()
        };
        let mut vitals = alive(20);
        let mut weapon = Weapon::new(Kind::Rifle);
        let mut loadout = Loadout(Kind::Rifle);
        reward_killer(
            &rules(false),
            &mut perks,
            &mut vitals,
            &mut weapon,
            &mut loadout,
            Kind::Sniper,
        );
        assert_eq!(vitals.hp, augments::POSSESSION_HP);
        assert_eq!((loadout.0, weapon.primary), (Kind::Sniper, Kind::Sniper));
    }

    #[test]
    fn a_dead_killer_still_climbs_a_level_but_gains_nothing_else() {
        let mut perks = Perks {
            active: vec![Aug::SoulDrain],
            ..Default::default()
        };
        let mut vitals = Vitals {
            alive: false,
            ..alive(0)
        };
        let mut weapon = Weapon::new(Kind::Rifle);
        let mut loadout = Loadout(Kind::Rifle);
        reward_killer(
            &rules(true),
            &mut perks,
            &mut vitals,
            &mut weapon,
            &mut loadout,
            Kind::Sniper,
        );
        assert_eq!(perks.level, big_head::after_kill(0));
        assert_eq!(vitals.hp, 0);
    }

    #[test]
    fn reaching_the_top_level_alive_crowns_the_killer() {
        let mut perks = Perks::default();
        perks.set_level(big_head::MAX_LEVEL.saturating_sub(1));
        let mut vitals = alive(10);
        let mut weapon = Weapon::new(Kind::Rifle);
        let mut loadout = Loadout(Kind::Rifle);
        reward_killer(
            &rules(true),
            &mut perks,
            &mut vitals,
            &mut weapon,
            &mut loadout,
            Kind::Sniper,
        );
        assert!(perks.king());
        assert_eq!((weapon.kind, vitals.hp), (Kind::Hammer, MAX_HP));
    }

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
            Some(pb::game_event::Event::Kill(k)) => Some(*k),
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
