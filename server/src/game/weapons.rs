//! The weapon table. The rifle is the baseline every other stat is judged against.

use super::resources::GameRng;
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
    spread_still: 0.0016 * SPREAD_SCALE,
    spread_moving: 0.009 * SPREAD_SCALE,
    bloom_per_shot: 0.0022 * SPREAD_SCALE,
    bloom_max: 0.018 * SPREAD_SCALE,
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
    // Still just above the pistol's, which stays the more accurate gun at rest.
    spread_still: 0.0013 * SPREAD_SCALE,
    spread_moving: 0.006 * SPREAD_SCALE,
    bloom_per_shot: 0.0016 * SPREAD_SCALE,
    bloom_max: 0.013 * SPREAD_SCALE,
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

/// Slot 4. `magazine` is how many a life starts with; there is no reserve and no reload. The
/// damage is the blast's at its centre, falling off with distance (systems.rs `blast`).
const GRENADE: Stats = Stats {
    name: "HG-3",
    magazine: 1,
    reserve: 0,
    // After a throw: until the next one, or until the weapon the last one goes back to is drawn.
    fire_interval: 0.6,
    reload_time: 0.0,
    move_speed: 5.6 * SPEED_SCALE,
    damage_head: 110,
    damage_body: 110,
    damage_legs: 110,
    spread_still: 0.0,
    spread_moving: 0.0,
    bloom_per_shot: 0.0,
    bloom_max: 0.0,
    has_scope: false,
    scoped_spread_still: 0.0,
    scoped_spread_moving: 0.0,
    melee_range: 0.0,
    pellets: 0,
    shell_reload: false,
    splash_radius: 5.0,
    stun: 0.0,
};

/// Big Head mode: the King's toy hammer, the reign's only weapon. A hit anywhere fells anyone, and
/// its move speed, a little above the knife's, is the King's.
const HAMMER: Stats = Stats {
    name: "TOY HAMMER",
    magazine: 0,
    reserve: 0,
    fire_interval: 0.5,
    reload_time: 0.0,
    move_speed: 6.6 * SPEED_SCALE,
    damage_head: 200,
    damage_body: 200,
    damage_legs: 200,
    spread_still: 0.0,
    spread_moving: 0.0,
    bloom_per_shot: 0.0,
    bloom_max: 0.0,
    has_scope: false,
    scoped_spread_still: 0.0,
    scoped_spread_moving: 0.0,
    melee_range: 2.5,
    pellets: 0,
    shell_reload: false,
    splash_radius: 0.0,
    stun: 0.0,
};

/// Time between rounds of the upgraded pistol's burst, and between bursts.
pub const BURST_GAP: f32 = 0.07;
pub const BURST_RECOVERY: f32 = 0.2;
pub const BURST_ROUNDS: u32 = 3;

pub fn reload_time(stats: &Stats, active: &[pb::Augment]) -> f32 {
    if active.contains(&pb::Augment::Borrow) {
        return 0.0;
    }
    let quick = if active.contains(&pb::Augment::QuickReload) {
        0.5
    } else {
        1.0
    };
    let slow = if active.contains(&pb::Augment::NoRecoil) {
        2.0
    } else {
        1.0
    };
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
        WeaponKind::Grenade => &GRENADE,
        WeaponKind::Hammer => &HAMMER,
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

pub const ALL: [WeaponKind; 14] = [
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
    WeaponKind::Grenade,
    WeaponKind::Hammer,
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
    if kind == WeaponKind::AkimboSmg {
        WeaponKind::Smg
    } else {
        kind
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::RIFLE;
    use crate::game::test_support::*;

    const MODES: [pb::GameMode; 3] = [
        pb::GameMode::Normal,
        pb::GameMode::Augment,
        pb::GameMode::BigHead,
    ];

    const ZONES: [pb::HitZone; 3] = [pb::HitZone::Head, pb::HitZone::Body, pb::HitZone::Legs];

    /// Every augment there is.
    fn every_augment() -> impl Iterator<Item = Aug> {
        (0..64)
            .filter_map(|n| Aug::try_from(n).ok())
            .filter(|&augment| augment != Aug::Unspecified)
    }

    #[test]
    fn every_weapon_kind_has_its_own_entry_and_an_unspecified_one_is_the_rifle() {
        let kinds: Vec<Kind> = (0..64)
            .filter_map(|n| Kind::try_from(n).ok())
            .filter(|&kind| kind != Kind::Unspecified)
            .collect();
        assert_eq!(kinds.len(), ALL.len(), "ALL is what clients are sent");
        for kind in kinds {
            assert!(ALL.contains(&kind), "{kind:?} is missing from ALL");
        }
        let mut names: Vec<&str> = ALL.iter().map(|&kind| stats(kind).name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALL.len(), "two kinds share an entry");
        assert_eq!(stats(Kind::Unspecified).name, stats(Kind::Rifle).name);
    }

    #[test]
    fn only_melee_weapons_carry_no_ammo() {
        for kind in ALL {
            let s = stats(kind);
            let melee = s.melee_range > 0.0;
            assert_eq!(melee, s.magazine == 0, "{kind:?}");
            assert!(!melee || s.reserve == 0, "{kind:?}");
        }
    }

    #[test]
    fn quick_reload_shortens_a_reload_no_recoil_lengthens_it_and_together_they_multiply() {
        for kind in ALL {
            let s = stats(kind);
            let base = reload_time(s, &[]);
            assert_eq!(base, s.reload_time, "{kind:?}");
            if base == 0.0 {
                continue;
            }
            let quick = reload_time(s, &[Aug::QuickReload]);
            let slow = reload_time(s, &[Aug::NoRecoil]);
            let both = reload_time(s, &[Aug::QuickReload, Aug::NoRecoil]);
            assert!(
                quick < base && base < slow,
                "{kind:?}: {quick} {base} {slow}"
            );
            assert!(
                (both / base - (quick / base) * (slow / base)).abs() < 1e-5,
                "{kind:?}: {both}"
            );
        }
    }

    #[test]
    fn borrow_reloads_instantly_whatever_else_is_held() {
        for kind in ALL {
            let s = stats(kind);
            for others in [&[][..], &[Aug::QuickReload], &[Aug::NoRecoil]] {
                let mut active = others.to_vec();
                active.push(Aug::Borrow);
                assert_eq!(reload_time(s, &active), 0.0, "{kind:?} with {active:?}");
            }
        }
    }

    #[test]
    fn augments_that_are_not_about_reloading_leave_the_reload_time_alone() {
        let reloading = [Aug::QuickReload, Aug::NoRecoil, Aug::Borrow];
        for augment in every_augment().filter(|a| !reloading.contains(a)) {
            for kind in ALL {
                let s = stats(kind);
                assert_eq!(
                    reload_time(s, &[augment]),
                    s.reload_time,
                    "{augment:?} {kind:?}"
                );
            }
        }
    }

    #[test]
    fn a_head_hit_deals_at_least_a_body_hit_and_a_body_hit_at_least_a_leg_hit() {
        for kind in ALL {
            for mode in MODES {
                let [head, body, legs] = ZONES.map(|zone| damage(kind, mode, zone));
                assert!(
                    head >= body && body >= legs,
                    "{kind:?} in {mode:?}: {head} {body} {legs}"
                );
            }
        }
    }

    #[test]
    fn outside_augment_mode_a_hit_deals_what_the_table_says() {
        for kind in ALL {
            let s = stats(kind);
            for mode in [pb::GameMode::Normal, pb::GameMode::BigHead] {
                assert_eq!(
                    ZONES.map(|zone| damage(kind, mode, zone)),
                    [s.damage_head, s.damage_body, s.damage_legs],
                    "{kind:?} in {mode:?}"
                );
            }
        }
    }

    #[test]
    fn augment_mode_raises_every_zone_for_the_sniper_and_the_smgs_and_for_no_other_weapon() {
        for kind in ALL {
            let boosted = matches!(kind, Kind::Sniper | Kind::Smg | Kind::AkimboSmg);
            for zone in ZONES {
                let normal = damage(kind, pb::GameMode::Normal, zone);
                let augment = damage(kind, pb::GameMode::Augment, zone);
                if boosted {
                    assert!(augment > normal, "{kind:?} {zone:?}: {normal} -> {augment}");
                } else {
                    assert_eq!(augment, normal, "{kind:?} {zone:?}");
                }
            }
        }
    }

    #[test]
    fn the_akimbo_smg_hits_like_the_smg_in_every_mode_and_zone() {
        for mode in MODES {
            for zone in ZONES {
                assert_eq!(
                    damage(Kind::AkimboSmg, mode, zone),
                    damage(Kind::Smg, mode, zone),
                    "{mode:?} {zone:?}"
                );
            }
        }
    }

    #[test]
    fn the_kings_hammer_fells_anyone_with_a_hit_anywhere() {
        for zone in ZONES {
            assert!(
                damage(Kind::Hammer, pb::GameMode::BigHead, zone) >= MAX_HP,
                "{zone:?}"
            );
        }
    }

    #[test]
    fn the_taser_stuns_and_deals_no_damage_and_nothing_else_stuns() {
        assert!(stats(Kind::Taser).stun > 0.0);
        for mode in MODES {
            for zone in ZONES {
                assert_eq!(damage(Kind::Taser, mode, zone), 0, "{mode:?} {zone:?}");
            }
        }
        for kind in ALL.into_iter().filter(|&kind| kind != Kind::Taser) {
            assert_eq!(stats(kind).stun, 0.0, "{kind:?}");
        }
    }

    #[test]
    fn each_slot_two_augment_brings_its_own_sidearm_and_every_other_keeps_the_pistol() {
        let own = [
            (Aug::Shotgun, Kind::Shotgun),
            (Aug::Magnum, Kind::Magnum),
            (Aug::Taser, Kind::Taser),
            (Aug::PistolUpgrade, Kind::PistolPlus),
        ];
        assert_eq!(sidearm_for(&[]), Kind::Pistol);
        for augment in every_augment() {
            let expected = own
                .iter()
                .find(|(owner, _)| *owner == augment)
                .map_or(Kind::Pistol, |&(_, kind)| kind);
            assert_eq!(sidearm_for(&[augment]), expected, "{augment:?}");
            // Also as one of Double Time's two.
            assert_eq!(sidearm_for(&[Aug::Vest, augment]), expected, "{augment:?}");
        }
    }

    #[test]
    fn akimbo_doubles_a_chosen_smg_and_leaves_every_other_primary_alone() {
        for kind in PRIMARIES {
            let akimbo = if kind == Kind::Smg {
                Kind::AkimboSmg
            } else {
                kind
            };
            assert_eq!(primary_for(kind, &[Aug::Akimbo]), akimbo, "{kind:?}");
            assert_eq!(primary_for(kind, &[Aug::Vest, Aug::Akimbo]), akimbo);
            for augment in every_augment().filter(|&a| a != Aug::Akimbo) {
                assert_eq!(primary_for(kind, &[augment]), kind, "{kind:?} {augment:?}");
            }
        }
    }

    #[test]
    fn the_base_of_any_augmented_primary_is_the_weapon_that_was_chosen() {
        for kind in PRIMARIES {
            assert_eq!(base_primary(kind), kind);
            for augment in every_augment() {
                assert_eq!(
                    base_primary(primary_for(kind, &[augment])),
                    kind,
                    "{kind:?} {augment:?}"
                );
            }
        }
    }

    #[test]
    fn bots_get_only_primaries_rifles_most_often_and_snipers_least() {
        let mut rng = GameRng(7);
        let mut counts = [0u32; PRIMARIES.len()];
        for _ in 0..10_000 {
            let kind = random_for_bot(&mut rng);
            let i = PRIMARIES
                .iter()
                .position(|&primary| primary == kind)
                .unwrap_or_else(|| panic!("{kind:?} is no primary"));
            counts[i] = counts[i].saturating_add(1);
        }
        let count = |kind: Kind| counts[PRIMARIES.iter().position(|&p| p == kind).unwrap()];
        for kind in [Kind::Sniper, Kind::MachineGun, Kind::Smg] {
            assert!(count(Kind::Rifle) > count(kind), "{counts:?}");
        }
        for kind in [Kind::MachineGun, Kind::Smg] {
            assert!(count(Kind::Sniper) < count(kind), "{counts:?}");
        }
        assert!(count(Kind::Sniper) > 0, "{counts:?}");
    }

    #[test]
    fn weapon_table_matches_the_spec() {
        use pb::WeaponKind::*;
        let (rifle, sniper, mg, smg) = (
            weapons::stats(Rifle),
            weapons::stats(Sniper),
            weapons::stats(MachineGun),
            weapons::stats(Smg),
        );
        assert_eq!((rifle.magazine, rifle.reserve), (30, 120));
        assert_eq!((sniper.magazine, sniper.reserve), (5, 20));
        assert_eq!((mg.magazine, mg.reserve), (100, 200));
        assert_eq!((smg.magazine, smg.reserve), (25, 125));
        // Damage: sniper > machine gun > rifle > SMG.
        assert!(
            sniper.damage_body > mg.damage_body
                && mg.damage_body > rifle.damage_body
                && rifle.damage_body > smg.damage_body
        );
        // Speed: SMG > rifle > sniper > machine gun.
        assert!(
            smg.move_speed > rifle.move_speed
                && rifle.move_speed > sniper.move_speed
                && sniper.move_speed > mg.move_speed
        );
        // Fire rate: machine gun > SMG > rifle > bolt action.
        assert!(
            mg.fire_interval < smg.fire_interval
                && smg.fire_interval < rifle.fire_interval
                && rifle.fire_interval < sniper.fire_interval
        );
        // Hip accuracy: SMG > rifle > sniper > machine gun, and the scope beats all.
        assert!(
            smg.spread_still < rifle.spread_still
                && rifle.spread_still < sniper.spread_still
                && sniper.spread_still < mg.spread_still
        );
        assert!(sniper.scoped_spread_still < smg.spread_still);
    }

    #[test]
    fn fire_rates_are_distinct_at_a_30hz_tick() {
        assert_eq!(shots_in_one_second(pb::WeaponKind::Sniper), 1);
        assert_eq!(shots_in_one_second(pb::WeaponKind::Rifle), 10);
        // 800 and 1000 rpm land between whole counts within one second.
        let smg = shots_in_one_second(pb::WeaponKind::Smg);
        let mg = shots_in_one_second(pb::WeaponKind::MachineGun);
        assert!((13..=14).contains(&smg), "smg fired {smg}");
        assert!((16..=17).contains(&mg), "mg fired {mg}");
    }

    #[test]
    fn sidearm_stats_match_the_spec() {
        use pb::WeaponKind::*;
        let (rifle, pistol, knife, smg) = (
            weapons::stats(Rifle),
            weapons::stats(Pistol),
            weapons::stats(Knife),
            weapons::stats(Smg),
        );
        assert_eq!((pistol.magazine, pistol.reserve), (12, 36));
        assert!(pistol.damage_body < rifle.damage_body);
        assert!(pistol.fire_interval > rifle.fire_interval, "low fire rate");
        assert!(pistol.move_speed > rifle.move_speed, "slightly high speed");
        assert!(pistol.spread_still < smg.spread_still, "high accuracy");
        assert!(knife.damage_body > rifle.damage_body, "high damage");
        assert!(knife.move_speed > pistol.move_speed && knife.move_speed < smg.move_speed);
        assert!(knife.melee_range > 0.0);
    }

    #[test]
    fn augment_mode_hits_harder_with_the_sniper_and_smg_only() {
        let hit = |kind, mode| weapons::damage(kind, mode, pb::HitZone::Body);
        let (normal, augment) = (pb::GameMode::Normal, pb::GameMode::Augment);
        assert_eq!(hit(Kind::Sniper, normal), 100);
        assert_eq!(
            hit(Kind::Sniper, augment),
            150,
            "still one body shot at 150 HP"
        );
        assert_eq!(hit(Kind::Smg, normal), 24);
        assert_eq!(hit(Kind::Smg, augment), 28);
        assert_eq!(hit(Kind::AkimboSmg, augment), 28);
        assert_eq!(hit(RIFLE, augment), hit(RIFLE, normal));
        assert!(
            hit(Kind::Magnum, augment) > 100 && hit(Kind::Magnum, augment) < 150,
            "a body survives a clean magnum shot"
        );
        assert!(weapons::damage(Kind::Magnum, augment, pb::HitZone::Head) > 200);
    }

    #[test]
    fn upgraded_pistol_has_more_ammo_and_damage() {
        let mut game = with_augment(Aug::PistolUpgrade);
        draw_sidearm(&mut game, SHOOTER);
        assert_eq!(held(&game, SHOOTER), Kind::PistolPlus);
        assert_eq!(weapon(&game, SHOOTER), (12, 120, false));
        assert!(
            weapons::stats(Kind::PistolPlus).damage_body > weapons::stats(Kind::Pistol).damage_body
        );
    }

    #[test]
    fn akimbo_doubles_magazine_and_fire_rate_only_for_the_smg() {
        let mut rifle = with_augment(Aug::Akimbo);
        assert_eq!(held(&rifle, SHOOTER), RIFLE, "needs an SMG");
        rifle.select_weapon(SHOOTER, Kind::Smg);
        assert_eq!(held(&rifle, SHOOTER), Kind::AkimboSmg);
        assert_eq!(
            weapon(&rifle, SHOOTER).0,
            weapons::stats(Kind::Smg).magazine * 2
        );

        let shots = |augment: Aug| {
            let mut game = with_augment(augment);
            game.select_weapon(SHOOTER, Kind::Smg);
            let e = game.entities[&SHOOTER];
            game.world.get_mut::<Weapon>(e).unwrap().cooldown = 0.0;
            let yaw = -std::f32::consts::FRAC_PI_2;
            place(&mut game, SHOOTER, -46.0, 0.0, yaw);
            place(&mut game, VICTIM, -46.0, -8.0, 0.0);
            let mut fired = 0;
            for seq in 1..=30 {
                game.push_inputs(
                    SHOOTER,
                    vec![pb::InputCmd {
                        fire: true,
                        yaw,
                        ..cmd(seq, tick_now(&game))
                    }],
                );
                game.tick();
                fired += shot_events(&game.shared_snapshot().events);
            }
            fired
        };
        let (double, single) = (shots(Aug::Akimbo), shots(Aug::SoulDrain));
        assert!(double >= single * 2 - 1, "akimbo {double}, single {single}");
    }
}
