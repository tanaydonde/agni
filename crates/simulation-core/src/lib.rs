//! Deterministic simulation lifecycle and renderer-facing state snapshots.

mod agent;
mod crowd;
mod evacuation;
mod exit_field;
mod firefighting;
mod grid;
mod nav;
mod rehearsal;
mod results;
mod scenario;
mod surface_fire;

use serde::{Deserialize, Serialize};
use tflop_world_core::{OpeningTopologyError, Scene, SceneValidationErrors, Vec3};
use thiserror::Error;

pub use agent::{
    Agent, AgentFrame, AgentFrameRole, AgentRole, Exposure, FIREFIGHTER_RADIUS_M, FirefighterPhase,
    FirefighterState, Movement, OCCUPANT_RADIUS_M, OccupantPhase, OccupantState,
};
pub use crowd::{CROWD_GIVE_M, CrowdConfig, CrowdConfigError, SINGLE_FILE_MAX_WIDTH_M};
pub use evacuation::{AlarmConfig, Evacuation, EvacuationError, reachable_exit_cells};
pub use firefighting::{
    CrewMemory, CrewReport, Firefighting, FirefightingConfig, FirefightingError,
};
pub use grid::{
    GridBounds, GridBuildError, GridCell, GridCellKind, GridCoordinate, GridSummary, SimulationGrid,
};
pub use rehearsal::{Rehearsal, RehearsalError, RehearsalFrame};
pub use results::{
    FireConditions, GasLayerConditions, OpeningConditions, ResultsValidationError,
    ResultsValidationErrors, RoomConditions, SIMULATION_RESULTS_SCHEMA_VERSION, SimulationEngine,
    SimulationFrame, SimulationResults,
};
pub use scenario::{
    CrewSpawn, DEFAULT_FIREFIGHTER_WALKING_SPEED_M_PER_S, DEFAULT_OCCUPANT_WALKING_SPEED_M_PER_S,
    MAX_CREW_MEMBERS, OccupantSpawn, SCENARIO_SCHEMA_VERSION, Scenario, ScenarioValidationError,
    ScenarioValidationErrors, exterior_exits,
};
pub use surface_fire::{
    CombustionState, DEFAULT_NOZZLE_COOLING_KW, DEFAULT_NOZZLE_REACH_M, FURNITURE_FIRE_CELL_SIZE_M,
    FireMaterialCatalog, FireMaterialProfile, FurnitureFireCell, FurnitureFireMaterial,
    RoomHeatRelease, SURFACE_FIRE_SCHEMA_VERSION, Suppression, SuppressionOutcome,
    SurfaceFireConfig, SurfaceFireError, SurfaceFireFrame, SurfaceFireNode, SurfaceFireNodeFrame,
    SurfaceFireNodeSource, SurfaceFireWorld,
};

/// Inputs that remain fixed for one simulation run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SimulationConfig {
    /// Reserved now so randomized fire and occupant behavior can remain
    /// repeatable when those systems are implemented.
    pub random_seed: u64,
    /// Width and depth of each square floor cell.
    pub grid_cell_size_m: f64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            random_seed: 0,
            grid_cell_size_m: 0.25,
        }
    }
}

/// Owns one simulation run. The canonical scene remains unchanged while the
/// simulation's derived state advances.
#[derive(Debug)]
pub struct Simulation {
    scene: Scene,
    config: SimulationConfig,
    grids: Vec<SimulationGrid>,
    time_seconds: f64,
    status: SimulationStatus,
    fires: Vec<FireState>,
    smoke: Vec<SmokeState>,
    agents: Vec<Agent>,
    /// Agents as spawned, so `reset` can restore them.
    initial_agents: Vec<Agent>,
}

impl Simulation {
    /// Creates a simulation at time zero from a structurally valid scene, with
    /// no agents.
    pub fn new(scene: Scene, config: SimulationConfig) -> Result<Self, SimulationError> {
        scene.validate()?;
        if !config.grid_cell_size_m.is_finite() || config.grid_cell_size_m <= 0.0 {
            return Err(GridBuildError::InvalidCellSize {
                cell_size_m: config.grid_cell_size_m,
            }
            .into());
        }
        let grids = scene
            .floors
            .iter()
            .map(|floor| SimulationGrid::from_floor(floor, &scene.assets, config.grid_cell_size_m))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            scene,
            config,
            grids,
            time_seconds: 0.0,
            status: SimulationStatus::Ready,
            fires: Vec::new(),
            smoke: Vec::new(),
            agents: Vec::new(),
            initial_agents: Vec::new(),
        })
    }

    /// Creates a simulation at time zero with the scenario's occupants and
    /// crews spawned. Reports every scenario problem at once.
    pub fn from_scenario(
        scene: Scene,
        scenario: &Scenario,
        config: SimulationConfig,
    ) -> Result<Self, SimulationError> {
        let mut simulation = Self::new(scene, config)?;
        let exits = if scenario.crews.is_empty() {
            Vec::new()
        } else {
            exterior_exits(&simulation.scene)?
        };
        let agents = scenario.spawn_agents(&simulation.scene, &simulation.grids, &exits)?;
        simulation.initial_agents.clone_from(&agents);
        simulation.agents = agents;
        Ok(simulation)
    }

    /// Advances simulation time and returns the state the renderer should show.
    /// Fire, smoke, and agent updates will be added behind this method.
    pub fn step(&mut self, delta_seconds: f64) -> Result<SimulationSnapshot, SimulationError> {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            return Err(SimulationError::InvalidDeltaSeconds { delta_seconds });
        }

        self.time_seconds += delta_seconds;
        self.status = SimulationStatus::Running;
        Ok(self.snapshot())
    }

    /// Reads the current state without advancing simulation time.
    #[must_use]
    pub fn snapshot(&self) -> SimulationSnapshot {
        SimulationSnapshot {
            scene_id: self.scene.id.clone(),
            time_seconds: self.time_seconds,
            status: self.status,
            fires: self.fires.clone(),
            smoke: self.smoke.clone(),
            agents: self.agents.iter().map(Agent::frame).collect(),
        }
    }

    /// Returns the simulation to the same state as a newly created run.
    pub fn reset(&mut self) -> SimulationSnapshot {
        self.time_seconds = 0.0;
        self.status = SimulationStatus::Ready;
        self.fires.clear();
        self.smoke.clear();
        self.agents.clone_from(&self.initial_agents);
        self.snapshot()
    }

    #[must_use]
    pub const fn config(&self) -> SimulationConfig {
        self.config
    }

    #[must_use]
    pub const fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Static floor grids are read separately from dynamic snapshots so the
    /// renderer does not receive unchanged geometry on every step.
    #[must_use]
    pub fn grids(&self) -> &[SimulationGrid] {
        &self.grids
    }

    #[must_use]
    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    /// Adds a batch of occupants to the run's initial state. The operation is
    /// atomic: if any occupant is invalid, none of them are added.
    pub fn spawn_occupants(
        &mut self,
        occupants: &[OccupantSpawn],
    ) -> Result<Vec<AgentFrame>, SimulationError> {
        let spawned = scenario::spawn_occupants(occupants, &self.grids, &self.agents)?;
        let frames = spawned.iter().map(Agent::frame).collect();
        self.initial_agents.extend(spawned.iter().cloned());
        self.agents.extend(spawned);
        Ok(frames)
    }

    #[must_use]
    pub fn grid(&self, floor_id: &str) -> Option<&SimulationGrid> {
        self.grids.iter().find(|grid| grid.floor_id == floor_id)
    }
}

/// Complete renderer-facing state at one simulation time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulationSnapshot {
    pub scene_id: String,
    pub time_seconds: f64,
    pub status: SimulationStatus,
    pub fires: Vec<FireState>,
    pub smoke: Vec<SmokeState>,
    pub agents: Vec<AgentFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationStatus {
    Ready,
    Running,
    Finished,
}

/// One renderer-visible fire sample. Grid generation will populate these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireState {
    pub position_m: Vec3,
    pub intensity: f64,
}

/// One renderer-visible smoke sample. Grid generation will populate these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmokeState {
    pub position_m: Vec3,
    pub concentration: f64,
}

#[derive(Debug, Error)]
pub enum SimulationError {
    #[error(transparent)]
    InvalidScene(#[from] SceneValidationErrors),
    #[error(transparent)]
    GridBuild(#[from] GridBuildError),
    #[error(transparent)]
    InvalidScenario(#[from] ScenarioValidationErrors),
    #[error(transparent)]
    OpeningTopology(#[from] OpeningTopologyError),
    #[error("delta_seconds must be finite and greater than zero, got {delta_seconds}")]
    InvalidDeltaSeconds { delta_seconds: f64 },
}
