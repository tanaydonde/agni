//! Fire simulation endpoints. `/api/fire/simulate` preserves the original
//! Rust-only surface simulation; `/api/fire/simulate/hybrid` feeds that
//! timeline into CFAST and returns the merged renderer contract.

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
    CombustionState, FireMaterialCatalog, SIMULATION_RESULTS_SCHEMA_VERSION, SimulationEngine,
    SimulationFrame, SimulationResults, SurfaceFireConfig, SurfaceFireError, SurfaceFireFrame,
    SurfaceFireWorld,
};
use tflop_world_core::{Scene, resolve_opening_connections};

use crate::{SharedWorld, decode, lock};

const DEFAULT_DURATION_SECONDS: f64 = 180.0;
const MAX_DURATION_SECONDS: f64 = 600.0;
const DEFAULT_STEP_SECONDS: f64 = 1.0;
const MIN_STEP_SECONDS: f64 = 0.1;
const CFAST_OUTPUT_INTERVAL_SECONDS: f64 = 5.0;

#[derive(Debug, Deserialize)]
pub(crate) struct SimulateFireRequest {
    object_id: String,
    #[serde(default)]
    duration_seconds: Option<f64>,
    #[serde(default)]
    step_seconds: Option<f64>,
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

    let (mut fire, excluded_object_ids) = build_fire_world(&mut scene)?;
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
    let first = fire.ignite_object(object_id).map_err(|error| match error {
        // The object exists, so it has no fire node only when its material
        // doesn't burn.
        SurfaceFireError::NodeNotFound { .. } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "code": "object_not_combustible", "object_id": object_id }),
        ),
        error => simulation_failed(&error),
    })?;

    let mut frames = vec![first];
    while frames
        .last()
        .is_some_and(|frame| frame.time_seconds < duration)
    {
        let frame = fire.step(step).map_err(|error| simulation_failed(&error))?;
        let active = frame.active_nodes.iter().any(|node| {
            matches!(
                node.state,
                CombustionState::Heating | CombustionState::Burning
            )
        });
        frames.push(frame);
        if !active {
            break;
        }
    }

    Ok(SimulateFireResponse {
        revision,
        excluded_object_ids,
        results: SimulationResults {
            schema_version: SIMULATION_RESULTS_SCHEMA_VERSION,
            scene_id: scene.id.clone(),
            scenario_id: format!("surface-fire:{object_id}"),
            engine: SimulationEngine {
                name: "tflop-surface-fire".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            frames: frames.into_iter().map(simulation_frame).collect(),
        },
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

    let (mut fire, excluded_object_ids) = build_fire_world(&mut scene)?;
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
    let surface_frames = run_surface_timeline(&mut fire, object_id, duration, step)?;
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
    let results = merge_cfast_with_surface_fire(&scene, &cfast_results, &surface_frames)
        .map_err(|error| simulation_failure("cfast_merge_failed", error.to_string()))?;

    Ok(SimulateFireResponse {
        revision,
        excluded_object_ids,
        results,
    })
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

fn run_surface_timeline(
    fire: &mut SurfaceFireWorld,
    object_id: &str,
    duration: f64,
    step: f64,
) -> Result<Vec<SurfaceFireFrame>, Failure> {
    let first = fire.ignite_object(object_id).map_err(|error| match error {
        SurfaceFireError::NodeNotFound { .. } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "code": "object_not_combustible", "object_id": object_id }),
        ),
        error => simulation_failed(&error),
    })?;
    let mut frames = vec![first];
    while frames
        .last()
        .is_some_and(|frame| frame.time_seconds < duration)
    {
        let frame = fire.step(step).map_err(|error| simulation_failed(&error))?;
        let active = frame.active_nodes.iter().any(|node| {
            matches!(
                node.state,
                CombustionState::Heating | CombustionState::Burning
            )
        });
        frames.push(frame);
        if !active {
            break;
        }
    }
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

fn simulation_frame(frame: SurfaceFireFrame) -> SimulationFrame {
    SimulationFrame {
        time_seconds: frame.time_seconds,
        rooms: Vec::new(),
        fires: Vec::new(),
        openings: Vec::new(),
        surface_fire: frame.active_nodes,
    }
}

fn simulation_failed(error: &SurfaceFireError) -> Failure {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "code": "simulation_failed", "message": error.to_string() }),
    )
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
