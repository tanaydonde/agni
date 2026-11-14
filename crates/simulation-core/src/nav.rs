//! Grid navigation shared by every agent role: hazard maps derived from the
//! current fire, A* routing, and movement along a route.
//!
//! Roles differ only through [`NavProfile`]: body size decides which cells a
//! body fits through and how close to flames counts as blocked, and the heat
//! penalty decides how strongly routes avoid cells near fire.

use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, HashSet},
};

use tflop_world_core::{OpeningTopologyError, Scene, Vec3};

use crate::{
    CombustionState, GridCoordinate, OCCUPANT_RADIUS_M, SimulationGrid, SurfaceFireNodeFrame,
    SurfaceFireNodeSource, exterior_exits,
};

/// Cost of one step onto an unaffected cell.
pub(crate) const NORMAL_STEP_COST: u64 = 1_000;
/// Distance beyond a burning node's footprint that still counts as near fire.
const FIRE_AVOIDANCE_RADIUS_M: f64 = 1.0;
/// How far from an exterior opening's center to look for a walkable cell.
const EXIT_SEARCH_RADIUS_M: f64 = 1.5;

/// How one kind of agent moves through hazards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NavProfile {
    /// The body must fit on walkable cells, and cells closer than this to a
    /// burning footprint are impassable.
    pub body_radius_m: f64,
    /// Added to the step cost of a cell near fire.
    pub heat_penalty: u64,
}

impl NavProfile {
    pub(crate) const OCCUPANT: Self = Self {
        body_radius_m: OCCUPANT_RADIUS_M,
        heat_penalty: 8_000,
    };
    /// Turnout gear makes heat near fire less of a deterrent. The clearance
    /// matches occupants' so a crew can reach anywhere an occupant can;
    /// scanned doors can be under 0.6 m wide.
    pub(crate) const FIREFIGHTER: Self = Self {
        body_radius_m: OCCUPANT_RADIUS_M,
        heat_penalty: 2_000,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hazard {
    /// Near fire: passable at extra cost.
    Penalized,
    /// Too close to a burning node to pass.
    Blocked,
}

/// Hazards for one fire state and one body size, keyed by floor.
#[derive(Debug, Default)]
pub(crate) struct HazardMap {
    floors: HashMap<String, HashMap<GridCoordinate, Hazard>>,
}

impl HazardMap {
    /// Builds the hazards that `fire` creates for a body of `body_radius_m`.
    /// Heating and burning nodes penalize nearby cells; only burning nodes
    /// block them.
    pub(crate) fn from_fire(
        grids: &[SimulationGrid],
        room_floors: &HashMap<String, String>,
        fire: &[SurfaceFireNodeFrame],
        body_radius_m: f64,
    ) -> Self {
        let mut map = Self::default();
        for node in fire {
            if matches!(
                node.state,
                CombustionState::BurnedOut
                    | CombustionState::Unignited
                    | CombustionState::Extinguished
            ) {
                continue;
            }
            let floor_id = match &node.source {
                SurfaceFireNodeSource::FloorCell { floor_id, .. } => Some(floor_id),
                SurfaceFireNodeSource::Furniture { .. } => room_floors.get(&node.room_id),
            };
            let Some(floor_id) = floor_id else { continue };
            let Some(grid) = grids.iter().find(|grid| grid.floor_id == *floor_id) else {
                continue;
            };
            let footprint_radius = node.dimensions_m.width_m.hypot(node.dimensions_m.depth_m) / 2.0;
            let affected_radius = footprint_radius + FIRE_AVOIDANCE_RADIUS_M;
            let (min_column, max_column) = coordinate_range(
                node.position_m.x,
                affected_radius,
                grid.bounds.min_x,
                grid.cell_size_m,
                grid.columns,
            );
            let (min_row, max_row) = coordinate_range(
                node.position_m.y,
                affected_radius,
                grid.bounds.min_y,
                grid.cell_size_m,
                grid.rows,
            );
            let floor = map.floors.entry(floor_id.clone()).or_default();
            for row in min_row..=max_row {
                for column in min_column..=max_column {
                    let Some(cell) = grid.cell(GridCoordinate { column, row }) else {
                        continue;
                    };
                    if !cell.is_walkable() {
                        continue;
                    }
                    let distance = distance_2d(cell.center_m, node.position_m);
                    let hazard = if node.state == CombustionState::Burning
                        && distance < footprint_radius + body_radius_m
                    {
                        Hazard::Blocked
                    } else if distance < affected_radius {
                        Hazard::Penalized
                    } else {
                        continue;
                    };
                    floor
                        .entry(cell.coordinate)
                        .and_modify(|current| {
                            if hazard == Hazard::Blocked {
                                *current = Hazard::Blocked;
                            }
                        })
                        .or_insert(hazard);
                }
            }
        }
        map
    }

    pub(crate) fn get(&self, floor_id: &str, coordinate: GridCoordinate) -> Option<Hazard> {
        self.floors
            .get(floor_id)
            .and_then(|floor| floor.get(&coordinate))
            .copied()
    }
}

/// Room ID to floor ID, for placing furniture fire nodes on a floor grid.
pub(crate) fn room_floor_index(scene: &Scene) -> HashMap<String, String> {
    scene
        .floors
        .iter()
        .flat_map(|floor| {
            floor
                .rooms
                .iter()
                .map(move |room| (room.id.clone(), floor.id.clone()))
        })
        .collect()
}

/// Walkable target cells next to each exterior exit, keyed by floor, for a
/// body of `body_radius_m`. Sorted and deduplicated so routing is
/// deterministic.
pub(crate) fn exit_targets(
    scene: &Scene,
    grids: &[SimulationGrid],
    body_radius_m: f64,
) -> Result<HashMap<String, Vec<GridCoordinate>>, OpeningTopologyError> {
    let mut exits = HashMap::<String, Vec<GridCoordinate>>::new();
    for exit in exterior_exits(scene)? {
        let Some(grid) = grids.iter().find(|grid| grid.floor_id == exit.floor_id) else {
            continue;
        };
        if let Some(target) =
            nearest_walkable_cell(grid, exit.center_m, &exit.room_a_id, body_radius_m)
        {
            exits.entry(exit.floor_id).or_default().push(target);
        }
    }
    for targets in exits.values_mut() {
        targets.sort_by_key(|target| (target.row, target.column));
        targets.dedup();
    }
    Ok(exits)
}

/// Body-walkable cell of `room_id` nearest `center_m`, within the exit search
/// radius.
pub(crate) fn nearest_walkable_cell(
    grid: &SimulationGrid,
    center_m: Vec3,
    room_id: &str,
    body_radius_m: f64,
) -> Option<GridCoordinate> {
    grid.cells()
        .iter()
        .filter(|cell| {
            cell.room_ids.iter().any(|id| id == room_id)
                && grid.circle_is_walkable(cell.center_m, body_radius_m)
        })
        .filter_map(|cell| {
            let distance = distance_2d(cell.center_m, center_m);
            (distance <= EXIT_SEARCH_RADIUS_M).then_some((cell.coordinate, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(coordinate, _)| coordinate)
}

/// Cheapest route from `start` to any of `targets`, as cells from start to
/// target inclusive. `None` when every route is blocked.
pub(crate) fn find_path(
    grid: &SimulationGrid,
    start: GridCoordinate,
    targets: &[GridCoordinate],
    hazards: &HazardMap,
    profile: NavProfile,
) -> Option<Vec<GridCoordinate>> {
    let floor_id = grid.floor_id.as_str();
    let targets = targets.iter().copied().collect::<HashSet<_>>();
    let mut open = BinaryHeap::new();
    let mut costs = HashMap::new();
    let mut previous = HashMap::new();
    costs.insert(start, 0_u64);
    open.push(PathNode {
        coordinate: start,
        estimated_total: heuristic(start, &targets),
        cost: 0,
    });

    while let Some(node) = open.pop() {
        if targets.contains(&node.coordinate) {
            return Some(reconstruct_path(node.coordinate, &previous));
        }
        if node.cost > *costs.get(&node.coordinate).unwrap_or(&u64::MAX) {
            continue;
        }
        for neighbor in neighbors(grid, node.coordinate) {
            let Some(step) = step_cost(grid, floor_id, neighbor, hazards, profile) else {
                continue;
            };
            let next_cost = node.cost + step;
            if next_cost >= *costs.get(&neighbor).unwrap_or(&u64::MAX) {
                continue;
            }
            costs.insert(neighbor, next_cost);
            previous.insert(neighbor, node.coordinate);
            open.push(PathNode {
                coordinate: neighbor,
                estimated_total: next_cost + heuristic(neighbor, &targets),
                cost: next_cost,
            });
        }
    }
    None
}

/// Cheapest cost from `start` to every reachable cell, by the same step costs
/// as [`find_path`]. Used to rank many destinations with one search.
pub(crate) fn distance_field(
    grid: &SimulationGrid,
    start: GridCoordinate,
    hazards: &HazardMap,
    profile: NavProfile,
) -> HashMap<GridCoordinate, u64> {
    let floor_id = grid.floor_id.as_str();
    let mut costs = HashMap::from([(start, 0_u64)]);
    let mut open = BinaryHeap::from([PathNode {
        coordinate: start,
        estimated_total: 0,
        cost: 0,
    }]);
    while let Some(node) = open.pop() {
        if node.cost > *costs.get(&node.coordinate).unwrap_or(&u64::MAX) {
            continue;
        }
        for neighbor in neighbors(grid, node.coordinate) {
            let Some(step) = step_cost(grid, floor_id, neighbor, hazards, profile) else {
                continue;
            };
            let next_cost = node.cost + step;
            if next_cost >= *costs.get(&neighbor).unwrap_or(&u64::MAX) {
                continue;
            }
            costs.insert(neighbor, next_cost);
            open.push(PathNode {
                coordinate: neighbor,
                estimated_total: next_cost,
                cost: next_cost,
            });
        }
    }
    costs
}

/// Cost of stepping onto `coordinate`, or `None` when the body can't stand
/// there.
fn step_cost(
    grid: &SimulationGrid,
    floor_id: &str,
    coordinate: GridCoordinate,
    hazards: &HazardMap,
    profile: NavProfile,
) -> Option<u64> {
    let cell = grid.cell(coordinate)?;
    if !grid.circle_is_walkable(cell.center_m, profile.body_radius_m) {
        return None;
    }
    match hazards.get(floor_id, coordinate) {
        Some(Hazard::Blocked) => None,
        Some(Hazard::Penalized) => Some(NORMAL_STEP_COST + profile.heat_penalty),
        None => Some(NORMAL_STEP_COST),
    }
}

/// The four edge-adjacent cells that exist on the grid.
pub(crate) fn neighbors(grid: &SimulationGrid, coordinate: GridCoordinate) -> Vec<GridCoordinate> {
    let mut neighbors = Vec::with_capacity(4);
    if coordinate.column > 0 {
        neighbors.push(GridCoordinate {
            column: coordinate.column - 1,
            row: coordinate.row,
        });
    }
    if coordinate.column + 1 < grid.columns {
        neighbors.push(GridCoordinate {
            column: coordinate.column + 1,
            row: coordinate.row,
        });
    }
    if coordinate.row > 0 {
        neighbors.push(GridCoordinate {
            column: coordinate.column,
            row: coordinate.row - 1,
        });
    }
    if coordinate.row + 1 < grid.rows {
        neighbors.push(GridCoordinate {
            column: coordinate.column,
            row: coordinate.row + 1,
        });
    }
    neighbors
}

/// Moves up to `distance_m` along `path` (whose first cell is the current
/// one). Returns the new position and whether it ended on one of `targets`.
pub(crate) fn advance_along_path(
    mut position: Vec3,
    path: &[GridCoordinate],
    grid: &SimulationGrid,
    mut distance_m: f64,
    targets: &[GridCoordinate],
) -> (Vec3, bool) {
    for coordinate in path.iter().skip(1) {
        let Some(target) = grid.cell_center(*coordinate) else {
            break;
        };
        let distance = distance_2d(position, target);
        if distance > distance_m {
            let fraction = distance_m / distance;
            position.x += (target.x - position.x) * fraction;
            position.y += (target.y - position.y) * fraction;
            return (position, false);
        }
        position = target;
        distance_m -= distance;
    }
    let coordinate = grid.world_to_cell(position);
    (
        position,
        coordinate.is_some_and(|cell| targets.contains(&cell)),
    )
}

/// Whether moving from `start` to `end` passes within `radius` of `center`.
pub(crate) fn segment_overlaps_circle(start: Vec3, end: Vec3, center: Vec3, radius: f64) -> bool {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_squared = dx * dx + dy * dy;
    let fraction = if length_squared == 0.0 {
        0.0
    } else {
        (((center.x - start.x) * dx + (center.y - start.y) * dy) / length_squared).clamp(0.0, 1.0)
    };
    let closest = Vec3 {
        x: start.x + dx * fraction,
        y: start.y + dy * fraction,
        z: start.z,
    };
    distance_2d(closest, center) < radius
}

pub(crate) fn distance_2d(left: Vec3, right: Vec3) -> f64 {
    (left.x - right.x).hypot(left.y - right.y)
}

fn coordinate_range(
    center: f64,
    radius: f64,
    grid_min: f64,
    cell_size: f64,
    count: u32,
) -> (u32, u32) {
    let minimum = ((center - radius - grid_min) / cell_size)
        .floor()
        .clamp(0.0, f64::from(count - 1)) as u32;
    let maximum = ((center + radius - grid_min) / cell_size)
        .floor()
        .clamp(0.0, f64::from(count - 1)) as u32;
    (minimum, maximum)
}

fn heuristic(coordinate: GridCoordinate, targets: &HashSet<GridCoordinate>) -> u64 {
    targets
        .iter()
        .map(|target| {
            u64::from(
                coordinate.column.abs_diff(target.column) + coordinate.row.abs_diff(target.row),
            ) * NORMAL_STEP_COST
        })
        .min()
        .unwrap_or(0)
}

fn reconstruct_path(
    mut coordinate: GridCoordinate,
    previous: &HashMap<GridCoordinate, GridCoordinate>,
) -> Vec<GridCoordinate> {
    let mut path = vec![coordinate];
    while let Some(parent) = previous.get(&coordinate) {
        coordinate = *parent;
        path.push(coordinate);
    }
    path.reverse();
    path
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PathNode {
    coordinate: GridCoordinate,
    estimated_total: u64,
    cost: u64,
}

impl Ord for PathNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimated_total
            .cmp(&self.estimated_total)
            .then_with(|| other.cost.cmp(&self.cost))
            .then_with(|| other.coordinate.row.cmp(&self.coordinate.row))
            .then_with(|| other.coordinate.column.cmp(&self.coordinate.column))
    }
}

impl PartialOrd for PathNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
