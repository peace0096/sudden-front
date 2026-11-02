//! Movement collision and ray tests. `step_move` is mirrored by the client's
//! `movement.ts` for prediction, so keep the two in sync.

use super::{PLAYER_HALF_WIDTH, PLAYER_HEIGHT};
use crate::pb::MapBox;

/// Slack for a player resting against a wall, whose `wall + radius` position can
/// round to a hair inside it. Larger than the rounding, far below a move step.
const CONTACT_EPS: f32 = 1e-3;

/// Moves one axis at a time so players slide along walls. A box only stops a move
/// that crosses into it from outside; touching one sideways must never push the
/// player to its far face.
pub fn step_move(boxes: &[MapBox], x: &mut f32, z: &mut f32, dx: f32, dz: f32) {
    let r = PLAYER_HALF_WIDTH;
    let e = CONTACT_EPS;
    let old_x = *x;
    *x += dx;
    if dx != 0.0 {
        for b in boxes {
            if blocks(b) && *z + r - e > b.min_z && *z - r + e < b.max_z {
                if dx > 0.0 && old_x + r <= b.min_x + e && *x + r > b.min_x {
                    *x = b.min_x - r;
                } else if dx < 0.0 && old_x - r >= b.max_x - e && *x - r < b.max_x {
                    *x = b.max_x + r;
                }
            }
        }
    }
    let old_z = *z;
    *z += dz;
    if dz != 0.0 {
        for b in boxes {
            if blocks(b) && *x + r - e > b.min_x && *x - r + e < b.max_x {
                if dz > 0.0 && old_z + r <= b.min_z + e && *z + r > b.min_z {
                    *z = b.min_z - r;
                } else if dz < 0.0 && old_z - r >= b.max_z - e && *z - r < b.max_z {
                    *z = b.max_z + r;
                }
            }
        }
    }
}

/// Lintels and stacked containers above head height don't block walking.
pub fn blocks(b: &MapBox) -> bool {
    b.min_y < PLAYER_HEIGHT
}

/// Forward vector for a view angle. Yaw 0 looks down -Z, positive pitch looks up.
pub fn view_dir(yaw: f32, pitch: f32) -> [f32; 3] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    [-sy * cp, sp, -cy * cp]
}

pub fn ray_aabb(origin: [f32; 3], dir: [f32; 3], min: [f32; 3], max: [f32; 3]) -> Option<f32> {
    let mut t_min = 0.0f32;
    let mut t_max = f32::MAX;
    for axis in 0..3 {
        if dir[axis].abs() < 1e-7 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
        } else {
            let inv = 1.0 / dir[axis];
            let (mut t0, mut t1) = ((min[axis] - origin[axis]) * inv, (max[axis] - origin[axis]) * inv);
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
            }
            t_min = t_min.max(t0);
            t_max = t_max.min(t1);
            if t_min > t_max {
                return None;
            }
        }
    }
    Some(t_min)
}

/// Distance to the first world surface along a ray.
pub fn ray_world(boxes: &[MapBox], origin: [f32; 3], dir: [f32; 3], max_dist: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    for b in boxes {
        let hit = ray_aabb(
            origin,
            dir,
            [b.min_x, b.min_y, b.min_z],
            [b.max_x, b.max_y, b.max_z],
        );
        if let Some(t) = hit {
            if t <= max_dist && best.is_none_or(|current| t < current) {
                best = Some(t);
            }
        }
    }
    best
}

/// Clear line between two points, for bot vision.
pub fn line_clear(boxes: &[MapBox], from: [f32; 3], to: [f32; 3]) -> bool {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-4 {
        return true;
    }
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    ray_world(boxes, from, dir, len).is_none()
}
