//! The map a match is played on, with its navigation grid and spawn points.

use crate::game::nav::NavGrid;
use crate::pb;
use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct Arena {
    pub map: pb::MapData,
    pub nav: NavGrid,
    pub spawns: Vec<(f32, f32)>,
    /// Where anyone may spawn in a free-for-all, in world coordinates.
    pub ffa_spawns: Vec<(f32, f32)>,
    pub lanes: Vec<(f32, f32)>,
}
