//! A bot's memory between ticks.

use bevy_ecs::prelude::*;

#[derive(Component, Default)]
pub struct BotBrain {
    pub path: Vec<(f32, f32)>,
    pub path_index: usize,
    pub repath_in: f32,
    pub target: Option<Entity>,
    pub tracked_for: f32,
    pub burst_left: u32,
    pub burst_pause: f32,
    pub strafe_left: bool,
    pub strafe_timer: f32,
    pub aim_error_yaw: f32,
    pub aim_error_pitch: f32,
    pub stuck_time: f32,
    pub last_x: f32,
    pub last_z: f32,
    /// Taking out the grenade to throw it at the target, or about to.
    pub throwing: bool,
}
