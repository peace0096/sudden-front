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
        let (Some(start), Some(goal)) =
            (self.nearest_open(from.0, from.1), self.nearest_open(to.0, to.1))
        else {
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
                if dx != 0 && dz != 0 && !(self.is_open(cell.0 + dx, cell.1) && self.is_open(cell.0, cell.1 + dz)) {
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
