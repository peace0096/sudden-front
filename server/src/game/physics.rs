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
            let (mut t0, mut t1) = (
                (min[axis] - origin[axis]) * inv,
                (max[axis] - origin[axis]) * inv,
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    /// A grenade-sized ball's radius, and the half size of the map it is swept in.
    const BALL: f32 = 0.1;
    const HALF: f32 = 10.0;

    /// A box from `min` to `max`.
    fn solid(min: [f32; 3], max: [f32; 3]) -> MapBox {
        MapBox {
            min_x: min[0],
            min_y: min[1],
            min_z: min[2],
            max_x: max[0],
            max_y: max[1],
            max_z: max[2],
            ..Default::default()
        }
    }

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    // ---------------------------------------------------------------- aim and rays

    #[test]
    fn the_view_looks_down_minus_z_at_yaw_zero_turns_with_yaw_and_rises_with_pitch() {
        assert!(near(view_dir(0.0, 0.0), [0.0, 0.0, -1.0]));
        // A quarter turn the negative way looks along +x, the positive way along -x.
        assert!(near(view_dir(-FRAC_PI_2, 0.0), [1.0, 0.0, 0.0]));
        assert!(near(view_dir(FRAC_PI_2, 0.0), [-1.0, 0.0, 0.0]));
        assert!(near(view_dir(PI, 0.0), [0.0, 0.0, 1.0]));
        // Straight up whatever the yaw.
        assert!(near(view_dir(1.0, FRAC_PI_2), [0.0, 1.0, 0.0]));
        assert!(view_dir(0.0, 0.2)[1] > 0.0 && view_dir(0.0, -0.2)[1] < 0.0);
    }

    #[test]
    fn every_view_direction_is_a_unit_vector() {
        for yaw in [-4.0, -PI, -1.0, 0.0, 0.5, 2.0, 7.0] {
            for pitch in [-MAX_PITCH, -0.5, 0.0, 0.7, MAX_PITCH] {
                let d = view_dir(yaw, pitch);
                let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                assert!(
                    (length - 1.0).abs() < 1e-6,
                    "yaw {yaw}, pitch {pitch}: {length}"
                );
            }
        }
    }

    #[test]
    fn a_ray_meets_a_box_ahead_at_its_near_face_and_misses_one_behind_or_beside() {
        let (min, max) = ([2.0, -1.0, -1.0], [3.0, 1.0, 1.0]);
        let east = [1.0, 0.0, 0.0];
        assert_eq!(ray_aabb([0.0; 3], east, min, max), Some(2.0));
        assert_eq!(
            ray_aabb([0.0; 3], [-1.0, 0.0, 0.0], min, max),
            None,
            "behind"
        );
        assert_eq!(ray_aabb([0.0, 1.01, 0.0], east, min, max), None, "over it");
        assert_eq!(ray_aabb([0.0, 0.99, 0.0], east, min, max), Some(2.0));
        // Aslant, the distance is along the ray, to where it crosses the near face.
        let length = (1.0f32 + 0.25 * 0.25).sqrt();
        let aslant = [1.0 / length, 0.25 / length, 0.0];
        let t = ray_aabb([0.0; 3], aslant, min, max).unwrap();
        assert!((t * aslant[0] - 2.0).abs() < 1e-5, "{t}");
    }

    #[test]
    fn a_ray_from_inside_a_box_meets_it_at_once() {
        let (min, max) = ([2.0, -1.0, -1.0], [3.0, 1.0, 1.0]);
        for dir in [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, -1.0],
        ] {
            assert_eq!(
                ray_aabb([2.5, 0.0, 0.0], dir, min, max),
                Some(0.0),
                "{dir:?}"
            );
        }
    }

    #[test]
    fn a_ray_into_the_world_stops_at_the_nearest_box_within_reach_in_any_order() {
        let near = solid([2.0, 0.0, -1.0], [3.0, 2.0, 1.0]);
        let far = solid([5.0, 0.0, -1.0], [6.0, 2.0, 1.0]);
        let (from, east) = ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
        assert_eq!(ray_world(&[near, far], from, east, 10.0), Some(2.0));
        assert_eq!(ray_world(&[far, near], from, east, 10.0), Some(2.0));
        assert_eq!(
            ray_world(&[near, far], from, east, 1.9),
            None,
            "out of reach"
        );
        assert_eq!(ray_world(&[], from, east, 10.0), None);
    }

    #[test]
    fn a_line_is_clear_unless_a_box_stands_between_its_ends_either_way() {
        let wall = solid([2.0, 0.0, -1.0], [3.0, 2.0, 1.0]);
        let (here, there) = ([0.0, 1.0, 0.0], [5.0, 1.0, 0.0]);
        assert!(!line_clear(&[wall], here, there));
        assert!(!line_clear(&[wall], there, here));
        assert!(
            line_clear(&[wall], here, [1.5, 1.0, 0.0]),
            "short of the wall"
        );
        assert!(
            line_clear(&[wall], [0.0, 2.5, 0.0], [5.0, 2.5, 0.0]),
            "over it"
        );
        assert!(line_clear(&[], here, there));
        assert!(line_clear(&[wall], here, here), "a line of no length");
    }

    // ---------------------------------------------------------------- standing

    #[test]
    fn the_ground_is_the_highest_box_top_underfoot_that_is_not_above_the_feet() {
        let low = solid([-1.0, 0.0, -1.0], [1.0, 0.5, 1.0]);
        let high = solid([-1.0, 0.5, -1.0], [1.0, 1.2, 1.0]);
        let stack = [low, high];
        assert_eq!(ground_height(&[], 0.0, 0.0, 2.0), 0.0, "the floor");
        assert_eq!(ground_height(&stack, 0.0, 0.0, 1.2), 1.2, "on top");
        assert_eq!(ground_height(&stack, 0.0, 0.0, 3.0), 1.2, "falling onto it");
        assert_eq!(ground_height(&stack, 0.0, 0.0, 0.8), 0.5, "under the top");
        assert_eq!(ground_height(&stack, 3.0, 0.0, 3.0), 0.0, "beside it");
        // A top a hair above the feet, where rounding leaves them, is still underfoot.
        assert_eq!(ground_height(&stack, 0.0, 0.0, 1.2 - FOOTING / 2.0), 1.2);
    }

    #[test]
    fn a_box_is_underfoot_only_where_the_footprint_overlaps_it_past_the_contact_slack() {
        let c = crate_box();
        let (r, top) = (PLAYER_HALF_WIDTH, c.max_y);
        let ground = |x: f32| ground_height(&[c], x, 0.0, 2.0);
        assert_eq!(ground(c.min_x - r), 0.0, "touching its side");
        assert_eq!(
            ground(c.min_x - r + CONTACT_EPS / 2.0),
            0.0,
            "within the slack"
        );
        assert_eq!(ground(c.min_x - r + 0.05), top, "a toe over its edge");
        assert_eq!(ground(c.max_x + r - 0.05), top, "a heel over its far edge");
        assert_eq!(ground(c.max_x + r), 0.0, "touching its far side");
    }

    #[test]
    fn only_a_box_that_starts_below_head_height_blocks_a_player_on_the_floor() {
        let starting_at = |min_y: f32| solid([0.0, min_y, 0.0], [1.0, min_y + 1.0, 1.0]);
        assert!(blocks(&starting_at(0.0)));
        assert!(blocks(&starting_at(PLAYER_HEIGHT - 0.01)));
        assert!(
            !blocks(&starting_at(PLAYER_HEIGHT)),
            "a lintel at head height"
        );
        assert!(!blocks(&starting_at(PLAYER_HEIGHT + 2.0)));
        for min_y in [0.0, 1.0, PLAYER_HEIGHT - 0.01, PLAYER_HEIGHT, 4.0] {
            let b = starting_at(min_y);
            assert_eq!(blocks_at(&b, 0.0), blocks(&b), "starting {min_y} m up");
        }
    }

    #[test]
    fn a_box_is_in_the_way_only_between_the_feet_and_the_head() {
        let c = crate_box();
        assert!(blocks_at(&c, 0.0), "on the floor beside it");
        assert!(
            blocks_at(&c, c.max_y - 0.2),
            "jumping, the feet below its top"
        );
        assert!(!blocks_at(&c, c.max_y), "standing on it");
        assert!(
            !blocks_at(&c, c.max_y - FOOTING / 2.0),
            "a hair below its top"
        );
        assert!(!blocks_at(&c, c.max_y + 0.8), "over it");
        let lid = solid([0.0, 2.3, 0.0], [1.0, 2.6, 1.0]);
        assert!(!blocks_at(&lid, 0.0), "overhead");
        assert!(
            blocks_at(&lid, lid.min_y - PLAYER_HEIGHT + 0.01),
            "the head reaches it"
        );
        let marking = solid([0.0, 0.0, 0.0], [1.0, FOOTING, 1.0]);
        assert!(!blocks_at(&marking, 0.0), "flush with the floor");
    }

    // ---------------------------------------------------------------- a rolling ball

    #[test]
    fn a_ball_that_does_not_move_touches_nothing() {
        for y in [1.0, BALL] {
            assert_eq!(
                sweep_ball(&[], [0.0, y, 0.0], [0.0; 3], BALL, HALF, HALF),
                None
            );
        }
    }

    #[test]
    fn the_floor_stops_a_falling_ball_one_radius_up_and_lets_a_rolling_one_go() {
        let (t, axis, sign) =
            sweep_ball(&[], [0.0, 1.0, 0.0], [0.0, -2.0, 0.0], BALL, HALF, HALF).unwrap();
        assert_eq!((axis, sign), (1, 1.0));
        assert!(
            (1.0 - 2.0 * t - BALL).abs() < 1e-6,
            "stopped at {}",
            1.0 - 2.0 * t
        );
        let rolling = sweep_ball(&[], [0.0, BALL, 0.0], [1.0, 0.0, 0.5], BALL, HALF, HALF);
        assert_eq!(rolling, None);
        let short = sweep_ball(&[], [0.0, 1.0, 0.0], [0.0, -0.5, 0.0], BALL, HALF, HALF);
        assert_eq!(short, None, "still above the floor");
    }

    #[test]
    fn a_ball_meets_a_box_one_radius_out_with_the_faces_outward_normal() {
        let block = solid([2.0, 0.0, -1.0], [3.0, 2.0, 1.0]);
        let sweep =
            |from: [f32; 3], delta: [f32; 3]| sweep_ball(&[block], from, delta, BALL, HALF, HALF);
        let (t, axis, sign) = sweep([0.0, 1.0, 0.0], [4.0, 0.0, 0.0]).unwrap();
        assert_eq!((axis, sign), (0, -1.0), "its -x face");
        assert!((4.0 * t - (2.0 - BALL)).abs() < 1e-5, "{t}");
        let (t, axis, sign) = sweep([5.0, 1.0, 0.0], [-4.0, 0.0, 0.0]).unwrap();
        assert_eq!((axis, sign), (0, 1.0), "its +x face");
        assert!((5.0 - 4.0 * t - (3.0 + BALL)).abs() < 1e-5, "{t}");
        let (t, axis, sign) = sweep([2.5, 3.0, 0.0], [0.0, -2.0, 0.0]).unwrap();
        assert_eq!((axis, sign), (1, 1.0), "its top");
        assert!((3.0 - 2.0 * t - (2.0 + BALL)).abs() < 1e-5, "{t}");
        assert_eq!(sweep([0.0, 1.0, 0.0], [-4.0, 0.0, 0.0]), None, "away");
        assert_eq!(sweep([0.0, 1.0, 3.0], [8.0, 0.0, 0.0]), None, "beside it");
        assert_eq!(
            sweep([2.2, 2.0 + BALL, 0.0], [0.5, 0.0, 0.0]),
            None,
            "rolling along its top"
        );
    }

    #[test]
    fn the_map_edge_keeps_a_ball_in() {
        let (half_x, half_z) = (10.0, 6.0);
        let (t, axis, sign) =
            sweep_ball(&[], [9.0, 1.0, 0.0], [2.0, 0.0, 0.0], BALL, half_x, half_z).unwrap();
        assert_eq!((axis, sign), (0, -1.0));
        assert!((9.0 + 2.0 * t - (half_x - BALL)).abs() < 1e-5, "{t}");
        let (t, axis, sign) = sweep_ball(
            &[],
            [0.0, 1.0, -5.0],
            [0.0, 0.0, -2.0],
            BALL,
            half_x,
            half_z,
        )
        .unwrap();
        assert_eq!((axis, sign), (2, 1.0));
        assert!((-5.0 - 2.0 * t - (BALL - half_z)).abs() < 1e-5, "{t}");
    }

    #[test]
    fn the_first_of_several_surfaces_in_the_way_is_the_one_touched() {
        let near = solid([2.0, 0.0, -1.0], [3.0, 2.0, 1.0]);
        let far = solid([5.0, 0.0, -1.0], [6.0, 2.0, 1.0]);
        for boxes in [[near, far], [far, near]] {
            let (t, axis, _) =
                sweep_ball(&boxes, [0.0, 1.0, 0.0], [8.0, -0.5, 0.0], BALL, HALF, HALF).unwrap();
            assert_eq!(axis, 0);
            assert!((8.0 * t - (2.0 - BALL)).abs() < 1e-5, "{t}");
        }
        let (_, axis, _) =
            sweep_ball(&[near], [0.0, 0.5, 0.0], [4.0, -2.0, 0.0], BALL, HALF, HALF).unwrap();
        assert_eq!(axis, 1, "the floor comes before the box");
    }

    #[test]
    fn a_ball_a_hair_inside_a_surface_still_touches_it_and_one_deeper_is_let_out() {
        let block = solid([2.0, 0.0, -1.0], [3.0, 2.0, 1.0]);
        let sweep =
            |x: f32, dx: f32| sweep_ball(&[block], [x, 1.0, 0.0], [dx, 0.0, 0.0], BALL, HALF, HALF);
        let touching = Some((0.0, 0, -1.0));
        assert_eq!(sweep(2.0 - BALL, 1.0), touching, "resting against it");
        assert_eq!(
            sweep(2.0 - BALL + BALL_SLACK / 2.0, 1.0),
            touching,
            "a hair in"
        );
        assert_eq!(sweep(2.0 - BALL + BALL_SLACK * 3.0, 1.0), None, "deeper in");
        assert_eq!(sweep(2.5, -1.0), None, "inside, on the way out");
    }

    // ---------------------------------------------------------------- moving

    #[test]
    fn walls_stop_movement() {
        let map = build_map(YARD).data;
        let (mut x, mut z) = (-20.0, -3.0);
        for _ in 0..200 {
            step_move(&map.boxes, &mut x, &mut z, 0.0, 0.0, -0.1);
        }
        // The loading dock's front edge is at z = -5.4.
        assert!((z - (-5.4 + PLAYER_HALF_WIDTH)).abs() < 1e-3, "z = {z}");
    }

    #[test]
    fn a_jump_rises_about_a_metre_and_a_third_and_comes_back_down() {
        let (mut y, mut vy) = (0.0f32, 0.0f32);
        step_vertical(&[], 0.0, 0.0, &mut y, &mut vy, true);
        let (mut apex, mut steps) = (y, 1);
        while y > 0.0 {
            step_vertical(&[], 0.0, 0.0, &mut y, &mut vy, false);
            apex = apex.max(y);
            steps += 1;
            assert!(steps < 200, "never landed");
        }
        assert!((1.3..1.4).contains(&apex), "apex {apex}");
        assert_eq!((y, vy), (0.0, 0.0));
        // About three quarters of a second in the air.
        assert!((40..50).contains(&steps), "{steps} steps");
    }

    #[test]
    fn a_jump_clears_a_crate_lands_on_it_and_walking_off_falls_to_the_floor() {
        let boxes = [crate_box()];
        let (mut x, mut z, mut y, mut vy) = (-1.0f32, 0.0f32, 0.0f32, 0.0f32);
        let step = 5.0 * CMD_DT;
        let mut stood = false;
        for i in 0..120 {
            step_move(&boxes, &mut x, &mut z, y, step, 0.0);
            step_vertical(&boxes, x, z, &mut y, &mut vy, i == 0);
            // Over the crate (with the collision's 1 mm of slack) the feet are never below its top.
            let over = x + PLAYER_HALF_WIDTH - 0.001 > 0.0 && x - PLAYER_HALF_WIDTH + 0.001 < 1.2;
            assert!(
                !over || y >= 1.19,
                "step {i}: inside the crate at x {x}, y {y}"
            );
            stood |= over && y == 1.2 && vy == 0.0;
        }
        assert!(stood, "never stood on the crate");
        assert!(
            x > 1.2 + PLAYER_HALF_WIDTH && y == 0.0 && vy == 0.0,
            "x {x}, y {y}, vy {vy}"
        );
    }

    #[test]
    fn a_crate_too_high_to_clear_stops_the_player_at_its_side() {
        let tall = pb::MapBox {
            max_y: 2.6,
            ..crate_box()
        };
        let (mut x, mut z, mut y, mut vy) = (-1.0f32, 0.0f32, 0.0f32, 0.0f32);
        for i in 0..90 {
            step_move(&[tall], &mut x, &mut z, y, 5.0 * CMD_DT, 0.0);
            step_vertical(&[tall], x, z, &mut y, &mut vy, i == 0);
        }
        assert!((x - (0.0 - PLAYER_HALF_WIDTH)).abs() < 1e-4, "x {x}");
        assert_eq!((y, vy), (0.0, 0.0));
    }

    #[test]
    fn a_box_overhead_stops_a_jump() {
        // The warehouse vent tunnel's lid starts 2.3 m up: a head 1.8 m up has 0.5 m to rise.
        let lid = pb::MapBox {
            min_x: -1.0,
            max_x: 1.0,
            min_y: 2.3,
            max_y: 2.6,
            min_z: -1.0,
            max_z: 1.0,
            ..Default::default()
        };
        let (mut y, mut vy) = (0.0f32, 0.0f32);
        let mut apex = 0.0f32;
        for i in 0..60 {
            step_vertical(&[lid], 0.0, 0.0, &mut y, &mut vy, i == 0);
            apex = apex.max(y);
        }
        assert!((apex - 0.5).abs() < 1e-4, "apex {apex}");
        assert_eq!((y, vy), (0.0, 0.0));
    }

    #[test]
    fn a_jump_takes_off_only_from_the_ground() {
        // Holding jump in the air adds nothing: the next jump waits for the landing.
        let (mut y, mut vy) = (0.0f32, 0.0f32);
        let mut apex = 0.0f32;
        for _ in 0..40 {
            step_vertical(&[], 0.0, 0.0, &mut y, &mut vy, true);
            apex = apex.max(y);
        }
        assert!(apex < 1.4, "apex {apex}");
    }

    #[test]
    fn sliding_along_any_wall_never_teleports() {
        // Rounding can leave a player at `wall + radius` looking a hair inside the
        // wall. Moving sideways must not treat that as a collision.
        for id in [pb::MapId::SupplyYard, pb::MapId::Warehouse] {
            let map = build_map(id).data;
            for kind in weapons::ALL {
                let step = weapons::stats(kind).move_speed * CMD_DT;
                for b in map.boxes.iter().filter(|b| blocks(b)) {
                    // Each of the four faces: (start x, start z, along x, along z, into x, into z).
                    let r = PLAYER_HALF_WIDTH;
                    let faces = [
                        (b.min_x, b.max_z + r, step, 0.0, 0.0, -step),
                        (b.max_x, b.min_z - r, -step, 0.0, 0.0, step),
                        (b.min_x - r, b.min_z, 0.0, step, step, 0.0),
                        (b.max_x + r, b.max_z, 0.0, -step, -step, 0.0),
                    ];
                    for (mut x, mut z, along_x, along_z, into_x, into_z) in faces {
                        for _ in 0..60 {
                            let (old_x, old_z) = (x, z);
                            step_move(
                                &map.boxes,
                                &mut x,
                                &mut z,
                                0.0,
                                along_x + into_x,
                                along_z + into_z,
                            );
                            let moved = (x - old_x).abs().max((z - old_z).abs());
                            assert!(
                                moved <= step * 1.5,
                                "{id:?}: jumped {moved} from ({old_x},{old_z}) to ({x},{z})"
                            );
                        }
                    }
                }
            }
        }
    }
}
