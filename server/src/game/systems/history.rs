//! Position history: the recent poses a lag-compensated shot rewinds its targets to.

use crate::game::components::*;
use crate::game::resources::*;
use bevy_ecs::prelude::*;

/// How far back a shot may rewind targets.
pub(super) const MAX_REWIND_TICKS: u32 = 15;

pub fn record_history(state: Res<MatchState>, mut players: Query<(&Pose, &mut History)>) {
    for (pose, mut history) in &mut players {
        history
            .samples
            .push_back((state.tick, pose.x, pose.y, pose.z, pose.crouching, pose.yaw));
        while history.samples.len() > (MAX_REWIND_TICKS + 2) as usize {
            history.samples.pop_front();
        }
    }
}
