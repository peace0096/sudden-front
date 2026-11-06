//! Position history: the recent poses a lag-compensated shot rewinds its targets to.

use crate::game::components::*;
use crate::game::resources::*;
use bevy_ecs::prelude::*;

/// How far back a shot may rewind targets.
pub(super) const MAX_REWIND_TICKS: u32 = 15;

/// Adds where the player stands at `tick` to their history, which keeps a couple of ticks more
/// than the furthest a shot may rewind.
fn record(history: &mut History, tick: u32, pose: &Pose) {
    history
        .samples
        .push_back((tick, pose.x, pose.y, pose.z, pose.crouching, pose.yaw));
    while history.samples.len() > (MAX_REWIND_TICKS + 2) as usize {
        history.samples.pop_front();
    }
}

pub fn record_history(state: Res<MatchState>, mut players: Query<(&Pose, &mut History)>) {
    for (pose, mut history) in &mut players {
        record(&mut history, state.tick, pose);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_keeps_just_past_the_rewind_window() {
        let mut history = History::default();
        let pose = Pose::default();
        for tick in 0..100 {
            record(&mut history, tick, &pose);
        }
        assert_eq!(history.samples.len(), (MAX_REWIND_TICKS + 2) as usize);
        assert_eq!(history.samples.back().map(|s| s.0), Some(99));
        assert!(history
            .at(99_u32.saturating_sub(MAX_REWIND_TICKS))
            .is_some());
    }

    #[test]
    fn a_recorded_pose_is_found_again_at_its_tick() {
        let mut history = History::default();
        let pose = Pose {
            x: 1.0,
            y: 0.5,
            z: -2.0,
            yaw: 0.3,
            crouching: true,
            ..Default::default()
        };
        record(&mut history, 7, &pose);
        assert_eq!(history.at(7), Some((1.0, 0.5, -2.0, true, 0.3)));
        assert_eq!(history.at(6), None);
    }
}
