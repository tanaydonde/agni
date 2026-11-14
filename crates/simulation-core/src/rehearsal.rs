//! One deterministic run in which the fire and every agent advance together,
//! one frame at a time.
//!
//! Each step decides, acts, advances the fire, then moves agents against the
//! fire as it now stands. Running everything in one loop, rather than
//! replaying agents over a finished fire timeline, is what lets agents change
//! the fire (firefighters suppressing it). The loop needs no renderer, so the
//! same code serves a single interactive run and many Monte Carlo samples.

use thiserror::Error;

use crate::{
    AgentFrame, CombustionState, Evacuation, EvacuationError, Firefighting, FirefightingError,
    SimulationFrame, Suppression, SurfaceFireError, SurfaceFireFrame, SurfaceFireNodeFrame,
    SurfaceFireWorld,
};

#[derive(Debug)]
pub struct Rehearsal {
    fire: SurfaceFireWorld,
    evacuation: Option<Evacuation>,
    firefighting: Option<Firefighting>,
    fire_active: bool,
    /// Fire as of the last recorded frame: what firefighters act on next.
    last_fire: Vec<SurfaceFireNodeFrame>,
}

/// Everything that changed at one simulation time.
#[derive(Debug, Clone, PartialEq)]
pub struct RehearsalFrame {
    /// Kept whole (including room heat release) so CFAST can be run on the
    /// timeline afterwards.
    pub fire: SurfaceFireFrame,
    pub agents: Vec<AgentFrame>,
}

impl RehearsalFrame {
    /// Renderer frame without CFAST room conditions.
    #[must_use]
    pub fn into_simulation_frame(self) -> SimulationFrame {
        SimulationFrame {
            time_seconds: self.fire.time_seconds,
            rooms: Vec::new(),
            fires: Vec::new(),
            openings: Vec::new(),
            surface_fire: self.fire.active_nodes,
            agents: self.agents,
        }
    }
}

impl Rehearsal {
    /// Starts at the fire world's current time. Occupants are optional so
    /// fire-only runs use the same loop.
    #[must_use]
    pub fn new(fire: SurfaceFireWorld, evacuation: Option<Evacuation>) -> Self {
        Self {
            fire,
            evacuation,
            firefighting: None,
            fire_active: false,
            last_fire: Vec::new(),
        }
    }

    /// Adds firefighter crews. Each step they act on the fire as it stood at
    /// the previous frame, and their water is applied before the fire
    /// advances.
    #[must_use]
    pub fn with_firefighting(mut self, firefighting: Firefighting) -> Self {
        self.firefighting = Some(firefighting);
        self
    }

    #[must_use]
    pub const fn firefighting(&self) -> Option<&Firefighting> {
        self.firefighting.as_ref()
    }

    /// Ignites an object without advancing time and records the frame.
    pub fn ignite_object(&mut self, object_id: &str) -> Result<RehearsalFrame, RehearsalError> {
        let fire = self.fire.ignite_object(object_id)?;
        self.record(fire)
    }

    /// Advances the fire and every agent by `delta_seconds`.
    pub fn step(&mut self, delta_seconds: f64) -> Result<RehearsalFrame, RehearsalError> {
        self.step_with_suppression(delta_seconds, &[])
    }

    /// Like [`step`](Self::step), with water applied over the interval before
    /// the fire advances, so suppression shapes this step's spread.
    pub fn step_with_suppression(
        &mut self,
        delta_seconds: f64,
        suppressions: &[Suppression],
    ) -> Result<RehearsalFrame, RehearsalError> {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            return Err(RehearsalError::InvalidStep {
                step_seconds: delta_seconds,
            });
        }
        // Validate every nozzle first so a bad one can't leave the fire
        // partly suppressed or the crews moved.
        for suppression in suppressions {
            suppression.validate()?;
        }
        let mut water = Vec::new();
        if let Some(firefighting) = &mut self.firefighting {
            water = firefighting
                .advance_to(self.fire.time_seconds() + delta_seconds, &self.last_fire)?;
            for suppression in &water {
                suppression.validate()?;
            }
        }
        for suppression in water.iter().chain(suppressions) {
            self.fire.suppress(suppression, delta_seconds)?;
        }
        let fire = self.fire.step(delta_seconds)?;
        self.record(fire)
    }

    /// Steps until `duration_seconds` is reached or nothing is left to
    /// simulate, and returns the new frames. The last frame may pass
    /// `duration_seconds` by less than one step.
    pub fn run_until(
        &mut self,
        duration_seconds: f64,
        step_seconds: f64,
    ) -> Result<Vec<RehearsalFrame>, RehearsalError> {
        if !step_seconds.is_finite() || step_seconds <= 0.0 {
            return Err(RehearsalError::InvalidStep { step_seconds });
        }
        let mut frames = Vec::new();
        while self.fire.time_seconds() < duration_seconds && !self.is_settled() {
            frames.push(self.step(step_seconds)?);
        }
        Ok(frames)
    }

    /// True once no node is burning, every occupant has exited or
    /// is incapacitated, and every crew has left. A fire that has gone out
    /// does not end the run while people are still moving.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        !self.fire_active
            && self
                .evacuation
                .as_ref()
                .is_none_or(|evacuation| !evacuation.has_active_agents())
            && self
                .firefighting
                .as_ref()
                .is_none_or(|firefighting| !firefighting.has_active_agents())
    }

    fn record(&mut self, fire: SurfaceFireFrame) -> Result<RehearsalFrame, RehearsalError> {
        // Only burning nodes give off heat, so once none burn, heating nodes
        // can only cool: the fire is over even while some are still warm.
        self.fire_active = fire
            .active_nodes
            .iter()
            .any(|node| node.state == CombustionState::Burning);
        let mut agents = match &mut self.evacuation {
            Some(evacuation) => evacuation.advance_to(fire.time_seconds, &fire.active_nodes)?,
            None => Vec::new(),
        };
        if let Some(firefighting) = &self.firefighting {
            agents.extend(firefighting.frames());
        }
        self.last_fire.clone_from(&fire.active_nodes);
        Ok(RehearsalFrame { fire, agents })
    }
}

#[derive(Debug, Error)]
pub enum RehearsalError {
    #[error(transparent)]
    Fire(#[from] SurfaceFireError),
    #[error(transparent)]
    Evacuation(#[from] EvacuationError),
    #[error(transparent)]
    Firefighting(#[from] FirefightingError),
    #[error("step_seconds must be finite and greater than zero, got {step_seconds}")]
    InvalidStep { step_seconds: f64 },
}
