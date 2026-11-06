//! Weapon upkeep: cooldown, bloom recovery, reloading and the scope, plus the weapons a player is issued.

use crate::game::components::*;
use crate::game::resources::*;
use crate::game::*;
use crate::pb;
use crate::pb::game_event::Event;
use bevy_ecs::prelude::*;

const BLOOM_DECAY: f32 = 0.09;

fn load_rounds(weapon: &mut Weapon, wanted: u32, infinite: bool) {
    let moved = if infinite {
        wanted
    } else {
        wanted.min(weapon.reserve)
    };
    weapon.magazine = weapon.magazine.saturating_add(moved);
    if !infinite {
        weapon.reserve = weapon.reserve.saturating_sub(moved);
    }
}

/// One tick of a weapon settling: the cooldown runs down, and the bloom recovers once its hold is
/// over. While the trigger is held, or a burst goes on, the cooldown may go slightly negative,
/// which carries the remainder into the next shot. That is what lets fire intervals that aren't a
/// multiple of the tick length average correctly.
fn settle(weapon: &mut Weapon, trigger_held: bool) {
    if weapon.cooldown > 0.0 {
        weapon.cooldown -= TICK_DT;
    }
    if !trigger_held && weapon.burst_left == 0 {
        weapon.cooldown = weapon.cooldown.max(0.0);
    }
    if weapon.bloom_hold > 0.0 {
        weapon.bloom_hold -= TICK_DT;
    } else {
        weapon.bloom = (weapon.bloom - BLOOM_DECAY * TICK_DT).max(0.0);
    }
}

/// Whether there is anything to reload: room in the magazine, and rounds to fill it with.
fn can_reload(weapon: &Weapon, stats: &weapons::Stats, infinite: bool) -> bool {
    weapon.magazine < stats.magazine && (infinite || weapon.reserve > 0)
}

/// One tick of reloading. A running reload counts down and fills the magazine when it is done; a
/// shotgun loads one shell, and goes on to the next while there is room. Otherwise an empty
/// magazine, or `asked` (R), starts a reload. `true` when a reload starts, which players hear.
fn reload(
    weapon: &mut Weapon,
    stats: &weapons::Stats,
    reload_time: f32,
    infinite: bool,
    asked: bool,
) -> bool {
    if weapon.reloading() {
        weapon.reload_left -= TICK_DT;
        if weapon.reload_left <= 0.0 {
            weapon.reload_left = 0.0;
            if stats.shell_reload {
                load_rounds(weapon, 1, infinite);
                if can_reload(weapon, stats, infinite) {
                    weapon.reload_left = reload_time;
                    return true;
                }
            } else {
                let wanted = stats.magazine.saturating_sub(weapon.magazine);
                load_rounds(weapon, wanted, infinite);
            }
        }
        return false;
    }
    // An empty magazine reloads by itself; R reloads early.
    if !(asked || weapon.magazine == 0) || !can_reload(weapon, stats, infinite) {
        return false;
    }
    if reload_time <= 0.0 {
        let wanted = stats.magazine.saturating_sub(weapon.magazine);
        load_rounds(weapon, wanted, infinite);
    } else {
        weapon.reload_left = reload_time;
        weapon.cooldown = weapon.cooldown.max(0.0);
    }
    true
}

/// How long the scope has been held, after this tick. It drops out while the bolt cycles and
/// during a reload.
fn scope_time(weapon: &Weapon, has_scope: bool, aiming: bool) -> f32 {
    if has_scope && aiming && !weapon.reloading() && weapon.cooldown <= 0.0 {
        weapon.aim_time + TICK_DT
    } else {
        0.0
    }
}

pub fn update_weapons(
    mut log: ResMut<EventLog>,
    mut players: Query<(&Player, &mut Weapon, &Intent, &Vitals, &Perks)>,
) {
    for (player, mut weapon, intent, vitals, perks) in &mut players {
        let stats = weapons::stats(weapon.kind);
        settle(&mut weapon, intent.fire.is_some());
        if !vitals.alive || vitals.stun_left > 0.0 {
            continue;
        }
        // Grenades are thrown, not reloaded, Infinite Ammo or not: a life has the ones it starts with.
        if weapon.kind == pb::WeaponKind::Grenade {
            continue;
        }
        let infinite = perks.has(pb::Augment::InfiniteAmmo);
        let reload_time = weapons::reload_time(stats, &perks.active);
        if reload(&mut weapon, stats, reload_time, infinite, intent.reload) {
            log.push(Event::Reload(pb::ReloadEvent {
                player_id: player.id,
            }));
        }
        weapon.aim_time = scope_time(&weapon, stats.has_scope, intent.aim);
    }
}

/// The slot 1 weapon an augment comes with, which the player can't change.
pub fn forced_primary(active: &[pb::Augment]) -> Option<pb::WeaponKind> {
    active
        .contains(&pb::Augment::Missile)
        .then_some(pb::WeaponKind::Missile)
}

/// A spawning player's weapons. Bots never change slot, so those whose augment
/// lives in slot 2 start with it in hand.
pub fn issue_weapons(primary: pb::WeaponKind, active: &[pb::Augment], is_bot: bool) -> Weapon {
    let mut weapon = Weapon::equipped(primary, active);
    if is_bot && (weapon.sidearm != pb::WeaponKind::Pistol || active.contains(&pb::Augment::Shield))
    {
        weapon.switch_to(2);
        weapon.cooldown = 0.0;
    }
    weapon
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn loading_takes_rounds_from_the_reserve_unless_ammo_is_infinite() {
        let mut weapon = Weapon::new(Kind::Rifle);
        weapon.magazine = 0;
        weapon.reserve = 10;
        load_rounds(&mut weapon, 30, false);
        assert_eq!((weapon.magazine, weapon.reserve), (10, 0));
        load_rounds(&mut weapon, 5, true);
        assert_eq!((weapon.magazine, weapon.reserve), (15, 0));
    }

    #[test]
    fn the_cooldown_carries_its_remainder_only_while_the_trigger_is_held() {
        let mut weapon = Weapon::new(Kind::Rifle);
        weapon.cooldown = 0.01;
        settle(&mut weapon, true);
        assert!(weapon.cooldown < 0.0);
        weapon.cooldown = 0.01;
        settle(&mut weapon, false);
        assert_eq!(weapon.cooldown, 0.0);
    }

    #[test]
    fn bloom_recovers_only_once_its_hold_is_over_and_never_below_zero() {
        let mut weapon = Weapon::new(Kind::Rifle);
        weapon.bloom = 0.05;
        weapon.bloom_hold = 0.5;
        settle(&mut weapon, false);
        assert_eq!(weapon.bloom, 0.05);
        weapon.bloom_hold = 0.0;
        settle(&mut weapon, false);
        assert!(weapon.bloom < 0.05);
        weapon.bloom = 0.0;
        settle(&mut weapon, false);
        assert_eq!(weapon.bloom, 0.0);
    }

    #[test]
    fn an_empty_magazine_starts_a_reload_by_itself_and_a_full_one_never_does() {
        let stats = weapons::stats(Kind::Rifle);
        let mut weapon = Weapon::new(Kind::Rifle);
        assert!(!reload(&mut weapon, stats, 1.0, false, true));
        weapon.magazine = 0;
        assert!(reload(&mut weapon, stats, 1.0, false, false));
        assert!(weapon.reloading());
    }

    #[test]
    fn a_reload_fills_the_magazine_from_the_reserve_when_its_time_is_up() {
        let stats = weapons::stats(Kind::Rifle);
        let mut weapon = Weapon::new(Kind::Rifle);
        let reserve = weapon.reserve;
        weapon.magazine = 5;
        assert!(reload(&mut weapon, stats, 2.0 * TICK_DT, false, true));
        assert!(!reload(&mut weapon, stats, 2.0 * TICK_DT, false, false));
        assert_eq!(weapon.magazine, 5);
        assert!(!reload(&mut weapon, stats, 2.0 * TICK_DT, false, false));
        assert_eq!(weapon.magazine, stats.magazine);
        let used = stats.magazine.saturating_sub(5);
        assert_eq!(weapon.reserve, reserve.saturating_sub(used));
        assert!(!weapon.reloading());
    }

    #[test]
    fn without_a_reserve_only_infinite_ammo_reloads() {
        let stats = weapons::stats(Kind::Rifle);
        let mut weapon = Weapon::new(Kind::Rifle);
        weapon.magazine = 0;
        weapon.reserve = 0;
        assert!(!reload(&mut weapon, stats, 1.0, false, true));
        assert!(reload(&mut weapon, stats, 1.0, true, true));
    }

    #[test]
    fn an_instant_reload_fills_the_magazine_at_once() {
        let stats = weapons::stats(Kind::Rifle);
        let mut weapon = Weapon::new(Kind::Rifle);
        weapon.magazine = 0;
        assert!(reload(&mut weapon, stats, 0.0, false, false));
        assert_eq!(weapon.magazine, stats.magazine);
        assert!(!weapon.reloading());
    }

    #[test]
    fn a_shotgun_loads_a_shell_per_reload_and_goes_on_while_there_is_room() {
        let stats = weapons::stats(Kind::Shotgun);
        assert!(stats.shell_reload);
        let mut weapon = Weapon::new(Kind::Shotgun);
        weapon.magazine = 0;
        assert!(reload(&mut weapon, stats, TICK_DT, false, false));
        assert!(reload(&mut weapon, stats, TICK_DT, false, false));
        assert_eq!(weapon.magazine, 1);
        assert!(weapon.reloading());
    }

    #[test]
    fn the_scope_counts_only_while_aiming_a_ready_scoped_weapon() {
        let mut weapon = Weapon::new(Kind::Sniper);
        weapon.aim_time = 0.1;
        assert_eq!(scope_time(&weapon, true, true), 0.1 + TICK_DT);
        assert_eq!(scope_time(&weapon, false, true), 0.0);
        assert_eq!(scope_time(&weapon, true, false), 0.0);
        weapon.cooldown = 0.5;
        assert_eq!(scope_time(&weapon, true, true), 0.0);
    }

    #[test]
    fn only_the_missile_augment_forces_a_primary() {
        assert_eq!(forced_primary(&[Aug::Missile]), Some(Kind::Missile));
        assert_eq!(forced_primary(&[Aug::Taser, Aug::Vest]), None);
    }

    #[test]
    fn a_human_starts_with_the_primary_in_hand_and_a_bot_with_its_slot_two_augment() {
        assert_eq!(
            issue_weapons(Kind::Rifle, &[Aug::Taser], false).kind,
            Kind::Rifle
        );
        assert_eq!(
            issue_weapons(Kind::Rifle, &[Aug::Taser], true).kind,
            Kind::Taser
        );
        assert_eq!(issue_weapons(Kind::Rifle, &[], true).kind, Kind::Rifle);
    }

    #[test]
    fn empty_magazine_reloads_automatically() {
        let mut game = duel();
        game.world
            .get_mut::<Weapon>(game.entities[&SHOOTER])
            .unwrap()
            .magazine = 0;
        game.tick();
        assert!(
            weapon(&game, SHOOTER).2,
            "reload should start with no R press"
        );
        for _ in 0..(weapons::stats(RIFLE).reload_time * TICK_RATE as f32) as u32 + 2 {
            game.tick();
        }
        assert_eq!(weapon(&game, SHOOTER), (30, 90, false));
    }

    #[test]
    fn no_auto_reload_without_reserve() {
        let mut game = duel();
        let entity = game.entities[&SHOOTER];
        {
            let mut w = game.world.get_mut::<Weapon>(entity).unwrap();
            w.magazine = 0;
            w.reserve = 0;
        }
        game.tick();
        assert_eq!(weapon(&game, SHOOTER), (0, 0, false));
    }

    #[test]
    fn scope_needs_a_moment_and_only_counts_for_scoped_weapons() {
        use pb::WeaponKind::*;
        let sniper = weapons::stats(Sniper);
        assert!(
            systems::fire::shot_cone(sniper, true, false, 0.0)
                < systems::fire::shot_cone(sniper, false, false, 0.0) / 10.0
        );
        let rifle = weapons::stats(Rifle);
        assert_eq!(
            systems::fire::shot_cone(rifle, true, false, 0.0),
            systems::fire::shot_cone(rifle, false, false, 0.0)
        );

        let mut game = duel();
        game.select_weapon(SHOOTER, Sniper);
        let entity = game.entities[&SHOOTER];
        for seq in 0..3 {
            game.push_inputs(
                SHOOTER,
                vec![pb::InputCmd {
                    aim: true,
                    ..cmd(seq + 1, 0)
                }],
            );
            game.tick();
        }
        let early = game.world.get::<Weapon>(entity).unwrap().aim_time;
        assert!(early < systems::fire::SCOPE_READY_TIME);
        for seq in 3..12 {
            game.push_inputs(
                SHOOTER,
                vec![pb::InputCmd {
                    aim: true,
                    ..cmd(seq + 1, 0)
                }],
            );
            game.tick();
        }
        assert!(
            game.world.get::<Weapon>(entity).unwrap().aim_time >= systems::fire::SCOPE_READY_TIME
        );
    }

    #[test]
    fn bloom_recovers_once_the_trigger_is_released() {
        let mut game = duel();
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -46.0, 0.0, yaw);
        place(&mut game, VICTIM, -46.0, -8.0, 0.0);
        for seq in 1..=30 {
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
        // Ten shots in a second fill the rifle's bloom.
        assert!(game.world.get::<Weapon>(e).unwrap().bloom > weapons::stats(RIFLE).bloom_max * 0.8);
        for _ in 0..TICK_RATE {
            game.tick();
        }
        assert_eq!(game.world.get::<Weapon>(e).unwrap().bloom, 0.0);
    }

    #[test]
    fn infinite_ammo_reloads_without_spending_reserve() {
        let mut game = with_augment(Aug::InfiniteAmmo);
        let e = game.entities[&SHOOTER];
        {
            let mut w = game.world.get_mut::<Weapon>(e).unwrap();
            w.magazine = 0;
            w.reserve = 0;
        }
        for _ in 0..(weapons::stats(RIFLE).reload_time * TICK_RATE as f32) as u32 + 3 {
            game.tick();
        }
        assert_eq!(weapon(&game, SHOOTER), (30, 0, false));
    }

    #[test]
    fn reload_augments_change_the_time_it_takes() {
        let base = reload_seconds(Aug::SoulDrain);
        assert!((base - 2.2).abs() < 0.1, "{base}");
        assert!((reload_seconds(Aug::QuickReload) - base / 2.0).abs() < 0.1);
        assert!((reload_seconds(Aug::NoRecoil) - base * 2.0).abs() < 0.1);
        assert_eq!(reload_seconds(Aug::Borrow), 0.0, "no reload motion at all");
    }

    #[test]
    fn borrow_still_spends_reserve() {
        let mut game = with_augment(Aug::Borrow);
        let e = game.entities[&SHOOTER];
        game.world.get_mut::<Weapon>(e).unwrap().magazine = 10;
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                reload: true,
                ..cmd(1, 0)
            }],
        );
        game.tick();
        assert_eq!(weapon(&game, SHOOTER), (30, 100, false));
    }

    #[test]
    fn shotgun_loads_a_shell_at_a_time_and_can_fire_mid_reload() {
        let mut game = with_augment(Aug::Shotgun);
        let e = game.entities[&SHOOTER];
        draw_sidearm(&mut game, SHOOTER);
        game.world.get_mut::<Weapon>(e).unwrap().magazine = 2;
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                reload: true,
                ..cmd(1, 0)
            }],
        );
        let shell_ticks = (weapons::stats(Kind::Shotgun).reload_time * TICK_RATE as f32) as u32 + 2;
        for _ in 0..shell_ticks {
            game.tick();
        }
        let (magazine, reserve, reloading) = weapon(&game, SHOOTER);
        assert_eq!((magazine, reserve), (3, 35), "one shell in");
        assert!(reloading, "and on to the next");

        let yaw = -std::f32::consts::FRAC_PI_2;
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                fire: true,
                yaw,
                ..cmd(2, tick_now(&game))
            }],
        );
        game.tick();
        let (magazine, _, reloading) = weapon(&game, SHOOTER);
        assert_eq!(magazine, 2, "fired during the reload");
        assert!(!reloading, "firing stops the reload");
    }
}
