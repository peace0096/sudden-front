//! Who a player is and their body: identity, pose, health, loadout and score.

use crate::game::{CROUCH_EYE_HEIGHT, EYE_HEIGHT};
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

impl Pose {
    /// Where the player's eyes are: their eye height, standing or crouched, above their feet.
    pub fn eye(&self) -> [f32; 3] {
        let height = if self.crouching {
            CROUCH_EYE_HEIGHT
        } else {
            EYE_HEIGHT
        };
        [self.x, self.y + height, self.z]
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eyes_sit_at_eye_height_above_the_feet_and_lower_when_crouched() {
        let mut pose = Pose {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            ..Default::default()
        };
        assert_eq!(pose.eye(), [1.0, 2.0 + EYE_HEIGHT, 3.0]);
        pose.crouching = true;
        assert_eq!(pose.eye(), [1.0, 2.0 + CROUCH_EYE_HEIGHT, 3.0]);
    }
}
