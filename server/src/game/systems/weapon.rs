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
            if wants_reload && weapon.magazine < stats.magazine && (infinite || weapon.reserve > 0)
            {
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
