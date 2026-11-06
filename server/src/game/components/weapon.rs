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

    /// A full load of `kind`: its magazine and reserve.
    fn full(kind: Kind) -> (u32, u32) {
        let stats = weapons::stats(kind);
        (stats.magazine, stats.reserve)
    }

    #[test]
    fn a_fresh_set_holds_the_primary_fully_loaded_with_a_pistol_a_knife_and_grenades() {
        let w = Weapon::new(Kind::Sniper);
        assert_eq!(
            (w.kind, w.primary, w.sidearm, w.active_slot),
            (Kind::Sniper, Kind::Sniper, Kind::Pistol, 0)
        );
        assert_eq!((w.magazine, w.reserve), full(Kind::Sniper));
        let grenades = weapons::stats(Kind::Grenade).magazine;
        assert_eq!(
            w.holstered[1..],
            [full(Kind::Pistol), (0, 0), (grenades, 0)]
        );
        assert_eq!(w.grenades(), grenades);
        assert!(!w.reloading());
    }

    #[test]
    fn equipping_with_augments_follows_the_weapon_rules_for_each_slot() {
        let w = Weapon::equipped(Kind::Smg, &[Aug::Akimbo]);
        assert_eq!((w.kind, w.primary), (Kind::AkimboSmg, Kind::AkimboSmg));
        assert_eq!((w.magazine, w.reserve), full(Kind::AkimboSmg));
        let w = Weapon::equipped(RIFLE, &[Aug::Shotgun]);
        assert_eq!((w.kind, w.sidearm), (RIFLE, Kind::Shotgun));
        assert_eq!(w.holstered[1], full(Kind::Shotgun));
    }

    #[test]
    fn switching_brings_out_the_slots_own_ammo_drops_a_reload_and_has_to_draw() {
        let mut w = Weapon::new(RIFLE);
        w.magazine = 17;
        w.reload_left = 1.0;
        w.bloom = 0.01;
        w.aim_time = 0.5;
        w.switch_to(2);
        assert_eq!((w.kind, w.active_slot), (Kind::Pistol, 1));
        assert_eq!((w.magazine, w.reserve), full(Kind::Pistol));
        assert!(!w.reloading(), "the reload is dropped");
        assert_eq!((w.bloom, w.aim_time, w.cooldown), (0.0, 0.0, DRAW_TIME));
        w.switch_to(3);
        assert_eq!((w.kind, w.magazine, w.reserve), (Kind::Knife, 0, 0));
        w.switch_to(1);
        assert_eq!((w.kind, w.magazine, w.reserve), (RIFLE, 17, full(RIFLE).1));
        // A longer wait, such as a bolt cycle, outlasts the draw.
        w.cooldown = DRAW_TIME * 3.0;
        w.switch_to(2);
        assert_eq!(w.cooldown, DRAW_TIME * 3.0);
    }

    #[test]
    fn taking_the_slot_in_hand_or_one_that_does_not_exist_changes_nothing() {
        let mut w = Weapon::new(RIFLE);
        w.magazine = 9;
        w.reload_left = 1.0;
        for slot in [0, 1, 5, 99] {
            w.switch_to(slot);
            assert_eq!(
                (w.kind, w.active_slot, w.magazine, w.cooldown),
                (RIFLE, 0, 9, 0.0),
                "slot {slot}"
            );
            assert!(w.reloading(), "slot {slot}");
        }
    }

    #[test]
    fn the_grenade_comes_out_only_while_one_is_left_and_a_throw_goes_back_where_it_came_from() {
        for from in 1..=3 {
            let mut w = Weapon::new(RIFLE);
            w.switch_to(from);
            w.switch_to(4);
            assert_eq!((w.kind, w.active_slot), (Kind::Grenade, GRENADE_SLOT));
            assert_eq!(w.grenades(), w.magazine, "the grenades in hand");
            assert_eq!(w.slot_before_grenade(), from);
        }
        let mut w = Weapon::new(RIFLE);
        w.switch_to(4);
        // The last one thrown.
        w.magazine = 0;
        assert_eq!(w.grenades(), 0);
        w.switch_to(w.slot_before_grenade());
        assert_eq!((w.kind, w.grenades()), (RIFLE, 0));
        w.switch_to(4);
        assert_eq!(w.kind, RIFLE, "none left to draw");
    }

    #[test]
    fn a_new_primary_is_drawn_full_if_in_hand_and_waits_full_in_its_holster_if_not() {
        let mut w = Weapon::new(RIFLE);
        w.magazine = 3;
        w.reload_left = 1.0;
        w.set_primary(Kind::Smg);
        assert_eq!((w.kind, w.primary), (Kind::Smg, Kind::Smg));
        assert_eq!((w.magazine, w.reserve), full(Kind::Smg));
        assert!(!w.reloading());
        assert_eq!(w.cooldown, DRAW_TIME);

        let mut w = Weapon::new(RIFLE);
        w.switch_to(2);
        w.magazine = 5;
        w.set_primary(Kind::Sniper);
        assert_eq!(
            (w.kind, w.magazine, w.primary),
            (Kind::Pistol, 5, Kind::Sniper),
            "the pistol in hand stays as it was"
        );
        w.switch_to(1);
        assert_eq!(w.kind, Kind::Sniper);
        assert_eq!((w.magazine, w.reserve), full(Kind::Sniper));
    }

    #[test]
    fn the_hammer_replaces_whatever_is_in_hand_and_nothing_replaces_it() {
        for slot in [1, 4] {
            let mut w = Weapon::new(RIFLE);
            w.switch_to(slot);
            w.reload_left = 1.0;
            w.burst_left = 2;
            w.burst_fire = Some(FireIntent {
                yaw: 0.0,
                pitch: 0.0,
                render_tick: 0,
            });
            w.take_hammer();
            assert_eq!(
                (w.kind, w.magazine, w.reserve, w.grenades()),
                (Kind::Hammer, 0, 0, 0),
                "from slot {slot}"
            );
            assert!(!w.reloading() && w.burst_left == 0 && w.burst_fire.is_none());
            assert!(w.cooldown >= DRAW_TIME);
            for other in 1..=4 {
                w.switch_to(other);
                assert_eq!(w.kind, Kind::Hammer, "slot {other}");
            }
        }
    }

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
