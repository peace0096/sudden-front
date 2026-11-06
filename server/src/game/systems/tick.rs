//! The match clock: the last system of every tick.

use crate::game::resources::*;
use bevy_ecs::prelude::*;

pub fn advance_tick(mut state: ResMut<MatchState>) {
    state.tick += 1;
}
