//! Movement collision and ray tests. `step_move` and `step_vertical` are mirrored by the
//! client's `movement.ts` for prediction, so keep them in sync.

use super::{CMD_DT, GRAVITY, JUMP_SPEED, PLAYER_HALF_WIDTH, PLAYER_HEIGHT};
use crate::pb::MapBox;

/// Slack for a player resting against a wall, whose `wall + radius` position can
/// round to a hair inside it. Larger than the rounding, far below a move step.
const CONTACT_EPS: f32 = 1e-3;
/// A box top this little above the feet is underfoot, not in the way: where a player who stands
/// or lands on it rounds to.
const FOOTING: f32 = 0.01;

/// Moves one axis at a time so players slide along walls. A box only stops a move
/// that crosses into it from outside; touching one sideways must never push the
/// player to its far face. Only boxes that reach between the feet (at `feet`) and the
/// head are in the way: a player in the air or on a box top passes over lower ones.
pub fn step_move(boxes: &[MapBox], x: &mut f32, z: &mut f32, feet: f32, dx: f32, dz: f32) {
    let r = PLAYER_HALF_WIDTH;
    let e = CONTACT_EPS;
    let old_x = *x;
    *x += dx;
    if dx != 0.0 {
        for b in boxes {
            if blocks_at(b, feet) && *z + r - e > b.min_z && *z - r + e < b.max_z {
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
            if blocks_at(b, feet) && *x + r - e > b.min_x && *x - r + e < b.max_x {
                if dz > 0.0 && old_z + r <= b.min_z + e && *z + r > b.min_z {
                    *z = b.min_z - r;
                } else if dz < 0.0 && old_z - r >= b.max_z - e && *z - r < b.max_z {
                    *z = b.max_z + r;
                }
            }
        }
    }
}

/// Lintels and stacked containers above head height don't block walking on the floor.
pub fn blocks(b: &MapBox) -> bool {
    b.min_y < PLAYER_HEIGHT
}

/// Whether a box is in the way of a player whose feet are at `feet`: it starts below their head
/// and reaches above their feet. On the floor that is any box `blocks`.
pub fn blocks_at(b: &MapBox, feet: f32) -> bool {
    b.min_y < feet + PLAYER_HEIGHT && b.max_y > feet + FOOTING
}

/// Whether a box lies under any of a player's footprint at (x, z).
fn under(b: &MapBox, x: f32, z: f32) -> bool {
    let r = PLAYER_HALF_WIDTH;
    let e = CONTACT_EPS;
    x + r - e > b.min_x && x - r + e < b.max_x && z + r - e > b.min_z && z - r + e < b.max_z
}

/// What a player at (x, z) with their feet at `feet` stands on or would land on: the highest box
/// top under their footprint that is not above their feet, or the floor (0).
pub fn ground_height(boxes: &[MapBox], x: f32, z: f32, feet: f32) -> f32 {
    boxes
        .iter()
        .filter(|b| b.max_y <= feet + FOOTING && under(b, x, z))
        .fold(0.0, |ground, b| ground.max(b.max_y))
}

/// The underside of the lowest box over the head of a player at (x, z) with their feet at `feet`.
fn ceiling_height(boxes: &[MapBox], x: f32, z: f32, feet: f32) -> Option<f32> {
    boxes
        .iter()
        .filter(|b| b.min_y >= feet + PLAYER_HEIGHT - FOOTING && under(b, x, z))
        .map(|b| b.min_y)
        .reduce(f32::min)
}

/// One command's rise or fall at (x, z), after `step_move`: a jump from where the player stands,
/// then gravity, stopped by a box overhead and by the floor or a box top underfoot. A player
/// stands exactly when `vy` is 0 and the feet are on the ground, so a jump needs both.
pub fn step_vertical(boxes: &[MapBox], x: f32, z: f32, y: &mut f32, vy: &mut f32, jump: bool) {
    let ground = ground_height(boxes, x, z, *y);
    if jump && *vy == 0.0 && *y <= ground + FOOTING {
        *vy = JUMP_SPEED;
    }
    *vy -= GRAVITY * CMD_DT;
    let mut next = *y + *vy * CMD_DT;
    if *vy > 0.0 {
        if let Some(top) = ceiling_height(boxes, x, z, *y) {
            if next + PLAYER_HEIGHT > top {
                next = top - PLAYER_HEIGHT;
                *vy = 0.0;
            }
        }
    }
    if next <= ground {
        next = ground;
        *vy = 0.0;
    }
    *y = next;
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

/// How far inside a surface rounding may leave a ball resting on it, in metres: closer than this it
/// still touches the surface, deeper it is taken to be leaving.
const BALL_SLACK: f32 = 0.01;

/// Where a ball of `radius` moving from `from` by `delta` first touches the world: the share of
/// `delta` it gets through, and the outward normal of what it touches as an axis (0 x, 1 y, 2 z)
/// and a sign. The world is the boxes, the floor (y 0) and the map's edge (|x| `half_x`,
/// |z| `half_z`), which keeps a grenade lobbed over the perimeter wall in. `None` when the whole
/// move is clear.
pub fn sweep_ball(
    boxes: &[MapBox],
    from: [f32; 3],
    delta: [f32; 3],
    radius: f32,
    half_x: f32,
    half_z: f32,
) -> Option<(f32, usize, f32)> {
    if delta.iter().all(|d| d.abs() < 1e-9) {
        return None;
    }
    let mut best: Option<(f32, usize, f32)> = None;
    let mut consider = |t: f32, axis: usize, sign: f32| {
        if t <= 1.0 && best.is_none_or(|(current, _, _)| t < current) {
            best = Some((t.max(0.0), axis, sign));
        }
    };
    // Planes the ball stays on one side of: (axis, where its centre stops, the side it stays on).
    let planes = [
        (1, radius, 1.0),
        (0, -half_x + radius, 1.0),
        (0, half_x - radius, -1.0),
        (2, -half_z + radius, 1.0),
        (2, half_z - radius, -1.0),
    ];
    for (axis, at, sign) in planes {
        let d = delta[axis];
        // Only a move toward the far side; one already over it (rounding) touches at once.
        if d * sign < 0.0 {
            consider((at - from[axis]) / d, axis, sign);
        }
    }
    for b in boxes {
        // The box grown by the radius, which the centre then must not enter.
        let min = [b.min_x - radius, b.min_y - radius, b.min_z - radius];
        let max = [b.max_x + radius, b.max_y + radius, b.max_z + radius];
        let (mut enter, mut exit) = (f32::NEG_INFINITY, f32::INFINITY);
        let (mut axis, mut sign) = (0, 0.0);
        let mut missed = false;
        for a in 0..3 {
            if delta[a].abs() < 1e-9 {
                if from[a] <= min[a] || from[a] >= max[a] {
                    missed = true;
                    break;
                }
                continue;
            }
            let inv = 1.0 / delta[a];
            let (mut t0, mut t1) = ((min[a] - from[a]) * inv, (max[a] - from[a]) * inv);
            // Entering through the min face, whose outward normal points down the axis.
            let mut face = -1.0;
            if t0 > t1 {
                std::mem::swap(&mut t0, &mut t1);
                face = 1.0;
            }
            if t0 > enter {
                (enter, axis, sign) = (t0, a, face);
            }
            exit = exit.min(t1);
        }
        if missed || enter > exit || exit <= 0.0 {
            continue;
        }
        if enter < 0.0 && -enter * delta[axis].abs() > BALL_SLACK {
            continue;
        }
        consider(enter, axis, sign);
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
