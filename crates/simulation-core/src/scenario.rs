//! Scenario inputs: where the fire starts, who is in the building, and which
//! crews respond. A scenario never edits the scene, so several scenarios can be
//! compared against the same building.

use std::{collections::HashSet, error::Error, fmt};

use serde::{Deserialize, Serialize};
use tflop_world_core::{
    OpeningConnection, OpeningKind, OpeningTopologyError, Scene, Vec3, resolve_opening_connections,
};

use crate::{
    agent::{
        Agent, AgentRole, Exposure, FIREFIGHTER_RADIUS_M, FirefighterPhase, FirefighterState,
        Movement, OCCUPANT_RADIUS_M, OccupantPhase, OccupantState,
    },
    grid::SimulationGrid,
    nav::nearest_walkable_cell,
};

pub const SCENARIO_SCHEMA_VERSION: u32 = 1;
/// A hurried walk: people evacuating move faster than they stroll.
pub const DEFAULT_OCCUPANT_WALKING_SPEED_M_PER_S: f64 = 1.4;
pub const DEFAULT_FIREFIGHTER_WALKING_SPEED_M_PER_S: f64 = 1.0;
pub const MAX_CREW_MEMBERS: u32 = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub schema_version: u32,
    pub id: String,
    pub scene_id: String,
    /// Furniture object where the fire starts.
    pub ignition_object_id: String,
    #[serde(default)]
    pub occupants: Vec<OccupantSpawn>,
    #[serde(default)]
    pub crews: Vec<CrewSpawn>,
}

/// One occupant present at ignition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OccupantSpawn {
    pub id: String,
    pub floor_id: String,
    /// Must fall on a walkable cell of the floor. Only `x` and `y` are used;
    /// the agent is placed on the floor surface.
    pub position_m: Vec3,
    #[serde(default = "default_occupant_speed")]
    pub walking_speed_m_per_s: f64,
    /// Time from hearing the alarm until the occupant starts moving
    /// (seeking information, preparing). See [`AlarmConfig`](crate::AlarmConfig).
    #[serde(default)]
    pub pre_movement_delay_seconds: f64,
    /// Hurt and unable to move; waits for a firefighter to carry them out.
    #[serde(default)]
    pub injured: bool,
}

/// Marks a repeatable random `fraction` of `occupants` injured (rounded to
/// the nearest whole person) and the rest not.
pub fn choose_injured(occupants: &mut [OccupantSpawn], fraction: f64, seed: u64) {
    let count = (occupants.len() as f64 * fraction.clamp(0.0, 1.0)).round() as usize;
    let mut order = (0..occupants.len()).collect::<Vec<_>>();
    order.sort_by(|&left, &right| {
        let roll = |index: usize| crate::crowd::unit(seed, INJURY_ROLL, index as u64, 0);
        roll(left).total_cmp(&roll(right)).then(left.cmp(&right))
    });
    for (rank, index) in order.into_iter().enumerate() {
        occupants[index].injured = rank < count;
    }
}

/// Keeps injury rolls apart from other random draws.
const INJURY_ROLL: u64 = 0x014A_0BED;

/// One firefighter crew. Members are spawned as agents `"{id}/1"`,
/// `"{id}/2"`, ... just inside the entry opening.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrewSpawn {
    pub id: String,
    /// An exterior door or passage (see [`exterior_exits`]). Either the
    /// connection ID or any of its source opening IDs is accepted. When
    /// absent, the first usable exterior exit by ID is used.
    #[serde(default)]
    pub entry_opening_id: Option<String>,
    /// Rooms to walk into, in order, while looking for the fire. Rooms not
    /// listed are entered afterwards, nearest first. Empty means nearest
    /// first throughout. Accepts the old name `search_order`.
    #[serde(default, alias = "search_order")]
    pub route: Vec<String>,
    /// Seconds after ignition when the crew reaches the entry.
    #[serde(default)]
    pub arrival_time_seconds: f64,
    #[serde(default = "default_member_count")]
    pub member_count: u32,
    #[serde(default = "default_firefighter_speed")]
    pub walking_speed_m_per_s: f64,
}

const fn default_member_count() -> u32 {
    1
}

const fn default_occupant_speed() -> f64 {
    DEFAULT_OCCUPANT_WALKING_SPEED_M_PER_S
}

const fn default_firefighter_speed() -> f64 {
    DEFAULT_FIREFIGHTER_WALKING_SPEED_M_PER_S
}

/// Doors and passages that lead out of the modeled rooms. These are where
/// occupants can leave and crews can enter.
pub fn exterior_exits(scene: &Scene) -> Result<Vec<OpeningConnection>, OpeningTopologyError> {
    Ok(resolve_opening_connections(scene)?
        .into_iter()
        .filter(|connection| {
            connection.room_b_id.is_none()
                && matches!(connection.kind, OpeningKind::Door | OpeningKind::Passage)
        })
        .collect())
}

impl Scenario {
    /// Checks the scenario on its own, without a scene. Returns every problem.
    pub fn validate(&self) -> Result<(), ScenarioValidationErrors> {
        let mut errors = Vec::new();
        self.validate_into(&mut errors);
        finish(errors)
    }

    fn validate_into(&self, errors: &mut Vec<ScenarioValidationError>) {
        if self.schema_version != SCENARIO_SCHEMA_VERSION {
            push_error(
                "schema_version",
                "unsupported scenario schema version",
                errors,
            );
        }
        require_nonempty(&self.id, "id", errors);
        require_nonempty(&self.scene_id, "scene_id", errors);
        require_nonempty(&self.ignition_object_id, "ignition_object_id", errors);

        let mut agent_ids = HashSet::new();
        validate_occupants_into(&self.occupants, &mut agent_ids, errors);

        validate_crews_into(&self.crews, &mut agent_ids, errors);
    }

    /// Validates the scenario against the scene it runs in and creates the
    /// initial agents. `grids` must hold one grid per floor of `scene`.
    pub(crate) fn spawn_agents(
        &self,
        scene: &Scene,
        grids: &[SimulationGrid],
        exits: &[OpeningConnection],
    ) -> Result<Vec<Agent>, ScenarioValidationErrors> {
        let mut errors = Vec::new();
        self.validate_into(&mut errors);

        if self.scene_id != scene.id {
            push_error(
                "scene_id",
                "scenario scene ID does not match the scene",
                &mut errors,
            );
        }
        if !scene.floors.iter().any(|floor| {
            floor
                .objects
                .iter()
                .any(|object| object.id == self.ignition_object_id)
        }) {
            push_error(
                "ignition_object_id",
                "object does not exist in the scene",
                &mut errors,
            );
        }

        let mut agents = create_occupants(&self.occupants, grids, &[], &mut errors);

        agents.extend(create_crews(&self.crews, scene, grids, exits, &mut errors));

        finish(errors).map(|()| agents)
    }
}

/// Validates and constructs a batch of occupants without mutating a
/// simulation. Existing agents participate in ID and body collision checks.
pub(crate) fn spawn_occupants(
    occupants: &[OccupantSpawn],
    grids: &[SimulationGrid],
    existing_agents: &[Agent],
) -> Result<Vec<Agent>, ScenarioValidationErrors> {
    let mut errors = Vec::new();
    let mut agent_ids = existing_agents
        .iter()
        .map(|agent| agent.id.clone())
        .collect();
    validate_occupants_into(occupants, &mut agent_ids, &mut errors);
    let agents = create_occupants(occupants, grids, existing_agents, &mut errors);
    finish(errors).map(|()| agents)
}

/// Validates and constructs a batch of crews without mutating a simulation.
/// Existing agents participate in ID collision checks.
pub(crate) fn spawn_crews(
    crews: &[CrewSpawn],
    scene: &Scene,
    grids: &[SimulationGrid],
    exits: &[OpeningConnection],
    existing_agents: &[Agent],
) -> Result<Vec<Agent>, ScenarioValidationErrors> {
    let mut errors = Vec::new();
    let mut agent_ids = existing_agents
        .iter()
        .map(|agent| agent.id.clone())
        .collect();
    validate_crews_into(crews, &mut agent_ids, &mut errors);
    let agents = create_crews(crews, scene, grids, exits, &mut errors);
    finish(errors).map(|()| agents)
}

fn validate_crews_into(
    crews: &[CrewSpawn],
    agent_ids: &mut HashSet<String>,
    errors: &mut Vec<ScenarioValidationError>,
) {
    let mut crew_ids = HashSet::new();
    for (index, crew) in crews.iter().enumerate() {
        let path = format!("crews[{index}]");
        require_unique(&crew.id, &format!("{path}.id"), &mut crew_ids, errors);
        if let Some(entry_opening_id) = &crew.entry_opening_id {
            require_nonempty(
                entry_opening_id,
                &format!("{path}.entry_opening_id"),
                errors,
            );
        }
        let mut room_ids = HashSet::new();
        for (room_index, room_id) in crew.route.iter().enumerate() {
            require_unique(
                room_id,
                &format!("{path}.route[{room_index}]"),
                &mut room_ids,
                errors,
            );
        }
        require_nonnegative(
            crew.arrival_time_seconds,
            &format!("{path}.arrival_time_seconds"),
            errors,
        );
        require_positive(
            crew.walking_speed_m_per_s,
            &format!("{path}.walking_speed_m_per_s"),
            errors,
        );
        if crew.member_count == 0 || crew.member_count > MAX_CREW_MEMBERS {
            push_error(
                &format!("{path}.member_count"),
                &format!("crew must have between 1 and {MAX_CREW_MEMBERS} members"),
                errors,
            );
            continue;
        }
        for member in 1..=crew.member_count {
            let id = firefighter_id(&crew.id, member);
            if !agent_ids.insert(id.clone()) {
                push_error(
                    &format!("{path}.id"),
                    &format!("firefighter ID {id} collides with another agent"),
                    errors,
                );
            }
        }
    }
}

/// Places each crew just inside its entry, on the nearest cell a body fits.
fn create_crews(
    crews: &[CrewSpawn],
    scene: &Scene,
    grids: &[SimulationGrid],
    exits: &[OpeningConnection],
    errors: &mut Vec<ScenarioValidationError>,
) -> Vec<Agent> {
    let mut sorted_exits = exits.iter().collect::<Vec<_>>();
    sorted_exits.sort_by(|left, right| left.id.cmp(&right.id));
    let approach = |exit: &OpeningConnection| {
        let grid = grids.iter().find(|grid| grid.floor_id == exit.floor_id)?;
        let cell = nearest_walkable_cell(grid, exit.center_m, &exit.room_a_id, OCCUPANT_RADIUS_M)?;
        Some((grid, cell))
    };

    let mut agents = Vec::new();
    for (index, crew) in crews.iter().enumerate() {
        let path = format!("crews[{index}]");
        let entry = match &crew.entry_opening_id {
            Some(entry_id) => sorted_exits
                .iter()
                .copied()
                .find(|exit| exit.id == *entry_id || exit.source_opening_ids.contains(entry_id)),
            None => sorted_exits
                .iter()
                .copied()
                .find(|exit| approach(exit).is_some()),
        };
        let Some(entry) = entry else {
            push_error(
                &format!("{path}.entry_opening_id"),
                "opening is not an exterior door or passage",
                errors,
            );
            continue;
        };
        let Some((grid, cell)) = approach(entry) else {
            push_error(
                &format!("{path}.entry_opening_id"),
                "entry has no walkable cell inside it",
                errors,
            );
            continue;
        };
        for (room_index, room_id) in crew.route.iter().enumerate() {
            let floor = scene
                .floors
                .iter()
                .find(|floor| floor.rooms.iter().any(|room| room.id == *room_id));
            match floor {
                None => push_error(
                    &format!("{path}.route[{room_index}]"),
                    "room does not exist in the scene",
                    errors,
                ),
                Some(floor) if floor.id != entry.floor_id => push_error(
                    &format!("{path}.route[{room_index}]"),
                    "room is not on the crew's entry floor",
                    errors,
                ),
                Some(_) => {}
            }
        }
        let Some(position_m) = grid.cell_center(cell) else {
            continue;
        };
        for member in 1..=crew.member_count.min(MAX_CREW_MEMBERS) {
            agents.push(Agent {
                id: firefighter_id(&crew.id, member),
                floor_id: entry.floor_id.clone(),
                position_m: on_floor(position_m, grid),
                velocity_m_per_s: ZERO,
                radius_m: FIREFIGHTER_RADIUS_M,
                exposure: Exposure::NONE,
                movement: Movement::Stopped,
                role: AgentRole::Firefighter(FirefighterState {
                    crew_id: crew.id.clone(),
                    phase: FirefighterPhase::Staging,
                    walking_speed_m_per_s: crew.walking_speed_m_per_s,
                    entry_opening_id: entry.id.clone(),
                    arrival_time_seconds: crew.arrival_time_seconds,
                    target_fire_node_id: None,
                    nozzle_target_m: None,
                }),
            });
        }
    }
    agents
}

const ZERO: Vec3 = Vec3 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

fn firefighter_id(crew_id: &str, member: u32) -> String {
    format!("{crew_id}/{member}")
}

fn on_floor(position_m: Vec3, grid: &SimulationGrid) -> Vec3 {
    Vec3 {
        z: grid.bounds.elevation_m,
        ..position_m
    }
}

fn validate_occupants_into(
    occupants: &[OccupantSpawn],
    agent_ids: &mut HashSet<String>,
    errors: &mut Vec<ScenarioValidationError>,
) {
    for (index, occupant) in occupants.iter().enumerate() {
        let path = format!("occupants[{index}]");
        require_unique(&occupant.id, &format!("{path}.id"), agent_ids, errors);
        require_nonempty(&occupant.floor_id, &format!("{path}.floor_id"), errors);
        if !occupant.position_m.x.is_finite() || !occupant.position_m.y.is_finite() {
            push_error(
                &format!("{path}.position_m"),
                "position must be finite",
                errors,
            );
        }
        require_positive(
            occupant.walking_speed_m_per_s,
            &format!("{path}.walking_speed_m_per_s"),
            errors,
        );
        require_nonnegative(
            occupant.pre_movement_delay_seconds,
            &format!("{path}.pre_movement_delay_seconds"),
            errors,
        );
    }
}

fn create_occupants(
    occupants: &[OccupantSpawn],
    grids: &[SimulationGrid],
    existing_agents: &[Agent],
    errors: &mut Vec<ScenarioValidationError>,
) -> Vec<Agent> {
    let mut agents = Vec::new();
    for (index, occupant) in occupants.iter().enumerate() {
        let path = format!("occupants[{index}]");
        let Some(grid) = grids.iter().find(|grid| grid.floor_id == occupant.floor_id) else {
            push_error(
                &format!("{path}.floor_id"),
                "floor does not exist in the scene",
                errors,
            );
            continue;
        };
        if !grid.circle_is_walkable(occupant.position_m, OCCUPANT_RADIUS_M) {
            push_error(
                &format!("{path}.position_m"),
                "position does not leave enough walkable clearance for the occupant",
                errors,
            );
            continue;
        }

        let candidate = Agent {
            id: occupant.id.clone(),
            floor_id: occupant.floor_id.clone(),
            position_m: on_floor(occupant.position_m, grid),
            velocity_m_per_s: ZERO,
            radius_m: OCCUPANT_RADIUS_M,
            exposure: Exposure::NONE,
            movement: Movement::Stopped,
            role: AgentRole::Occupant(OccupantState {
                phase: if occupant.injured {
                    OccupantPhase::Injured
                } else {
                    OccupantPhase::Unaware
                },
                injured: occupant.injured,
                walking_speed_m_per_s: occupant.walking_speed_m_per_s,
                pre_movement_delay_seconds: occupant.pre_movement_delay_seconds,
                target_exit_id: None,
            }),
        };
        if existing_agents
            .iter()
            .chain(agents.iter())
            .any(|other| agents_overlap(&candidate, other))
        {
            push_error(
                &format!("{path}.position_m"),
                "occupant overlaps another agent",
                errors,
            );
            continue;
        }
        agents.push(candidate);
    }
    agents
}

fn agents_overlap(left: &Agent, right: &Agent) -> bool {
    if left.floor_id != right.floor_id {
        return false;
    }
    let dx = left.position_m.x - right.position_m.x;
    let dy = left.position_m.y - right.position_m.y;
    let combined_radius = left.radius_m + right.radius_m;
    dx * dx + dy * dy < combined_radius * combined_radius
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioValidationError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for ScenarioValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioValidationErrors(pub Vec<ScenarioValidationError>);

impl fmt::Display for ScenarioValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "scenario validation failed with {} error(s)",
            self.0.len()
        )
    }
}

impl Error for ScenarioValidationErrors {}

fn finish(errors: Vec<ScenarioValidationError>) -> Result<(), ScenarioValidationErrors> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ScenarioValidationErrors(errors))
    }
}

fn push_error(path: &str, message: &str, errors: &mut Vec<ScenarioValidationError>) {
    errors.push(ScenarioValidationError {
        path: path.to_owned(),
        message: message.to_owned(),
    });
}

fn require_nonempty(value: &str, path: &str, errors: &mut Vec<ScenarioValidationError>) {
    if value.trim().is_empty() {
        push_error(path, "must not be empty", errors);
    }
}

fn require_unique(
    id: &str,
    path: &str,
    seen: &mut HashSet<String>,
    errors: &mut Vec<ScenarioValidationError>,
) {
    require_nonempty(id, path, errors);
    if !seen.insert(id.to_owned()) {
        push_error(path, "ID must be unique", errors);
    }
}

fn require_positive(value: f64, path: &str, errors: &mut Vec<ScenarioValidationError>) {
    if !value.is_finite() || value <= 0.0 {
        push_error(path, "must be finite and greater than zero", errors);
    }
}

fn require_nonnegative(value: f64, path: &str, errors: &mut Vec<ScenarioValidationError>) {
    if !value.is_finite() || value < 0.0 {
        push_error(path, "must be finite and not negative", errors);
    }
}
