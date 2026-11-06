//! Missiles and grenades in flight.

use crate::pb;
use bevy_ecs::prelude::*;

/// A missile in flight.
pub struct Missile {
    pub id: u32,
    pub owner: Entity,
    pub owner_id: u32,
    pub team: pb::Team,
    pub pos: [f32; 3],
    pub dir: [f32; 3],
    pub life: f32,
}

#[derive(Resource, Default)]
pub struct Missiles {
    pub next_id: u32,
    pub live: Vec<Missile>,
}

/// A thrown grenade: flying, bouncing, rolling or lying still until its fuse runs out.
pub struct Grenade {
    pub id: u32,
    pub owner: Entity,
    pub owner_id: u32,
    pub team: pb::Team,
    /// The centre of the grenade, which keeps `GRENADE_RADIUS` off every surface.
    pub pos: [f32; 3],
    /// m/s; all 0 once it lies still.
    pub vel: [f32; 3],
    /// Ticks until it goes off: whole ticks, so the fuse is exact.
    pub fuse_ticks: u32,
}

#[derive(Resource, Default)]
pub struct Grenades {
    pub next_id: u32,
    pub live: Vec<Grenade>,
}
