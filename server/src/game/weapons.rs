//! The weapon table. The rifle is the baseline every other stat is judged against.

use super::components::GameRng;
use crate::pb::{self, WeaponKind};

pub struct Stats {
    pub name: &'static str,
    pub magazine: u32,
    pub reserve: u32,
    pub fire_interval: f32,
    pub reload_time: f32,
    pub move_speed: f32,
    pub damage_head: i32,
    pub damage_body: i32,
    pub damage_legs: i32,
    pub spread_still: f32,
    pub spread_moving: f32,
    pub bloom_per_shot: f32,
    pub bloom_max: f32,
    pub has_scope: bool,
    /// Cone when fully scoped; these replace the `spread_*` values.
    pub scoped_spread_still: f32,
    pub scoped_spread_moving: f32,
    /// Reach in metres for melee weapons, 0 for guns.
    pub melee_range: f32,
    /// Projectiles per shot; 0 means one.
    pub pellets: u32,
    /// Reloads a round at a time and can fire between rounds.
    pub shell_reload: bool,
    /// Blast radius in metres for explosive weapons, 0 for others.
    pub splash_radius: f32,
    /// Seconds of paralysis per hit.
    pub stun: f32,
}

/// Everyone's move speed, relative to the values in the table.
const SPEED_SCALE: f32 = 0.9;
/// Everyone's spread and bloom, scoped values included, relative to the values in the
/// table. The shotgun's pellet cone is not scaled.
const SPREAD_SCALE: f32 = 0.75;

const RIFLE: Stats = Stats {
    name: "M416",
    magazine: 30,
    reserve: 120,
    fire_interval: 0.1,
    reload_time: 2.2,
    move_speed: 5.2 * SPEED_SCALE,
    damage_head: 100,
    damage_body: 34,
    damage_legs: 25,
    spread_still: 0.002 * SPREAD_SCALE,
    spread_moving: 0.014 * SPREAD_SCALE,
    bloom_per_shot: 0.0035 * SPREAD_SCALE,
    bloom_max: 0.03 * SPREAD_SCALE,
    has_scope: false,
    scoped_spread_still: 0.0,
    scoped_spread_moving: 0.0,
    melee_range: 0.0,
    pellets: 0,
    shell_reload: false,
    splash_radius: 0.0,
    stun: 0.0,
};

const SNIPER: Stats = Stats {
    name: "AWP",
    magazine: 5,
    reserve: 20,
    // Bolt cycle.
    fire_interval: 1.3,
    reload_time: 3.0,
    move_speed: 4.7 * SPEED_SCALE,
    damage_head: 200,
    damage_body: 100,
    damage_legs: 70,
    spread_still: 0.009 * SPREAD_SCALE,
    spread_moving: 0.022 * SPREAD_SCALE,
    bloom_per_shot: 0.0,
    bloom_max: 0.0,
    has_scope: true,
    scoped_spread_still: 0.0002 * SPREAD_SCALE,
    scoped_spread_moving: 0.008 * SPREAD_SCALE,
    melee_range: 0.0,
    pellets: 0,
    shell_reload: false,
    splash_radius: 0.0,
    stun: 0.0,
};

const MACHINE_GUN: Stats = Stats {
    name: "M249",
    magazine: 100,
    reserve: 200,
    fire_interval: 0.06,
    reload_time: 4.5,
    move_speed: 3.7 * SPEED_SCALE,
    damage_head: 100,
    damage_body: 40,
    damage_legs: 30,
    spread_still: 0.018 * SPREAD_SCALE,
    spread_moving: 0.03 * SPREAD_SCALE,
    bloom_per_shot: 0.0012 * SPREAD_SCALE,
    bloom_max: 0.03 * SPREAD_SCALE,
    has_scope: false,
    scoped_spread_still: 0.0,
    scoped_spread_moving: 0.0,
    melee_range: 0.0,
    pellets: 0,
    shell_reload: false,
    splash_radius: 0.0,
    stun: 0.0,
};

const SMG: Stats = Stats {
    name: "UZI",
    magazine: 25,
    reserve: 125,
    fire_interval: 0.075,
    reload_time: 1.8,
    move_speed: 6.3 * SPEED_SCALE,
    damage_head: 72,
    damage_body: 24,
    damage_legs: 18,
    spread_still: 0.0015 * SPREAD_SCALE,
    spread_moving: 0.009 * SPREAD_SCALE,
    bloom_per_shot: 0.0025 * SPREAD_SCALE,
    bloom_max: 0.022 * SPREAD_SCALE,
    has_scope: false,
    scoped_spread_still: 0.0,
    scoped_spread_moving: 0.0,
    melee_range: 0.0,
    pellets: 0,
    shell_reload: false,
    splash_radius: 0.0,
    stun: 0.0,
};

const PISTOL: Stats = Stats {
    name: "PT-9",
    magazine: 12,
    reserve: 36,
    fire_interval: 0.25,
    reload_time: 1.5,
    move_speed: 5.6 * SPEED_SCALE,
    damage_head: 70,
    damage_body: 22,
    damage_legs: 16,
    spread_still: 0.0012 * SPREAD_SCALE,
    spread_moving: 0.008 * SPREAD_SCALE,
    bloom_per_shot: 0.002 * SPREAD_SCALE,
    bloom_max: 0.012 * SPREAD_SCALE,
    has_scope: false,
    scoped_spread_still: 0.0,
    scoped_spread_moving: 0.0,
    melee_range: 0.0,
    pellets: 0,
    shell_reload: false,
    splash_radius: 0.0,
    stun: 0.0,
};

const KNIFE: Stats = Stats {
    name: "KNIFE",
    magazine: 0,
    reserve: 0,
    fire_interval: 0.55,
    reload_time: 0.0,
    move_speed: 6.0 * SPEED_SCALE,
    damage_head: 100,
    damage_body: 60,
    damage_legs: 45,
    spread_still: 0.0,
    spread_moving: 0.0,
    bloom_per_shot: 0.0,
    bloom_max: 0.0,
    has_scope: false,
    scoped_spread_still: 0.0,
    scoped_spread_moving: 0.0,
    melee_range: 2.2,
    pellets: 0,
    shell_reload: false,
    splash_radius: 0.0,
    stun: 0.0,
};

const SHOTGUN: Stats = Stats {
    name: "SG-12",
    magazine: 6,
    reserve: 36,
    // Pump cycle.
    fire_interval: 1.0,
    // Per round.
    reload_time: 0.55,
    move_speed: 5.2 * SPEED_SCALE,
    // Per pellet. Six body hits at close range are 180.
    damage_head: 50,
    damage_body: 30,
    damage_legs: 20,
    spread_still: 0.03,
    spread_moving: 0.015,
    bloom_per_shot: 0.0,
    bloom_max: 0.0,
    has_scope: false,
    scoped_spread_still: 0.0,
    scoped_spread_moving: 0.0,
    melee_range: 0.0,
    pellets: 6,
    shell_reload: true,
    splash_radius: 0.0,
    stun: 0.0,
};

const MISSILE: Stats = Stats {
    name: "ML-9",
    magazine: 9,
    reserve: 36,
    fire_interval: 0.3,
    reload_time: 3.0,
    move_speed: 5.2 * SPEED_SCALE,
    // At the blast's centre, falling off with distance.
    damage_head: 150,
    damage_body: 150,
    damage_legs: 150,
    spread_still: 0.002 * SPREAD_SCALE,
    spread_moving: 0.02 * SPREAD_SCALE,
    bloom_per_shot: 0.0,
    bloom_max: 0.0,
    has_scope: false,
    scoped_spread_still: 0.0,
    scoped_spread_moving: 0.0,
    melee_range: 0.0,
    pellets: 0,
    shell_reload: false,
    splash_radius: 3.5,
    stun: 0.0,
};

const PISTOL_PLUS: Stats = Stats {
    name: "PT-9+",
    reserve: 120,
    fire_interval: BURST_RECOVERY,
    damage_head: 100,
    damage_body: 32,
    damage_legs: 24,
    ..PISTOL
};

const MAGNUM: Stats = Stats {
    name: "RV-44",
    magazine: 6,
    reserve: 30,
    fire_interval: 0.9,
    reload_time: 2.6,
    move_speed: 6.0 * SPEED_SCALE,
    // A head always drops; a body drops if it's already hurt.
    damage_head: 300,
    damage_body: 130,
    damage_legs: 90,
    // Goes exactly where it's aimed; only the kick moves the aim.
    spread_still: 0.0,
    spread_moving: 0.0,
    bloom_per_shot: 0.0,
    bloom_max: 0.0,
    ..PISTOL
};

const TASER: Stats = Stats {
    name: "ZAP-1",
    magazine: 1,
    reserve: 6,
    fire_interval: 0.5,
    reload_time: 2.0,
    damage_head: 0,
    damage_body: 0,
    damage_legs: 0,
    stun: 5.0,
    ..PISTOL
};

const AKIMBO_SMG: Stats = Stats {
    name: "UZI Akimbo",
    magazine: SMG.magazine * 2,
    // The same number of reloads as a single gun.
    reserve: SMG.reserve * 2,
    fire_interval: SMG.fire_interval / 2.0,
    ..SMG
};

/// Time between rounds of the upgraded pistol's burst, and between bursts.
pub const BURST_GAP: f32 = 0.07;
pub const BURST_RECOVERY: f32 = 0.2;
pub const BURST_ROUNDS: u32 = 3;

pub fn reload_time(stats: &Stats, active: &[pb::Augment]) -> f32 {
    if active.contains(&pb::Augment::Borrow) {
        return 0.0;
    }
    let quick = if active.contains(&pb::Augment::QuickReload) { 0.5 } else { 1.0 };
    let slow = if active.contains(&pb::Augment::NoRecoil) { 2.0 } else { 1.0 };
    stats.reload_time * quick * slow
}

/// What a hit deals. Augment mode's larger health bars call for harder hits from
/// the guns meant to stay competitive against them.
pub fn damage(kind: WeaponKind, mode: pb::GameMode, zone: pb::HitZone) -> i32 {
    let stats = stats(kind);
    let (head, body, legs) = match (mode, kind) {
        (pb::GameMode::Augment, WeaponKind::Sniper) => (300, 150, 105),
        (pb::GameMode::Augment, WeaponKind::Smg | WeaponKind::AkimboSmg) => (84, 28, 21),
        _ => (stats.damage_head, stats.damage_body, stats.damage_legs),
    };
    match zone {
        pb::HitZone::Head => head,
        pb::HitZone::Legs => legs,
        _ => body,
    }
}

pub fn stats(kind: WeaponKind) -> &'static Stats {
    match kind {
        WeaponKind::Shotgun => &SHOTGUN,
        WeaponKind::Missile => &MISSILE,
        WeaponKind::Taser => &TASER,
        WeaponKind::Pistol => &PISTOL,
        WeaponKind::PistolPlus => &PISTOL_PLUS,
        WeaponKind::Magnum => &MAGNUM,
        WeaponKind::AkimboSmg => &AKIMBO_SMG,
        WeaponKind::Knife => &KNIFE,
        WeaponKind::Sniper => &SNIPER,
        WeaponKind::MachineGun => &MACHINE_GUN,
        WeaponKind::Smg => &SMG,
        WeaponKind::Rifle | WeaponKind::Unspecified => &RIFLE,
    }
}

/// What a player may choose for slot 1.
pub const PRIMARIES: [WeaponKind; 4] = [
    WeaponKind::Rifle,
    WeaponKind::Sniper,
    WeaponKind::MachineGun,
    WeaponKind::Smg,
];

pub const ALL: [WeaponKind; 12] = [
    WeaponKind::Rifle,
    WeaponKind::Sniper,
    WeaponKind::MachineGun,
    WeaponKind::Smg,
    WeaponKind::Pistol,
    WeaponKind::Knife,
    WeaponKind::Shotgun,
    WeaponKind::Missile,
    WeaponKind::Taser,
    WeaponKind::PistolPlus,
    WeaponKind::Magnum,
    WeaponKind::AkimboSmg,
];

/// What slot 2 holds for a player with these augments.
pub fn sidearm_for(active: &[pb::Augment]) -> WeaponKind {
    if active.contains(&pb::Augment::Shotgun) {
        WeaponKind::Shotgun
    } else if active.contains(&pb::Augment::Magnum) {
        WeaponKind::Magnum
    } else if active.contains(&pb::Augment::Taser) {
        WeaponKind::Taser
    } else if active.contains(&pb::Augment::PistolUpgrade) {
        WeaponKind::PistolPlus
    } else {
        WeaponKind::Pistol
    }
}

/// What slot 1 holds for a player who chose `kind`.
pub fn primary_for(kind: WeaponKind, active: &[pb::Augment]) -> WeaponKind {
    if kind == WeaponKind::Smg && active.contains(&pb::Augment::Akimbo) {
        WeaponKind::AkimboSmg
    } else {
        kind
    }
}

/// The ordinary weapon behind an augment's version of it.
pub fn base_primary(kind: WeaponKind) -> WeaponKind {
    if kind == WeaponKind::AkimboSmg { WeaponKind::Smg } else { kind }
}

pub fn all_stats() -> Vec<pb::WeaponStats> {
    ALL.iter()
        .map(|&kind| {
            let s = stats(kind);
            pb::WeaponStats {
                kind: kind as i32,
                name: s.name.to_string(),
                magazine: s.magazine,
                reserve: s.reserve,
                fire_interval: s.fire_interval,
                reload_time: s.reload_time,
                move_speed: s.move_speed,
                damage_head: s.damage_head,
                damage_body: s.damage_body,
                damage_legs: s.damage_legs,
                spread_still: s.spread_still,
                spread_moving: s.spread_moving,
                bloom_per_shot: s.bloom_per_shot,
                bloom_max: s.bloom_max,
                has_scope: s.has_scope,
                melee_range: s.melee_range,
                pellets: s.pellets,
                shell_reload: s.shell_reload,
                splash_radius: s.splash_radius,
            }
        })
        .collect()
}

/// Rifles are most common; snipers rare enough not to dominate a small map.
pub fn random_for_bot(rng: &mut GameRng) -> WeaponKind {
    let roll = rng.next_f32();
    if roll < 0.4 {
        WeaponKind::Rifle
    } else if roll < 0.65 {
        WeaponKind::Smg
    } else if roll < 0.85 {
        WeaponKind::MachineGun
    } else {
        WeaponKind::Sniper
    }
}
