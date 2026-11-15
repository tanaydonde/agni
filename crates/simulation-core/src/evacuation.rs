//! Deterministic occupant evacuation over the static simulation grid.
//!
//! Nobody reacts at ignition. Once a fire in some room is big enough to
//! notice and someone is in that room, they raise the alarm; everyone else
//! hears it later the farther they are from them by walking distance (see
//! [`AlarmConfig`]). Each person then takes their own pre-movement delay
//! before moving. A fire in an empty room alerts nobody until it spreads to
//! a room with people in it.
//!
//! Not everyone heads for the same door: each occupant is given a random
//! exit, among those reachable from where they start, and keeps it unless
//! fire cuts it off; then they head for the nearest exit they can still
//! reach, for good. Each frame, every exit in use gets an [`ExitField`]
//! costed around fire and crowds, and every occupant who has started moving
//! walks downhill on their exit's field. The [`Crowd`] does the walking, so
//! occupants queue at doors, step around each other, and contest space with
//! firefighters.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use tflop_world_core::{OpeningTopologyError, Scene, Vec3};
use thiserror::Error;

use crate::{
    Agent, AgentFrame, AgentRole, CombustionState, GridCoordinate, OCCUPANT_RADIUS_M,
    OccupantPhase, OccupantSpawn, Simulation, SimulationConfig, SimulationError, SimulationGrid,
    SurfaceFireNodeFrame,
    crowd::{
        Crowd, CrowdConfig, CrowdGroup, Goal, Intent, PassageMap, SINGLE_FILE_MAX_WIDTH_M, unit,
    },
    exit_field::ExitField,
    nav::{
        HazardMap, NORMAL_STEP_COST, NavProfile, body_walkable_cells, distance_2d,
        exit_target_list, neighbors, room_floor_index,
    },
};

/// When occupants learn about the fire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlarmConfig {
    /// Everyone knows from ignition.
    Immediate,
    /// Someone in a room whose fire reaches `notice_hrr_kw` raises the
    /// alarm. People in that room hear it after `same_room_seconds`, and
    /// everyone else `seconds_per_metre` later for each metre they are from
    /// whoever raised it, by walking distance.
    Detected {
        notice_hrr_kw: f64,
        same_room_seconds: f64,
        seconds_per_metre: f64,
    },
}

impl Default for AlarmConfig {
    fn default() -> Self {
        Self::Detected {
            // A small furniture fire: flames clearly visible across a room.
            notice_hrr_kw: 50.0,
            same_room_seconds: 2.0,
            seconds_per_metre: 0.2,
        }
    }
}

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
    /// The exit each occupant is heading for, by exit index.
    assigned_exits: Vec<Option<u32>>,
    alarm: AlarmConfig,
    /// When the alarm was raised, once it has been.
    alarm_raised_at: Option<f64>,
    /// When each occupant hears the alarm, once it has been raised.
    alerted_at: Vec<Option<f64>>,
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

        let walkable = grids
            .iter()
            .map(|grid| body_walkable_cells(grid, OCCUPANT_RADIUS_M))
            .collect::<Vec<_>>();
        let assigned_exits =
            assign_exits(&agents, &grids, &walkable, &exit_cells, config.random_seed);
        Ok(Self {
            walkable,
            assigned_exits,
            alarm: AlarmConfig::default(),
            alarm_raised_at: None,
            alerted_at: vec![None; agents.len()],
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

    /// Changes when occupants learn about the fire. Call before the run
    /// starts.
    #[must_use]
    pub fn with_alarm(mut self, alarm: AlarmConfig) -> Self {
        self.alarm = alarm;
        if alarm == AlarmConfig::Immediate {
            self.alarm_raised_at = Some(0.0);
            self.alerted_at.fill(Some(0.0));
        }
        self
    }

    /// When the alarm was raised, if it has been.
    #[must_use]
    pub const fn alarm_raised_at(&self) -> Option<f64> {
        self.alarm_raised_at
    }

    /// When each occupant heard the alarm, in agent order.
    #[must_use]
    pub fn alerted_at(&self) -> &[Option<f64>] {
        &self.alerted_at
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
        if self.alarm_raised_at.is_none() {
            self.notice(time_seconds, fire);
        }
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

    /// Moves people a firefighter is carrying with them, and lets go of those
    /// carried out.
    pub(crate) fn apply_carries(&mut self, carries: &[Carry]) {
        for carry in carries {
            let Some(agent) = self
                .agents
                .iter_mut()
                .find(|agent| agent.id == carry.occupant_id)
            else {
                continue;
            };
            match carry.state {
                CarryState::Holding {
                    position_m,
                    velocity_m_per_s,
                } => {
                    agent.velocity_m_per_s = velocity_m_per_s;
                    agent.position_m = position_m;
                    agent.movement = crate::Movement::Walking;
                    set_phase(agent, OccupantPhase::Carried);
                }
                CarryState::Delivered => {
                    agent.velocity_m_per_s = ZERO;
                    agent.movement = crate::Movement::Stopped;
                    set_phase(agent, OccupantPhase::Exited);
                }
            }
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

    /// Raises the alarm if someone is in a room whose fire is big enough to
    /// notice, and works out when everyone hears it.
    fn notice(&mut self, time_seconds: f64, fire: &[SurfaceFireNodeFrame]) {
        let AlarmConfig::Detected {
            notice_hrr_kw,
            same_room_seconds,
            seconds_per_metre,
        } = self.alarm
        else {
            return;
        };
        let mut room_hrr = HashMap::<&str, f64>::new();
        for node in fire {
            if node.state == CombustionState::Burning {
                *room_hrr.entry(node.room_id.as_str()).or_default() += node.heat_release_rate_kw;
            }
        }
        let noticeable = room_hrr
            .into_iter()
            .filter(|&(_, hrr)| hrr >= notice_hrr_kw)
            .map(|(room, _)| room)
            .collect::<HashSet<_>>();
        if noticeable.is_empty() {
            return;
        }
        let noticers = (0..self.agents.len())
            .filter(|&index| {
                self.agents[index].is_active()
                    && self
                        .rooms_of(index)
                        .iter()
                        .any(|room| noticeable.contains(room.as_str()))
            })
            .collect::<Vec<_>>();
        if noticers.is_empty() {
            return;
        }

        // Walking distance from whoever noticed, per floor.
        let distances = self
            .grids
            .iter()
            .zip(&self.walkable)
            .map(|(grid, walkable)| {
                let targets = noticers
                    .iter()
                    .filter(|&&index| self.agents[index].floor_id == grid.floor_id)
                    .filter_map(|&index| grid.world_to_cell(self.agents[index].position_m))
                    .map(|cell| (cell, 0))
                    .collect::<Vec<_>>();
                (!targets.is_empty()).then(|| {
                    ExitField::build(
                        grid,
                        walkable,
                        &targets,
                        &HazardMap::default(),
                        NavProfile::OCCUPANT,
                        &[],
                    )
                })
            })
            .collect::<Vec<_>>();
        for index in 0..self.agents.len() {
            let delay = if noticers.contains(&index) {
                0.0
            } else {
                same_room_seconds
                    + self.metres_from_noticers(index, &noticers, &distances) * seconds_per_metre
            };
            self.alerted_at[index] = Some(time_seconds + delay);
        }
        self.alarm_raised_at = Some(time_seconds);
    }

    /// Walking distance from the nearest person who noticed the fire, or the
    /// straight-line distance where no walk reaches.
    fn metres_from_noticers(
        &self,
        index: usize,
        noticers: &[usize],
        distances: &[Option<ExitField>],
    ) -> f64 {
        let agent = &self.agents[index];
        let walked = self
            .grids
            .iter()
            .position(|grid| grid.floor_id == agent.floor_id)
            .and_then(|grid| {
                let field = distances[grid].as_ref()?;
                let cell = self.grids[grid].world_to_cell(agent.position_m)?;
                let cost = field
                    .cost(cell)
                    .or_else(|| field.next_cell(cell).and_then(|next| field.cost(next)))?;
                Some(cost as f64 / NORMAL_STEP_COST as f64 * self.grids[grid].cell_size_m)
            });
        walked.unwrap_or_else(|| {
            noticers
                .iter()
                .map(|&noticer| distance_2d(agent.position_m, self.agents[noticer].position_m))
                .fold(f64::INFINITY, f64::min)
        })
    }

    /// The rooms of the cell occupant `index` stands on.
    fn rooms_of(&self, index: usize) -> Vec<String> {
        let agent = &self.agents[index];
        self.grids
            .iter()
            .find(|grid| grid.floor_id == agent.floor_id)
            .and_then(|grid| grid.cell(grid.world_to_cell(agent.position_m)?))
            .map(|cell| cell.room_ids.clone())
            .unwrap_or_default()
    }

    /// Per grid: a field to the nearest exit, and one to each exit someone
    /// still inside is heading for. Every body standing in the building adds
    /// crowding cost.
    fn exit_fields(&self, others: &[Agent]) -> Vec<Option<FloorFields>> {
        let crowding_cost = self.crowd.config().crowding_cost_per_body;
        let in_use = self
            .agents
            .iter()
            .zip(&self.assigned_exits)
            .filter(|(agent, _)| agent.is_active())
            .filter_map(|(_, exit)| *exit)
            .collect::<BTreeSet<_>>();
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
                let build = |targets: &[(GridCoordinate, u32)]| {
                    Arc::new(ExitField::build(
                        grid,
                        walkable,
                        targets,
                        &self.hazards,
                        NavProfile::OCCUPANT,
                        &crowding,
                    ))
                };
                let mut per_exit = HashMap::new();
                for &exit in &in_use {
                    let cells = targets
                        .iter()
                        .copied()
                        .filter(|&(_, label)| label == exit)
                        .collect::<Vec<_>>();
                    if !cells.is_empty() {
                        per_exit.insert(exit, build(&cells));
                    }
                }
                Some(FloorFields {
                    nearest: build(targets),
                    per_exit,
                })
            })
            .collect()
    }

    fn plan_agent(
        &mut self,
        index: usize,
        time_seconds: f64,
        delta_seconds: f64,
        fields: &[Option<FloorFields>],
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
        if matches!(
            phase,
            OccupantPhase::Exited
                | OccupantPhase::Incapacitated
                | OccupantPhase::Injured
                | OccupantPhase::Carried
        ) {
            return Intent::HOLD;
        }
        let Some(alerted_at) = self.alerted_at[index] else {
            return Intent::HOLD;
        };
        // Their pre-movement delay runs from when they hear the alarm.
        let start = alerted_at + delay;
        let movement_seconds = (time_seconds - self.time_seconds.max(start)).max(0.0);
        if movement_seconds == 0.0 || delta_seconds == 0.0 {
            if time_seconds >= alerted_at && time_seconds > 0.0 {
                set_phase(agent, OccupantPhase::Preparing);
            }
            return Intent::HOLD;
        }

        let grid_index = self
            .grids
            .iter()
            .position(|grid| grid.floor_id == agent.floor_id);
        let floor_fields = grid_index
            .and_then(|grid| fields.get(grid))
            .and_then(Option::as_ref);
        let (Some(grid_index), Some(floor_fields)) = (grid_index, floor_fields) else {
            set_phase(agent, OccupantPhase::Rerouting);
            return Intent::HOLD;
        };
        let grid = &self.grids[grid_index];
        let Some(cell) = grid.world_to_cell(agent.position_m) else {
            set_phase(agent, OccupantPhase::Rerouting);
            return Intent::HOLD;
        };

        // Their own exit, if fire hasn't cut it off; otherwise the nearest
        // one they can still reach, from now on.
        let assigned = self.assigned_exits[index];
        let own = assigned
            .and_then(|exit| floor_fields.per_exit.get(&exit))
            .filter(|field| reached_exit(field, cell).is_some());
        let (field, exit, changed) = if let Some(field) = own {
            (Arc::clone(field), assigned, false)
        } else {
            let field = &floor_fields.nearest;
            let Some(exit) = reached_exit(field, cell) else {
                set_phase(agent, OccupantPhase::Rerouting);
                return Intent::HOLD;
            };
            self.assigned_exits[index] = Some(exit);
            (Arc::clone(field), Some(exit), assigned.is_some())
        };

        let exit_id = exit.and_then(|exit| self.exit_ids.get(exit as usize).cloned());
        if let AgentRole::Occupant(state) = &mut agent.role {
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

/// One floor's exit fields for a frame.
struct FloorFields {
    /// To whichever exit is cheapest from each cell.
    nearest: Arc<ExitField>,
    /// To one exit each, for the exits someone inside is heading for.
    per_exit: HashMap<u32, Arc<ExitField>>,
}

/// The exit `field` leads to from `cell`. A body standing just off a
/// reachable cell center routes from the neighbor it steps onto.
fn reached_exit(field: &ExitField, cell: GridCoordinate) -> Option<u32> {
    field
        .exit(cell)
        .or_else(|| field.next_cell(cell).and_then(|next| field.exit(next)))
}

/// Gives each occupant a random exit, evenly among the exits on their floor
/// they can reach from where they start in the empty building. Repeatable
/// for a seed.
fn assign_exits(
    agents: &[Agent],
    grids: &[SimulationGrid],
    walkable: &[Vec<bool>],
    exit_cells: &HashMap<String, Vec<(GridCoordinate, u32)>>,
    seed: u64,
) -> Vec<Option<u32>> {
    // One field per exit, with no fire and nobody in the way.
    let fields = grids
        .iter()
        .zip(walkable)
        .map(|(grid, walkable)| {
            let mut exits = exit_cells
                .get(&grid.floor_id)
                .map(|cells| cells.iter().map(|&(_, exit)| exit).collect::<Vec<_>>())
                .unwrap_or_default();
            exits.sort_unstable();
            exits.dedup();
            exits
                .into_iter()
                .map(|exit| {
                    let cells = exit_cells[&grid.floor_id]
                        .iter()
                        .copied()
                        .filter(|&(_, label)| label == exit)
                        .collect::<Vec<_>>();
                    let field = ExitField::build(
                        grid,
                        walkable,
                        &cells,
                        &HazardMap::default(),
                        NavProfile::OCCUPANT,
                        &[],
                    );
                    (exit, field)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    agents
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            let grid = grids
                .iter()
                .position(|grid| grid.floor_id == agent.floor_id)?;
            let cell = grids[grid].world_to_cell(agent.position_m)?;
            let reachable = fields[grid]
                .iter()
                .filter(|(_, field)| reached_exit(field, cell).is_some())
                .map(|(exit, _)| *exit)
                .collect::<Vec<_>>();
            if reachable.is_empty() {
                return None;
            }
            let roll = unit(seed, u64::MAX, index as u64, EXIT_ROLL);
            let pick = ((roll * reachable.len() as f64) as usize).min(reachable.len() - 1);
            Some(reachable[pick])
        })
        .collect()
}

/// Keeps exit rolls apart from the crowd's other random draws.
const EXIT_ROLL: u64 = 0x0E_C17;

/// Whether a body takes up floor space others must route around.
fn blocks_the_way(agent: &Agent) -> bool {
    match &agent.role {
        AgentRole::Occupant(state) => {
            !matches!(state.phase, OccupantPhase::Exited | OccupantPhase::Carried)
        }
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

/// A firefighter carrying an occupant, after the crowd has moved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Carry {
    pub occupant_id: String,
    pub state: CarryState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CarryState {
    /// Being carried, moving with the firefighter.
    Holding {
        position_m: Vec3,
        velocity_m_per_s: Vec3,
    },
    /// Carried out of the building.
    Delivered,
}

const ZERO: Vec3 = Vec3 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

fn set_phase(agent: &mut Agent, phase: OccupantPhase) {
    if let AgentRole::Occupant(state) = &mut agent.role {
        state.phase = phase;
    }
}
