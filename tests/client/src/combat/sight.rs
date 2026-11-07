//! Line of sight through the map's boxes, the same test the server's bots use
//! (`server/src/game/physics.rs`: `ray_aabb`, `ray_world`, `line_clear`).

use crate::pb::MapBox;

/// Distance along a ray to where it enters a box, or `None` if it misses.
fn ray_aabb(origin: [f32; 3], dir: [f32; 3], min: [f32; 3], max: [f32; 3]) -> Option<f32> {
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

/// Whether nothing in the world stands between two points.
pub fn line_clear(boxes: &[MapBox], from: [f32; 3], to: [f32; 3]) -> bool {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-4 {
        return true;
    }
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    !boxes.iter().any(|b| {
        ray_aabb(
            from,
            dir,
            [b.min_x, b.min_y, b.min_z],
            [b.max_x, b.max_y, b.max_z],
        )
        .is_some_and(|t| t <= len)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall() -> MapBox {
        MapBox {
            min_x: 2.0,
            max_x: 3.0,
            min_y: 0.0,
            max_y: 2.0,
            min_z: -5.0,
            max_z: 5.0,
            ..Default::default()
        }
    }

    #[test]
    fn a_wall_between_two_points_blocks_the_line_both_ways() {
        let (here, there) = ([0.0, 1.0, 0.0], [5.0, 1.0, 0.0]);
        assert!(!line_clear(&[wall()], here, there));
        assert!(!line_clear(&[wall()], there, here));
    }

    #[test]
    fn a_line_short_of_the_wall_or_over_it_is_clear() {
        assert!(line_clear(&[wall()], [0.0, 1.0, 0.0], [1.5, 1.0, 0.0]));
        assert!(line_clear(&[wall()], [0.0, 2.5, 0.0], [5.0, 2.5, 0.0]));
        assert!(line_clear(&[], [0.0, 1.0, 0.0], [5.0, 1.0, 0.0]));
        assert!(line_clear(&[wall()], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]));
    }
}
