//! Deterministic occupant evacuation over the static simulation grid.

use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, HashSet},
};

use tflop_world_core::{OpeningTopologyError, Scene, Vec3};
use thiserror::Error;

use crate::{
    Agent, AgentFrame, AgentRole, CombustionState, GridCoordinate, OCCUPANT_RADIUS_M,
    OccupantPhase, OccupantSpawn, Simulation, SimulationConfig, SimulationError, SimulationGrid,
    SurfaceFireNodeFrame, SurfaceFireNodeSource, exterior_exits,
};

const NORMAL_STEP_COST: u64 = 1_000;
const HEAT_PENALTY: u64 = 8_000;
const EXIT_SEARCH_RADIUS_M: f64 = 1.5;
const FIRE_AVOIDANCE_RADIUS_M: f64 = 1.0;

#[derive(Debug)]
pub struct Evacuation {
    grids: Vec<SimulationGrid>,
    agents: Vec<Agent>,
    exits: HashMap<String, Vec<GridCoordinate>>,
    room_floors: HashMap<String, String>,
    time_seconds: f64,
}

impl Evacuation {
    pub fn new(
        scene: &Scene,
        occupants: &[OccupantSpawn],
        config: SimulationConfig,
    ) -> Result<Self, EvacuationError> {
        let mut simulation = Simulation::new(scene.clone(), config)?;
        simulation.spawn_occupants(occupants)?;
        if occupants.is_empty() {
            return Ok(Self {
                grids: simulation.grids().to_vec(),
                agents: Vec::new(),
                exits: HashMap::new(),
                room_floors: room_floor_index(scene),
                time_seconds: 0.0,
            });
        }

        let exits = exit_targets(scene, simulation.grids())?;
        if exits.values().all(Vec::is_empty) {
            return Err(EvacuationError::NoUsableExteriorExits);
        }

        Ok(Self {
            grids: simulation.grids().to_vec(),
            agents: simulation.agents().to_vec(),
            exits,
            room_floors: room_floor_index(scene),
            time_seconds: 0.0,
        })
    }

    /// Advances every occupant to `time_seconds` and returns a renderer frame.
    /// Fire changes trigger a fresh route search, so a newly burning corridor
    /// can cause rerouting on the next simulation frame.
    pub fn advance_to(
        &mut self,
        time_seconds: f64,
        fire: &[SurfaceFireNodeFrame],
    ) -> Result<Vec<AgentFrame>, EvacuationError> {
        if !time_seconds.is_finite() || time_seconds < self.time_seconds {
            return Err(EvacuationError::InvalidTime { time_seconds });
        }
        let delta_seconds = time_seconds - self.time_seconds;
        let hazards = hazard_cells(&self.grids, &self.room_floors, fire);

        for index in 0..self.agents.len() {
            self.advance_agent(index, time_seconds, delta_seconds, &hazards);
        }
        self.time_seconds = time_seconds;
        Ok(self.agents.iter().map(Agent::frame).collect())
    }

    fn advance_agent(
        &mut self,
        index: usize,
        time_seconds: f64,
        delta_seconds: f64,
        hazards: &HashMap<(String, GridCoordinate), Hazard>,
    ) {
        let (delay, speed, phase) = match &self.agents[index].role {
            AgentRole::Occupant(state) => (
                state.pre_movement_delay_seconds,
                state.walking_speed_m_per_s,
                state.phase,
            ),
            AgentRole::Firefighter(_) => return,
        };
        if matches!(phase, OccupantPhase::Exited | OccupantPhase::Incapacitated) {
            self.agents[index].velocity_m_per_s = ZERO;
            return;
        }
        let movement_seconds = (time_seconds - self.time_seconds.max(delay)).max(0.0);
        if movement_seconds == 0.0 || delta_seconds == 0.0 {
            if time_seconds > 0.0
                && let AgentRole::Occupant(state) = &mut self.agents[index].role
            {
                state.phase = OccupantPhase::Preparing;
            }
            self.agents[index].velocity_m_per_s = ZERO;
            return;
        }

        let floor_id = self.agents[index].floor_id.clone();
        let Some(grid) = self.grids.iter().find(|grid| grid.floor_id == floor_id) else {
            return;
        };
        let Some(start) = grid.world_to_cell(self.agents[index].position_m) else {
            return;
        };
        let Some(targets) = self.exits.get(&floor_id) else {
            set_phase(&mut self.agents[index], OccupantPhase::Rerouting);
            return;
        };
        let Some(path) = find_path(grid, start, targets, &floor_id, hazards) else {
            self.agents[index].velocity_m_per_s = ZERO;
            set_phase(&mut self.agents[index], OccupantPhase::Rerouting);
            return;
        };

        let old_position = self.agents[index].position_m;
        let (candidate, reached_exit) =
            advance_along_path(old_position, &path, grid, speed * movement_seconds, targets);
        let blocked_by_person = self.agents.iter().enumerate().any(|(other_index, other)| {
            other_index != index
                && other.is_active()
                && segment_overlaps_circle(
                    old_position,
                    candidate,
                    other.position_m,
                    OCCUPANT_RADIUS_M + other.radius_m,
                )
        });
        if blocked_by_person {
            self.agents[index].velocity_m_per_s = ZERO;
            set_phase(&mut self.agents[index], OccupantPhase::MovingToExit);
            return;
        }

        self.agents[index].position_m = candidate;
        self.agents[index].velocity_m_per_s = if delta_seconds > 0.0 {
            Vec3 {
                x: (candidate.x - old_position.x) / delta_seconds,
                y: (candidate.y - old_position.y) / delta_seconds,
                z: 0.0,
            }
        } else {
            ZERO
        };
        if reached_exit {
            self.agents[index].velocity_m_per_s = ZERO;
            set_phase(&mut self.agents[index], OccupantPhase::Exited);
        } else {
            set_phase(&mut self.agents[index], OccupantPhase::MovingToExit);
        }
    }
}

/// Grid cells from which a body-sized occupant can reach an exterior exit in
/// the static building. The scenario generator uses this to avoid placing
/// people in disconnected scan fragments.
pub fn reachable_exit_cells(
    scene: &Scene,
    config: SimulationConfig,
) -> Result<HashSet<(String, GridCoordinate)>, EvacuationError> {
    let simulation = Simulation::new(scene.clone(), config)?;
    let exits = exit_targets(scene, simulation.grids())?;
    if exits.values().all(Vec::is_empty) {
        return Err(EvacuationError::NoUsableExteriorExits);
    }

    let mut reachable = HashSet::new();
    for grid in simulation.grids() {
        let Some(targets) = exits.get(&grid.floor_id) else {
            continue;
        };
        let mut pending = targets.clone();
        while let Some(coordinate) = pending.pop() {
            if !reachable.insert((grid.floor_id.clone(), coordinate)) {
                continue;
            }
            for neighbor in neighbors(grid, coordinate) {
                let Some(cell) = grid.cell(neighbor) else {
                    continue;
                };
                if grid.circle_is_walkable(cell.center_m, OCCUPANT_RADIUS_M)
                    && !reachable.contains(&(grid.floor_id.clone(), neighbor))
                {
                    pending.push(neighbor);
                }
            }
        }
    }
    Ok(reachable)
}

#[derive(Debug, Error)]
pub enum EvacuationError {
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error(transparent)]
    OpeningTopology(#[from] OpeningTopologyError),
    #[error("the scene has no exterior door or passage with a usable interior approach")]
    NoUsableExteriorExits,
    #[error("evacuation time must be finite and nondecreasing, got {time_seconds}")]
    InvalidTime { time_seconds: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hazard {
    Penalized,
    Blocked,
}

fn nearest_exit_cell(
    grid: &SimulationGrid,
    center_m: Vec3,
    room_id: &str,
) -> Option<GridCoordinate> {
    grid.cells()
        .iter()
        .filter(|cell| {
            cell.room_ids.iter().any(|id| id == room_id)
                && grid.circle_is_walkable(cell.center_m, OCCUPANT_RADIUS_M)
        })
        .filter_map(|cell| {
            let distance = distance_2d(cell.center_m, center_m);
            (distance <= EXIT_SEARCH_RADIUS_M).then_some((cell.coordinate, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(coordinate, _)| coordinate)
}

fn exit_targets(
    scene: &Scene,
    grids: &[SimulationGrid],
) -> Result<HashMap<String, Vec<GridCoordinate>>, OpeningTopologyError> {
    let mut exits = HashMap::<String, Vec<GridCoordinate>>::new();
    for exit in exterior_exits(scene)? {
        let Some(grid) = grids.iter().find(|grid| grid.floor_id == exit.floor_id) else {
            continue;
        };
        if let Some(target) = nearest_exit_cell(grid, exit.center_m, &exit.room_a_id) {
            exits.entry(exit.floor_id).or_default().push(target);
        }
    }
    for targets in exits.values_mut() {
        targets.sort_by_key(|target| (target.row, target.column));
        targets.dedup();
    }
    Ok(exits)
}

fn room_floor_index(scene: &Scene) -> HashMap<String, String> {
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

fn hazard_cells(
    grids: &[SimulationGrid],
    room_floors: &HashMap<String, String>,
    fire: &[SurfaceFireNodeFrame],
) -> HashMap<(String, GridCoordinate), Hazard> {
    let mut hazards = HashMap::new();
    for node in fire {
        if matches!(
            node.state,
            CombustionState::BurnedOut | CombustionState::Unignited
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
                    && distance < footprint_radius + OCCUPANT_RADIUS_M
                {
                    Some(Hazard::Blocked)
                } else if distance < affected_radius {
                    Some(Hazard::Penalized)
                } else {
                    None
                };
                if let Some(hazard) = hazard {
                    hazards
                        .entry((floor_id.clone(), cell.coordinate))
                        .and_modify(|current| {
                            if hazard == Hazard::Blocked {
                                *current = Hazard::Blocked;
                            }
                        })
                        .or_insert(hazard);
                }
            }
        }
    }
    hazards
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

fn find_path(
    grid: &SimulationGrid,
    start: GridCoordinate,
    targets: &[GridCoordinate],
    floor_id: &str,
    hazards: &HashMap<(String, GridCoordinate), Hazard>,
) -> Option<Vec<GridCoordinate>> {
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
            let Some(cell) = grid.cell(neighbor) else {
                continue;
            };
            if !grid.circle_is_walkable(cell.center_m, OCCUPANT_RADIUS_M) {
                continue;
            }
            let hazard = hazards.get(&(floor_id.to_owned(), neighbor));
            if hazard == Some(&Hazard::Blocked) {
                continue;
            }
            let step = NORMAL_STEP_COST
                + if hazard == Some(&Hazard::Penalized) {
                    HEAT_PENALTY
                } else {
                    0
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

fn neighbors(grid: &SimulationGrid, coordinate: GridCoordinate) -> Vec<GridCoordinate> {
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

fn advance_along_path(
    mut position: Vec3,
    path: &[GridCoordinate],
    grid: &SimulationGrid,
    mut distance_left: f64,
    exits: &[GridCoordinate],
) -> (Vec3, bool) {
    for coordinate in path.iter().skip(1) {
        let Some(target) = grid.cell_center(*coordinate) else {
            break;
        };
        let distance = distance_2d(position, target);
        if distance > distance_left {
            let fraction = distance_left / distance;
            position.x += (target.x - position.x) * fraction;
            position.y += (target.y - position.y) * fraction;
            return (position, false);
        }
        position = target;
        distance_left -= distance;
    }
    let coordinate = grid.world_to_cell(position);
    (
        position,
        coordinate.is_some_and(|cell| exits.contains(&cell)),
    )
}

fn set_phase(agent: &mut Agent, phase: OccupantPhase) {
    if let AgentRole::Occupant(state) = &mut agent.role {
        state.phase = phase;
    }
}

fn segment_overlaps_circle(start: Vec3, end: Vec3, center: Vec3, radius: f64) -> bool {
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

fn distance_2d(left: Vec3, right: Vec3) -> f64 {
    (left.x - right.x).hypot(left.y - right.y)
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

const ZERO: Vec3 = Vec3 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};
