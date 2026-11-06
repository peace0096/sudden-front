//! Queued commands and the intent the input system reads from them.

use crate::pb;
use bevy_ecs::prelude::*;
use std::collections::VecDeque;

#[derive(Component, Default)]
pub struct Inputs {
    pub queue: VecDeque<pb::InputCmd>,
    pub last_seq: u32,
    pub budget: u32,
}

/// What the player's commands asked for this tick.
#[derive(Component, Default)]
pub struct Intent {
    pub fire: Option<FireIntent>,
    pub reload: bool,
    pub moving: bool,
    pub aim: bool,
}

#[derive(Clone, Copy)]
pub struct FireIntent {
    pub yaw: f32,
    pub pitch: f32,
    pub render_tick: u32,
}
