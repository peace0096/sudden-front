//! Walkability grid and breadth-first pathfinding for bots.

use super::physics::blocks;
use crate::pb::MapBox;
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
    reason = "cell coordinates and indices stay inside the grid, at most 100 by 64 cells"
)]
impl NavGrid {
    pub fn new(boxes: &[MapBox], half_x: f32, half_z: f32) -> Self {
        let width = (half_x * 2.0) as i32;
        let height = (half_z * 2.0) as i32;
        let mut open = vec![true; (width * height) as usize];
        for cz in 0..height {
            for cx in 0..width {
                let x = cx as f32 + 0.5 - half_x;
                let z = cz as f32 + 0.5 - half_z;
                let blocked = boxes.iter().any(|b| {
                    blocks(b)
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

    /// Nearest open cell to a world position, searching outward.
    fn nearest_open(&self, x: f32, z: f32) -> Option<(i32, i32)> {
        let cx = (x + self.half_x).floor() as i32;
        let cz = (z + self.half_z).floor() as i32;
        for radius in 0..self.width {
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

    /// Cell-centre waypoints from start to goal, excluding the start cell.
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
            let cx = (current as i32 % self.width) as f32;
            let cz = (current as i32 / self.width) as f32;
            cells.push((cx + 0.5 - self.half_x, cz + 0.5 - self.half_z));
            current = parent[current] as usize;
        }
        cells.reverse();
        cells
    }

    #[cfg(test)]
    pub fn open_cells(&self) -> usize {
        self.open.iter().filter(|o| **o).count()
    }

    /// Number of open cells reachable from a position.
    #[cfg(test)]
    pub fn reachable_from(&self, x: f32, z: f32) -> usize {
        let start = self.nearest_open(x, z).unwrap();
        let mut seen = vec![false; self.open.len()];
        let idx = |c: (i32, i32)| (c.1 * self.width + c.0) as usize;
        seen[idx(start)] = true;
        let mut stack = vec![start];
        let mut count = 0;
        while let Some(c) = stack.pop() {
            count += 1;
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = (c.0 + dx, c.1 + dz);
                if self.is_open(n.0, n.1) && !seen[idx(n)] {
                    seen[idx(n)] = true;
                    stack.push(n);
                }
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{PLAYER_HALF_WIDTH, PLAYER_HEIGHT};

    /// A box from `(min_x, min_z)` to `(max_x, max_z)`, taller than a player.
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

    /// A thin post that closes the one cell whose centre is at (x, z).
    fn post(x: f32, z: f32) -> MapBox {
        wall(x - 0.01, x + 0.01, z - 0.01, z + 0.01)
    }

    /// The cell a point is in.
    fn cell_of(grid: &NavGrid, (x, z): (f32, f32)) -> (i32, i32) {
        (
            (x + grid.half_x).floor() as i32,
            (z + grid.half_z).floor() as i32,
        )
    }

    /// Checks that `path` goes from `from`'s cell, one cell at a time, through the centres of open
    /// cells, and never squeezes diagonally past a closed one.
    fn assert_walkable(grid: &NavGrid, from: (f32, f32), path: &[(f32, f32)]) {
        let mut at = cell_of(grid, from);
        for &point in path {
            let next = cell_of(grid, point);
            let centre = (
                next.0 as f32 + 0.5 - grid.half_x,
                next.1 as f32 + 0.5 - grid.half_z,
            );
            assert_eq!(point, centre, "not a cell centre");
            assert!(grid.is_open(next.0, next.1), "{next:?} is closed");
            let (dx, dz) = (next.0.saturating_sub(at.0), next.1.saturating_sub(at.1));
            assert!(
                dx.unsigned_abs().max(dz.unsigned_abs()) == 1,
                "{at:?} to {next:?}"
            );
            if dx != 0 && dz != 0 {
                assert!(
                    grid.is_open(next.0, at.1) && grid.is_open(at.0, next.1),
                    "cut the corner from {at:?} to {next:?}"
                );
            }
            at = next;
        }
    }

    #[test]
    fn an_empty_map_is_open_everywhere_and_boxes_above_head_height_close_nothing() {
        // Ten cells by eight.
        assert_eq!(NavGrid::new(&[], 5.0, 4.0).open_cells(), 80);
        let lintel = MapBox {
            min_y: PLAYER_HEIGHT,
            ..wall(-5.0, 5.0, -4.0, 4.0)
        };
        assert_eq!(NavGrid::new(&[lintel], 5.0, 4.0).open_cells(), 80);
    }

    #[test]
    fn an_open_cell_leaves_room_for_a_player_at_its_centre() {
        let walls = [wall(-0.05, 0.05, -4.0, 2.0), wall(2.2, 3.7, 1.3, 1.9)];
        let grid = NavGrid::new(&walls, 5.0, 4.0);
        assert!(grid.open_cells() < 80, "the walls close some cells");
        let r = PLAYER_HALF_WIDTH;
        for cz in 0..grid.height {
            for cx in (0..grid.width).filter(|&cx| grid.is_open(cx, cz)) {
                let (x, z) = (cx as f32 + 0.5 - grid.half_x, cz as f32 + 0.5 - grid.half_z);
                for b in &walls {
                    let touches =
                        x + r > b.min_x && x - r < b.max_x && z + r > b.min_z && z - r < b.max_z;
                    assert!(!touches, "a player at ({x}, {z}) is inside {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_path_across_an_open_field_takes_the_fewest_steps_to_the_goals_cell() {
        let grid = NavGrid::new(&[], 5.0, 5.0);
        let from = (-4.5, -4.5);
        for (to, centre, steps) in [
            ((4.5, 4.5), (4.5, 4.5), 9),
            ((4.5, -4.5), (4.5, -4.5), 9),
            ((-4.5, 0.5), (-4.5, 0.5), 5),
            ((1.2, -2.7), (1.5, -2.5), 6),
        ] {
            let path = grid.path(from, to);
            assert_walkable(&grid, from, &path);
            assert_eq!(path.len(), steps, "to {to:?}");
            assert_eq!(path.last(), Some(&centre), "to {to:?}");
        }
        assert!(grid.path(from, (-4.2, -4.9)).is_empty(), "already there");
    }

    #[test]
    fn a_path_goes_round_a_wall_through_its_gap() {
        // A wall up the middle of the field, open only along the top row.
        let grid = NavGrid::new(&[wall(-0.1, 0.1, -6.0, 3.0)], 5.0, 5.0);
        let (from, to) = ((-4.5, -4.5), (4.5, -4.5));
        let path = grid.path(from, to);
        assert_walkable(&grid, from, &path);
        assert_eq!(path.last(), Some(&to));
        assert!(path.iter().any(|&(_, z)| z == 4.5), "not through the gap");
        assert!(
            path.len() > 9,
            "the way round is longer than the way across"
        );
    }

    #[test]
    fn a_path_never_squeezes_diagonally_past_a_closed_cell() {
        // Three cells a side. One closed cell beside the diagonal rules it out.
        let grid = NavGrid::new(&[post(0.0, -1.0)], 1.5, 1.5);
        let path = grid.path((-1.0, -1.0), (0.0, 0.0));
        assert_eq!(path, [(-1.0, 0.0), (0.0, 0.0)]);
        // Both closed seal the corner.
        let sealed = NavGrid::new(&[post(0.0, -1.0), post(-1.0, 0.0)], 1.5, 1.5);
        assert!(sealed.path((-1.0, -1.0), (0.0, 0.0)).is_empty());
    }

    #[test]
    fn a_walled_off_goal_has_no_path() {
        // A wall across the whole field, with four columns of ten cells open on each side.
        let grid = NavGrid::new(&[wall(-0.1, 0.1, -6.0, 6.0)], 5.0, 5.0);
        assert!(grid.path((-4.5, 0.5), (4.5, 0.5)).is_empty());
        assert_eq!(grid.reachable_from(-4.5, 0.5), 40);
        let path = grid.path((-4.5, 0.5), (-1.5, 3.5));
        assert_walkable(&grid, (-4.5, 0.5), &path);
        assert_eq!(path.last(), Some(&(-1.5, 3.5)), "its own side is open");
        // Nor is there one on a map closed everywhere.
        let closed = NavGrid::new(&[wall(-6.0, 6.0, -6.0, 6.0)], 5.0, 5.0);
        assert!(closed.path((-4.5, 0.5), (4.5, 0.5)).is_empty());
    }

    #[test]
    fn a_start_or_goal_inside_a_wall_moves_to_the_nearest_open_cell() {
        // A block closes the four by four cells in the middle of the field, two cells around the
        // centre's cell (5, 5).
        let grid = NavGrid::new(&[wall(-1.0, 1.0, -1.0, 1.0)], 5.0, 5.0);
        let (inside, outside) = ((0.0, 0.0), (4.5, 0.5));
        let out = grid.path(inside, outside);
        assert_eq!(out.last(), Some(&outside));
        for &point in &out {
            let (cx, cz) = cell_of(&grid, point);
            assert!(grid.is_open(cx, cz), "{point:?} is closed");
        }
        let back = grid.path(outside, inside);
        assert_walkable(&grid, outside, &back);
        let (cx, cz) = cell_of(&grid, *back.last().unwrap());
        let rings = cx
            .saturating_sub(5)
            .unsigned_abs()
            .max(cz.saturating_sub(5).unsigned_abs());
        assert_eq!(rings, 2, "ended at ({cx}, {cz})");
    }
}
