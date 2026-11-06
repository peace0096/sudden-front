//! Augment effects that run every tick: Rest heals a player crouching out of combat.

use crate::game::components::*;
use crate::game::resources::*;
use crate::game::*;
use crate::pb;
use bevy_ecs::prelude::*;

/// One tick of Rest for a living player who has it. Time out of combat adds up; once it reaches
/// REST_DELAY, a crouching player heals REST_HEAL_PER_SECOND up to `cap`, a whole HP at a time with
/// the fraction carried to the next tick.
fn rest(perks: &mut Perks, vitals: &mut Vitals, crouching: bool, cap: i32) {
    perks.idle += TICK_DT;
    if perks.idle < augments::REST_DELAY || !crouching || vitals.hp >= cap {
        return;
    }
    perks.regen += augments::REST_HEAL_PER_SECOND * TICK_DT;
    let whole = perks.regen as i32;
    perks.regen -= whole as f32;
    vitals.hp = vitals.hp.saturating_add(whole).min(cap);
}

/// Rest: heals a player crouching out of combat.
pub fn update_perks(rules: Res<Rules>, mut players: Query<(&Pose, &mut Vitals, &mut Perks)>) {
    for (pose, mut vitals, mut perks) in &mut players {
        if !perks.has(pb::Augment::Rest) || !vitals.alive {
            continue;
        }
        let cap = augments::max_hp(rules.max_hp, &perks.active);
        rest(&mut perks, &mut vitals, pose.crouching, cap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    fn vitals(hp: i32) -> Vitals {
        Vitals {
            hp,
            alive: true,
            respawn_left: 0.0,
            protect_left: 0.0,
            stun_left: 0.0,
        }
    }

    /// Rests for `seconds`, crouched or not, and returns the HP gained.
    fn rest_for(seconds: f32, perks: &mut Perks, crouching: bool) -> i32 {
        let mut vitals = vitals(10);
        for _ in 0..(seconds * TICK_RATE as f32).round() as u32 {
            rest(perks, &mut vitals, crouching, MAX_HP);
        }
        vitals.hp.saturating_sub(10)
    }

    #[test]
    fn rest_heals_nothing_until_its_delay_is_over() {
        let mut perks = Perks::default();
        assert_eq!(rest_for(augments::REST_DELAY - 0.5, &mut perks, true), 0);
    }

    #[test]
    fn a_crouching_player_heals_at_rests_rate_once_out_of_combat() {
        let mut perks = Perks {
            idle: augments::REST_DELAY,
            ..Default::default()
        };
        let healed = rest_for(2.0, &mut perks, true);
        let expected = (augments::REST_HEAL_PER_SECOND * 2.0) as i32;
        assert!(healed.abs_diff(expected) <= 1, "healed {healed}");
    }

    #[test]
    fn standing_up_or_full_health_stops_the_healing() {
        let mut perks = Perks {
            idle: augments::REST_DELAY,
            ..Default::default()
        };
        assert_eq!(rest_for(2.0, &mut perks, false), 0);
        let mut full = vitals(MAX_HP);
        rest(&mut perks, &mut full, true, MAX_HP);
        assert_eq!(full.hp, MAX_HP);
    }

    #[test]
    fn rest_heals_only_when_crouched_and_out_of_combat() {
        let mut game = with_augment(Aug::Rest);
        let e = game.entities[&SHOOTER];
        game.world.get_mut::<Vitals>(e).unwrap().hp = 50;
        game.world.get_mut::<Vitals>(e).unwrap().protect_left = 0.0;
        let crouch = |game: &mut Match, seq: u32, down: bool| {
            game.push_inputs(
                SHOOTER,
                vec![pb::InputCmd {
                    crouch: down,
                    ..cmd(seq, 0)
                }],
            );
            game.tick();
        };
        // Standing never heals.
        for seq in 1..=(TICK_RATE * 5) {
            crouch(&mut game, seq, false);
        }
        assert_eq!(hp(&game, SHOOTER), 50);

        // Fresh from a fight: four seconds must pass first.
        game.world.get_mut::<Perks>(e).unwrap().idle = 0.0;
        for seq in 1..=(TICK_RATE * 3) {
            crouch(&mut game, 1000 + seq, true);
        }
        assert_eq!(hp(&game, SHOOTER), 50, "still in combat");
        for seq in 1..=(TICK_RATE * 3) {
            crouch(&mut game, 2000 + seq, true);
        }
        let healed = hp(&game, SHOOTER);
        assert!(healed > 50 && healed < 150, "gradual, got {healed}");
        for seq in 1..=(TICK_RATE * 10) {
            crouch(&mut game, 3000 + seq, true);
        }
        assert_eq!(hp(&game, SHOOTER), 150, "stops at the maximum");
    }
}
