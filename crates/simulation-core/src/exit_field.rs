//! Shared route to the nearest exit for every occupant on a floor.
//!
//! One multi-source Dijkstra from every exit's approach cell gives each cell
//! its cheapest cost to leave and the exit that cost leads to. Occupants walk
//! downhill on it, so routing costs one search per floor per frame instead of
//! one per person. Crowded cells cost more, so people farther back in a long
//! queue switch to another exit when its route is cheaper.

use std::{cmp::Reverse, collections::BinaryHeap};

use crate::{
    GridCoordinate, SimulationGrid,
    nav::{Hazard, HazardMap, NORMAL_STEP_COST, NavProfile},
};

/// Cost to an exit from every cell of one floor.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExitField {
    columns: u32,
    rows: u32,
    /// Row-major; `u64::MAX` where no exit can be reached.
    costs: Vec<u64>,
    /// Index of the exit each cell drains to; `u32::MAX` where none.
    exits: Vec<u32>,
}

impl ExitField {
    /// Builds the field for one floor.
    ///
    /// `walkable` marks the cells a body fits on, `targets` pairs each exit
    /// approach cell with its exit index, and `crowding` adds a per-cell cost
    /// for the people standing there.
    pub(crate) fn build(
        grid: &SimulationGrid,
        walkable: &[bool],
        targets: &[(GridCoordinate, u32)],
        hazards: &HazardMap,
        profile: NavProfile,
        crowding: &[u64],
    ) -> Self {
        let cell_count = grid.columns as usize * grid.rows as usize;
        let mut field = Self {
            columns: grid.columns,
            rows: grid.rows,
            costs: vec![u64::MAX; cell_count],
            exits: vec![u32::MAX; cell_count],
        };
        let floor_id = grid.floor_id.as_str();
        let step_cost = |index: usize, coordinate: GridCoordinate| -> Option<u64> {
            if !walkable.get(index).copied().unwrap_or(false) {
                return None;
            }
            let hazard = match hazards.get(floor_id, coordinate) {
                Some(Hazard::Blocked) => return None,
                Some(Hazard::Penalized) => profile.heat_penalty,
                None => 0,
            };
            Some(NORMAL_STEP_COST + hazard + crowding.get(index).copied().unwrap_or(0))
        };

        let mut open = BinaryHeap::new();
        for &(cell, exit) in targets {
            let Some(index) = field.index(cell) else {
                continue;
            };
            if step_cost(index, cell).is_none() {
                continue;
            }
            // Ties between exits sharing a cell go to the lower index.
            if field.costs[index] == 0 && field.exits[index] <= exit {
                continue;
            }
            field.costs[index] = 0;
            field.exits[index] = exit;
            open.push(Reverse((0_u64, index)));
        }

        while let Some(Reverse((cost, index))) = open.pop() {
            if cost > field.costs[index] {
                continue;
            }
            let exit = field.exits[index];
            let neighbors = field.neighbors4(index).collect::<Vec<_>>();
            for neighbor in neighbors {
                let coordinate = field.coordinate(neighbor);
                let Some(step) = step_cost(neighbor, coordinate) else {
                    continue;
                };
                let next = cost + step;
                if next < field.costs[neighbor]
                    || (next == field.costs[neighbor] && exit < field.exits[neighbor])
                {
                    field.costs[neighbor] = next;
                    field.exits[neighbor] = exit;
                    open.push(Reverse((next, neighbor)));
                }
            }
        }
        field
    }

    /// Cost to the nearest exit from `cell`, or `None` when cut off.
    pub(crate) fn cost(&self, cell: GridCoordinate) -> Option<u64> {
        self.index(cell)
            .map(|index| self.costs[index])
            .filter(|&cost| cost != u64::MAX)
    }

    /// Index of the exit `cell` drains to.
    pub(crate) fn exit(&self, cell: GridCoordinate) -> Option<u32> {
        self.cost(cell)?;
        self.index(cell).map(|index| self.exits[index])
    }

    /// Whether `cell` is an exit's approach cell.
    pub(crate) fn is_exit(&self, cell: GridCoordinate) -> bool {
        self.cost(cell) == Some(0)
    }

    /// The neighbor (of eight) to step to from `cell`: the steepest descent
    /// per metre. Diagonals need both side cells reachable, so bodies don't
    /// cut wall corners. From a cell off the field (a body standing slightly
    /// off a walkable cell center), the cheapest reachable neighbor.
    pub(crate) fn next_cell(&self, cell: GridCoordinate) -> Option<GridCoordinate> {
        let here = self.cost(cell);
        let mut best: Option<(f64, GridCoordinate)> = None;
        for (dc, dr) in [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ] {
            let Some(neighbor) = self.offset(cell, dc, dr) else {
                continue;
            };
            let Some(cost) = self.cost(neighbor) else {
                continue;
            };
            let diagonal = dc != 0 && dr != 0;
            if diagonal
                && (self
                    .offset(cell, dc, 0)
                    .and_then(|side| self.cost(side))
                    .is_none()
                    || self
                        .offset(cell, 0, dr)
                        .and_then(|side| self.cost(side))
                        .is_none())
            {
                continue;
            }
            let score = match here {
                Some(here) => {
                    let drop = cost as f64 - here as f64;
                    if drop >= 0.0 {
                        continue;
                    }
                    drop / if diagonal {
                        std::f64::consts::SQRT_2
                    } else {
                        1.0
                    }
                }
                None => cost as f64,
            };
            if best.is_none_or(|(best_score, _)| score < best_score) {
                best = Some((score, neighbor));
            }
        }
        best.map(|(_, neighbor)| neighbor)
    }

    fn index(&self, cell: GridCoordinate) -> Option<usize> {
        (cell.column < self.columns && cell.row < self.rows)
            .then(|| cell.row as usize * self.columns as usize + cell.column as usize)
    }

    fn coordinate(&self, index: usize) -> GridCoordinate {
        let columns = self.columns as usize;
        GridCoordinate {
            column: (index % columns) as u32,
            row: (index / columns) as u32,
        }
    }

    fn offset(&self, cell: GridCoordinate, dc: i64, dr: i64) -> Option<GridCoordinate> {
        let column = u32::try_from(i64::from(cell.column) + dc).ok()?;
        let row = u32::try_from(i64::from(cell.row) + dr).ok()?;
        (column < self.columns && row < self.rows).then_some(GridCoordinate { column, row })
    }

    fn neighbors4(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let cell = self.coordinate(index);
        [(-1, 0), (1, 0), (0, -1), (0, 1)]
            .into_iter()
            .filter_map(move |(dc, dr)| self.offset(cell, dc, dr))
            .filter_map(|neighbor| self.index(neighbor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_from(rows: &[&str], targets: &[(u32, u32, u32)], crowding: &[u64]) -> ExitField {
        let columns = rows[0].len() as u32;
        let walkable = rows
            .iter()
            .flat_map(|row| row.chars().map(|cell| cell == '.'))
            .collect::<Vec<_>>();
        let grid = SimulationGrid::test_grid("floor-1", columns, rows.len() as u32, 0.25);
        let targets = targets
            .iter()
            .map(|&(column, row, exit)| (GridCoordinate { column, row }, exit))
            .collect::<Vec<_>>();
        ExitField::build(
            &grid,
            &walkable,
            &targets,
            &HazardMap::default(),
            NavProfile::OCCUPANT,
            crowding,
        )
    }

    fn at(column: u32, row: u32) -> GridCoordinate {
        GridCoordinate { column, row }
    }

    #[test]
    fn each_cell_drains_to_its_nearest_exit() {
        let field = field_from(&["........"], &[(0, 0, 0), (7, 0, 1)], &[]);
        assert_eq!(field.exit(at(2, 0)), Some(0));
        assert_eq!(field.exit(at(5, 0)), Some(1));
        assert_eq!(field.cost(at(2, 0)), Some(2 * NORMAL_STEP_COST));
        assert!(field.is_exit(at(7, 0)));
    }

    #[test]
    fn walls_cut_cells_off() {
        let field = field_from(&["..#.."], &[(0, 0, 0)], &[]);
        assert_eq!(field.cost(at(3, 0)), None);
        assert_eq!(field.next_cell(at(3, 0)), None);
    }

    #[test]
    fn crowding_sends_people_to_a_farther_exit() {
        let mut crowding = vec![0; 8];
        crowding[1] = 10 * NORMAL_STEP_COST;
        let field = field_from(&["........"], &[(0, 0, 0), (7, 0, 1)], &crowding);
        assert_eq!(field.exit(at(2, 0)), Some(1));
    }

    #[test]
    fn descent_takes_diagonals_but_not_around_corners() {
        let field = field_from(&["...", "...", "..."], &[(0, 0, 0)], &[]);
        assert_eq!(field.next_cell(at(2, 2)), Some(at(1, 1)));
        let blocked = field_from(&["...", ".#.", "..."], &[(0, 0, 0)], &[]);
        let next = blocked.next_cell(at(2, 2)).expect("a route exists");
        assert!(next == at(1, 2) || next == at(2, 1));
    }
}
