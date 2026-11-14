//! Fire simulation endpoints. Both advance the surface fire and any supplied
//! occupants together, frame by frame (`Rehearsal`). `/api/fire/simulate`
//! returns that timeline as is; `/api/fire/simulate/hybrid` also feeds the
//! fire timeline into CFAST and returns the merged renderer contract.

use axum::{
    Json,
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
    AlarmConfig, CrewSpawn, Evacuation, FireMaterialCatalog, Firefighting, FirefightingConfig,
    OccupantSpawn, Rehearsal, RehearsalError, RehearsalFrame, SIMULATION_RESULTS_SCHEMA_VERSION,
    SimulationConfig, SimulationEngine, SimulationResults, SurfaceFireConfig, SurfaceFireError,
    SurfaceFireWorld, choose_injured,
};
use tflop_world_core::{Scene, resolve_opening_connections};

use crate::{SharedWorld, decode, lock};

const DEFAULT_DURATION_SECONDS: f64 = 180.0;
const MAX_DURATION_SECONDS: f64 = 600.0;
/// Half-second frames keep crowd movement smooth in playback.
const DEFAULT_STEP_SECONDS: f64 = 0.5;
const MIN_STEP_SECONDS: f64 = 0.1;
const CFAST_OUTPUT_INTERVAL_SECONDS: f64 = 5.0;

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Clone, Copy, Default, Deserialize)]
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
}

pub(crate) async fn simulate(
    State(world): State<SharedWorld>,
    body: Result<Json<SimulateFireRequest>, JsonRejection>,
) -> Response {
    let request = match decode(body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
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
    body: Result<Json<SimulateFireRequest>, JsonRejection>,
) -> Response {
    let request = match decode(body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    let Some(cfast_path) = resolve_cfast_bin() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "code": "cfast_not_configured",
                "message": "CFAST wasn't built because gfortran was missing when the server \
                            was compiled. Install it (macOS: `brew install gcc`, Debian/Ubuntu: \
                            `sudo apt install gfortran`), then `touch crates/cfast/build.rs` and \
                            restart the server, or set CFAST_BIN to a CFAST 7.7.7 executable"
            })),
        )
            .into_response();
    };

    let snapshot = lock(&world).get_scene();
    let result = tokio::task::spawn_blocking(move || {
        run_hybrid(snapshot.scene, snapshot.revision, &request, &cfast_path)
    })
    .await;
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
    let (fire, excluded_object_ids) = build_fire_world(&mut scene)?;
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

    let (fire, excluded_object_ids) = build_fire_world(&mut scene)?;
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
        // CFAST emits rows on [0, duration), so run one output interval beyond
        // the surface endpoint to make interpolation at that endpoint valid.
        duration_seconds: cfast_timeline.duration_seconds + CFAST_OUTPUT_INTERVAL_SECONDS,
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
        AlarmMode::Detected => AlarmConfig::default(),
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
fn build_fire_world(scene: &mut Scene) -> Result<(SurfaceFireWorld, Vec<String>), Failure> {
    let mut excluded = Vec::new();
    loop {
        match SurfaceFireWorld::from_scene(
            scene,
            &FireMaterialCatalog::default(),
            SurfaceFireConfig::default(),
        ) {
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
