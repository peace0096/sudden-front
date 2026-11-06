//! The match's mode and the rules that follow from it.

use crate::pb;
use bevy_ecs::prelude::*;

/// What differs between game modes and match types.
#[derive(Resource)]
pub struct Rules {
    pub mode: pb::GameMode,
    pub max_hp: i32,
    /// Free-for-all: every player's team is `Unspecified` and everyone fights everyone.
    pub ffa: bool,
    /// Big Head mode: kills and deaths move levels and head sizes, and the top level crowns the King.
    pub big_head: bool,
}

impl Rules {
    /// Whether players on these teams can hurt each other. Callers still rule out hurting
    /// yourself: in a free-for-all everyone is on the same (no) team.
    pub fn hostile(&self, a: pb::Team, b: pb::Team) -> bool {
        self.ffa || a != b
    }
}
