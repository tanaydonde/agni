//! Moves every body in a run together, so occupants and firefighters share
//! one space: they queue, step around each other, and squeeze past each
//! other in doors and hallways.
//!
//! Controllers ([`Evacuation`](crate::Evacuation) and
//! [`Firefighting`](crate::Firefighting)) decide where each of their agents
//! wants to go, as an [`Intent`]. [`Crowd::step`] then moves all bodies in
//! short sub-steps. A body may move as far as it can toward its goal, or step
//! aside, but never closer into another body.
//!
//! Doors, and hallways narrower than [`SINGLE_FILE_MAX_WIDTH_M`], are single
//! file: two bodies in the same passage are measured apart along it only, so
//! nobody walks abreast or overtakes. People meeting head-on there contest
//! the space and then squeeze past each other slowly. Firefighters usually
//! win contests with occupants, and are always slowed by them.

use std::{collections::HashMap, sync::Arc};

use tflop_world_core::Vec3;
use thiserror::Error;

use crate::{
    Agent, AgentRole, FirefighterPhase, GridCoordinate, Movement, OCCUPANT_RADIUS_M, OccupantPhase,
    SimulationGrid,
    exit_field::ExitField,
    nav::{Hazard, HazardMap, distance_2d},
};

/// Passages narrower than this are single file.
pub const SINGLE_FILE_MAX_WIDTH_M: f64 = 1.8;
/// How far past a door's own cells its single-file rule reaches, so people
/// line up before stepping into the doorway.
const DOOR_APPROACH_M: f64 = 0.5;
/// Spatial hash bucket.
const BUCKET_SIZE_M: f64 = 1.0;
/// Farthest apart two bodies' centers can be and still touch, with margin.
const CONTACT_REACH_M: f64 = 1.0;
/// Farthest apart two bodies in one passage can be and still touch along it.
const PASSAGE_REACH_M: f64 = SINGLE_FILE_MAX_WIDTH_M + 0.7;
/// A body that covers less than this share of its desired move is blocked.
const BLOCKED_FRACTION: f64 = 0.3;
/// Moving less than this from where a body last made progress counts as
/// stalled.
const STALL_DISTANCE_M: f64 = 0.1;
/// Two bodies are head-on when their directions' dot product is below this.
const HEAD_ON_DOT: f64 = -0.5;
const COUNTERFLOW_DOT: f64 = -0.3;
const COUNTERFLOW_RADIUS_M: f64 = 1.5;
const COUNTERFLOW_MIN_PEOPLE: usize = 2;
/// A squeeze that hasn't got the pair past each other by then ends as soon
/// as they are clear of each other, even if neither got past, or once both
/// have been stuck for [`STALL_RELEASE_SECONDS`].
const PERMIT_TIMEOUT_SECONDS: f64 = 8.0;
const STALL_RELEASE_SECONDS: f64 = 2.0;
/// How far two bodies in an open crowd may press into each other. Without
/// some give, people converging on a door wedge into an arch nobody can
/// break.
pub const CROWD_GIVE_M: f64 = 0.08;
/// Farthest a body is shoved in one sub-step, and the clearance a shove
/// leaves.
const MAX_PUSH_M: f64 = 0.2;
const PUSH_MARGIN_M: f64 = 0.005;
/// Sidesteps cover this share of a full step.
const SIDESTEP_FRACTION: f64 = 0.8;
const SIDESTEP_DEGREES: [f64; 4] = [35.0, -35.0, 70.0, -70.0];
const YIELD_SIDESTEP_DEGREES: [f64; 6] = [35.0, -35.0, 70.0, -70.0, 100.0, -100.0];
const GAP_EPSILON_M: f64 = 1.0e-9;

/// How bodies push through each other. Every duration is in seconds of
/// contact; every speed factor multiplies walking speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrowdConfig {
    /// Longest movement sub-step. At walking speed it keeps every move under
    /// one body radius, so contact is never skipped over.
    pub max_substep_seconds: f64,
    /// Occupants meeting head-on in a passage squeeze past after this long.
    pub head_on_squeeze_after_seconds: f64,
    /// A firefighter and an occupant blocking each other contest the space
    /// after this long.
    pub firefighter_contest_after_seconds: f64,
    /// Chance a firefighter gives way in a contest instead of pushing
    /// through. Either way the two then squeeze past each other.
    pub firefighter_yield_probability: f64,
    /// How long a firefighter who gives way stands aside.
    pub firefighter_yield_seconds: f64,
    /// An occupant blocked this long in an open room tries wider sidesteps,
    /// giving way.
    pub room_yield_after_seconds: f64,
    /// Bodies that block each other in a circle (A waits on B, who waits on
    /// C, who waits on A) and have made no progress for this long squeeze
    /// past, so a jam can never last forever. A queue that is only waiting
    /// for its front to move is not a deadlock.
    pub deadlock_after_seconds: f64,
    pub occupant_squeeze_speed_factor: f64,
    /// Turnout gear and hose make squeezing past people slower.
    pub firefighter_squeeze_speed_factor: f64,
    /// Stepping over someone who has collapsed.
    pub fallen_speed_factor: f64,
    /// Firefighters walking against evacuating occupants.
    pub counterflow_speed_factor: f64,
    /// A body blocked this long shows as [`Movement::Queued`].
    pub queued_after_seconds: f64,
    /// Route cost each person adds to the floor cells under them, compared
    /// with 1000 for one step across an empty cell. Makes people farther back
    /// in a long queue head for another exit.
    pub crowding_cost_per_body: u64,
}

impl Default for CrowdConfig {
    fn default() -> Self {
        Self {
            max_substep_seconds: 0.1,
            head_on_squeeze_after_seconds: 1.5,
            firefighter_contest_after_seconds: 1.0,
            firefighter_yield_probability: 0.2,
            firefighter_yield_seconds: 2.0,
            room_yield_after_seconds: 2.0,
            deadlock_after_seconds: 5.0,
            occupant_squeeze_speed_factor: 0.4,
            firefighter_squeeze_speed_factor: 0.3,
            fallen_speed_factor: 0.5,
            counterflow_speed_factor: 0.6,
            queued_after_seconds: 0.5,
            crowding_cost_per_body: 1_500,
        }
    }
}

impl CrowdConfig {
    pub fn validate(&self) -> Result<(), CrowdConfigError> {
        let positive = [
            ("max_substep_seconds", self.max_substep_seconds),
            (
                "head_on_squeeze_after_seconds",
                self.head_on_squeeze_after_seconds,
            ),
            (
                "firefighter_contest_after_seconds",
                self.firefighter_contest_after_seconds,
            ),
            ("firefighter_yield_seconds", self.firefighter_yield_seconds),
            ("room_yield_after_seconds", self.room_yield_after_seconds),
            ("deadlock_after_seconds", self.deadlock_after_seconds),
            ("queued_after_seconds", self.queued_after_seconds),
        ];
        for (field, value) in positive {
            if !value.is_finite() || value <= 0.0 {
                return Err(CrowdConfigError { field });
            }
        }
        let factors = [
            (
                "occupant_squeeze_speed_factor",
                self.occupant_squeeze_speed_factor,
            ),
            (
                "firefighter_squeeze_speed_factor",
                self.firefighter_squeeze_speed_factor,
            ),
            ("fallen_speed_factor", self.fallen_speed_factor),
            ("counterflow_speed_factor", self.counterflow_speed_factor),
        ];
        for (field, value) in factors {
            if !value.is_finite() || value <= 0.0 || value > 1.0 {
                return Err(CrowdConfigError { field });
            }
        }
        if !(0.0..=1.0).contains(&self.firefighter_yield_probability) {
            return Err(CrowdConfigError {
                field: "firefighter_yield_probability",
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid crowd config field {field}")]
pub struct CrowdConfigError {
    pub field: &'static str,
}

/// The direction of a single-file passage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
}

/// Which cells of one floor are single file, and along which axis.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PassageMap {
    axes: Vec<Option<Axis>>,
}

impl PassageMap {
    /// A walkable cell is single file when the walkable run through it in
    /// one direction is narrower than `max_width_m`; its axis is the longer
    /// run. Door and passage cells are always single file, across the wall,
    /// and so is a chute as wide as the door reaching [`DOOR_APPROACH_M`]
    /// out from it on both sides, where people line up to go through.
    pub(crate) fn build(grid: &SimulationGrid, max_width_m: f64) -> Self {
        let columns = grid.columns as usize;
        let rows = grid.rows as usize;
        let walkable = grid
            .cells()
            .iter()
            .map(crate::GridCell::is_walkable)
            .collect::<Vec<_>>();
        let index = |column: usize, row: usize| row * columns + column;

        let mut run_x = vec![0_usize; walkable.len()];
        for row in 0..rows {
            let mut column = 0;
            while column < columns {
                if !walkable[index(column, row)] {
                    column += 1;
                    continue;
                }
                let start = column;
                while column < columns && walkable[index(column, row)] {
                    column += 1;
                }
                for run_column in start..column {
                    run_x[index(run_column, row)] = column - start;
                }
            }
        }
        let mut run_y = vec![0_usize; walkable.len()];
        for column in 0..columns {
            let mut row = 0;
            while row < rows {
                if !walkable[index(column, row)] {
                    row += 1;
                    continue;
                }
                let start = row;
                while row < rows && walkable[index(column, row)] {
                    row += 1;
                }
                for run_row in start..row {
                    run_y[index(column, run_row)] = row - start;
                }
            }
        }

        let axis_of = |cell: usize| {
            if run_x[cell] >= run_y[cell] {
                Axis::X
            } else {
                Axis::Y
            }
        };
        let mut axes = (0..walkable.len())
            .map(|cell| {
                let width_m = run_x[cell].min(run_y[cell]) as f64 * grid.cell_size_m;
                (walkable[cell] && width_m < max_width_m).then(|| axis_of(cell))
            })
            .collect::<Vec<_>>();

        // A doorway is single file only where a body fits through it. The
        // strips along its jambs are not: nobody standing there is in line.
        let fits = grid
            .cells()
            .iter()
            .map(|cell| grid.circle_is_walkable(cell.center_m, OCCUPANT_RADIUS_M))
            .collect::<Vec<_>>();
        let reach = (DOOR_APPROACH_M / grid.cell_size_m).ceil() as i64;
        let door_cells = grid
            .cells()
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.is_walkable() && cell.opening_id.is_some())
            .collect::<Vec<_>>();
        for &(cell_index, _) in &door_cells {
            axes[cell_index] = None;
        }
        let doors = door_cells
            .into_iter()
            .filter(|&(cell_index, _)| fits[cell_index])
            .map(|(cell_index, cell)| (cell.coordinate, axis_of(cell_index)))
            .collect::<Vec<_>>();
        for &(door, axis) in &doors {
            for offset in -reach..=reach {
                let (column, row) = match axis {
                    Axis::X => (i64::from(door.column) + offset, i64::from(door.row)),
                    Axis::Y => (i64::from(door.column), i64::from(door.row) + offset),
                };
                if column < 0 || row < 0 || column >= columns as i64 || row >= rows as i64 {
                    continue;
                }
                let cell = index(column as usize, row as usize);
                if fits[cell] && axes[cell].is_none() {
                    axes[cell] = Some(axis);
                }
            }
        }
        for &(door, axis) in &doors {
            axes[index(door.column as usize, door.row as usize)] = Some(axis);
        }
        Self { axes }
    }

    fn axis_at(&self, grid: &SimulationGrid, position: Vec3) -> Option<Axis> {
        let cell = grid.world_to_cell(position)?;
        self.axes
            .get(cell.row as usize * grid.columns as usize + cell.column as usize)
            .copied()
            .flatten()
    }
}

/// Where an agent wants to go this step.
#[derive(Debug, Clone)]
pub(crate) enum Goal {
    /// Stand still. The body still blocks others.
    Hold,
    /// Follow these cells; the first is where the agent stands.
    Path(Vec<GridCoordinate>),
    /// Walk downhill to an exit, and arrive on reaching an exit cell.
    Field(Arc<ExitField>),
}

#[derive(Debug, Clone)]
pub(crate) struct Intent {
    pub goal: Goal,
    pub speed_m_per_s: f64,
}

impl Intent {
    pub(crate) const HOLD: Self = Self {
        goal: Goal::Hold,
        speed_m_per_s: 0.0,
    };
}

/// One controller's agents and what they want, for one step.
pub(crate) struct CrowdGroup<'a> {
    pub agents: &'a mut [Agent],
    /// One per agent.
    pub intents: &'a [Intent],
    pub grids: &'a [SimulationGrid],
    /// One per grid.
    pub passages: &'a [PassageMap],
    /// Cells a body may not step into.
    pub hazards: &'a HazardMap,
    /// Clearance a body needs from walls and furniture.
    pub walk_radius_m: f64,
}

/// Contact state that lasts across steps.
#[derive(Debug)]
pub(crate) struct Crowd {
    config: CrowdConfig,
    seed: u64,
    substeps: u64,
    time_seconds: f64,
    states: Vec<BodyState>,
    permits: Vec<Permit>,
}

#[derive(Debug, Clone, Default)]
struct BodyState {
    /// Trying to move this sub-step.
    mobile: bool,
    /// Reached an exit cell this step; out of everyone's way.
    arrived: bool,
    /// Unit direction it last wanted to move in.
    direction: Option<(f64, f64)>,
    /// Seconds in a row it has been blocked by a body.
    blocked_seconds: f64,
    blocker: Option<usize>,
    /// Seconds since it last moved [`STALL_DISTANCE_M`].
    stalled_seconds: f64,
    stall_anchor: Option<Vec3>,
    /// A firefighter giving way stands still until then.
    hold_until: f64,
    path_progress: usize,
    /// How far it still has to go, for deciding who pushes whom. Lower is
    /// closer.
    remaining: u64,
}

/// Two bodies allowed to pass through each other while they squeeze past.
#[derive(Debug, Clone)]
struct Permit {
    mover: usize,
    other: usize,
    /// The mover's direction when the squeeze began. The squeeze is over
    /// once the other body is behind the mover along it, and clear of it.
    direction: (f64, f64),
    started_at: f64,
    /// Wedged and given up: the pair still overlap but may now only move
    /// apart, and the squeeze ends once they are clear.
    separating: bool,
}

/// One body for the length of a step.
#[derive(Debug, Clone, Copy)]
struct Body {
    group: usize,
    local: usize,
    grid: usize,
    floor: usize,
    radius_m: f64,
    firefighter: bool,
    /// Crew members let each other through quickly.
    crew: Option<usize>,
    /// Blocks other bodies.
    collidable: bool,
    /// Has collapsed: others step over it, slowly.
    fallen: bool,
}

impl Crowd {
    pub(crate) fn new(config: CrowdConfig, seed: u64) -> Self {
        Self {
            config,
            seed,
            substeps: 0,
            time_seconds: 0.0,
            states: Vec::new(),
            permits: Vec::new(),
        }
    }

    pub(crate) const fn config(&self) -> CrowdConfig {
        self.config
    }

    pub(crate) const fn seed(&self) -> u64 {
        self.seed
    }

    /// Moves every body of every group over `delta_seconds`, then records
    /// each agent's velocity and [`Movement`]. Groups must be passed in the
    /// same order with the same agents every step.
    pub(crate) fn step(&mut self, groups: &mut [CrowdGroup<'_>], delta_seconds: f64) {
        let mut floors = Vec::<String>::new();
        let mut crews = Vec::<String>::new();
        let mut bodies = Vec::new();
        let mut positions = Vec::new();
        for (group_index, group) in groups.iter().enumerate() {
            for (local, agent) in group.agents.iter().enumerate() {
                let floor = match floors.iter().position(|floor| *floor == agent.floor_id) {
                    Some(floor) => floor,
                    None => {
                        floors.push(agent.floor_id.clone());
                        floors.len() - 1
                    }
                };
                let grid = group
                    .grids
                    .iter()
                    .position(|grid| grid.floor_id == agent.floor_id)
                    .unwrap_or(0);
                let (firefighter, collidable, fallen) = match &agent.role {
                    AgentRole::Occupant(state) => (
                        false,
                        // Someone lying hurt is stepped over; someone being
                        // carried moves with the firefighter.
                        !matches!(
                            state.phase,
                            OccupantPhase::Exited
                                | OccupantPhase::Incapacitated
                                | OccupantPhase::Injured
                                | OccupantPhase::Carried
                        ),
                        matches!(
                            state.phase,
                            OccupantPhase::Incapacitated | OccupantPhase::Injured
                        ),
                    ),
                    // A crew staging outside isn't in anyone's way yet.
                    AgentRole::Firefighter(state) => (
                        true,
                        !matches!(
                            state.phase,
                            FirefighterPhase::Staging
                                | FirefighterPhase::Exited
                                | FirefighterPhase::Incapacitated
                        ),
                        state.phase == FirefighterPhase::Incapacitated,
                    ),
                };
                let crew = match &agent.role {
                    AgentRole::Firefighter(state) => {
                        Some(match crews.iter().position(|crew| *crew == state.crew_id) {
                            Some(crew) => crew,
                            None => {
                                crews.push(state.crew_id.clone());
                                crews.len() - 1
                            }
                        })
                    }
                    AgentRole::Occupant(_) => None,
                };
                bodies.push(Body {
                    group: group_index,
                    local,
                    grid,
                    floor,
                    radius_m: agent.radius_m,
                    firefighter,
                    crew,
                    collidable,
                    fallen,
                });
                positions.push(agent.position_m);
            }
        }
        if self.states.len() < bodies.len() {
            self.states.resize(bodies.len(), BodyState::default());
        }
        for state in &mut self.states {
            state.path_progress = 0;
            state.arrived = false;
        }
        self.permits
            .retain(|permit| bodies[permit.mover].collidable && bodies[permit.other].collidable);

        let start = positions.clone();
        if delta_seconds > 0.0 {
            let substeps = (delta_seconds / self.config.max_substep_seconds)
                .ceil()
                .max(1.0);
            let substep_seconds = delta_seconds / substeps;
            for _ in 0..substeps as usize {
                self.substep(groups, &bodies, &mut positions, substep_seconds);
            }
        }

        for (index, body) in bodies.iter().enumerate() {
            let movement = self.movement(index, body);
            let agent = &mut groups[body.group].agents[body.local];
            agent.position_m = positions[index];
            agent.velocity_m_per_s = if delta_seconds > 0.0 {
                Vec3 {
                    x: (positions[index].x - start[index].x) / delta_seconds,
                    y: (positions[index].y - start[index].y) / delta_seconds,
                    z: 0.0,
                }
            } else {
                ZERO
            };
            agent.movement = movement;
        }
    }

    fn movement(&self, index: usize, body: &Body) -> Movement {
        let state = &self.states[index];
        if !state.mobile || state.arrived {
            Movement::Stopped
        } else if body.firefighter && self.time_seconds < state.hold_until {
            Movement::Queued
        } else if self.in_permit(index) {
            Movement::Squeezing
        } else if state.blocked_seconds >= self.config.queued_after_seconds {
            Movement::Queued
        } else {
            Movement::Walking
        }
    }

    fn substep(
        &mut self,
        groups: &[CrowdGroup<'_>],
        bodies: &[Body],
        positions: &mut [Vec3],
        seconds: f64,
    ) {
        self.substeps += 1;
        let mut axes = bodies
            .iter()
            .zip(positions.iter())
            .map(|(body, &position)| axis_for(groups, body, position))
            .collect::<Vec<_>>();
        let buckets = Buckets::new(bodies, positions, &self.states);

        // The front of the crowd moves first, so space it leaves is there
        // for the people behind. Ties (and everyone, on the first step) go
        // in a repeatable shuffled order.
        let mut order = shuffled(bodies.len(), mix(self.seed ^ mix(self.substeps)));
        order.sort_by_key(|&index| (!bodies[index].firefighter, self.states[index].remaining));
        for index in order {
            self.move_body(
                index,
                &MoveContext {
                    groups,
                    bodies,
                    buckets: &buckets,
                    seconds,
                },
                positions,
                &mut axes,
            );
        }
        self.start_contests(bodies, &axes);
        self.end_permits(groups, bodies, positions, &axes);
        self.time_seconds += seconds;
    }

    fn move_body(
        &mut self,
        index: usize,
        context: &MoveContext<'_, '_>,
        positions: &mut [Vec3],
        axes: &mut [Option<Axis>],
    ) {
        let body = context.bodies[index];
        let group = &context.groups[body.group];
        let intent = &group.intents[body.local];
        let grid = &group.grids[body.grid];
        let position = positions[index];

        let wants_to_move = body.collidable
            && !self.states[index].arrived
            && intent.speed_m_per_s > 0.0
            && !matches!(intent.goal, Goal::Hold);
        if !wants_to_move {
            let state = &mut self.states[index];
            state.mobile = false;
            state.blocked_seconds = 0.0;
            state.blocker = None;
            state.stalled_seconds = 0.0;
            state.stall_anchor = None;
            state.direction = None;
            return;
        }
        self.states[index].mobile = true;
        if let Goal::Field(field) = &intent.goal
            && grid
                .world_to_cell(position)
                .is_some_and(|cell| field.is_exit(cell))
        {
            self.states[index].arrived = true;
            return;
        }
        if body.firefighter && self.time_seconds < self.states[index].hold_until {
            self.states[index].blocker = None;
            return;
        }

        let factor = self.speed_factor(index, context, positions);
        let step_m = intent.speed_m_per_s * factor * context.seconds;
        let target = match &intent.goal {
            Goal::Hold => None,
            Goal::Path(path) => path_target(
                grid,
                path,
                &mut self.states[index].path_progress,
                position,
                step_m,
            ),
            Goal::Field(field) => field_target(grid, field, position, step_m),
        };
        self.states[index].remaining = match &intent.goal {
            Goal::Hold => u64::MAX,
            Goal::Path(path) => {
                (path.len() - self.states[index].path_progress.min(path.len())) as u64
            }
            Goal::Field(field) => grid
                .world_to_cell(position)
                .and_then(|cell| field.cost(cell))
                .unwrap_or(u64::MAX),
        };
        let Some(target) = target else {
            // Already where it wants to be: standing, not queuing.
            let state = &mut self.states[index];
            state.mobile = false;
            state.direction = None;
            state.blocked_seconds = 0.0;
            state.blocker = None;
            state.stalled_seconds = 0.0;
            state.stall_anchor = None;
            return;
        };
        let desired = (target.x - position.x, target.y - position.y);
        let length = desired.0.hypot(desired.1);
        if length <= GAP_EPSILON_M {
            return;
        }
        let direction = (desired.0 / length, desired.1 / length);
        self.states[index].direction = Some(direction);

        let shift = |fraction: f64| Vec3 {
            x: position.x + desired.0 * fraction,
            y: position.y + desired.1 * fraction,
            z: position.z,
        };
        let (mut destination, mut moved, mut blocker) =
            match self.check(index, shift(1.0), context, positions, axes) {
                Ok(()) => (shift(1.0), length, None),
                Err(blocker) => {
                    // Close the gap: the farthest fraction of the move that's free.
                    let (mut free, mut taken) = (0.0, 1.0);
                    for _ in 0..6 {
                        let middle = f64::midpoint(free, taken);
                        if self
                            .check(index, shift(middle), context, positions, axes)
                            .is_ok()
                        {
                            free = middle;
                        } else {
                            taken = middle;
                        }
                    }
                    (shift(free), free * length, blocker)
                }
            };

        // Step around whoever is in the way. In a single-file passage bodies
        // are measured apart along it, so a sidestep there only lines a body
        // up with the doorway; it can never get it past anyone.
        if moved < BLOCKED_FRACTION * length {
            let blocker_on_left = blocker.is_some_and(|other| {
                let to_other = (
                    positions[other].x - position.x,
                    positions[other].y - position.y,
                );
                direction.0 * to_other.1 - direction.1 * to_other.0 > 0.0
            });
            let away = if blocker_on_left { -1.0 } else { 1.0 };
            let yielding = !body.firefighter
                && self.states[index].blocked_seconds >= self.config.room_yield_after_seconds;
            let angles: &[f64] = if yielding {
                &YIELD_SIDESTEP_DEGREES
            } else {
                &SIDESTEP_DEGREES
            };
            for &degrees in angles {
                let (sin, cos) = (away * degrees).to_radians().sin_cos();
                let side = (
                    direction.0 * cos - direction.1 * sin,
                    direction.0 * sin + direction.1 * cos,
                );
                let side_step = length * SIDESTEP_FRACTION;
                let candidate = Vec3 {
                    x: position.x + side.0 * side_step,
                    y: position.y + side.1 * side_step,
                    z: position.z,
                };
                if self
                    .check(index, candidate, context, positions, axes)
                    .is_ok()
                {
                    destination = candidate;
                    moved = side_step;
                    blocker = None;
                    break;
                }
            }
        }

        // Still stuck: shove a lower-priority body that isn't in a
        // single-file line aside, just enough to take the step, if it has
        // room to give. Nobody in a line is shoved; there they squeeze past.
        if moved < BLOCKED_FRACTION * length
            && let Some(other) = blocker
            && axes[other].is_none()
            && self.outranks(index, other, context.bodies)
            && self.push(index, other, shift(1.0), context, positions, axes)
        {
            destination = shift(1.0);
            moved = length;
            blocker = None;
        }

        positions[index] = destination;
        axes[index] = axis_for(context.groups, &body, destination);
        let state = &mut self.states[index];
        if moved < BLOCKED_FRACTION * length && blocker.is_some() {
            state.blocked_seconds += context.seconds;
            state.blocker = blocker;
        } else {
            state.blocked_seconds = 0.0;
            state.blocker = None;
        }
        match state.stall_anchor {
            Some(anchor) if distance_2d(anchor, destination) < STALL_DISTANCE_M => {
                state.stalled_seconds += context.seconds;
            }
            _ => {
                state.stall_anchor = Some(destination);
                state.stalled_seconds = 0.0;
            }
        }
        if let Goal::Field(field) = &intent.goal
            && grid
                .world_to_cell(destination)
                .is_some_and(|cell| field.is_exit(cell))
        {
            state.arrived = true;
        }
    }

    /// Whether body `index` may stand at `candidate`. On failure, the body in
    /// the way, or `None` for a wall or flames.
    fn check(
        &self,
        index: usize,
        candidate: Vec3,
        context: &MoveContext<'_, '_>,
        positions: &[Vec3],
        axes: &[Option<Axis>],
    ) -> Result<(), Option<usize>> {
        let body = context.bodies[index];
        let group = &context.groups[body.group];
        let grid = &group.grids[body.grid];
        if !grid.circle_is_walkable(candidate, group.walk_radius_m) {
            return Err(None);
        }
        let from = grid.world_to_cell(positions[index]);
        let to = grid.world_to_cell(candidate);
        let blocked = |cell: Option<GridCoordinate>| {
            cell.is_some_and(|cell| {
                group.hazards.get(&grid.floor_id, cell) == Some(Hazard::Blocked)
            })
        };
        // A body caught by spreading fire may always step out of it.
        if to != from && blocked(to) && !blocked(from) {
            return Err(None);
        }

        let candidate_axis = group
            .passages
            .get(body.grid)
            .and_then(|passages| passages.axis_at(grid, candidate));
        let reach = if candidate_axis.is_some() {
            PASSAGE_REACH_M
        } else {
            CONTACT_REACH_M
        };
        let mut worst: Option<(f64, usize)> = None;
        for other in context.buckets.near(body.floor, candidate, reach) {
            let other_body = context.bodies[other];
            if other == index || !other_body.collidable || self.states[other].arrived {
                continue;
            }
            let needed = body.radius_m + other_body.radius_m;
            // Cheap rejection before measuring along passages.
            let apart = distance_2d(candidate, positions[other]);
            let same_passage = candidate_axis.is_some() && candidate_axis == axes[other];
            if (apart >= needed - GAP_EPSILON_M && !same_passage)
                || apart > PASSAGE_REACH_M
                || self.permitted(index, other)
            {
                continue;
            }
            let new_gap = gap(
                grid,
                candidate,
                candidate_axis,
                positions[other],
                axes[other],
            );
            // People pressed together in an open crowd give a little; in a
            // single-file passage they keep their spacing. Whoever goes first
            // (someone already in line, a firefighter, or whoever is closer
            // to where they're going) may press harder, so the front of a
            // crowd edges into a doorway instead of wedging in an arch.
            let give = if same_passage {
                0.0
            } else if (axes[index].is_some() && axes[other].is_none())
                || self.outranks(index, other, context.bodies)
            {
                2.0 * CROWD_GIVE_M
            } else {
                CROWD_GIVE_M
            };
            if new_gap >= needed - give - GAP_EPSILON_M {
                continue;
            }
            // Bodies already too close (separating after a squeeze) may
            // still move apart, never closer.
            let old_gap = gap(
                grid,
                positions[index],
                axes[index],
                positions[other],
                axes[other],
            );
            if new_gap >= old_gap - GAP_EPSILON_M {
                continue;
            }
            if worst.is_none_or(|(worst_gap, _)| new_gap < worst_gap) {
                worst = Some((new_gap, other));
            }
        }
        worst.map_or(Ok(()), |(_, other)| Err(Some(other)))
    }

    fn speed_factor(&self, index: usize, context: &MoveContext<'_, '_>, positions: &[Vec3]) -> f64 {
        let body = context.bodies[index];
        let position = positions[index];
        let mut factor = 1.0;
        if self.in_permit(index) {
            factor *= if body.firefighter {
                self.config.firefighter_squeeze_speed_factor
            } else {
                self.config.occupant_squeeze_speed_factor
            };
        }
        let mut stepping_over = false;
        let mut oncoming = 0;
        let direction = self.states[index].direction;
        for other in context
            .buckets
            .near(body.floor, position, COUNTERFLOW_RADIUS_M)
        {
            if other == index {
                continue;
            }
            let other_body = context.bodies[other];
            let distance = distance_2d(position, positions[other]);
            if other_body.fallen && distance < body.radius_m + other_body.radius_m {
                stepping_over = true;
            }
            if body.firefighter
                && !other_body.firefighter
                && other_body.collidable
                && distance < COUNTERFLOW_RADIUS_M
                && let (Some(mine), Some(theirs)) = (direction, self.states[other].direction)
                && self.states[other].mobile
                && dot(mine, theirs) < COUNTERFLOW_DOT
            {
                oncoming += 1;
            }
        }
        if stepping_over {
            factor *= self.config.fallen_speed_factor;
        }
        if oncoming >= COUNTERFLOW_MIN_PEOPLE {
            factor *= self.config.counterflow_speed_factor;
        }
        factor
    }

    /// Grants squeezes to bodies that have been blocked long enough.
    fn start_contests(&mut self, bodies: &[Body], axes: &[Option<Axis>]) {
        for index in 0..bodies.len() {
            let Some(other) = self.states[index].blocker else {
                continue;
            };
            if !self.states[index].mobile || self.permitted(index, other) {
                continue;
            }
            // In an open crowd, occupants squeeze past one person at a time:
            // pressing past several at once would pile them up. In a passage
            // people squeeze past a whole line, one after another, and a
            // firefighter forces through as many people as are in the way.
            let in_passage = axes[index].is_some() || axes[other].is_some();
            let mixed = bodies[index].firefighter != bodies[other].firefighter;
            if !in_passage && !mixed && (self.in_permit(index) || self.in_permit(other)) {
                continue;
            }
            let mine = &self.states[index];
            let theirs = &self.states[other];
            let head_on = theirs.blocker == Some(index)
                || (theirs.mobile
                    && matches!((mine.direction, theirs.direction), (Some(a), Some(b)) if dot(a, b) < HEAD_ON_DOT));
            let crewmates =
                bodies[index].crew.is_some() && bodies[index].crew == bodies[other].crew;
            let contest = if mixed {
                mine.blocked_seconds >= self.config.firefighter_contest_after_seconds
            } else if crewmates {
                mine.blocked_seconds >= self.config.queued_after_seconds
            } else {
                // Squeeze past someone coming the other way (or pressing
                // against this body), or standing still in the way; queue
                // behind someone walking ahead.
                (head_on || !theirs.mobile)
                    && mine.blocked_seconds >= self.config.head_on_squeeze_after_seconds
            };
            let deadlock = mine.stalled_seconds >= self.config.deadlock_after_seconds
                && self.blocked_in_a_circle(index);
            if !contest && !deadlock {
                continue;
            }
            if mixed && contest {
                let firefighter = if bodies[index].firefighter {
                    index
                } else {
                    other
                };
                let roll = unit(self.seed, self.substeps, index as u64, other as u64);
                if roll < self.config.firefighter_yield_probability {
                    self.states[firefighter].hold_until =
                        self.time_seconds + self.config.firefighter_yield_seconds;
                }
            }
            let direction = self.states[index].direction.unwrap_or((1.0, 0.0));
            self.permits.push(Permit {
                mover: index,
                other,
                direction,
                started_at: self.time_seconds,
                separating: false,
            });
            for body in [index, other] {
                self.states[body].blocked_seconds = 0.0;
                self.states[body].blocker = None;
                self.states[body].stalled_seconds = 0.0;
                self.states[body].stall_anchor = None;
            }
        }
    }

    /// Ends squeezes whose pair is clear of each other, once past each other
    /// or after [`PERMIT_TIMEOUT_SECONDS`]. A pair still overlapping keeps
    /// its permit, so neither body is ever left inside another.
    fn end_permits(
        &mut self,
        groups: &[CrowdGroup<'_>],
        bodies: &[Body],
        positions: &[Vec3],
        axes: &[Option<Axis>],
    ) {
        let now = self.time_seconds;
        let states = &self.states;
        self.permits.retain_mut(|permit| {
            let (mover, other) = (permit.mover, permit.other);
            if states[mover].arrived || states[other].arrived {
                return false;
            }
            let body = bodies[mover];
            let grid = &groups[body.group].grids[body.grid];
            let clear = gap(
                grid,
                positions[mover],
                axes[mover],
                positions[other],
                axes[other],
            ) >= body.radius_m + bodies[other].radius_m;
            let expired = now - permit.started_at > PERMIT_TIMEOUT_SECONDS;
            if !clear {
                // A pair wedged in place gives up; they then only ever move
                // apart from each other.
                let wedged = states[mover].stalled_seconds >= STALL_RELEASE_SECONDS
                    && states[other].stalled_seconds >= STALL_RELEASE_SECONDS;
                permit.separating |= expired && wedged;
                return true;
            }
            if permit.separating {
                return false;
            }
            let offset = (
                positions[other].x - positions[mover].x,
                positions[other].y - positions[mover].y,
            );
            let passed = dot(offset, permit.direction) < 0.0;
            !(passed || expired)
        });
    }

    /// Who goes first when two bodies are in each other's way: firefighters
    /// before occupants, then whoever is closer to where they're going.
    fn outranks(&self, index: usize, other: usize, bodies: &[Body]) -> bool {
        match (bodies[index].firefighter, bodies[other].firefighter) {
            (true, false) => true,
            (false, true) => false,
            _ => (self.states[index].remaining, index) < (self.states[other].remaining, other),
        }
    }

    /// Moves body `other` straight away from where `index` wants to stand,
    /// far enough to make room, if `other` can stand there. Returns whether
    /// it did and `index` can now take its step.
    fn push(
        &self,
        index: usize,
        other: usize,
        destination: Vec3,
        context: &MoveContext<'_, '_>,
        positions: &mut [Vec3],
        axes: &mut [Option<Axis>],
    ) -> bool {
        let needed = context.bodies[index].radius_m + context.bodies[other].radius_m;
        let offset = (
            positions[other].x - destination.x,
            positions[other].y - destination.y,
        );
        let apart = offset.0.hypot(offset.1);
        let shove = needed - CROWD_GIVE_M - apart + PUSH_MARGIN_M;
        if apart <= GAP_EPSILON_M || shove <= 0.0 || shove > MAX_PUSH_M {
            return false;
        }
        let original = positions[other];
        let pushed = Vec3 {
            x: original.x + offset.0 / apart * shove,
            y: original.y + offset.1 / apart * shove,
            z: original.z,
        };
        if self.check(other, pushed, context, positions, axes).is_err() {
            return false;
        }
        positions[other] = pushed;
        if self
            .check(index, destination, context, positions, axes)
            .is_ok()
        {
            axes[other] = axis_for(context.groups, &context.bodies[other], pushed);
            true
        } else {
            positions[other] = original;
            false
        }
    }

    /// Whether following who blocks whom from `index` comes back around.
    fn blocked_in_a_circle(&self, index: usize) -> bool {
        let mut seen = vec![index];
        let mut current = index;
        while let Some(next) = self.states[current].blocker {
            if next == index {
                return true;
            }
            if seen.contains(&next) || seen.len() > 64 {
                return false;
            }
            seen.push(next);
            current = next;
        }
        false
    }

    /// Whether two bodies may pass through each other right now.
    fn permitted(&self, left: usize, right: usize) -> bool {
        self.permits.iter().any(|permit| {
            !permit.separating
                && ((permit.mover == left && permit.other == right)
                    || (permit.mover == right && permit.other == left))
        })
    }

    fn in_permit(&self, index: usize) -> bool {
        self.permits
            .iter()
            .any(|permit| permit.mover == index || permit.other == index)
    }
}

struct MoveContext<'a, 'g> {
    groups: &'a [CrowdGroup<'g>],
    bodies: &'a [Body],
    buckets: &'a Buckets,
    seconds: f64,
}

/// Bodies by floor and coarse square, for finding who is near.
struct Buckets {
    cells: HashMap<(usize, i64, i64), Vec<usize>>,
}

impl Buckets {
    fn new(bodies: &[Body], positions: &[Vec3], states: &[BodyState]) -> Self {
        let mut cells = HashMap::<_, Vec<usize>>::new();
        for (index, body) in bodies.iter().enumerate() {
            if (!body.collidable && !body.fallen) || states[index].arrived {
                continue;
            }
            let (x, y) = bucket(positions[index]);
            cells.entry((body.floor, x, y)).or_default().push(index);
        }
        Self { cells }
    }

    /// Bodies in the buckets within `reach_m` of `position`, and maybe a
    /// few farther away.
    fn near(&self, floor: usize, position: Vec3, reach_m: f64) -> impl Iterator<Item = usize> + '_ {
        let (x, y) = bucket(position);
        let span = (reach_m / BUCKET_SIZE_M).ceil() as i64;
        (-span..=span).flat_map(move |dy| {
            (-span..=span).flat_map(move |dx| {
                self.cells
                    .get(&(floor, x + dx, y + dy))
                    .into_iter()
                    .flatten()
                    .copied()
            })
        })
    }
}

fn bucket(position: Vec3) -> (i64, i64) {
    (
        (position.x / BUCKET_SIZE_M).floor() as i64,
        (position.y / BUCKET_SIZE_M).floor() as i64,
    )
}

fn axis_for(groups: &[CrowdGroup<'_>], body: &Body, position: Vec3) -> Option<Axis> {
    let group = &groups[body.group];
    let grid = group.grids.get(body.grid)?;
    group.passages.get(body.grid)?.axis_at(grid, position)
}

/// Distance between two bodies' centers. Inside one single-file passage it
/// is measured along the passage only, so bodies can't stand side by side.
fn gap(
    grid: &SimulationGrid,
    left: Vec3,
    left_axis: Option<Axis>,
    right: Vec3,
    right_axis: Option<Axis>,
) -> f64 {
    match (left_axis, right_axis) {
        (Some(axis), Some(other)) if axis == other && same_passage(grid, left, right) => match axis
        {
            Axis::X => (left.x - right.x).abs(),
            Axis::Y => (left.y - right.y).abs(),
        },
        _ => distance_2d(left, right),
    }
}

/// Whether two points close together see each other across open floor,
/// rather than through the wall between two parallel passages.
fn same_passage(grid: &SimulationGrid, left: Vec3, right: Vec3) -> bool {
    let distance = distance_2d(left, right);
    if distance > SINGLE_FILE_MAX_WIDTH_M + 1.0 {
        return false;
    }
    let samples = (distance / (grid.cell_size_m / 2.0)).ceil().max(1.0) as usize;
    (0..=samples).all(|sample| {
        let fraction = sample as f64 / samples as f64;
        let point = Vec3 {
            x: left.x + (right.x - left.x) * fraction,
            y: left.y + (right.y - left.y) * fraction,
            z: left.z,
        };
        grid.world_to_cell(point)
            .and_then(|cell| grid.cell(cell))
            .is_some_and(crate::GridCell::is_walkable)
    })
}

/// Where a body walking `step_m` along `path` ends up, if it has anywhere
/// left to go. Cuts corners within a cell rather than visiting each center.
fn path_target(
    grid: &SimulationGrid,
    path: &[GridCoordinate],
    progress: &mut usize,
    position: Vec3,
    step_m: f64,
) -> Option<Vec3> {
    // Catch up with the path when the body has moved (or been moved) along it.
    if let Some(cell) = grid.world_to_cell(position)
        && let Some(offset) = path[(*progress).min(path.len())..]
            .iter()
            .position(|&step| step == cell)
    {
        *progress += offset;
    }
    let mut point = position;
    let mut remaining = step_m;
    let mut next = *progress + 1;
    while remaining > 0.0 {
        let Some(center) = path.get(next).and_then(|&cell| grid.cell_center(cell)) else {
            break;
        };
        let distance = distance_2d(point, center);
        if distance > remaining {
            point.x += (center.x - point.x) * remaining / distance;
            point.y += (center.y - point.y) * remaining / distance;
            break;
        }
        point.x = center.x;
        point.y = center.y;
        remaining -= distance;
        next += 1;
    }
    // At the last cell, finish at its center rather than at its edge.
    if next >= path.len()
        && remaining > 0.0
        && let Some(center) = path.last().and_then(|&cell| grid.cell_center(cell))
    {
        let distance = distance_2d(point, center);
        let fraction = if distance > remaining {
            remaining / distance
        } else {
            1.0
        };
        point.x += (center.x - point.x) * fraction;
        point.y += (center.y - point.y) * fraction;
    }
    (distance_2d(point, position) > GAP_EPSILON_M).then_some(point)
}

/// Where a body walking `step_m` downhill on `field` ends up, if it can move.
fn field_target(
    grid: &SimulationGrid,
    field: &ExitField,
    position: Vec3,
    step_m: f64,
) -> Option<Vec3> {
    let mut point = position;
    let mut remaining = step_m;
    while remaining > 0.0 {
        let Some(cell) = grid.world_to_cell(point) else {
            break;
        };
        if field.is_exit(cell) {
            break;
        }
        let Some(center) = field
            .next_cell(cell)
            .and_then(|next| grid.cell_center(next))
        else {
            break;
        };
        let distance = distance_2d(point, center);
        if distance > remaining {
            point.x += (center.x - point.x) * remaining / distance;
            point.y += (center.y - point.y) * remaining / distance;
            break;
        }
        point.x = center.x;
        point.y = center.y;
        remaining -= distance;
    }
    (distance_2d(point, position) > GAP_EPSILON_M).then_some(point)
}

fn dot(left: (f64, f64), right: (f64, f64)) -> f64 {
    left.0 * right.0 + left.1 * right.1
}

/// SplitMix64 finalizer: a well-mixed hash for repeatable randomness.
fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A repeatable number in `[0, 1)` for these inputs.
pub(crate) fn unit(seed: u64, substep: u64, left: u64, right: u64) -> f64 {
    let hash = mix(seed ^ mix(substep ^ mix(left ^ mix(right.wrapping_add(1)))));
    (hash >> 11) as f64 / (1_u64 << 53) as f64
}

/// `0..count` in a repeatable shuffled order, so no body always moves first.
fn shuffled(count: usize, seed: u64) -> Vec<usize> {
    let mut order = (0..count).collect::<Vec<_>>();
    for index in (1..count).rev() {
        let pick = (mix(seed ^ mix(index as u64)) % (index as u64 + 1)) as usize;
        order.swap(index, pick);
    }
    order
}

const ZERO: Vec3 = Vec3 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrow_hallway_is_single_file_along_its_length() {
        let grid = SimulationGrid::from_rows(
            "floor-1",
            &["##########", "..........", "..........", "##########"],
            0.25,
        );
        let passages = PassageMap::build(&grid, SINGLE_FILE_MAX_WIDTH_M);
        let middle = grid
            .cell_center(GridCoordinate { column: 5, row: 1 })
            .unwrap();
        assert_eq!(passages.axis_at(&grid, middle), Some(Axis::X));
    }

    #[test]
    fn an_open_room_is_not_single_file() {
        let grid = SimulationGrid::test_grid("floor-1", 12, 12, 0.25);
        let passages = PassageMap::build(&grid, SINGLE_FILE_MAX_WIDTH_M);
        let middle = grid
            .cell_center(GridCoordinate { column: 6, row: 6 })
            .unwrap();
        assert_eq!(passages.axis_at(&grid, middle), None);
    }

    #[test]
    fn a_door_and_its_approach_are_single_file_across_the_wall() {
        let mut rows = vec![".".repeat(12); 21];
        rows[10] = "#####DDD####".to_owned();
        let rows = rows.iter().map(String::as_str).collect::<Vec<_>>();
        let grid = SimulationGrid::from_rows("floor-1", &rows, 0.25);
        let passages = PassageMap::build(&grid, SINGLE_FILE_MAX_WIDTH_M);
        let axis = |column, row| {
            passages.axis_at(
                &grid,
                grid.cell_center(GridCoordinate { column, row }).unwrap(),
            )
        };
        for row in [8, 10, 12] {
            assert_eq!(axis(6, row), Some(Axis::Y), "row {row}");
        }
        assert_eq!(axis(6, 2), None, "far from the door");
        assert_eq!(axis(9, 9), None, "the chute is only door-wide");
    }

    #[test]
    fn bodies_in_one_passage_are_measured_along_it() {
        let grid = SimulationGrid::from_rows(
            "floor-1",
            &["##########", "..........", "..........", "##########"],
            0.25,
        );
        let left = Vec3 {
            x: 1.0,
            y: 0.3,
            z: 0.0,
        };
        let right = Vec3 {
            x: 1.1,
            y: 0.7,
            z: 0.0,
        };
        let along = gap(&grid, left, Some(Axis::X), right, Some(Axis::X));
        assert!((along - 0.1).abs() < 1.0e-9);
        assert!(gap(&grid, left, None, right, Some(Axis::X)) > 0.4);
    }

    fn walker(id: &str, x: f64, y: f64, firefighter: bool) -> Agent {
        use crate::{Exposure, FirefighterState, OccupantState};
        Agent {
            id: id.to_owned(),
            floor_id: "floor-1".to_owned(),
            position_m: Vec3 { x, y, z: 0.0 },
            velocity_m_per_s: ZERO,
            radius_m: if firefighter { 0.3 } else { 0.25 },
            exposure: Exposure::NONE,
            movement: Movement::Stopped,
            role: if firefighter {
                AgentRole::Firefighter(FirefighterState {
                    crew_id: "engine-1".to_owned(),
                    phase: FirefighterPhase::Advancing,
                    walking_speed_m_per_s: 1.0,
                    entry_opening_id: "exit".to_owned(),
                    arrival_time_seconds: 0.0,
                    target_fire_node_id: None,
                    nozzle_target_m: None,
                })
            } else {
                AgentRole::Occupant(OccupantState {
                    phase: OccupantPhase::MovingToExit,
                    injured: false,
                    walking_speed_m_per_s: 1.2,
                    pre_movement_delay_seconds: 0.0,
                    target_exit_id: None,
                })
            },
        }
    }

    /// Walks two bodies toward each other's end of a 1 m wide, 6 m long
    /// corridor. Returns where they are after `seconds`, whether they
    /// squeezed, and when the east one reached the west end.
    fn meet_head_on(firefighter: bool, seconds: f64) -> (Vec3, Vec3, bool, Option<f64>) {
        let grid = SimulationGrid::from_rows(
            "floor-1",
            &[
                "########################",
                "........................",
                "........................",
                "........................",
                "........................",
                "########################",
            ],
            0.25,
        );
        let grids = [grid];
        let passages = [PassageMap::build(&grids[0], SINGLE_FILE_MAX_WIDTH_M)];
        let row = |column: u32| GridCoordinate { column, row: 2 };
        let east = (1..23).map(row).collect::<Vec<_>>();
        let west = (1..23).rev().map(row).collect::<Vec<_>>();
        let mut agents = [
            walker("a", 0.375, 0.625, false),
            walker("b", 5.625, 0.625, firefighter),
        ];
        let intents = [
            Intent {
                goal: Goal::Path(east),
                speed_m_per_s: 1.2,
            },
            Intent {
                goal: Goal::Path(west),
                speed_m_per_s: 1.0,
            },
        ];
        let hazards = HazardMap::default();
        let mut crowd = Crowd::new(CrowdConfig::default(), 3);
        let mut squeezed = false;
        let mut east_arrival = None;
        for frame in 1..=(seconds / 0.5) as usize {
            let mut groups = [CrowdGroup {
                agents: &mut agents,
                intents: &intents,
                grids: &grids,
                passages: &passages,
                hazards: &hazards,
                walk_radius_m: 0.25,
            }];
            crowd.step(&mut groups, 0.5);
            squeezed |= agents
                .iter()
                .any(|agent| agent.movement == Movement::Squeezing);
            if east_arrival.is_none() && agents[1].position_m.x < 0.5 {
                east_arrival = Some(frame as f64 * 0.5);
            }
            let gap = distance_2d(agents[0].position_m, agents[1].position_m);
            assert!(
                squeezed || gap >= 0.5 - 1.0e-6,
                "overlapped by {gap:.2} m without squeezing"
            );
        }
        (
            agents[0].position_m,
            agents[1].position_m,
            squeezed,
            east_arrival,
        )
    }

    #[test]
    fn people_meeting_head_on_in_a_corridor_squeeze_past() {
        let (west, east, squeezed, _) = meet_head_on(false, 20.0);
        assert!(squeezed);
        assert!(west.x > 5.0, "the west walker reached {:.2}", west.x);
        assert!(east.x < 1.0, "the east walker reached {:.2}", east.x);
    }

    #[test]
    fn a_firefighter_squeezes_past_but_is_slowed() {
        let (occupant, firefighter, squeezed, arrival) = meet_head_on(true, 20.0);
        assert!(squeezed);
        assert!(occupant.x > 5.0 && firefighter.x < 1.0);
        // Alone, 5.25 m at 1 m/s takes 5.25 s.
        let arrival = arrival.expect("the firefighter gets through");
        assert!(arrival > 5.25 + 1.0, "arrived at {arrival} s");
    }

    #[test]
    fn firefighters_yield_about_as_often_as_configured() {
        let probability = CrowdConfig::default().firefighter_yield_probability;
        let trials = 10_000;
        let yields = (0..trials)
            .filter(|&trial| unit(0, trial, 3, 7) < probability)
            .count();
        let rate = yields as f64 / trials as f64;
        assert!(
            (rate - probability).abs() < 0.02,
            "yielded {rate:.3} of the time"
        );
    }

    #[test]
    fn shuffles_are_repeatable_permutations() {
        let order = shuffled(20, 7);
        assert_eq!(order, shuffled(20, 7));
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>());
        assert_ne!(order, shuffled(20, 8));
    }

    #[test]
    fn default_config_is_valid() {
        assert_eq!(CrowdConfig::default().validate(), Ok(()));
        let bad = CrowdConfig {
            firefighter_yield_probability: 1.5,
            ..CrowdConfig::default()
        };
        assert!(bad.validate().is_err());
    }
}
