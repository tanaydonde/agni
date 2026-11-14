//! People who move through the building during a run: occupants evacuating
//! and firefighters responding.
//!
//! Agents are simulation state, not scene content. They have continuous
//! positions and read the static [`SimulationGrid`](crate::SimulationGrid)
//! instead of occupying grid cells. Occupants and firefighters share one
//! [`Agent`] type so movement, collision avoidance, and exposure can treat
//! every body the same way; only decision-making branches on [`AgentRole`].

use serde::{Deserialize, Serialize};
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
            role: match &self.role {
                AgentRole::Occupant(state) => AgentFrameRole::Occupant { phase: state.phase },
                AgentRole::Firefighter(state) => AgentFrameRole::Firefighter {
                    crew_id: state.crew_id.clone(),
                    phase: state.phase,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirefighterPhase {
    /// Outside, waiting for the crew's arrival time.
    Staging,
    /// Moving toward an assigned task.
    Advancing,
    Searching,
    Suppressing,
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
    pub position_m: Vec3,
    pub velocity_m_per_s: Vec3,
    pub radius_m: f64,
    pub exposure: Exposure,
    pub role: AgentFrameRole,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentFrameRole {
    Occupant {
        phase: OccupantPhase,
    },
    Firefighter {
        crew_id: String,
        phase: FirefighterPhase,
    },
}
