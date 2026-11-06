//! The weapons a player carries, slot by slot, and the rules for switching between them.

use super::input::FireIntent;
use crate::game::{weapons, DRAW_TIME};
use crate::pb;
use bevy_ecs::prelude::*;

#[derive(Component)]
pub struct Weapon {
    /// The weapon in hand; `magazine`, `reserve` and the timers belong to it.
    pub kind: pb::WeaponKind,
    /// What slot 1 holds for this life.
    pub primary: pb::WeaponKind,
    /// What slot 2 holds.
    pub sidearm: pb::WeaponKind,
    /// Index of the slot in hand, and the ammo left in the holstered ones. Slot 4's (index 3)
    /// magazine is the grenades left.
    pub active_slot: usize,
    pub holstered: [(u32, u32); 4],
    /// Index of the slot in hand before the grenade was drawn: a throw goes back to it.
    pub last_slot: usize,
    /// Seconds the scope has been steady; see `SCOPE_READY_TIME`.
    pub aim_time: f32,
    pub magazine: u32,
    pub reserve: u32,
    pub cooldown: f32,
    pub reload_left: f32,
    /// Extra cone angle from sustained fire, in radians.
    pub bloom: f32,
    /// Seconds until bloom starts recovering; each shot restarts this.
    pub bloom_hold: f32,
    /// Rounds still to come in the upgraded pistol's burst, and where it aims.
    pub burst_left: u32,
    pub burst_fire: Option<FireIntent>,
}

impl Weapon {
    pub fn new(kind: pb::WeaponKind) -> Self {
        Self::equipped(kind, &[])
    }

    /// A fresh weapon set for a player with these augments.
    pub fn equipped(kind: pb::WeaponKind, active: &[pb::Augment]) -> Self {
        let kind = weapons::primary_for(kind, active);
        let stats = weapons::stats(kind);
        let sidearm = weapons::sidearm_for(active);
        let side = weapons::stats(sidearm);
        let grenades = weapons::stats(pb::WeaponKind::Grenade).magazine;
        Self {
            kind,
            primary: kind,
            sidearm,
            active_slot: 0,
            holstered: [
                (stats.magazine, stats.reserve),
                (side.magazine, side.reserve),
                (0, 0),
                (grenades, 0),
            ],
            last_slot: 0,
            aim_time: 0.0,
            magazine: stats.magazine,
            reserve: stats.reserve,
            cooldown: 0.0,
            reload_left: 0.0,
            bloom: 0.0,
            bloom_hold: 0.0,
            burst_left: 0,
            burst_fire: None,
        }
    }

    /// Replaces what slot 1 holds, with a full load.
    pub fn set_primary(&mut self, kind: pb::WeaponKind) {
        let stats = weapons::stats(kind);
        self.primary = kind;
        if self.active_slot == 0 {
            self.kind = kind;
            self.magazine = stats.magazine;
            self.reserve = stats.reserve;
            self.reload_left = 0.0;
            self.aim_time = 0.0;
            self.bloom = 0.0;
            self.bloom_hold = 0.0;
            self.burst_left = 0;
            self.cooldown = self.cooldown.max(DRAW_TIME);
        } else {
            self.holstered[0] = (stats.magazine, stats.reserve);
        }
    }

    /// Takes a slot (1 primary, 2 pistol, 3 knife, 4 grenade) in hand. Ammo stays with each
    /// weapon, a reload in progress is abandoned, and the new weapon must be drawn. The grenade
    /// only comes out while one is left, and nothing does while the King's hammer is in hand.
    pub fn switch_to(&mut self, slot: u32) {
        if self.kind == pb::WeaponKind::Hammer {
            return;
        }
        let (index, kind) = match slot {
            1 => (0, self.primary),
            2 => (1, self.sidearm),
            3 => (2, pb::WeaponKind::Knife),
            4 if self.grenades() > 0 => (GRENADE_SLOT, pb::WeaponKind::Grenade),
            _ => return,
        };
        if index == self.active_slot {
            return;
        }
        if index == GRENADE_SLOT {
            self.last_slot = self.active_slot;
        }
        self.holstered[self.active_slot] = (self.magazine, self.reserve);
        (self.magazine, self.reserve) = self.holstered[index];
        self.active_slot = index;
        self.kind = kind;
        self.reload_left = 0.0;
        self.aim_time = 0.0;
        self.bloom = 0.0;
        self.bloom_hold = 0.0;
        self.burst_left = 0;
        self.cooldown = self.cooldown.max(DRAW_TIME);
    }

    /// Big Head mode: the King's hammer in hand, drawn like any weapon, and nothing else for the
    /// reign. switch_to refuses while it is held and the grenade is gone; the reign's end issues
    /// a fresh set (systems::update_kings), so nothing else is kept.
    pub fn take_hammer(&mut self) {
        self.kind = pb::WeaponKind::Hammer;
        self.magazine = 0;
        self.reserve = 0;
        self.holstered[GRENADE_SLOT] = (0, 0);
        self.reload_left = 0.0;
        self.aim_time = 0.0;
        self.bloom = 0.0;
        self.bloom_hold = 0.0;
        self.burst_left = 0;
        self.burst_fire = None;
        self.cooldown = self.cooldown.max(DRAW_TIME);
    }

    pub fn reloading(&self) -> bool {
        self.reload_left > 0.0
    }

    /// Grenades left to throw, in hand or not.
    pub fn grenades(&self) -> u32 {
        if self.active_slot == GRENADE_SLOT {
            self.magazine
        } else {
            self.holstered[GRENADE_SLOT].0
        }
    }

    /// The slot (1 to 3) a throw goes back to.
    pub fn slot_before_grenade(&self) -> u32 {
        (self.last_slot as u32).saturating_add(1)
    }
}

/// Index of the grenade's slot (4) in `Weapon::holstered`.
pub const GRENADE_SLOT: usize = 3;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn spawns_with_30_mag_and_120_reserve() {
        assert_eq!(weapon(&duel(), SHOOTER), (30, 120, false));
    }

    #[test]
    fn slots_keep_their_own_ammo() {
        let mut game = duel();
        let e = game.entities[&SHOOTER];
        game.world.get_mut::<Weapon>(e).unwrap().magazine = 17;
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                slot: 2,
                ..cmd(1, 0)
            }],
        );
        game.tick();
        assert_eq!(weapon(&game, SHOOTER), (12, 36, false));
        assert_eq!(
            game.world.get::<Weapon>(e).unwrap().kind,
            pb::WeaponKind::Pistol
        );

        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                slot: 3,
                ..cmd(2, 0)
            }],
        );
        game.tick();
        assert_eq!(
            game.world.get::<Weapon>(e).unwrap().kind,
            pb::WeaponKind::Knife
        );

        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                slot: 1,
                ..cmd(3, 0)
            }],
        );
        game.tick();
        assert_eq!(weapon(&game, SHOOTER), (17, 120, false));
        assert_eq!(game.world.get::<Weapon>(e).unwrap().kind, RIFLE);
    }

    #[test]
    fn switching_abandons_a_reload_and_takes_time_to_draw() {
        let mut game = duel();
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
        assert!(weapon(&game, SHOOTER).2);

        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                slot: 2,
                fire: true,
                ..cmd(2, 0)
            }],
        );
        game.tick();
        assert_eq!(
            weapon(&game, SHOOTER),
            (12, 36, false),
            "no shot during the draw"
        );

        for _ in 0..(DRAW_TIME * TICK_RATE as f32) as u32 + 1 {
            game.tick();
        }
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                fire: true,
                ..cmd(3, 0)
            }],
        );
        game.tick();
        assert_eq!(weapon(&game, SHOOTER).0, 11);

        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                slot: 1,
                ..cmd(4, 0)
            }],
        );
        game.tick();
        assert_eq!(
            weapon(&game, SHOOTER),
            (10, 120, false),
            "the reload did not complete"
        );
    }

    #[test]
    fn switching_cannot_skip_a_bolt_cycle() {
        let mut game = duel();
        game.select_weapon(SHOOTER, pb::WeaponKind::Sniper);
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                fire: true,
                ..cmd(1, 0)
            }],
        );
        game.tick();
        let e = game.entities[&SHOOTER];
        let before = game.world.get::<Weapon>(e).unwrap().cooldown;
        assert!(before > DRAW_TIME);
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                slot: 2,
                ..cmd(2, 0)
            }],
        );
        game.push_inputs(
            SHOOTER,
            vec![pb::InputCmd {
                slot: 1,
                ..cmd(3, 0)
            }],
        );
        game.tick();
        assert!(game.world.get::<Weapon>(e).unwrap().cooldown > DRAW_TIME);
    }
}
