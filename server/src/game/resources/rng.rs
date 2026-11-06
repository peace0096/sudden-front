//! The match's seeded random numbers.

use bevy_ecs::prelude::*;

/// Small seeded xorshift so matches and tests are reproducible.
#[derive(Resource)]
pub struct GameRng(pub u64);

impl GameRng {
    pub fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }

    pub fn index(&mut self, len: usize) -> usize {
        ((self.next_f32() * len as f32) as usize).min(len.saturating_sub(1))
    }
}
