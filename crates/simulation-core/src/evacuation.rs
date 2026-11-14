//! Deterministic occupant evacuation over the static simulation grid.
//!
//! Each frame, every floor gets one [`ExitField`] from its exits, costed
//! around fire and crowds, and every occupant who has started moving walks
//! downhill on it. The [`Crowd`] does the walking, so occupants queue at
//! doors, step around each other, and contest space with firefighters.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use tflop_world_core::{OpeningTopologyError, Scene};
use thiserror::Error;

use crate::{
    Agent, AgentFrame, AgentRole, GridCoordinate, OCCUPANT_RADIUS_M, OccupantPhase, OccupantSpawn,
    Simulation, SimulationConfig, SimulationError, SimulationGrid, SurfaceFireNodeFrame,
    crowd::{Crowd, CrowdConfig, CrowdGroup, Goal, Intent, PassageMap, SINGLE_FILE_MAX_WIDTH_M},
    exit_field::ExitField,
    nav::{
        HazardMap, NavProfile, body_walkable_cells, exit_target_list, neighbors, room_floor_index,
    },
};

/// Cells around a body's center that its crowding cost covers.
const CROWDING_RADIUS_M: f64 = 0.5;

#[derive(Debug)]
pub struct Evacuation {
    grids: Vec<SimulationGrid>,
    /// Per grid: cells an occupant's body fits on.
    walkable: Vec<Vec<bool>>,
    passages: Vec<PassageMap>,
    agents: Vec<Agent>,
    /// `OpeningConnection` IDs, indexed by exit field label.
    exit_ids: Vec<String>,
    /// Per floor: each exit approach cell and its exit index.
    exit_cells: HashMap<String, Vec<(GridCoordinate, u32)>>,
    room_floors: HashMap<String, String>,
    /// What this frame's plan wants, one per agent.
    intents: Vec<Intent>,
    /// Fire hazards this frame's plan was made against.
    hazards: HazardMap,
    /// Moves occupants when the evacuation runs on its own, outside a
    /// [`Rehearsal`](crate::Rehearsal).
    crowd: Crowd,
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
        let grids = simulation.grids().to_vec();
        let (agents, exit_ids, exit_cells) = if occupants.is_empty() {
            (Vec::new(), Vec::new(), HashMap::new())
        } else {
            let targets = exit_target_list(scene, &grids, OCCUPANT_RADIUS_M)?;
            if targets.is_empty() {
                return Err(EvacuationError::NoUsableExteriorExits);
            }
            let mut exit_ids = Vec::<String>::new();
            let mut exit_cells = HashMap::<String, Vec<(GridCoordinate, u32)>>::new();
            for target in targets {
                let label = exit_ids.len() as u32;
                exit_ids.push(target.exit_id);
                exit_cells
                    .entry(target.floor_id)
                    .or_default()
                    .push((target.cell, label));
            }
            (simulation.agents().to_vec(), exit_ids, exit_cells)
        };

        Ok(Self {
            walkable: grids
                .iter()
                .map(|grid| body_walkable_cells(grid, OCCUPANT_RADIUS_M))
                .collect(),
            passages: grids
                .iter()
                .map(|grid| PassageMap::build(grid, SINGLE_FILE_MAX_WIDTH_M))
                .collect(),
            intents: vec![Intent::HOLD; agents.len()],
            grids,
            agents,
            exit_ids,
            exit_cells,
            room_floors: room_floor_index(scene),
            hazards: HazardMap::default(),
            crowd: Crowd::new(CrowdConfig::default(), config.random_seed),
            time_seconds: 0.0,
        })
    }

    /// Uses `config` for crowd movement and routing. Validated by the caller.
    pub(crate) fn set_crowd_config(&mut self, config: CrowdConfig) {
        self.crowd = Crowd::new(config, self.crowd_seed());
    }

    pub(crate) fn crowd_seed(&self) -> u64 {
        self.crowd.seed()
    }

    /// Whether any occupant has yet to exit or become incapacitated.
    #[must_use]
    pub fn has_active_agents(&self) -> bool {
        self.agents.iter().any(Agent::is_active)
    }

    #[must_use]
    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    /// Current renderer frames for every occupant.
    #[must_use]
    pub fn frames(&self) -> Vec<AgentFrame> {
        self.agents.iter().map(Agent::frame).collect()
    }

    /// Advances every occupant to `time_seconds` and returns a renderer frame.
    /// Fire changes reshape the exit fields, so a newly burning corridor can
    /// cause rerouting on the next simulation frame.
    pub fn advance_to(
        &mut self,
        time_seconds: f64,
        fire: &[SurfaceFireNodeFrame],
    ) -> Result<Vec<AgentFrame>, EvacuationError> {
        let delta_seconds = self.plan(time_seconds, fire, &[])?;
        let mut crowd = std::mem::replace(&mut self.crowd, Crowd::new(CrowdConfig::default(), 0));
        crowd.step(&mut [self.crowd_group()], delta_seconds);
        self.crowd = crowd;
        self.finish(time_seconds);
        Ok(self.frames())
    }

    /// Decides where every occupant wants to go over
    /// `[current time, time_seconds]`, against `fire` and around everyone
    /// standing in the building, `others` included. Returns the interval.
    pub(crate) fn plan(
        &mut self,
        time_seconds: f64,
        fire: &[SurfaceFireNodeFrame],
        others: &[Agent],
    ) -> Result<f64, EvacuationError> {
        if !time_seconds.is_finite() || time_seconds < self.time_seconds {
            return Err(EvacuationError::InvalidTime { time_seconds });
        }
        let delta_seconds = time_seconds - self.time_seconds;
        self.hazards = HazardMap::from_fire(
            &self.grids,
            &self.room_floors,
            fire,
            NavProfile::OCCUPANT.body_radius_m,
        );
        let fields = self.exit_fields(others);

        for index in 0..self.agents.len() {
            self.intents[index] = self.plan_agent(index, time_seconds, delta_seconds, &fields);
        }
        Ok(delta_seconds)
    }

    /// This frame's agents and intents, for the crowd to move.
    pub(crate) fn crowd_group(&mut self) -> CrowdGroup<'_> {
        CrowdGroup {
            agents: &mut self.agents,
            intents: &self.intents,
            grids: &self.grids,
            passages: &self.passages,
            hazards: &self.hazards,
            walk_radius_m: NavProfile::OCCUPANT.body_radius_m,
        }
    }

    /// Records who reached an exit once the crowd has moved.
    pub(crate) fn finish(&mut self, time_seconds: f64) {
        for index in 0..self.agents.len() {
            let Goal::Field(field) = &self.intents[index].goal else {
                continue;
            };
            let agent = &self.agents[index];
            let Some(grid) = self
                .grids
                .iter()
                .find(|grid| grid.floor_id == agent.floor_id)
            else {
                continue;
            };
            if grid
                .world_to_cell(agent.position_m)
                .is_some_and(|cell| field.is_exit(cell))
            {
                let agent = &mut self.agents[index];
                agent.velocity_m_per_s = ZERO;
                agent.movement = crate::Movement::Stopped;
                set_phase(agent, OccupantPhase::Exited);
            }
        }
        self.time_seconds = time_seconds;
    }

    /// One field per grid, with every body standing in the building adding
    /// crowding cost.
    fn exit_fields(&self, others: &[Agent]) -> Vec<Option<Arc<ExitField>>> {
        let crowding_cost = self.crowd.config().crowding_cost_per_body;
        self.grids
            .iter()
            .zip(&self.walkable)
            .map(|(grid, walkable)| {
                let targets = self.exit_cells.get(&grid.floor_id)?;
                let mut crowding = vec![0_u64; walkable.len()];
                if crowding_cost > 0 {
                    for body in self.agents.iter().chain(others) {
                        if body.floor_id == grid.floor_id && blocks_the_way(body) {
                            add_crowding(grid, body, crowding_cost, &mut crowding);
                        }
                    }
                }
                Some(Arc::new(ExitField::build(
                    grid,
                    walkable,
                    targets,
                    &self.hazards,
                    NavProfile::OCCUPANT,
                    &crowding,
                )))
            })
            .collect()
    }

    fn plan_agent(
        &mut self,
        index: usize,
        time_seconds: f64,
        delta_seconds: f64,
        fields: &[Option<Arc<ExitField>>],
    ) -> Intent {
        let agent = &mut self.agents[index];
        let AgentRole::Occupant(state) = &agent.role else {
            return Intent::HOLD;
        };
        let (delay, speed, phase) = (
            state.pre_movement_delay_seconds,
            state.walking_speed_m_per_s,
            state.phase,
        );
        if matches!(phase, OccupantPhase::Exited | OccupantPhase::Incapacitated) {
            return Intent::HOLD;
        }
        let movement_seconds = (time_seconds - self.time_seconds.max(delay)).max(0.0);
        if movement_seconds == 0.0 || delta_seconds == 0.0 {
            if time_seconds > 0.0 {
                set_phase(agent, OccupantPhase::Preparing);
            }
            return Intent::HOLD;
        }

        let grid_index = self
            .grids
            .iter()
            .position(|grid| grid.floor_id == agent.floor_id);
        let field = grid_index.and_then(|grid| fields.get(grid).cloned().flatten());
        let (Some(grid_index), Some(field)) = (grid_index, field) else {
            set_phase(agent, OccupantPhase::Rerouting);
            return Intent::HOLD;
        };
        let grid = &self.grids[grid_index];
        let Some(cell) = grid.world_to_cell(agent.position_m) else {
            set_phase(agent, OccupantPhase::Rerouting);
            return Intent::HOLD;
        };
        // A body standing just off a reachable cell center routes from the
        // neighbor it steps onto.
        let exit = field
            .exit(cell)
            .or_else(|| field.next_cell(cell).and_then(|next| field.exit(next)));
        let Some(exit) = exit else {
            set_phase(agent, OccupantPhase::Rerouting);
            return Intent::HOLD;
        };

        let exit_id = self.exit_ids.get(exit as usize).cloned();
        if let AgentRole::Occupant(state) = &mut agent.role {
            let changed = state.target_exit_id.is_some() && state.target_exit_id != exit_id;
            state.phase = if changed {
                OccupantPhase::Rerouting
            } else {
                OccupantPhase::MovingToExit
            };
            state.target_exit_id = exit_id;
        }
        Intent {
            goal: Goal::Field(field),
            // Someone who starts moving partway through the interval covers
            // only that part of it.
            speed_m_per_s: speed * movement_seconds / delta_seconds,
        }
    }
}

/// Whether a body takes up floor space others must route around.
fn blocks_the_way(agent: &Agent) -> bool {
    match &agent.role {
        AgentRole::Occupant(state) => !matches!(state.phase, OccupantPhase::Exited),
        AgentRole::Firefighter(state) => !matches!(
            state.phase,
            crate::FirefighterPhase::Staging | crate::FirefighterPhase::Exited
        ),
    }
}

fn add_crowding(grid: &SimulationGrid, agent: &Agent, cost: u64, crowding: &mut [u64]) {
    let span = (CROWDING_RADIUS_M / grid.cell_size_m).ceil() as i64;
    let Some(center) = grid.world_to_cell(agent.position_m) else {
        return;
    };
    for row_offset in -span..=span {
        for column_offset in -span..=span {
            let (Ok(column), Ok(row)) = (
                u32::try_from(i64::from(center.column) + column_offset),
                u32::try_from(i64::from(center.row) + row_offset),
            ) else {
                continue;
            };
            let coordinate = GridCoordinate { column, row };
            let Some(cell) = grid.cell(coordinate) else {
                continue;
            };
            if crate::nav::distance_2d(cell.center_m, agent.position_m) <= CROWDING_RADIUS_M {
                let index = row as usize * grid.columns as usize + column as usize;
                crowding[index] = crowding[index].saturating_add(cost);
            }
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
    let exits = crate::nav::exit_targets(scene, simulation.grids(), OCCUPANT_RADIUS_M)?;
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

const ZERO: tflop_world_core::Vec3 = tflop_world_core::Vec3 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

fn set_phase(agent: &mut Agent, phase: OccupantPhase) {
    if let AgentRole::Occupant(state) = &mut agent.role {
        state.phase = phase;
    }
}
