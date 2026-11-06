//! Augment effects that run every tick: Rest heals a player crouching out of combat.

use crate::game::components::*;
use crate::game::resources::*;
use crate::game::*;
use crate::pb;
use bevy_ecs::prelude::*;

/// Rest: heals a player crouching out of combat.
pub fn update_perks(rules: Res<Rules>, mut players: Query<(&Pose, &mut Vitals, &mut Perks)>) {
    for (pose, mut vitals, mut perks) in &mut players {
        if !perks.has(pb::Augment::Rest) || !vitals.alive {
            continue;
        }
        perks.idle += TICK_DT;
        if perks.idle < augments::REST_DELAY {
            continue;
        }
        let cap = augments::max_hp(rules.max_hp, &perks.active);
        if !pose.crouching || vitals.hp >= cap {
            continue;
        }
        perks.regen += augments::REST_HEAL_PER_SECOND * TICK_DT;
        let whole = perks.regen as i32;
        perks.regen -= whole as f32;
        vitals.hp = (vitals.hp + whole).min(cap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

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
