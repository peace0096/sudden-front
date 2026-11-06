//! Who a player is and their body: identity, pose, health, loadout and score.

use crate::pb;
use bevy_ecs::prelude::*;

#[derive(Component)]
pub struct Player {
    pub id: u32,
    pub name: String,
    pub team: pb::Team,
    pub is_bot: bool,
}

#[derive(Component, Clone, Copy, Default)]
pub struct Pose {
    pub x: f32,
    /// Feet height above the floor: a box's top while standing on it, more in the air.
    pub y: f32,
    pub z: f32,
    /// Vertical speed, m/s up; 0 while standing.
    pub vy: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub crouching: bool,
}

#[derive(Component)]
pub struct Vitals {
    pub hp: i32,
    pub alive: bool,
    pub respawn_left: f32,
    pub protect_left: f32,
    /// Seconds of paralysis left: no moving, firing, reloading or switching.
    pub stun_left: f32,
}

/// The weapon a player will get at their next spawn.
#[derive(Component)]
pub struct Loadout(pub pb::WeaponKind);

#[derive(Component, Default)]
pub struct Stats {
    pub kills: u32,
    pub deaths: u32,
}
