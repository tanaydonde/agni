//! Deterministic occupant evacuation over the static simulation grid.

use std::collections::{HashMap, HashSet};

use tflop_world_core::{OpeningTopologyError, Scene, Vec3};
use thiserror::Error;

use crate::{
    Agent, AgentFrame, AgentRole, GridCoordinate, OCCUPANT_RADIUS_M, OccupantPhase, OccupantSpawn,
    Simulation, SimulationConfig, SimulationError, SimulationGrid, SurfaceFireNodeFrame,
    nav::{
        HazardMap, NavProfile, advance_along_path, exit_targets, find_path, neighbors,
        room_floor_index, segment_overlaps_circle,
    },
};

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

        let exits = exit_targets(scene, simulation.grids(), OCCUPANT_RADIUS_M)?;
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

    /// Whether any occupant has yet to exit or become incapacitated.
    #[must_use]
    pub fn has_active_agents(&self) -> bool {
        self.agents.iter().any(Agent::is_active)
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
        let hazards = HazardMap::from_fire(
            &self.grids,
            &self.room_floors,
            fire,
            NavProfile::OCCUPANT.body_radius_m,
        );

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
        hazards: &HazardMap,
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
        let Some(path) = find_path(grid, start, targets, hazards, NavProfile::OCCUPANT) else {
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
    let exits = exit_targets(scene, simulation.grids(), OCCUPANT_RADIUS_M)?;
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

fn set_phase(agent: &mut Agent, phase: OccupantPhase) {
    if let AgentRole::Occupant(state) = &mut agent.role {
        state.phase = phase;
    }
}

const ZERO: Vec3 = Vec3 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};
