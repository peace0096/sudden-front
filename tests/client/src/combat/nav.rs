//! A walkability grid over the map and breadth-first paths through it, built from the `MapData` the
//! server sends at the start of a match. The same grid as the server's bots
//! (`server/src/game/nav.rs`): one metre cells, closed where a box a player cannot walk under
//! comes within `MARGIN` of the centre.

use crate::pb::MapBox;
use crate::rng::Rng;
use std::collections::VecDeque;

/// Clearance a cell centre needs from walls; slightly over the player half-width.
const MARGIN: f32 = 0.55;

pub struct NavGrid {
    width: i32,
    height: i32,
    half_x: f32,
    half_z: f32,
    open: Vec<bool>,
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "cell coordinates and indices stay inside the grid, at most a few hundred cells a side"
)]
impl NavGrid {
    /// The grid of a map `half_x` by `half_z` metres from its centre. A box blocks walking when it
    /// starts below `player_height`; lintels and stacked containers higher up do not.
    pub fn new(boxes: &[MapBox], half_x: f32, half_z: f32, player_height: f32) -> Self {
        let width = (half_x * 2.0).max(0.0) as i32;
        let height = (half_z * 2.0).max(0.0) as i32;
        let mut open = vec![true; (width * height) as usize];
        for cz in 0..height {
            for cx in 0..width {
                let x = cx as f32 + 0.5 - half_x;
                let z = cz as f32 + 0.5 - half_z;
                let blocked = boxes.iter().any(|b| {
                    b.min_y < player_height
                        && x + MARGIN > b.min_x
                        && x - MARGIN < b.max_x
                        && z + MARGIN > b.min_z
                        && z - MARGIN < b.max_z
                });
                open[(cz * width + cx) as usize] = !blocked;
            }
        }
        Self {
            width,
            height,
            half_x,
            half_z,
            open,
        }
    }

    fn is_open(&self, cx: i32, cz: i32) -> bool {
        cx >= 0
            && cz >= 0
            && cx < self.width
            && cz < self.height
            && self.open[(cz * self.width + cx) as usize]
    }

    fn centre(&self, cx: i32, cz: i32) -> (f32, f32) {
        (cx as f32 + 0.5 - self.half_x, cz as f32 + 0.5 - self.half_z)
    }

    /// Nearest open cell to a world position, searching outward.
    fn nearest_open(&self, x: f32, z: f32) -> Option<(i32, i32)> {
        let cx = (x + self.half_x).floor() as i32;
        let cz = (z + self.half_z).floor() as i32;
        for radius in 0..self.width.max(self.height) {
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    if dx.abs().max(dz.abs()) == radius && self.is_open(cx + dx, cz + dz) {
                        return Some((cx + dx, cz + dz));
                    }
                }
            }
        }
        None
    }

    /// The centre of an open cell picked at random, a place to patrol to; `None` on a grid with no
    /// open cell.
    pub fn random_open(&self, rng: &mut Rng) -> Option<(f32, f32)> {
        let open: Vec<usize> = (0..self.open.len()).filter(|&i| self.open[i]).collect();
        if open.is_empty() {
            return None;
        }
        let cell = open[rng.index(open.len())] as i32;
        Some(self.centre(cell % self.width, cell / self.width))
    }

    /// Cell-centre waypoints from start to goal, excluding the start cell; empty when there is no
    /// way through.
    pub fn path(&self, from: (f32, f32), to: (f32, f32)) -> Vec<(f32, f32)> {
        let (Some(start), Some(goal)) = (
            self.nearest_open(from.0, from.1),
            self.nearest_open(to.0, to.1),
        ) else {
            return Vec::new();
        };
        let index = |c: (i32, i32)| (c.1 * self.width + c.0) as usize;
        let mut parent = vec![-1i32; self.open.len()];
        parent[index(start)] = index(start) as i32;
        let mut frontier = VecDeque::from([start]);
        while let Some(cell) = frontier.pop_front() {
            if cell == goal {
                break;
            }
            for (dx, dz) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let next = (cell.0 + dx, cell.1 + dz);
                if !self.is_open(next.0, next.1) || parent[index(next)] >= 0 {
                    continue;
                }
                // No cutting corners past walls.
                if dx != 0
                    && dz != 0
                    && !(self.is_open(cell.0 + dx, cell.1) && self.is_open(cell.0, cell.1 + dz))
                {
                    continue;
                }
                parent[index(next)] = index(cell) as i32;
                frontier.push_back(next);
            }
        }
        if parent[index(goal)] < 0 {
            return Vec::new();
        }
        let mut cells = Vec::new();
        let mut current = index(goal);
        while current != index(start) {
            let cell = current as i32;
            cells.push(self.centre(cell % self.width, cell / self.width));
            current = parent[current] as usize;
        }
        cells.reverse();
        cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER_HEIGHT: f32 = 1.8;

    fn wall(min_x: f32, max_x: f32, min_z: f32, max_z: f32) -> MapBox {
        MapBox {
            min_x,
            max_x,
            min_y: 0.0,
            max_y: 3.0,
            min_z,
            max_z,
            ..Default::default()
        }
    }

    #[test]
    fn a_path_on_open_ground_ends_at_the_goals_cell() {
        let grid = NavGrid::new(&[], 10.0, 10.0, PLAYER_HEIGHT);
        let path = grid.path((-8.5, -8.5), (8.5, 8.5));
        assert_eq!(path.last(), Some(&(8.5, 8.5)));
        // Diagonal steps: one cell a step on both axes.
        assert_eq!(path.len(), 17);
    }

    #[test]
    fn a_path_goes_round_a_wall_and_never_through_it() {
        // A wall across the middle with a gap at the top.
        let blocks = [wall(-0.5, 0.5, -10.0, 6.0)];
        let grid = NavGrid::new(&blocks, 10.0, 10.0, PLAYER_HEIGHT);
        let path = grid.path((-5.0, 0.0), (5.0, 0.0));
        assert!(!path.is_empty(), "there is a way round");
        for &(x, z) in &path {
            assert!(
                !(x + MARGIN > -0.5 && x - MARGIN < 0.5 && z < 6.0 + MARGIN),
                "({x}, {z}) is in the wall"
            );
        }
    }

    #[test]
    fn a_box_above_head_height_does_not_block_walking() {
        let lintel = MapBox {
            min_y: PLAYER_HEIGHT + 0.5,
            max_y: PLAYER_HEIGHT + 1.0,
            ..wall(-10.0, 10.0, -0.5, 0.5)
        };
        let grid = NavGrid::new(&[lintel], 10.0, 10.0, PLAYER_HEIGHT);
        assert_eq!(grid.path((0.0, -5.0), (0.0, 5.0)).len(), 10);
    }

    #[test]
    fn a_walled_off_goal_has_no_path() {
        let blocks = [wall(-0.5, 0.5, -10.0, 10.0)];
        let grid = NavGrid::new(&blocks, 10.0, 10.0, PLAYER_HEIGHT);
        assert!(grid.path((-5.0, 0.0), (5.0, 0.0)).is_empty());
    }

    #[test]
    fn a_random_patrol_point_is_an_open_cell_centre() {
        let blocks = [wall(-10.0, 0.0, -10.0, 10.0)];
        let grid = NavGrid::new(&blocks, 10.0, 10.0, PLAYER_HEIGHT);
        let mut rng = Rng::new(3);
        for _ in 0..200 {
            let (x, z) = grid.random_open(&mut rng).expect("half the map is open");
            assert!(x > 0.0, "({x}, {z}) is inside the wall");
            assert_eq!(x.fract().abs(), 0.5);
        }
        let closed = NavGrid::new(&[wall(-10.0, 10.0, -10.0, 10.0)], 10.0, 10.0, PLAYER_HEIGHT);
        assert_eq!(closed.random_open(&mut rng), None);
    }
}
