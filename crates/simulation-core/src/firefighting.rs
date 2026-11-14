//! Firefighter crews: they arrive at an entrance, walk the building room by
//! room along their route without knowing where the fire is, put out any
//! fire they see, and leave once every room they can reach has been entered.
//!
//! Knowledge rule: [`decide`] reads only a crew's [`CrewMemory`], which is
//! updated from what its leader can observe: the room it stands in, and heat
//! within a short radius. Route planning avoids only fires the crew has seen.
//! The true fire state is used for one thing, physics: a body can't step into
//! flames, so movement stops short of them.
//!
//! A crew moves as a unit. The leader decides where to walk; followers keep
//! a short distance behind it, and share its phase. The shared
//! [`Crowd`](crate::crowd) does the walking, so crews have to push through
//! evacuating occupants in doors and hallways. Until a crew knows where the
//! fire is it moves at scouting speed; with a charged hose it slows down.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use tflop_world_core::{OpeningTopologyError, Scene, Vec3};
use thiserror::Error;

use crate::{
    Agent, AgentFrame, AgentRole, CombustionState, CrewSpawn, DEFAULT_NOZZLE_COOLING_KW,
    DEFAULT_NOZZLE_HALF_ANGLE_DEGREES, DEFAULT_NOZZLE_REACH_M, FirefighterPhase, GridCoordinate,
    ScenarioValidationErrors, Simulation, SimulationConfig, SimulationError, SimulationGrid,
    Suppression, SurfaceFireNodeFrame,
    crowd::{Crowd, CrowdConfig, CrowdGroup, Goal, Intent, PassageMap, SINGLE_FILE_MAX_WIDTH_M},
    exterior_exits,
    nav::{
        Hazard, HazardMap, NavProfile, distance_2d, distance_field, exit_targets, find_path,
        room_floor_index,
    },
    scenario::spawn_crews,
};

/// How crews move, sense, and spray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FirefightingConfig {
    /// Walking speed while the crew doesn't yet know of any fire. Crews
    /// arrive with urgency; once they know where the fire is they advance at
    /// their own (hose-laden) walking speed.
    pub scouting_speed_m_per_s: f64,
    /// Burning nodes this close to the leader are noticed even through a wall
    /// or doorway.
    pub heat_sense_radius_m: f64,
    pub nozzle_reach_m: f64,
    pub nozzle_cooling_kw: f64,
    /// The stream wets burning cells within this angle of where it's aimed.
    pub nozzle_half_angle_degrees: f64,
    /// Crews close to within this fraction of nozzle reach before spraying.
    pub approach_fraction: f64,
    /// Followers keep between these distances from the leader.
    pub follow_min_m: f64,
    pub follow_max_m: f64,
}

impl Default for FirefightingConfig {
    fn default() -> Self {
        Self {
            scouting_speed_m_per_s: 1.5,
            heat_sense_radius_m: 2.0,
            nozzle_reach_m: DEFAULT_NOZZLE_REACH_M,
            nozzle_cooling_kw: DEFAULT_NOZZLE_COOLING_KW,
            nozzle_half_angle_degrees: DEFAULT_NOZZLE_HALF_ANGLE_DEGREES,
            approach_fraction: 0.8,
            follow_min_m: 0.7,
            follow_max_m: 1.5,
        }
    }
}

/// Everything a crew has learned. Nothing here comes from fire it hasn't
/// observed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CrewMemory {
    /// Burning nodes the crew has seen and not yet seen go out, as last seen.
    pub known_fires: BTreeMap<String, SurfaceFireNodeFrame>,
    /// When the leader first stood in each room.
    pub visited: BTreeMap<String, f64>,
    /// Rooms the crew found no route to.
    pub unreachable: BTreeSet<String>,
}

/// One crew's state, for tests and run metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct CrewReport {
    pub crew_id: String,
    pub phase: FirefighterPhase,
    pub memory: CrewMemory,
}

#[derive(Debug)]
pub struct Firefighting {
    config: FirefightingConfig,
    grids: Vec<SimulationGrid>,
    room_floors: HashMap<String, String>,
    rooms: BTreeMap<String, RoomInfo>,
    exits: HashMap<String, Vec<GridCoordinate>>,
    passages: Vec<PassageMap>,
    agents: Vec<Agent>,
    crews: Vec<Crew>,
    /// What this frame's plan wants, one per agent.
    intents: Vec<Intent>,
    /// The fire as it stood when this frame was planned: bodies can't step
    /// into it.
    true_hazards: HazardMap,
    /// Moves crews when firefighting runs on its own, outside a
    /// [`Rehearsal`](crate::Rehearsal).
    crowd: Crowd,
    time_seconds: f64,
}

impl Firefighting {
    /// Spawns `crews` just inside their entries, staging until they arrive.
    pub fn new(
        scene: &Scene,
        crews: &[CrewSpawn],
        simulation_config: SimulationConfig,
        config: FirefightingConfig,
    ) -> Result<Self, FirefightingError> {
        validate_config(config)?;
        let simulation = Simulation::new(scene.clone(), simulation_config)?;
        let grids = simulation.grids().to_vec();
        let profile = NavProfile::FIREFIGHTER;
        let (agents, exits) = if crews.is_empty() {
            (Vec::new(), HashMap::new())
        } else {
            let entries = exterior_exits(scene)?;
            let agents = spawn_crews(crews, scene, &grids, &entries, &[])?;
            (agents, exit_targets(scene, &grids, profile.body_radius_m)?)
        };

        let mut crew_states = Vec::with_capacity(crews.len());
        for spawn in crews {
            let members = agents
                .iter()
                .enumerate()
                .filter(|(_, agent)| {
                    matches!(&agent.role, AgentRole::Firefighter(state) if state.crew_id == spawn.id)
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let leader = &agents[members[0]];
            let grid = grid_for(&grids, &leader.floor_id);
            let entry_cell = grid
                .world_to_cell(leader.position_m)
                .expect("crews spawn on a grid cell");
            crew_states.push(Crew {
                id: spawn.id.clone(),
                members,
                floor_id: leader.floor_id.clone(),
                entry_cell,
                arrival_time_seconds: spawn.arrival_time_seconds,
                walking_speed_m_per_s: spawn.walking_speed_m_per_s,
                plan: CrewPlan {
                    route: spawn.route.clone(),
                    memory: CrewMemory::default(),
                    room_target: None,
                    spray_target: None,
                },
                phase: FirefighterPhase::Staging,
            });
        }

        // Cells any crew can walk to from its entry, in a building with no
        // fire. Rooms are planned over these only.
        let mut reachable = HashMap::<String, HashSet<GridCoordinate>>::new();
        for crew in &crew_states {
            let grid = grid_for(&grids, &crew.floor_id);
            reachable.entry(crew.floor_id.clone()).or_default().extend(
                distance_field(grid, crew.entry_cell, &HazardMap::default(), profile).into_keys(),
            );
        }

        Ok(Self {
            config,
            rooms: index_rooms(scene, &grids, profile, &reachable),
            room_floors: room_floor_index(scene),
            passages: grids
                .iter()
                .map(|grid| PassageMap::build(grid, SINGLE_FILE_MAX_WIDTH_M))
                .collect(),
            grids,
            exits,
            intents: vec![Intent::HOLD; agents.len()],
            agents,
            crews: crew_states,
            true_hazards: HazardMap::default(),
            crowd: Crowd::new(CrowdConfig::default(), simulation_config.random_seed),
            time_seconds: 0.0,
        })
    }

    /// Acts over `[current time, time_seconds]` given the fire as it stood at
    /// the start of the interval, and returns the water to apply over it.
    /// Crews move on their own, with no occupants in the way.
    pub fn advance_to(
        &mut self,
        time_seconds: f64,
        fire: &[SurfaceFireNodeFrame],
    ) -> Result<Vec<Suppression>, FirefightingError> {
        let (suppressions, delta_seconds) = self.plan(time_seconds, fire)?;
        if delta_seconds > 0.0 {
            let mut crowd =
                std::mem::replace(&mut self.crowd, Crowd::new(CrowdConfig::default(), 0));
            crowd.step(&mut [self.crowd_group()], delta_seconds);
            self.crowd = crowd;
            self.finish(time_seconds);
        }
        Ok(suppressions)
    }

    /// Decides what every crew does over `[current time, time_seconds]`
    /// given the fire as it stood at the start of the interval. Returns the
    /// water to apply over it and the interval's length.
    pub(crate) fn plan(
        &mut self,
        time_seconds: f64,
        fire: &[SurfaceFireNodeFrame],
    ) -> Result<(Vec<Suppression>, f64), FirefightingError> {
        if !time_seconds.is_finite() || time_seconds < self.time_seconds {
            return Err(FirefightingError::InvalidTime { time_seconds });
        }
        let delta_seconds = time_seconds - self.time_seconds;
        if delta_seconds == 0.0 {
            return Ok((Vec::new(), 0.0));
        }
        let profile = NavProfile::FIREFIGHTER;
        let true_hazards =
            HazardMap::from_fire(&self.grids, &self.room_floors, fire, profile.body_radius_m);
        self.intents.fill(Intent::HOLD);

        let mut suppressions = Vec::new();
        for crew_index in 0..self.crews.len() {
            if let Some(suppression) = self.advance_crew(crew_index, fire, &true_hazards) {
                suppressions.push(suppression);
            }
        }
        self.true_hazards = true_hazards;
        Ok((suppressions, delta_seconds))
    }

    /// This frame's agents and intents, for the crowd to move.
    pub(crate) fn crowd_group(&mut self) -> CrowdGroup<'_> {
        CrowdGroup {
            agents: &mut self.agents,
            intents: &self.intents,
            grids: &self.grids,
            passages: &self.passages,
            hazards: &self.true_hazards,
            walk_radius_m: NavProfile::FIREFIGHTER.body_radius_m,
        }
    }

    /// Ends the interval once the crowd has moved.
    pub(crate) fn finish(&mut self, time_seconds: f64) {
        self.time_seconds = time_seconds;
    }

    pub(crate) fn set_crowd_config(&mut self, config: CrowdConfig) {
        self.crowd = Crowd::new(config, self.crowd.seed());
    }

    /// Current renderer frames for every firefighter.
    #[must_use]
    pub fn frames(&self) -> Vec<AgentFrame> {
        self.agents.iter().map(Agent::frame).collect()
    }

    #[must_use]
    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    /// Whether any crew is still staging or inside.
    #[must_use]
    pub fn has_active_agents(&self) -> bool {
        self.crews
            .iter()
            .any(|crew| crew.phase != FirefighterPhase::Exited)
    }

    #[must_use]
    pub fn crew_reports(&self) -> Vec<CrewReport> {
        self.crews
            .iter()
            .map(|crew| CrewReport {
                crew_id: crew.id.clone(),
                phase: crew.phase,
                memory: crew.plan.memory.clone(),
            })
            .collect()
    }

    fn advance_crew(
        &mut self,
        crew_index: usize,
        fire: &[SurfaceFireNodeFrame],
        true_hazards: &HazardMap,
    ) -> Option<Suppression> {
        let crew = &mut self.crews[crew_index];
        if crew.phase == FirefighterPhase::Exited {
            return None;
        }
        let grid = grid_for(&self.grids, &crew.floor_id);
        let leader_index = crew.members[0];
        let start_time = self.time_seconds;
        let leader_position = self.agents[leader_index].position_m;
        let leader_cell = grid
            .world_to_cell(leader_position)
            .unwrap_or(crew.entry_cell);

        if start_time >= crew.arrival_time_seconds {
            let observation = observe(
                leader_position,
                grid.cell(leader_cell)
                    .map_or(&[][..], |cell| &cell.room_ids),
                fire,
                self.config.heat_sense_radius_m,
            );
            remember(&mut crew.plan.memory, &observation, start_time);
        }
        let known_fires = crew
            .plan
            .memory
            .known_fires
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let known_hazards = HazardMap::from_fire(
            &self.grids,
            &self.room_floors,
            &known_fires,
            NavProfile::FIREFIGHTER.body_radius_m,
        );
        let exits = self
            .exits
            .get(&crew.floor_id)
            .map_or(&[][..], Vec::as_slice);
        let context = Context {
            time_seconds: start_time,
            arrival_time_seconds: crew.arrival_time_seconds,
            leader_cell,
            entry_cell: crew.entry_cell,
            grid,
            rooms: &self.rooms,
            hazards: &known_hazards,
            exits,
            config: &self.config,
        };
        let action = decide(&mut crew.plan, &context);

        let mut suppression = None;
        let mut target_fire = None;
        let mut nozzle_target = None;
        let speed = if crew.plan.memory.known_fires.is_empty() {
            self.config.scouting_speed_m_per_s
        } else {
            crew.walking_speed_m_per_s
        };
        crew.phase = match action {
            Action::Stage => FirefighterPhase::Staging,
            Action::Spray { node_id, room_id } => {
                // Aim at the fire as last seen.
                let aim = crew.plan.memory.known_fires[&node_id].position_m;
                let offset = (aim.x - leader_position.x, aim.y - leader_position.y);
                let length = offset.0.hypot(offset.1);
                suppression = Some(Suppression {
                    room_id,
                    position_m: leader_position,
                    reach_m: self.config.nozzle_reach_m,
                    cooling_kw: self.config.nozzle_cooling_kw,
                    direction: (length > 1.0e-6).then(|| (offset.0 / length, offset.1 / length)),
                    half_angle_degrees: self.config.nozzle_half_angle_degrees,
                });
                target_fire = Some(node_id);
                nozzle_target = Some(aim);
                FirefighterPhase::Suppressing
            }
            Action::Move { path, purpose } => {
                self.intents[leader_index] = Intent {
                    goal: Goal::Path(stop_short_of_flames(&path, &crew.floor_id, true_hazards)),
                    speed_m_per_s: speed,
                };
                match purpose {
                    Purpose::Fire(node_id) => {
                        target_fire = Some(node_id);
                        FirefighterPhase::Advancing
                    }
                    Purpose::Room => FirefighterPhase::Advancing,
                    Purpose::Exit => FirefighterPhase::Withdrawing,
                }
            }
            Action::Exit => FirefighterPhase::Exited,
            Action::Stuck => FirefighterPhase::Withdrawing,
        };

        let phase = crew.phase;
        let members = crew.members.clone();
        let floor_id = crew.floor_id.clone();
        if !matches!(phase, FirefighterPhase::Staging | FirefighterPhase::Exited) {
            self.plan_followers(&members, &floor_id, speed, &known_hazards, true_hazards);
        }
        for &member in &members {
            if let AgentRole::Firefighter(state) = &mut self.agents[member].role {
                state.phase = phase;
                state.target_fire_node_id.clone_from(&target_fire);
                state.nozzle_target_m = if member == leader_index {
                    nozzle_target
                } else {
                    None
                };
            }
        }
        suppression
    }

    /// Sends each follower toward a cell between `follow_min_m` and
    /// `follow_max_m` from the leader and from every member before it, so
    /// the crew stays together without overlapping.
    fn plan_followers(
        &mut self,
        members: &[usize],
        floor_id: &str,
        speed_m_per_s: f64,
        known_hazards: &HazardMap,
        true_hazards: &HazardMap,
    ) {
        let grid = grid_for(&self.grids, floor_id);
        let leader_position = self.agents[members[0]].position_m;
        let mut placed = vec![leader_position];
        for &member in &members[1..] {
            let old_position = self.agents[member].position_m;
            let in_place = distance_2d(old_position, leader_position) <= self.config.follow_max_m
                && placed
                    .iter()
                    .all(|other| distance_2d(old_position, *other) >= self.config.follow_min_m);
            if !in_place {
                let targets =
                    follow_targets(grid, leader_position, &placed, &self.config, true_hazards);
                if let Some(start) = grid.world_to_cell(old_position)
                    && let Some(path) = find_path(
                        grid,
                        start,
                        &targets,
                        known_hazards,
                        NavProfile::FIREFIGHTER,
                    )
                {
                    self.intents[member] = Intent {
                        goal: Goal::Path(stop_short_of_flames(&path, floor_id, true_hazards)),
                        speed_m_per_s,
                    };
                }
            }
            placed.push(old_position);
        }
    }
}

#[derive(Debug, Error)]
pub enum FirefightingError {
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error(transparent)]
    InvalidCrews(#[from] ScenarioValidationErrors),
    #[error(transparent)]
    OpeningTopology(#[from] OpeningTopologyError),
    #[error("invalid firefighting config field {field}")]
    InvalidConfig { field: &'static str },
    #[error("firefighting time must be finite and nondecreasing, got {time_seconds}")]
    InvalidTime { time_seconds: f64 },
}

#[derive(Debug)]
struct Crew {
    id: String,
    /// Agent indices; the first is the leader.
    members: Vec<usize>,
    floor_id: String,
    entry_cell: GridCoordinate,
    arrival_time_seconds: f64,
    walking_speed_m_per_s: f64,
    plan: CrewPlan,
    phase: FirefighterPhase,
}

/// The part of a crew [`decide`] may read and update.
#[derive(Debug, Clone, PartialEq)]
struct CrewPlan {
    /// Rooms to enter first, in order.
    route: Vec<String>,
    memory: CrewMemory,
    /// Room currently being walked to.
    room_target: Option<String>,
    /// Fire node the nozzle is on.
    spray_target: Option<String>,
}

/// What the leader can perceive at one instant.
#[derive(Debug, Clone, Default, PartialEq)]
struct Observation {
    /// Rooms of the leader's cell; everything in them is visible.
    rooms: Vec<String>,
    /// Nodes visible in those rooms or felt within the heat-sense radius.
    nodes: Vec<SurfaceFireNodeFrame>,
}

fn observe(
    leader_position: Vec3,
    leader_rooms: &[String],
    fire: &[SurfaceFireNodeFrame],
    heat_sense_radius_m: f64,
) -> Observation {
    Observation {
        rooms: leader_rooms.to_vec(),
        nodes: fire
            .iter()
            .filter(|node| {
                leader_rooms.contains(&node.room_id)
                    || distance_2d(node.position_m, leader_position) <= heat_sense_radius_m
            })
            .cloned()
            .collect(),
    }
}

/// Folds an observation into memory. A visible room is seen whole, so fires
/// remembered there that are no longer visible have gone out. A doorway
/// shows both rooms, but only standing inside one counts as visiting it.
fn remember(memory: &mut CrewMemory, observation: &Observation, time_seconds: f64) {
    if let [room_id] = observation.rooms.as_slice() {
        memory
            .visited
            .entry(room_id.clone())
            .or_insert(time_seconds);
    }
    for room_id in &observation.rooms {
        memory
            .known_fires
            .retain(|_, node| node.room_id != *room_id);
    }
    for node in &observation.nodes {
        if node.state == CombustionState::Burning {
            memory.known_fires.insert(node.id.clone(), node.clone());
        } else {
            memory.known_fires.remove(&node.id);
        }
    }
}

/// Static facts and current position a decision may use.
struct Context<'a> {
    time_seconds: f64,
    arrival_time_seconds: f64,
    leader_cell: GridCoordinate,
    entry_cell: GridCoordinate,
    grid: &'a SimulationGrid,
    rooms: &'a BTreeMap<String, RoomInfo>,
    /// Built from the crew's known fires only.
    hazards: &'a HazardMap,
    exits: &'a [GridCoordinate],
    config: &'a FirefightingConfig,
}

#[derive(Debug, Clone, PartialEq)]
enum Action {
    Stage,
    Spray {
        node_id: String,
        room_id: String,
    },
    Move {
        path: Vec<GridCoordinate>,
        purpose: Purpose,
    },
    Exit,
    /// Nothing left to do and no route out.
    Stuck,
}

#[derive(Debug, Clone, PartialEq)]
enum Purpose {
    Fire(String),
    Room,
    Exit,
}

/// Chooses the leader's next action from the crew's memory: stage until
/// arrival, then fight known fire, then enter the next room, then leave.
fn decide(plan: &mut CrewPlan, context: &Context<'_>) -> Action {
    if context.time_seconds < context.arrival_time_seconds {
        return Action::Stage;
    }
    if let Some(action) = fight_known_fire(plan, context) {
        return action;
    }
    if let Some(action) = next_room(plan, context) {
        return action;
    }
    if context.exits.contains(&context.leader_cell) {
        return Action::Exit;
    }
    let profile = NavProfile::FIREFIGHTER;
    find_path(
        context.grid,
        context.leader_cell,
        &[context.entry_cell],
        context.hazards,
        profile,
    )
    .or_else(|| {
        find_path(
            context.grid,
            context.leader_cell,
            context.exits,
            context.hazards,
            profile,
        )
    })
    .map_or(Action::Stuck, |path| Action::Move {
        path,
        purpose: Purpose::Exit,
    })
}

/// Sprays when standing within reach of a known fire, otherwise walks to the
/// nearest cell that is. The nozzle stays on one fire until it is out, then
/// turns to the nearest other one in reach.
fn fight_known_fire(plan: &mut CrewPlan, context: &Context<'_>) -> Option<Action> {
    let reach = context.config.nozzle_reach_m * context.config.approach_fraction;
    let floor_id = context.grid.floor_id.as_str();
    // Approach cell -> the fire it serves (lowest node ID wins).
    let mut approach = HashMap::<GridCoordinate, (&str, &str)>::new();
    for (node_id, node) in &plan.memory.known_fires {
        let Some(room) = context.rooms.get(&node.room_id) else {
            continue;
        };
        if room.floor_id != floor_id {
            continue;
        }
        for &cell in &room.cells {
            let Some(center) = context.grid.cell_center(cell) else {
                continue;
            };
            if distance_2d(center, node.position_m) <= reach
                && context.hazards.get(floor_id, cell) != Some(Hazard::Blocked)
            {
                approach.entry(cell).or_insert((node_id, &node.room_id));
            }
        }
    }
    if approach.contains_key(&context.leader_cell) {
        let leader = context.grid.cell_center(context.leader_cell)?;
        let leader_rooms = context
            .grid
            .cell(context.leader_cell)
            .map_or(&[][..], |cell| cell.room_ids.as_slice());
        let in_reach = plan
            .memory
            .known_fires
            .iter()
            .filter(|(_, node)| {
                leader_rooms.contains(&node.room_id)
                    && distance_2d(leader, node.position_m) <= reach
            })
            .collect::<Vec<_>>();
        let target = in_reach
            .iter()
            .find(|(node_id, _)| plan.spray_target.as_deref() == Some(node_id.as_str()))
            .or_else(|| {
                in_reach.iter().min_by(|left, right| {
                    distance_2d(leader, left.1.position_m)
                        .total_cmp(&distance_2d(leader, right.1.position_m))
                })
            })
            .map(|(node_id, node)| ((*node_id).clone(), node.room_id.clone()));
        if let Some((node_id, room_id)) = target {
            plan.spray_target = Some(node_id.clone());
            return Some(Action::Spray { node_id, room_id });
        }
    }
    if approach.is_empty() {
        return None;
    }
    let targets = approach.keys().copied().collect::<Vec<_>>();
    let path = find_path(
        context.grid,
        context.leader_cell,
        &targets,
        context.hazards,
        NavProfile::FIREFIGHTER,
    )?;
    let (node_id, _) = approach[path.last()?];
    Some(Action::Move {
        path,
        purpose: Purpose::Fire(node_id.to_owned()),
    })
}

/// Walks into the next room: the first unvisited one on the route, else the
/// nearest unvisited one. Entering it is enough; whatever is in a room is
/// seen from inside it. Rooms with no route are marked unreachable and
/// skipped.
fn next_room(plan: &mut CrewPlan, context: &Context<'_>) -> Option<Action> {
    loop {
        let target = match plan.room_target.take() {
            Some(room_id) if is_open(plan, &room_id) => room_id,
            _ => choose_room(plan, context)?,
        };
        let cells = context
            .rooms
            .get(&target)
            .map_or(&[][..], |room| room.cells.as_slice());
        if let Some(path) = (!cells.is_empty())
            .then(|| {
                find_path(
                    context.grid,
                    context.leader_cell,
                    cells,
                    context.hazards,
                    NavProfile::FIREFIGHTER,
                )
            })
            .flatten()
        {
            plan.room_target = Some(target);
            return Some(Action::Move {
                path,
                purpose: Purpose::Room,
            });
        }
        plan.memory.unreachable.insert(target);
    }
}

fn is_open(plan: &CrewPlan, room_id: &str) -> bool {
    !plan.memory.visited.contains_key(room_id) && !plan.memory.unreachable.contains(room_id)
}

/// The first open room on the route, else the nearest open room on this
/// floor by route cost. Rooms with no route are marked unreachable.
fn choose_room(plan: &mut CrewPlan, context: &Context<'_>) -> Option<String> {
    if let Some(room_id) = plan.route.iter().find(|room_id| is_open(plan, room_id)) {
        return Some(room_id.clone());
    }
    let floor_id = context.grid.floor_id.as_str();
    let open_rooms = context
        .rooms
        .iter()
        .filter(|(room_id, room)| room.floor_id == floor_id && is_open(plan, room_id))
        .collect::<Vec<_>>();
    if open_rooms.is_empty() {
        return None;
    }
    let costs = distance_field(
        context.grid,
        context.leader_cell,
        context.hazards,
        NavProfile::FIREFIGHTER,
    );
    let mut nearest: Option<(u64, &String)> = None;
    for (room_id, room) in open_rooms {
        match room.cells.iter().filter_map(|cell| costs.get(cell)).min() {
            Some(&cost) => {
                if nearest.is_none_or(|(best, _)| cost < best) {
                    nearest = Some((cost, room_id));
                }
            }
            None => {
                plan.memory.unreachable.insert(room_id.clone());
            }
        }
    }
    nearest.map(|(_, room_id)| room_id.clone())
}

/// Cuts a route before the first cell that is in flames right now. Planning
/// doesn't know about unseen fire; bodies still can't walk through it.
fn stop_short_of_flames(
    path: &[GridCoordinate],
    floor_id: &str,
    true_hazards: &HazardMap,
) -> Vec<GridCoordinate> {
    let end = path
        .iter()
        .skip(1)
        .position(|&cell| true_hazards.get(floor_id, cell) == Some(Hazard::Blocked))
        .map_or(path.len(), |blocked| blocked + 1);
    path[..end].to_vec()
}

/// Cells a follower may stand on: walkable, not in flames, and between the
/// follow distances from the leader and every member already placed.
fn follow_targets(
    grid: &SimulationGrid,
    leader_position: Vec3,
    placed: &[Vec3],
    config: &FirefightingConfig,
    true_hazards: &HazardMap,
) -> Vec<GridCoordinate> {
    let radius = config.follow_max_m;
    let span = (radius / grid.cell_size_m).ceil() as i64;
    let Some(center) = grid.world_to_cell(leader_position) else {
        return Vec::new();
    };
    let mut targets = Vec::new();
    for row_offset in -span..=span {
        for column_offset in -span..=span {
            let column = i64::from(center.column) + column_offset;
            let row = i64::from(center.row) + row_offset;
            let (Ok(column), Ok(row)) = (u32::try_from(column), u32::try_from(row)) else {
                continue;
            };
            let coordinate = GridCoordinate { column, row };
            let Some(cell) = grid.cell(coordinate) else {
                continue;
            };
            let distance = distance_2d(cell.center_m, leader_position);
            if distance > config.follow_max_m
                || placed
                    .iter()
                    .any(|other| distance_2d(cell.center_m, *other) < config.follow_min_m)
                || !grid.circle_is_walkable(cell.center_m, NavProfile::FIREFIGHTER.body_radius_m)
                || true_hazards.get(&grid.floor_id, coordinate) == Some(Hazard::Blocked)
            {
                continue;
            }
            targets.push(coordinate);
        }
    }
    targets
}

/// One room as the crew plans over it.
#[derive(Debug, Clone, PartialEq)]
struct RoomInfo {
    floor_id: String,
    /// Cells inside the room (not its doorways) that a firefighter's body
    /// fits on and can reach from an entry, sorted. Standing on any of them
    /// is being in the room.
    cells: Vec<GridCoordinate>,
}

/// Indexes every room in the scene. A room's planning cells are the body
/// cells reachable from a crew entry, so furniture that walls off part of a
/// room doesn't make it unreachable; a room with none will be reported
/// unreachable.
fn index_rooms(
    scene: &Scene,
    grids: &[SimulationGrid],
    profile: NavProfile,
    reachable: &HashMap<String, HashSet<GridCoordinate>>,
) -> BTreeMap<String, RoomInfo> {
    let mut rooms = scene
        .floors
        .iter()
        .flat_map(|floor| {
            floor.rooms.iter().map(|room| {
                (
                    room.id.clone(),
                    RoomInfo {
                        floor_id: floor.id.clone(),
                        cells: Vec::new(),
                    },
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    let no_cells = HashSet::new();
    for grid in grids {
        let reachable = reachable.get(&grid.floor_id).unwrap_or(&no_cells);
        for cell in grid.cells() {
            let usable = cell.is_walkable()
                && reachable.contains(&cell.coordinate)
                && grid.circle_is_walkable(cell.center_m, profile.body_radius_m);
            if !usable {
                continue;
            }
            // Doorway cells belong to two rooms; walking in means standing
            // on one that belongs to this room alone.
            if let [room_id] = cell.room_ids.as_slice()
                && let Some(room) = rooms.get_mut(room_id)
            {
                room.cells.push(cell.coordinate);
            }
        }
    }
    for room in rooms.values_mut() {
        room.cells.sort_by_key(|cell| (cell.row, cell.column));
    }
    rooms
}

fn validate_config(config: FirefightingConfig) -> Result<(), FirefightingError> {
    let positive = [
        ("scouting_speed_m_per_s", config.scouting_speed_m_per_s),
        ("heat_sense_radius_m", config.heat_sense_radius_m),
        ("nozzle_reach_m", config.nozzle_reach_m),
        ("nozzle_cooling_kw", config.nozzle_cooling_kw),
        (
            "nozzle_half_angle_degrees",
            config.nozzle_half_angle_degrees,
        ),
        ("approach_fraction", config.approach_fraction),
        ("follow_min_m", config.follow_min_m),
        ("follow_max_m", config.follow_max_m),
    ];
    for (field, value) in positive {
        if !value.is_finite() || value <= 0.0 {
            return Err(FirefightingError::InvalidConfig { field });
        }
    }
    if config.approach_fraction > 1.0 {
        return Err(FirefightingError::InvalidConfig {
            field: "approach_fraction",
        });
    }
    if config.follow_min_m >= config.follow_max_m {
        return Err(FirefightingError::InvalidConfig {
            field: "follow_max_m",
        });
    }
    Ok(())
}

fn grid_for<'a>(grids: &'a [SimulationGrid], floor_id: &str) -> &'a SimulationGrid {
    grids
        .iter()
        .find(|grid| grid.floor_id == floor_id)
        .expect("every floor has a grid")
}

#[cfg(test)]
mod tests {
    use tflop_world_core::{
        BoundaryEdge, BoundaryKind, Dimensions3, Floor, Opening, OpeningKind, Room,
        SCENE_SCHEMA_VERSION, SceneSource,
    };

    use super::*;
    use crate::{SurfaceFireNodeSource, nav::Hazard};

    pub(super) fn firefighting_for_index() -> Firefighting {
        firefighting()
    }

    fn firefighting() -> Firefighting {
        let crew = CrewSpawn {
            id: "crew".to_owned(),
            entry_opening_id: None,
            route: Vec::new(),
            arrival_time_seconds: 5.0,
            member_count: 2,
            walking_speed_m_per_s: 1.0,
        };
        Firefighting::new(
            &scene(),
            &[crew],
            SimulationConfig::default(),
            FirefightingConfig::default(),
        )
        .expect("unit fixture is valid")
    }

    /// One 6 x 4 m room with a front door centered on its south wall, and a
    /// sliver of a room too small to hold a single grid cell.
    pub(super) fn scene() -> Scene {
        let edge = |id: &str, start: Vec3, end: Vec3, openings: Vec<Opening>| BoundaryEdge {
            id: id.to_owned(),
            kind: BoundaryKind::Wall,
            start,
            end,
            thickness_m: Some(0.1),
            center_line_bias: Some(0.5),
            openings,
        };
        Scene {
            schema_version: SCENE_SCHEMA_VERSION,
            id: "unit".to_owned(),
            name: None,
            source: SceneSource {
                provider: "fixture".to_owned(),
                external_id: "unit".to_owned(),
            },
            assets: Vec::new(),
            floors: vec![Floor {
                id: "floor-1".to_owned(),
                label: None,
                elevation_m: Some(0.0),
                rooms: vec![
                    Room {
                        id: "sliver".to_owned(),
                        label: None,
                        classifications: Vec::new(),
                        dimensions: None,
                        boundary: vec![
                            edge("s-south", point(6.5, 0.0), point(6.55, 0.0), Vec::new()),
                            edge("s-east", point(6.55, 0.0), point(6.55, 0.05), Vec::new()),
                            edge("s-north", point(6.55, 0.05), point(6.5, 0.05), Vec::new()),
                            edge("s-west", point(6.5, 0.05), point(6.5, 0.0), Vec::new()),
                        ],
                    },
                    Room {
                        id: "room".to_owned(),
                        label: None,
                        classifications: Vec::new(),
                        dimensions: None,
                        boundary: vec![
                            edge(
                                "south",
                                point(0.0, 0.0),
                                point(6.0, 0.0),
                                vec![Opening {
                                    id: "front".to_owned(),
                                    label: None,
                                    kind: OpeningKind::Door,
                                    width_m: 1.0,
                                    height_m: Some(2.1),
                                    lower_elevation_m: Some(0.0),
                                    relative_center: 0.5,
                                }],
                            ),
                            edge("east", point(6.0, 0.0), point(6.0, 4.0), Vec::new()),
                            edge("north", point(6.0, 4.0), point(0.0, 4.0), Vec::new()),
                            edge("west", point(0.0, 4.0), point(0.0, 0.0), Vec::new()),
                        ],
                    },
                ],
                objects: Vec::new(),
            }],
        }
    }

    fn decide_at(
        firefighting: &Firefighting,
        plan: &mut CrewPlan,
        time_seconds: f64,
        leader: Vec3,
    ) -> Action {
        let grid = &firefighting.grids[0];
        let known = plan
            .memory
            .known_fires
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let hazards = HazardMap::from_fire(
            &firefighting.grids,
            &firefighting.room_floors,
            &known,
            NavProfile::FIREFIGHTER.body_radius_m,
        );
        let context = Context {
            time_seconds,
            arrival_time_seconds: 5.0,
            leader_cell: grid.world_to_cell(leader).expect("on grid"),
            entry_cell: firefighting.crews[0].entry_cell,
            grid,
            rooms: &firefighting.rooms,
            hazards: &hazards,
            exits: &firefighting.exits["floor-1"],
            config: &firefighting.config,
        };
        decide(plan, &context)
    }

    fn plan() -> CrewPlan {
        CrewPlan {
            route: Vec::new(),
            memory: CrewMemory::default(),
            room_target: None,
            spray_target: None,
        }
    }

    fn burning_node(id: &str, x: f64, y: f64) -> SurfaceFireNodeFrame {
        SurfaceFireNodeFrame {
            id: id.to_owned(),
            source: SurfaceFireNodeSource::FloorCell {
                floor_id: "floor-1".to_owned(),
                coordinate: GridCoordinate { column: 0, row: 0 },
            },
            room_id: "room".to_owned(),
            position_m: point(x, y),
            dimensions_m: Dimensions3 {
                width_m: 0.5,
                depth_m: 0.5,
                height_m: 0.02,
            },
            furniture_cell: None,
            material_id: "carpet".to_owned(),
            state: CombustionState::Burning,
            ignition_time_seconds: Some(0.0),
            surface_temperature_c: 500.0,
            heat_release_rate_kw: 40.0,
            fuel_remaining_fraction: 1.0,
        }
    }

    fn point(x: f64, y: f64) -> Vec3 {
        Vec3 { x, y, z: 0.0 }
    }

    #[test]
    fn stages_until_the_arrival_time() {
        let firefighting = firefighting();
        let mut plan = plan();
        plan.memory
            .known_fires
            .insert("fire".to_owned(), burning_node("fire", 3.0, 3.0));

        assert_eq!(
            decide_at(&firefighting, &mut plan, 4.9, point(3.0, 0.4)),
            Action::Stage
        );
    }

    #[test]
    fn with_nothing_known_it_walks_into_the_next_room() {
        let firefighting = firefighting();
        let mut plan = plan();

        let action = decide_at(&firefighting, &mut plan, 5.0, point(3.0, 0.4));

        assert!(
            matches!(
                action,
                Action::Move {
                    purpose: Purpose::Room,
                    ..
                }
            ),
            "{action:?}"
        );
        assert_eq!(plan.room_target.as_deref(), Some("room"));
    }

    #[test]
    fn a_known_fire_beats_scouting_and_is_sprayed_once_in_reach() {
        let firefighting = firefighting();
        let mut plan = plan();
        plan.memory
            .known_fires
            .insert("fire".to_owned(), burning_node("fire", 5.5, 3.5));

        // 5.4 m away: out of the 4 m approach distance, so walk closer.
        let action = decide_at(&firefighting, &mut plan, 5.0, point(0.4, 1.0));
        assert!(
            matches!(&action, Action::Move { purpose: Purpose::Fire(id), .. } if id == "fire"),
            "{action:?}"
        );

        // 2.8 m away: spray.
        let action = decide_at(&firefighting, &mut plan, 5.0, point(3.0, 2.25));
        assert_eq!(
            action,
            Action::Spray {
                node_id: "fire".to_owned(),
                room_id: "room".to_owned()
            }
        );
    }

    #[test]
    fn leaves_once_every_room_is_visited() {
        let firefighting = firefighting();
        let mut plan = plan();
        plan.memory.visited.insert("room".to_owned(), 10.0);

        let action = decide_at(&firefighting, &mut plan, 20.0, point(3.0, 3.0));
        assert!(
            matches!(
                action,
                Action::Move {
                    purpose: Purpose::Exit,
                    ..
                }
            ),
            "{action:?}"
        );
        let entry = firefighting.grids[0]
            .cell_center(firefighting.crews[0].entry_cell)
            .expect("entry cell");
        assert_eq!(
            decide_at(&firefighting, &mut plan, 20.0, entry),
            Action::Exit
        );
    }

    #[test]
    fn a_listed_room_that_cannot_be_reached_is_skipped() {
        let firefighting = firefighting();
        let mut plan = CrewPlan {
            route: vec!["no-walkable-cells".to_owned(), "room".to_owned()],
            ..plan()
        };

        let action = decide_at(&firefighting, &mut plan, 5.0, point(3.0, 0.4));

        assert!(plan.memory.unreachable.contains("no-walkable-cells"));
        assert!(
            matches!(
                action,
                Action::Move {
                    purpose: Purpose::Room,
                    ..
                }
            ),
            "{action:?}"
        );
        assert_eq!(plan.room_target.as_deref(), Some("room"));
    }

    #[test]
    fn movement_stops_before_the_first_cell_in_flames() {
        let firefighting = firefighting();
        let grid = &firefighting.grids[0];
        let fire = [burning_node("fire", 3.0, 2.0)];
        let hazards = HazardMap::from_fire(
            &firefighting.grids,
            &firefighting.room_floors,
            &fire,
            NavProfile::FIREFIGHTER.body_radius_m,
        );
        let row = grid.world_to_cell(point(1.0, 2.0)).expect("on grid").row;
        let path = (4..20)
            .map(|column| GridCoordinate { column, row })
            .collect::<Vec<_>>();
        let first_blocked = path
            .iter()
            .position(|&cell| hazards.get("floor-1", cell) == Some(Hazard::Blocked))
            .expect("the path crosses the fire");

        let kept = stop_short_of_flames(&path, "floor-1", &hazards);

        assert_eq!(kept, path[..first_blocked]);
        assert!(
            kept.iter()
                .all(|&cell| hazards.get("floor-1", cell) != Some(Hazard::Blocked))
        );
    }
}

#[cfg(test)]
mod room_index_tests {
    use super::*;

    #[test]
    fn every_scene_room_is_indexed_even_without_walkable_cells() {
        let firefighting = super::tests::firefighting_for_index();

        assert!(firefighting.rooms["sliver"].cells.is_empty());
        assert!(!firefighting.rooms["room"].cells.is_empty());
    }

    #[test]
    fn a_room_keeps_only_cells_reachable_from_an_entry() {
        let firefighting = super::tests::firefighting_for_index();
        let grid = &firefighting.grids[0];
        // Pretend furniture walls off everything right of x = 1.5 m.
        let left_strip = grid
            .cells()
            .iter()
            .filter(|cell| cell.center_m.x < 1.5)
            .map(|cell| cell.coordinate)
            .collect::<HashSet<_>>();
        let reachable = HashMap::from([("floor-1".to_owned(), left_strip.clone())]);

        let rooms = index_rooms(
            &super::tests::scene(),
            &firefighting.grids,
            NavProfile::FIREFIGHTER,
            &reachable,
        );

        let room = &rooms["room"];
        assert!(!room.cells.is_empty());
        assert!(room.cells.iter().all(|cell| left_strip.contains(cell)));
        assert!(room.cells.len() < firefighting.rooms["room"].cells.len());
    }

    #[test]
    fn a_room_with_no_reachable_cells_has_no_cells() {
        let firefighting = super::tests::firefighting_for_index();
        let rooms = index_rooms(
            &super::tests::scene(),
            &firefighting.grids,
            NavProfile::FIREFIGHTER,
            &HashMap::new(),
        );

        assert!(rooms["room"].cells.is_empty());
    }
}
