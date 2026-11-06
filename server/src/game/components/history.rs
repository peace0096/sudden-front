//! Recent poses kept for lag compensation.

use bevy_ecs::prelude::*;
use std::collections::VecDeque;

/// Recent positions for rewinding targets under lag compensation.
#[derive(Component, Default)]
pub struct History {
    /// (tick, x, feet height, z, crouching, yaw).
    pub samples: VecDeque<(u32, f32, f32, f32, bool, f32)>,
}

impl History {
    /// (x, feet height, z, crouching, yaw) as of `tick`.
    pub fn at(&self, tick: u32) -> Option<(f32, f32, f32, bool, f32)> {
        self.samples
            .iter()
            .rev()
            .find(|(t, ..)| *t <= tick)
            .map(|(_, x, y, z, crouching, yaw)| (*x, *y, *z, *crouching, *yaw))
    }
}
