//! Big Head mode: each King's reign runs down a tick at a time.

use super::weapon::issue_weapons;
use crate::game::components::*;
use crate::game::resources::*;
use crate::game::*;
use bevy_ecs::prelude::*;

/// Big Head mode: runs each King's reign down a tick. When it is over the player is back at
/// big_head::KING_END_LEVEL, the hammer gone, with a fresh set of their own weapons (full
/// ammunition, the primary in hand, as at a respawn).
pub fn update_kings(
    rules: Res<Rules>,
    mut players: Query<(&Player, &mut Perks, &mut Weapon, &Loadout)>,
) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn the_third_level_crowns_a_king_who_holds_only_the_hammer() {
        let mut game = big_head_duel();
        set_level(&mut game, SHOOTER, 2);
        game.world
            .get_mut::<Vitals>(game.entities[&SHOOTER])
            .unwrap()
            .hp = 40;
        assert_eq!(headshot(&mut game).len(), 1);
        let king = perks_of(&game, SHOOTER);
        assert_eq!(
            (king.level, king.head_scale),
            (
                big_head::MAX_LEVEL,
                big_head::head_scale(big_head::MAX_LEVEL)
            )
        );
        // The tick that crowned them is the reign's first.
        assert_eq!(king.king_ticks, big_head::KING_TICKS - 1);
        assert_eq!(hp(&game, SHOOTER), MAX_HP);
        assert_eq!(held(&game, SHOOTER), Kind::Hammer);
        let snapshot = game.shared_snapshot();
        let me = snapshot.players.iter().find(|p| p.id == SHOOTER).unwrap();
        assert!(
            (me.king_left - (big_head::KING_TIME - TICK_DT)).abs() < 1e-3,
            "{}",
            me.king_left
        );
        assert_eq!(me.weapon, Kind::Hammer as i32);

        // Nothing else comes out while the reign lasts: no slot, no grenade, and a primary chosen in
        // the B menu waits for its end even under spawn protection.
        for slot in 1..=4 {
            game.push_inputs(
                SHOOTER,
                vec![pb::InputCmd {
                    slot,
                    ..cmd(10 + slot, tick_now(&game))
                }],
            );
            game.tick();
            assert_eq!(held(&game, SHOOTER), Kind::Hammer, "slot {slot}");
        }
        assert_eq!(grenades_left(&mut game, SHOOTER), 0);
        game.world
            .get_mut::<Vitals>(game.entities[&SHOOTER])
            .unwrap()
            .protect_left = 1.0;
        game.select_weapon(SHOOTER, Kind::Sniper);
        assert_eq!(held(&game, SHOOTER), Kind::Hammer);
    }

    #[test]
    fn the_king_takes_no_harm_from_shots_or_blasts() {
        let mut game = big_head_duel();
        let shot = line_up(&mut game, 0.03);
        make_king(&mut game, VICTIM);
        // A second of fire at the head: every shot stops at the King and does nothing.
        let mut stopped = 0;
        for seq in 1..=30 {
            game.push_inputs(
                SHOOTER,
                vec![pb::InputCmd {
                    seq,
                    render_tick: tick_now(&game),
                    ..shot
                }],
            );
            game.tick();
            for event in game.shared_snapshot().events {
                match event.event {
                    Some(pb::game_event::Event::Shot(s)) => {
                        assert_eq!(s.hit_player_id, VICTIM, "a shot went past the King");
                        stopped += 1;
                    }
                    Some(pb::game_event::Event::Damage(d)) => panic!("the King was hurt: {d:?}"),
                    _ => {}
                }
            }
        }
        assert!(stopped >= 5, "{stopped} shots");
        assert_eq!(hp(&game, VICTIM), MAX_HP);
        // Nor does a grenade at their feet.
        drop_grenade(&mut game, SHOOTER, -42.3, 0.0);
        assert!(kills_in_two_ticks(&mut game).is_empty());
        assert_eq!(hp(&game, VICTIM), MAX_HP);
        assert!(
            game.world
                .get::<Vitals>(game.entities[&VICTIM])
                .unwrap()
                .alive
        );
    }

    #[test]
    fn the_kings_hammer_fells_anyone_with_one_blow_a_cute_shot() {
        let mut game = big_head_duel();
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -46.0, 0.0, yaw);
        place(&mut game, VICTIM, -44.5, 0.0, 0.0);
        make_king(&mut game, SHOOTER);
        // The hammer is drawn like any weapon.
        for _ in 0..12 {
            game.tick();
        }
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
        let kills = kills_now(&mut game);
        assert_eq!(kills.len(), 1, "the blow missed");
        assert_eq!(
            (kills[0].weapon, kills[0].special),
            (Kind::Hammer as i32, pb::SpecialKill::CuteShot as i32)
        );
        // A King's kill scores like any other and leaves the level at the top.
        assert_eq!(game.shared_snapshot().red_score, 1);
        assert_eq!(perks_of(&game, SHOOTER).level, big_head::MAX_LEVEL);
        assert_eq!(perks_of(&game, VICTIM).level, -1);
    }

    #[test]
    fn the_king_runs_a_little_faster_than_anyone_else() {
        let speed = |kind: Kind| weapons::stats(kind).move_speed;
        let fastest = [
            Kind::Rifle,
            Kind::Sniper,
            Kind::MachineGun,
            Kind::Smg,
            Kind::Pistol,
            Kind::Knife,
            Kind::Grenade,
        ]
        .map(speed)
        .into_iter()
        .fold(0.0, f32::max);
        assert!(
            speed(Kind::Hammer) > fastest && speed(Kind::Hammer) < fastest * 1.15,
            "{} against {fastest}",
            speed(Kind::Hammer)
        );

        // And the King moves at it: half a second of running, two commands a tick.
        let mut game = big_head_duel();
        let yaw = -std::f32::consts::FRAC_PI_2;
        place(&mut game, SHOOTER, -40.0, 0.0, yaw);
        make_king(&mut game, SHOOTER);
        for seq in 0..15 {
            let run = |n: u32| pb::InputCmd {
                forward: true,
                yaw,
                ..cmd(n, tick_now(&game))
            };
            game.push_inputs(SHOOTER, vec![run(2 * seq + 1), run(2 * seq + 2)]);
            game.tick();
        }
        let ran = game.world.get::<Pose>(game.entities[&SHOOTER]).unwrap().x + 40.0;
        assert!(
            (ran - speed(Kind::Hammer) * 0.5).abs() < 0.01,
            "ran {ran} m"
        );
    }

    #[test]
    fn a_reign_lasts_twenty_seconds_and_ends_at_level_zero_with_fresh_weapons() {
        assert_eq!(big_head::KING_TICKS, 20 * TICK_RATE);
        let mut game = big_head_duel();
        make_king(&mut game, SHOOTER);
        for _ in 0..big_head::KING_TICKS - 1 {
            game.tick();
        }
        assert!(perks_of(&game, SHOOTER).king(), "the reign ended early");
        assert_eq!(held(&game, SHOOTER), Kind::Hammer);
        game.tick();
        let perks = perks_of(&game, SHOOTER);
        assert!(!perks.king());
        assert_eq!(
            (perks.level, perks.head_scale),
            (big_head::KING_END_LEVEL, 1.0)
        );
        // Their own primary in hand, loaded, and a grenade again.
        assert_eq!(held(&game, SHOOTER), RIFLE);
        assert_eq!(weapon(&game, SHOOTER), (30, 120, false));
        assert_eq!(grenades_left(&mut game, SHOOTER), 1);
    }

    #[test]
    fn a_player_who_reaches_the_top_level_while_down_is_crowned_on_coming_back() {
        let mut game = big_head_duel();
        place(&mut game, SHOOTER, -46.0, 0.0, 0.0);
        place(&mut game, VICTIM, -42.0, 0.0, 0.0);
        set_level(&mut game, SHOOTER, 2);
        {
            let mut vitals = game
                .world
                .get_mut::<Vitals>(game.entities[&SHOOTER])
                .unwrap();
            (vitals.hp, vitals.alive, vitals.respawn_left) = (0, false, 1.0);
        }
        game.world
            .get_mut::<Vitals>(game.entities[&VICTIM])
            .unwrap()
            .hp = 20;
        drop_grenade(&mut game, SHOOTER, -42.3, 0.0);
        assert_eq!(kills_in_two_ticks(&mut game).len(), 1);
        assert_eq!(perks_of(&game, SHOOTER).level, big_head::MAX_LEVEL);
        assert!(!perks_of(&game, SHOOTER).king(), "crowned while down");
        for _ in 0..TICK_RATE {
            game.tick();
        }
        assert!(
            game.world
                .get::<Vitals>(game.entities[&SHOOTER])
                .unwrap()
                .alive
        );
        assert!(perks_of(&game, SHOOTER).king());
        assert_eq!(held(&game, SHOOTER), Kind::Hammer);
    }
}
