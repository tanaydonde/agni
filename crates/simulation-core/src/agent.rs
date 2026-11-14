//! People who move through the building during a run: occupants evacuating
//! and firefighters responding.
//!
//! Agents are simulation state, not scene content. They have continuous
//! positions and read the static [`SimulationGrid`](crate::SimulationGrid)
//! instead of occupying grid cells. Occupants and firefighters share one
//! [`Agent`] type so movement, collision avoidance, and exposure can treat
//! every body the same way; only decision-making branches on [`AgentRole`].

use serde::{Deserialize, Serialize, Serializer};
use tflop_world_core::Vec3;

/// Body radius used for collision avoidance.
pub const OCCUPANT_RADIUS_M: f64 = 0.25;
/// Larger than an occupant to account for turnout gear and equipment.
pub const FIREFIGHTER_RADIUS_M: f64 = 0.3;

/// One person in the simulation, with state shared by every role.
#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    /// Unique across every agent in a run. Firefighters are `"{crew_id}/{n}"`.
    pub id: String,
    pub floor_id: String,
    /// Canonical Z-up metres; `z` is the floor surface the agent stands on.
    pub position_m: Vec3,
    pub velocity_m_per_s: Vec3,
    pub radius_m: f64,
    pub exposure: Exposure,
    /// How the agent moved through the crowd during the last step.
    pub movement: Movement,
    pub role: AgentRole,
}

impl Agent {
    /// Whether the agent still moves and acts. Exited and incapacitated
    /// agents stay in the run so playback and metrics can report them.
    #[must_use]
    pub fn is_active(&self) -> bool {
        match &self.role {
            AgentRole::Occupant(state) => !matches!(
                state.phase,
                OccupantPhase::Exited | OccupantPhase::Incapacitated
            ),
            AgentRole::Firefighter(state) => !matches!(
                state.phase,
                FirefighterPhase::Exited | FirefighterPhase::Incapacitated
            ),
        }
    }

    /// Renderer-facing view of this agent.
    #[must_use]
    pub fn frame(&self) -> AgentFrame {
        AgentFrame {
            id: self.id.clone(),
            floor_id: self.floor_id.clone(),
            position_m: self.position_m,
            velocity_m_per_s: self.velocity_m_per_s,
            radius_m: self.radius_m,
            exposure: self.exposure,
            movement: self.movement,
            role: match &self.role {
                AgentRole::Occupant(state) => AgentFrameRole::Occupant {
                    phase: state.phase,
                    injured: state.injured,
                },
                AgentRole::Firefighter(state) => AgentFrameRole::Firefighter {
                    crew_id: state.crew_id.clone(),
                    phase: state.phase,
                    nozzle_target_m: state.nozzle_target_m,
                },
            },
        }
    }
}

/// Accumulated harm, as fractional effective doses (FED). Each dose starts at
/// 0.0, and reaching 1.0 means the agent is incapacitated. Heat and toxic gas
/// are tracked separately because they come from different sources (surface
/// fire proximity vs. CFAST room conditions) and are judged separately in
/// tenability analysis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Exposure {
    pub heat: f64,
    pub toxic_gas: f64,
}

impl Exposure {
    pub const NONE: Self = Self {
        heat: 0.0,
        toxic_gas: 0.0,
    };

    #[must_use]
    pub fn is_incapacitating(self) -> bool {
        self.heat >= 1.0 || self.toxic_gas >= 1.0
    }
}

/// How an agent moved through the crowd during the last step, so playback
/// can show where people are held up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Movement {
    /// Not trying to move: waiting to leave, searching, spraying, or out of
    /// the run.
    #[default]
    Stopped,
    Walking,
    /// Trying to move but held up by people in the way.
    Queued,
    /// Pressing past someone, in a doorway or hallway too narrow to pass
    /// side by side.
    Squeezing,
}

/// What kind of agent this is, with the state only that role needs.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentRole {
    Occupant(OccupantState),
    Firefighter(FirefighterState),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OccupantState {
    pub phase: OccupantPhase,
    pub walking_speed_m_per_s: f64,
    /// Time from ignition until the occupant starts moving.
    pub pre_movement_delay_seconds: f64,
    /// `OpeningConnection` ID of the exit the occupant is heading for.
    pub target_exit_id: Option<String>,
    /// Hurt at the start of the run and unable to move; only a firefighter
    /// can get them out. Stays true once rescued, for reporting.
    pub injured: bool,
}

/// Occupant behavior phases (see `TECH_STACK.md`, "Occupants"). Avoid a
/// universal "panic" phase; people mostly act rationally and help others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccupantPhase {
    /// Has not perceived a cue yet.
    Unaware,
    /// Perceived a cue; seeking information, preparing, or helping others.
    Preparing,
    MovingToExit,
    /// Changing exits because a route became blocked or congested.
    Rerouting,
    Exited,
    /// Exposure reached an incapacitating dose. Never labeled "dead".
    Incapacitated,
    /// Hurt and unable to move, waiting for a firefighter.
    Injured,
    /// Being carried out by a firefighter.
    Carried,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirefighterState {
    pub crew_id: String,
    pub phase: FirefighterPhase,
    pub walking_speed_m_per_s: f64,
    /// Exterior `OpeningConnection` the crew enters through.
    pub entry_opening_id: String,
    pub arrival_time_seconds: f64,
    /// Surface fire node the firefighter is advancing on or suppressing.
    pub target_fire_node_id: Option<String>,
    /// Where the nozzle is pointed while spraying.
    pub nozzle_target_m: Option<Vec3>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirefighterPhase {
    /// Outside, waiting for the crew's arrival time.
    Staging,
    /// Walking into the next room on the route, or toward a known fire.
    Advancing,
    Suppressing,
    /// Going to pick up an injured occupant.
    Rescuing,
    /// Carrying an injured occupant out.
    Carrying,
    Withdrawing,
    Exited,
    Incapacitated,
}

/// One agent in one renderer frame. Holds only what playback needs; runtime
/// details such as speeds or paths stay on [`Agent`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentFrame {
    pub id: String,
    pub floor_id: String,
    /// Written to the millimetre: a timeline of hundreds of people would
    /// otherwise spend most of its size on digits nobody can see.
    #[serde(serialize_with = "to_the_millimetre")]
    pub position_m: Vec3,
    #[serde(serialize_with = "to_the_millimetre")]
    pub velocity_m_per_s: Vec3,
    pub radius_m: f64,
    pub exposure: Exposure,
    /// Absent in frames written before crowd movement existed.
    #[serde(default)]
    pub movement: Movement,
    pub role: AgentFrameRole,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentFrameRole {
    Occupant {
        phase: OccupantPhase,
        /// Started the run injured. An injured occupant who has exited was
        /// carried out.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        injured: bool,
    },
    Firefighter {
        crew_id: String,
        phase: FirefighterPhase,
        /// Where the stream is aimed, while spraying.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nozzle_target_m: Option<Vec3>,
    },
}

fn to_the_millimetre<S: Serializer>(value: &Vec3, serializer: S) -> Result<S::Ok, S::Error> {
    let round = |component: f64| (component * 1000.0).round() / 1000.0;
    Vec3 {
        x: round(value.x),
        y: round(value.y),
        z: round(value.z),
    }
    .serialize(serializer)
}
