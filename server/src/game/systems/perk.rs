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
