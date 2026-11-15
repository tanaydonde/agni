//! Fire simulation endpoints. Both advance the surface fire and any supplied
//! occupants together, frame by frame (`Rehearsal`). `/api/fire/simulate`
//! returns that timeline as is; `/api/fire/simulate/hybrid` also feeds the
//! fire timeline into CFAST and returns the merged renderer contract.

use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tflop_cfast::{
    CfastOpeningState, CfastRunner, CfastScenario, SurfaceFireAdapterConfig,
    adapt_surface_fire_for_cfast, merge_cfast_with_surface_fire, prepare_cfast_scene,
    resolve_cfast_bin,
};
use tflop_simulation_core::{
    AgentFrame, AlarmConfig, CombustionState, CrewSpawn, Evacuation, FireEnsembleConfig,
    FireFrequencyCell, FireHeatmapCell, FireMaterialCatalog, FireRepresentative,
    FireRepresentativeAssignment, Firefighting, FirefightingConfig, OccupantSpawn, Rehearsal,
    RehearsalError, RehearsalFrame, SIMULATION_RESULTS_SCHEMA_VERSION, SimulationConfig,
    SimulationEngine, SimulationResults, SurfaceFireConfig, SurfaceFireError, SurfaceFireWorld,
    analyze_fire_ensemble, choose_injured,
};
// The same types `GET /api/property` suggests values in.
use tflop_costar::{AlarmTimings, FloorMaterial};
use tflop_world_core::{Scene, resolve_opening_connections};

use crate::{SharedProperty, SharedWorld, decode, lock};

const DEFAULT_DURATION_SECONDS: f64 = 180.0;
const MAX_DURATION_SECONDS: f64 = 600.0;
/// Half-second frames keep crowd movement smooth in playback.
const DEFAULT_STEP_SECONDS: f64 = 0.5;
const MIN_STEP_SECONDS: f64 = 0.1;
const CFAST_OUTPUT_INTERVAL_SECONDS: f64 = 5.0;
/// Keeps one request from monopolizing the server or producing an unbounded
/// response. A later job API can raise this without holding an HTTP request.
const MAX_SIMULATIONS_PER_BATCH: usize = 32;
/// Surface fire and CFAST are CPU-heavy. More workers than this usually adds
/// contention rather than throughput, even on machines with many cores.
const MAX_CONCURRENT_SIMULATIONS: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SimulateFireRequest {
    object_id: String,
    #[serde(default)]
    duration_seconds: Option<f64>,
    #[serde(default)]
    step_seconds: Option<f64>,
    #[serde(default)]
    occupants: Vec<OccupantSpawn>,
    /// Firefighter crews. They act on the same fire timeline, so their water
    /// changes how the fire spreads.
    #[serde(default)]
    crews: Vec<CrewSpawn>,
    /// When occupants learn about the fire.
    #[serde(default)]
    alarm: AlarmMode,
    /// Custom `detected` alarm timings, e.g. slower notice by sleepers
    /// (`GET /api/property` derives them from the CoStar record). Ignored
    /// when `alarm` is `immediate`.
    #[serde(default)]
    alarm_config: Option<AlarmTimings>,
    /// Floor finish: `carpet` (default) or `hardwood`.
    #[serde(default)]
    floor_material: Option<FloorMaterial>,
    /// Share of occupants who start the run injured and must be carried
    /// out. Chosen at random by `seed`.
    #[serde(default)]
    injured_fraction: Option<f64>,
    /// Repeatable randomness for the run: who is injured, exit choices, and
    /// crowd contests.
    #[serde(default)]
    seed: u64,
}

const DEFAULT_INJURED_FRACTION: f64 = 0.1;

impl SimulateFireRequest {
    /// Fills settings the request leaves out from the scan's CoStar profile,
    /// computed once at startup, so a plain client (the web app's Run button)
    /// gets the building's alarm timing, assistance share and floor finish.
    /// Values the request sends always win.
    fn fill_from_property(&mut self, property: &SharedProperty) {
        let Some(brief) = property.as_ref() else {
            return;
        };
        let profile = &brief.profile;
        self.alarm_config.get_or_insert(profile.alarm.value);
        self.injured_fraction
            .get_or_insert(profile.injured_fraction.value);
        self.floor_material
            .get_or_insert(profile.floor_material.value);
    }
}

/// The material catalog for a run, with the requested floor finish.
fn material_catalog(floor: Option<FloorMaterial>) -> FireMaterialCatalog {
    let mut catalog = FireMaterialCatalog::default();
    if floor == Some(FloorMaterial::Hardwood) {
        catalog.floor = FireMaterialCatalog::hardwood_floor();
    }
    catalog
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AlarmMode {
    /// Someone near a noticeable fire raises the alarm; it spreads from them.
    #[default]
    Detected,
    /// Everyone knows from ignition.
    Immediate,
}

#[derive(Debug, Serialize)]
struct SimulateFireResponse {
    /// World revision the scene snapshot was taken at.
    revision: u64,
    /// Objects left out of the simulation because they don't stand in a room.
    excluded_object_ids: Vec<String>,
    results: SimulationResults,
    /// Everyone's position at every simulation step, sent when `results.frames`
    /// are thinned for playback (`response_interval_seconds`). Agents are small
    /// next to the fire, and a straight line between thinned frames cuts
    /// corners through walls; at full resolution it follows the doorways.
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_frames: Option<Vec<AgentTimelineFrame>>,
}

#[derive(Debug, Serialize)]
struct AgentTimelineFrame {
    time_seconds: f64,
    agents: Vec<AgentFrame>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SimulateNRequest {
    /// Complete, distinct inputs for the existing single-run simulation.
    simulations: Vec<SimulateFireRequest>,
    /// When present, analyze successful runs and select this many diverse
    /// representative fire timelines.
    #[serde(default)]
    top_k: Option<usize>,
    /// Keep only the selected representative timelines in the response. The
    /// server still runs and ranks the complete batch; this only avoids
    /// sending unused timelines and analysis internals to the browser.
    #[serde(default)]
    representatives_only: bool,
    /// Optional spacing between frames returned for representative playback.
    /// Simulation and ranking still use the original full-resolution frames.
    #[serde(default)]
    response_interval_seconds: Option<f64>,
}

#[derive(Debug, Serialize)]
struct SimulateNResponse {
    /// Every run uses this snapshot of the world.
    revision: u64,
    /// Always ordered like the request, even though runs execute concurrently.
    runs: Vec<SimulateNRun>,
    /// Present when the request supplied `top_k` and at least one run
    /// completed. Representative indexes refer to the original input batch
    /// and match each returned run's `index` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    fire_ensemble: Option<BatchFireEnsembleAnalysis>,
}

#[derive(Debug, Serialize)]
struct BatchFireEnsembleAnalysis {
    requested_top_k: usize,
    completed_simulation_count: usize,
    config: FireEnsembleConfig,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    frequencies: Vec<FireFrequencyCell>,
    /// Fire summed over time and height, per floor-plan cell. Small, so it is
    /// kept even when `representatives_only` drops `frequencies`.
    heatmap: Vec<FireHeatmapCell>,
    representatives: Vec<FireRepresentative>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    assignments: Vec<FireRepresentativeAssignment>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SimulateNRun {
    Completed {
        index: usize,
        simulation: SimulateFireResponse,
    },
    Failed {
        index: usize,
        http_status: u16,
        error: serde_json::Value,
    },
}

pub(crate) async fn simulate(
    State(world): State<SharedWorld>,
    Extension(property): Extension<SharedProperty>,
    body: Result<Json<SimulateFireRequest>, JsonRejection>,
) -> Response {
    let mut request = match decode(body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    request.fill_from_property(&property);
    // Snapshot and release the lock: the run can take a moment and must not
    // block edits.
    let snapshot = lock(&world).get_scene();
    let result =
        tokio::task::spawn_blocking(move || run(snapshot.scene, snapshot.revision, &request)).await;
    match result {
        Ok(Ok(response)) => Json(response).into_response(),
        Ok(Err((status, body))) => (status, Json(body)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "code": "internal", "message": error.to_string() })),
        )
            .into_response(),
    }
}

/// Runs spatial surface fire, derives room-level CFAST fires, executes CFAST,
/// and merges the room conditions back onto every spatial frame.
pub(crate) async fn simulate_hybrid(
    State(world): State<SharedWorld>,
    Extension(property): Extension<SharedProperty>,
    body: Result<Json<SimulateFireRequest>, JsonRejection>,
) -> Response {
    let mut request = match decode(body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    request.fill_from_property(&property);
    let Some(cfast_path) = resolve_cfast_bin() else {
        return cfast_not_configured();
    };

    let snapshot = lock(&world).get_scene();
    let result = tokio::task::spawn_blocking(move || {
        run_hybrid(snapshot.scene, snapshot.revision, &request, &cfast_path)
    })
    .await;
    match result {
        Ok(Ok(response)) => Json(response).into_response(),
        Ok(Err((status, body))) => (status, Json(body)).into_response(),
        Err(error) => internal_error(error.to_string()),
    }
}

/// Runs several ordinary surface-fire simulations against one scene snapshot.
/// A failed run is returned beside the successful ones instead of failing the
/// whole batch.
pub(crate) async fn simulate_n(
    State(world): State<SharedWorld>,
    body: Result<Json<SimulateNRequest>, JsonRejection>,
) -> Response {
    let request = match decode(body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    if let Some(response) = validate_simulate_n(&request) {
        return response;
    }

    let snapshot = lock(&world).get_scene();
    let revision = snapshot.revision;
    let top_k = request.top_k;
    let representatives_only = request.representatives_only;
    let response_interval_seconds = request.response_interval_seconds;
    let simulations = request.simulations;
    let result = tokio::task::spawn_blocking(move || {
        run_simulations_bounded(snapshot.scene, revision, simulations, run)
    })
    .await;
    match result {
        Ok(runs) => batch_response(
            revision,
            runs,
            top_k,
            representatives_only,
            response_interval_seconds,
        ),
        Err(error) => internal_error(error.to_string()),
    }
}

/// Hybrid counterpart to [`simulate_n`]. Each worker gets an independent
/// CFAST temporary directory through `CfastRunner`.
pub(crate) async fn simulate_n_hybrid(
    State(world): State<SharedWorld>,
    body: Result<Json<SimulateNRequest>, JsonRejection>,
) -> Response {
    let request = match decode(body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    if let Some(response) = validate_simulate_n(&request) {
        return response;
    }
    let Some(cfast_path) = resolve_cfast_bin() else {
        return cfast_not_configured();
    };

    let snapshot = lock(&world).get_scene();
    let revision = snapshot.revision;
    let top_k = request.top_k;
    let representatives_only = request.representatives_only;
    let response_interval_seconds = request.response_interval_seconds;
    let simulations = request.simulations;
    let result = tokio::task::spawn_blocking(move || {
        run_simulations_bounded(
            snapshot.scene,
            revision,
            simulations,
            |scene, revision, request| run_hybrid(scene, revision, request, &cfast_path),
        )
    })
    .await;
    match result {
        Ok(runs) => batch_response(
            revision,
            runs,
            top_k,
            representatives_only,
            response_interval_seconds,
        ),
        Err(error) => internal_error(error.to_string()),
    }
}

fn validate_simulate_n(request: &SimulateNRequest) -> Option<Response> {
    let count = request.simulations.len();
    if count == 0 || count > MAX_SIMULATIONS_PER_BATCH {
        return Some(
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": "invalid_simulation_count",
                    "message": format!(
                        "simulations must contain between 1 and {MAX_SIMULATIONS_PER_BATCH} runs"
                    ),
                    "count": count,
                })),
            )
                .into_response(),
        );
    }
    if request
        .top_k
        .is_some_and(|top_k| top_k == 0 || top_k > count)
    {
        return Some(
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": "invalid_top_k",
                    "message": "top_k must be between 1 and the number of simulations",
                    "top_k": request.top_k,
                    "simulation_count": count,
                })),
            )
                .into_response(),
        );
    }
    if request
        .response_interval_seconds
        .is_some_and(|interval| !interval.is_finite() || interval < DEFAULT_STEP_SECONDS)
    {
        return Some(
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": "invalid_response_interval",
                    "message": format!(
                        "response_interval_seconds must be at least {DEFAULT_STEP_SECONDS}"
                    ),
                })),
            )
                .into_response(),
        );
    }

    let mut seen = std::collections::HashMap::new();
    for (index, simulation) in request.simulations.iter().enumerate() {
        // Struct serialization is stable here and captures every simulation
        // input, including the seed and complete occupant/crew layouts.
        let key = serde_json::to_string(simulation).expect("simulation request serializes");
        if let Some(first_index) = seen.insert(key, index) {
            return Some(
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "code": "duplicate_simulation",
                        "message": "every simulation in the batch must have distinct inputs",
                        "first_index": first_index,
                        "duplicate_index": index,
                    })),
                )
                    .into_response(),
            );
        }
    }
    None
}

fn batch_response(
    revision: u64,
    mut runs: Vec<SimulateNRun>,
    top_k: Option<usize>,
    representatives_only: bool,
    response_interval_seconds: Option<f64>,
) -> Response {
    let mut fire_ensemble = top_k.and_then(|top_k| analyze_completed_runs(&runs, top_k));
    if representatives_only && let Some(ensemble) = &mut fire_ensemble {
        let selected = ensemble
            .representatives
            .iter()
            .map(|representative| representative.simulation_index)
            .collect::<std::collections::HashSet<_>>();
        runs.retain(|run| match run {
            SimulateNRun::Completed { index, .. } => selected.contains(index),
            SimulateNRun::Failed { .. } => false,
        });
        if let Some(interval_seconds) = response_interval_seconds {
            for run in &mut runs {
                if let SimulateNRun::Completed { simulation, .. } = run {
                    compact_playback_frames(simulation, interval_seconds);
                }
            }
        }
        // The review UI only needs representative indexes and support. These
        // fields can dwarf the selected timelines and are not rendered.
        ensemble.frequencies.clear();
        ensemble.assignments.clear();
    }
    Json(SimulateNResponse {
        revision,
        runs,
        fire_ensemble,
    })
    .into_response()
}

/// Shrinks only the serialized playback copy after full-resolution ranking.
/// The renderer ignores burned-out/extinguished cells. Agents keep every step
/// in `agent_frames`, so movement between the thinned frames stays in rooms.
fn compact_playback_frames(simulation: &mut SimulateFireResponse, interval_seconds: f64) {
    let results = &mut simulation.results;
    simulation.agent_frames = Some(
        results
            .frames
            .iter()
            .map(|frame| AgentTimelineFrame {
                time_seconds: frame.time_seconds,
                agents: frame.agents.clone(),
            })
            .collect(),
    );
    let frames = std::mem::take(&mut results.frames);
    let last_index = frames.len().saturating_sub(1);
    let mut compact = Vec::with_capacity(frames.len());
    let mut last_kept_time = f64::NEG_INFINITY;
    for (index, mut frame) in frames.into_iter().enumerate() {
        let keep = index == 0
            || index == last_index
            || frame.time_seconds - last_kept_time + f64::EPSILON >= interval_seconds;
        if !keep {
            continue;
        }
        frame.surface_fire.retain(|node| {
            matches!(
                node.state,
                CombustionState::Burning | CombustionState::Heating
            )
        });
        last_kept_time = frame.time_seconds;
        compact.push(frame);
    }
    results.frames = compact;
}

fn analyze_completed_runs(
    runs: &[SimulateNRun],
    top_k: usize,
) -> Option<BatchFireEnsembleAnalysis> {
    let completed = runs
        .iter()
        .filter_map(|run| match run {
            SimulateNRun::Completed { index, simulation } => Some((*index, &simulation.results)),
            SimulateNRun::Failed { .. } => None,
        })
        .collect::<Vec<_>>();
    if completed.is_empty() {
        return None;
    }

    let completed_results = completed
        .iter()
        .map(|(_, results)| *results)
        .collect::<Vec<_>>();
    let config = FireEnsembleConfig::default();
    let mut analysis =
        analyze_fire_ensemble(&completed_results, top_k.min(completed.len()), config)
            .expect("completed batch runs share one scene and the default config is valid");

    for representative in &mut analysis.representatives {
        representative.simulation_index = completed[representative.simulation_index].0;
    }
    for assignment in &mut analysis.assignments {
        assignment.simulation_index = completed[assignment.simulation_index].0;
        assignment.representative_index = completed[assignment.representative_index].0;
    }

    Some(BatchFireEnsembleAnalysis {
        requested_top_k: top_k,
        completed_simulation_count: completed.len(),
        config: analysis.config,
        frequencies: analysis.frequencies,
        heatmap: analysis.heatmap,
        representatives: analysis.representatives,
        assignments: analysis.assignments,
    })
}

/// Executes `run_one` with a bounded number of OS workers and restores input
/// order in the returned vector. The outer caller already runs this function
/// on Tokio's blocking pool, keeping all CPU and subprocess work off the async
/// request threads.
fn run_simulations_bounded<F>(
    scene: Scene,
    revision: u64,
    simulations: Vec<SimulateFireRequest>,
    run_one: F,
) -> Vec<SimulateNRun>
where
    F: Fn(Scene, u64, &SimulateFireRequest) -> Result<SimulateFireResponse, Failure> + Sync,
{
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    let worker_count = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(MAX_CONCURRENT_SIMULATIONS)
        .min(simulations.len());
    let next = AtomicUsize::new(0);
    let ordered = Mutex::new(
        (0..simulations.len())
            .map(|_| None)
            .collect::<Vec<Option<SimulateNRun>>>(),
    );

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(request) = simulations.get(index) else {
                        break;
                    };
                    let item = match run_one(scene.clone(), revision, request) {
                        Ok(simulation) => SimulateNRun::Completed { index, simulation },
                        Err((status, error)) => SimulateNRun::Failed {
                            index,
                            http_status: status.as_u16(),
                            error,
                        },
                    };
                    ordered
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)[index] = Some(item);
                }
            });
        }
    });

    ordered
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .into_iter()
        .map(|item| item.expect("every queued simulation was run"))
        .collect()
}

fn internal_error(message: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "code": "internal", "message": message })),
    )
        .into_response()
}

fn cfast_not_configured() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "code": "cfast_not_configured",
            "message": "CFAST wasn't built because gfortran was missing when the server \
                        was compiled. Install it (macOS: `brew install gcc`, Debian/Ubuntu: \
                        `sudo apt install gfortran`), then `touch crates/cfast/build.rs` and \
                        restart the server, or set CFAST_BIN to a CFAST 7.7.7 executable"
        })),
    )
        .into_response()
}

type Failure = (StatusCode, serde_json::Value);

fn run(
    mut scene: Scene,
    revision: u64,
    request: &SimulateFireRequest,
) -> Result<SimulateFireResponse, Failure> {
    let object_id = request.object_id.as_str();
    if !scene
        .floors
        .iter()
        .any(|floor| floor.objects.iter().any(|object| object.id == object_id))
    {
        return Err((
            StatusCode::NOT_FOUND,
            json!({ "code": "object_not_found", "object_id": object_id }),
        ));
    }
    let duration = clamp(
        request.duration_seconds,
        DEFAULT_DURATION_SECONDS,
        MIN_STEP_SECONDS,
        MAX_DURATION_SECONDS,
    );
    let step = clamp(
        request.step_seconds,
        DEFAULT_STEP_SECONDS,
        MIN_STEP_SECONDS,
        duration,
    );

    let evacuation = build_evacuation(&scene, request)?;
    let (fire, excluded_object_ids) = build_fire_world(&mut scene, request.floor_material)?;
    if excluded_object_ids.iter().any(|id| id == object_id) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "code": "object_outside_rooms",
                "object_id": object_id,
                "message": "the object doesn't stand inside a room, so fire can't be simulated there",
            }),
        ));
    }
    let firefighting = build_firefighting(&scene, request)?;
    let frames = run_rehearsal(fire, evacuation, firefighting, object_id, duration, step)?;

    let results = SimulationResults {
        schema_version: SIMULATION_RESULTS_SCHEMA_VERSION,
        scene_id: scene.id.clone(),
        scenario_id: format!("surface-fire:{object_id}"),
        engine: SimulationEngine {
            name: "tflop-surface-fire".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        frames: frames
            .into_iter()
            .map(RehearsalFrame::into_simulation_frame)
            .collect(),
    };

    Ok(SimulateFireResponse {
        revision,
        excluded_object_ids,
        results,
        agent_frames: None,
    })
}

fn run_hybrid(
    source_scene: Scene,
    revision: u64,
    request: &SimulateFireRequest,
    cfast_path: &std::path::Path,
) -> Result<SimulateFireResponse, Failure> {
    let object_id = request.object_id.as_str();
    require_object(&source_scene, object_id)?;
    let evacuation = build_evacuation(&source_scene, request)?;

    let prepared = prepare_cfast_scene(&source_scene)
        .map_err(|error| simulation_failure("cfast_preparation_failed", error.to_string()))?;
    let mut scene = prepared.scene;
    let duration = clamp(
        request.duration_seconds,
        DEFAULT_DURATION_SECONDS,
        MIN_STEP_SECONDS,
        MAX_DURATION_SECONDS,
    );
    let step = clamp(
        request.step_seconds,
        DEFAULT_STEP_SECONDS,
        MIN_STEP_SECONDS,
        duration,
    );

    let (fire, excluded_object_ids) = build_fire_world(&mut scene, request.floor_material)?;
    if excluded_object_ids.iter().any(|id| id == object_id) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "code": "object_outside_rooms",
                "object_id": object_id,
                "message": "the object doesn't stand inside a CFAST-compatible room"
            }),
        ));
    }
    // Agents and fire advance together first; CFAST then derives room
    // conditions from the resulting fire timeline.
    let (surface_frames, agent_frames): (Vec<_>, Vec<_>) = run_rehearsal(
        fire,
        evacuation,
        build_firefighting(&scene, request)?,
        object_id,
        duration,
        step,
    )?
    .into_iter()
    .map(|frame| (frame.fire, frame.agents))
    .unzip();
    let cfast_timeline =
        adapt_surface_fire_for_cfast(&surface_frames, SurfaceFireAdapterConfig::default())
            .map_err(|error| simulation_failure("cfast_adapter_failed", error.to_string()))?;
    let openings = resolve_opening_connections(&scene)
        .map_err(|error| simulation_failure("cfast_preparation_failed", error.to_string()))?;
    let ignition_room_id = cfast_timeline
        .room_fires
        .first()
        .map(|fire| fire.room_id.clone())
        .ok_or_else(|| simulation_failure("cfast_adapter_failed", "no room fire was produced"))?;
    let scenario = CfastScenario {
        id: format!("hybrid:{object_id}"),
        title: format!("Hybrid simulation igniting {object_id}"),
        room_id: ignition_room_id,
        exterior_opening_id: "unused-in-all-room-run".to_owned(),
        // CFAST emits rows every output interval on [0, duration), so run to
        // a whole interval past the surface endpoint, which may fall on a
        // half second, to make interpolation at that endpoint valid.
        duration_seconds: ((cfast_timeline.duration_seconds / CFAST_OUTPUT_INTERVAL_SECONDS)
            .ceil()
            + 1.0)
            * CFAST_OUTPUT_INTERVAL_SECONDS,
        output_interval_seconds: CFAST_OUTPUT_INTERVAL_SECONDS,
        interior_temperature_c: 20.0,
        exterior_temperature_c: 20.0,
        relative_humidity_percent: 50.0,
        opening_states: openings
            .iter()
            .map(|opening| CfastOpeningState {
                opening_id: opening.id.clone(),
                open_fraction: 1.0,
            })
            .collect(),
        fires: cfast_timeline.room_fires,
    };
    let runner = CfastRunner::new(cfast_path).map_err(|error| cfast_failure(error.to_string()))?;
    let cfast_results = runner
        .run_all_rooms(&scene, &scenario)
        .map_err(|error| cfast_failure(error.to_string()))?;
    let mut results = merge_cfast_with_surface_fire(&scene, &cfast_results, &surface_frames)
        .map_err(|error| simulation_failure("cfast_merge_failed", error.to_string()))?;
    if results.frames.len() != agent_frames.len() {
        return Err(simulation_failure(
            "cfast_merge_failed",
            format!(
                "merged {} frames from a {}-frame timeline",
                results.frames.len(),
                agent_frames.len()
            ),
        ));
    }
    for (frame, agents) in results.frames.iter_mut().zip(agent_frames) {
        frame.agents = agents;
    }
    results
        .validate_against_scene(&scene)
        .map_err(|error| simulation_failure("evacuation_failed", error.to_string()))?;

    Ok(SimulateFireResponse {
        revision,
        excluded_object_ids,
        results,
        agent_frames: None,
    })
}

fn build_evacuation(
    scene: &Scene,
    request: &SimulateFireRequest,
) -> Result<Option<Evacuation>, Failure> {
    if request.occupants.is_empty() {
        return Ok(None);
    }
    let alarm = match request.alarm {
        AlarmMode::Detected => {
            request
                .alarm_config
                .map_or_else(AlarmConfig::default, |t| AlarmConfig::Detected {
                    notice_hrr_kw: t.notice_hrr_kw.max(0.0),
                    same_room_seconds: t.same_room_seconds.max(0.0),
                    seconds_per_metre: t.seconds_per_metre.max(0.0),
                })
        }
        AlarmMode::Immediate => AlarmConfig::Immediate,
    };
    let mut occupants = request.occupants.clone();
    choose_injured(
        &mut occupants,
        request
            .injured_fraction
            .filter(|fraction| fraction.is_finite())
            .unwrap_or(DEFAULT_INJURED_FRACTION),
        request.seed,
    );
    Evacuation::new(scene, &occupants, simulation_config(request))
        .map(|evacuation| Some(evacuation.with_alarm(alarm)))
        .map_err(|error| simulation_failure("evacuation_failed", error.to_string()))
}

/// Crews are built on the same scene as the fire world, so the rooms they
/// search and spray match the fire's room IDs.
fn build_firefighting(
    scene: &Scene,
    request: &SimulateFireRequest,
) -> Result<Option<Firefighting>, Failure> {
    if request.crews.is_empty() {
        return Ok(None);
    }
    Firefighting::new(
        scene,
        &request.crews,
        simulation_config(request),
        FirefightingConfig::default(),
    )
    .map(Some)
    .map_err(|error| simulation_failure("firefighting_failed", error.to_string()))
}

fn simulation_config(request: &SimulateFireRequest) -> SimulationConfig {
    SimulationConfig {
        random_seed: request.seed,
        ..SimulationConfig::default()
    }
}

fn require_object(scene: &Scene, object_id: &str) -> Result<(), Failure> {
    if scene
        .floors
        .iter()
        .any(|floor| floor.objects.iter().any(|object| object.id == object_id))
    {
        Ok(())
    } else {
        Err((
            StatusCode::NOT_FOUND,
            json!({ "code": "object_not_found", "object_id": object_id }),
        ))
    }
}

/// Ignites `object_id` and advances the fire and occupants together until
/// `duration` or until nothing is left to simulate. Frame 0 is the ignition.
fn run_rehearsal(
    fire: SurfaceFireWorld,
    evacuation: Option<Evacuation>,
    firefighting: Option<Firefighting>,
    object_id: &str,
    duration: f64,
    step: f64,
) -> Result<Vec<RehearsalFrame>, Failure> {
    let mut rehearsal = Rehearsal::new(fire, evacuation);
    if let Some(firefighting) = firefighting {
        rehearsal = rehearsal.with_firefighting(firefighting);
    }
    let first = rehearsal
        .ignite_object(object_id)
        .map_err(|error| match error {
            // The object exists, so it has no fire node only when its material
            // doesn't burn.
            RehearsalError::Fire(SurfaceFireError::NodeNotFound { .. }) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({ "code": "object_not_combustible", "object_id": object_id }),
            ),
            error => rehearsal_failed(&error),
        })?;
    let mut frames = vec![first];
    frames.extend(
        rehearsal
            .run_until(duration, step)
            .map_err(|error| rehearsal_failed(&error))?,
    );
    Ok(frames)
}

/// Builds the fire world, dropping any object the simulation can't place in a
/// room (e.g. scanned furniture outside every mapped room) instead of failing
/// the whole run.
fn build_fire_world(
    scene: &mut Scene,
    floor: Option<FloorMaterial>,
) -> Result<(SurfaceFireWorld, Vec<String>), Failure> {
    let catalog = material_catalog(floor);
    let mut excluded = Vec::new();
    loop {
        match SurfaceFireWorld::from_scene(scene, &catalog, SurfaceFireConfig::default()) {
            Ok(fire) => return Ok((fire, excluded)),
            Err(SurfaceFireError::ObjectOutsideGrid { object_id }) => {
                for floor in &mut scene.floors {
                    floor.objects.retain(|object| object.id != object_id);
                }
                excluded.push(object_id);
            }
            Err(error) => return Err(simulation_failed(&error)),
        }
    }
}

fn simulation_failed(error: &SurfaceFireError) -> Failure {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "code": "simulation_failed", "message": error.to_string() }),
    )
}

fn rehearsal_failed(error: &RehearsalError) -> Failure {
    match error {
        RehearsalError::Fire(error) => simulation_failed(error),
        RehearsalError::Evacuation(error) => {
            simulation_failure("evacuation_failed", error.to_string())
        }
        RehearsalError::Firefighting(error) => {
            simulation_failure("firefighting_failed", error.to_string())
        }
        RehearsalError::InvalidStep { .. } | RehearsalError::CrowdConfig(_) => {
            simulation_failure("simulation_failed", error.to_string())
        }
    }
}

fn simulation_failure(code: &'static str, message: impl Into<String>) -> Failure {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "code": code, "message": message.into() }),
    )
}

fn cfast_failure(message: impl Into<String>) -> Failure {
    (
        StatusCode::BAD_GATEWAY,
        json!({ "code": "cfast_failed", "message": message.into() }),
    )
}

fn clamp(value: Option<f64>, default: f64, min: f64, max: f64) -> f64 {
    value
        .filter(|value| value.is_finite())
        .unwrap_or(default)
        .clamp(min, max)
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use super::*;
    use crate::{PropertyBrief, load_scene};

    fn bungalow() -> SharedProperty {
        let kit = Path::new(env!("CARGO_MANIFEST_DIR")).join("../costar/fixtures/kit");
        let record = tflop_costar::find_by_model_id(&kit, "zvKHR9DHNjJ").expect("fixture joins");
        let scene = load_scene(crate::DEMO_SCENE_JSON).expect("demo scene");
        Arc::new(Some(PropertyBrief::new(record, &scene)))
    }

    fn request(json: serde_json::Value) -> SimulateFireRequest {
        serde_json::from_value(json).expect("valid request")
    }

    #[test]
    fn omitted_settings_come_from_the_costar_profile() {
        let mut r = request(json!({ "object_id": "chair-1" }));
        r.fill_from_property(&bungalow());
        assert_eq!(r.floor_material, Some(FloorMaterial::Hardwood));
        assert_eq!(r.injured_fraction, Some(0.15));
        assert_eq!(r.alarm_config.map(|a| a.same_room_seconds), Some(30.0));
    }

    #[test]
    fn sent_settings_win_and_no_property_changes_nothing() {
        let mut r = request(json!({
            "object_id": "chair-1",
            "floor_material": "carpet",
            "injured_fraction": 0.0
        }));
        r.fill_from_property(&bungalow());
        assert_eq!(r.floor_material, Some(FloorMaterial::Carpet));
        assert_eq!(r.injured_fraction, Some(0.0));

        let mut bare = request(json!({ "object_id": "chair-1" }));
        bare.fill_from_property(&Arc::new(None));
        assert!(bare.floor_material.is_none() && bare.alarm_config.is_none());
        assert!(bare.injured_fraction.is_none());
    }
}
