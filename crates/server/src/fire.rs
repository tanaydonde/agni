//! `POST /api/fire/simulate`: ignites one furniture object and runs the
//! surface-fire simulation to completion on a snapshot of the world's scene.
//!
//! The whole timeline comes back in one response, in the `SimulationResults`
//! shape the renderer reads (`docs/FIRE_FRONTEND.md`). Only `surface_fire` is
//! filled for now; CFAST rooms, fires, and openings stay empty until the
//! Rust-to-CFAST adapter exists.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tflop_simulation_core::{
    CombustionState, FireMaterialCatalog, SIMULATION_RESULTS_SCHEMA_VERSION, SimulationEngine,
    SimulationFrame, SimulationResults, SurfaceFireConfig, SurfaceFireError, SurfaceFireFrame,
    SurfaceFireWorld,
};
use tflop_world_core::Scene;

use crate::{SharedWorld, decode, lock};

const DEFAULT_DURATION_SECONDS: f64 = 180.0;
const MAX_DURATION_SECONDS: f64 = 600.0;
const DEFAULT_STEP_SECONDS: f64 = 1.0;
const MIN_STEP_SECONDS: f64 = 0.1;

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
    while frames.last().is_some_and(|frame| frame.time_seconds < duration) {
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

fn clamp(value: Option<f64>, default: f64, min: f64, max: f64) -> f64 {
    value
        .filter(|value| value.is_finite())
        .unwrap_or(default)
        .clamp(min, max)
}
