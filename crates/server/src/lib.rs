//! Thin HTTP bridge exposing the world-core agent API (`World`) to other
//! processes, chiefly the TypeScript designer agent in `apps/agent`.
//!
//! One route per tool in `docs/AGENT_TOOLS.md`. Request and response bodies are
//! the world-core types serialized as-is. Fire routes expose both the original
//! Rust-only simulation and the hybrid Rust + CFAST pipeline.

mod fire;
mod occupant_agent;

use std::sync::{Arc, Mutex, PoisonError};

use axum::{
    Extension, Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use tflop_costar::{
    FireProfile, PROPERTY_SCHEMA_VERSION, ProfileInputs, PropertyRecord, derive as derive_profile,
};
use tflop_furniture_detect::{DETECTION_SCHEMA_VERSION, Detection, MergeSummary, merge_into_scene};
use tflop_world_core::{
    AddObjectRequest, ApplyEditsRequest, CheckEditsRequest, MoveObjectRequest, RemoveObjectRequest,
    RotateObjectRequest, Scene, SetObjectTransformRequest, World, WorldApiResult,
    default_furniture_catalog,
};

/// The hand-made demo scene used when no scene file is given.
pub const DEMO_SCENE_JSON: &str = include_str!("../fixtures/demo-room.json");

pub(crate) type SharedWorld = Arc<Mutex<World>>;

/// The scan's CoStar record and the scenario assumptions derived from it.
#[derive(Debug, Clone, Serialize)]
pub struct PropertyBrief {
    pub record: PropertyRecord,
    pub profile: FireProfile,
}

impl PropertyBrief {
    /// Derives the profile, using the scene's room area for the occupant load.
    #[must_use]
    pub fn new(record: PropertyRecord, scene: &Scene) -> Self {
        let area: f64 = scene
            .floors
            .iter()
            .flat_map(|floor| &floor.rooms)
            .filter_map(|room| room.dimensions.as_ref()?.area_m2)
            .sum();
        let profile = derive_profile(
            &record,
            ProfileInputs {
                scanned_area_m2: (area > 0.0).then_some(area),
                max_occupants: occupant_agent::MAX_OCCUPANTS,
            },
        );
        Self { record, profile }
    }
}

pub(crate) type SharedProperty = Arc<Option<PropertyBrief>>;

/// Parses a `<scene>.property.json` written by `cargo run -p import-property`.
/// A record for another scan or schema version is rejected.
pub fn load_property(json: &str, scene: &Scene) -> Result<PropertyBrief, String> {
    let record: PropertyRecord = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if record.schema_version != PROPERTY_SCHEMA_VERSION {
        return Err(format!(
            "schema v{} (this server reads v{PROPERTY_SCHEMA_VERSION}); rerun import-property",
            record.schema_version
        ));
    }
    if record.scan.model_id != scene.source.external_id {
        return Err(format!(
            "it is for model {}, not {}",
            record.scan.model_id, scene.source.external_id
        ));
    }
    Ok(PropertyBrief::new(record, scene))
}

/// Parses and validates a canonical scene file.
///
/// Scenes imported before the furniture catalog existed have no assets; they
/// get the built-in catalog so the editor and agent can place furniture.
pub fn load_scene(json: &str) -> Result<Scene, String> {
    load_scene_with_detection(json, None).map(|(scene, _)| scene)
}

/// What happened to the scanned furniture offered to `load_scene_with_detection`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionOutcome {
    /// No detection file was given.
    None,
    Merged(MergeSummary),
    /// The file was unreadable or belongs to another scene or schema version;
    /// the scene loaded without it.
    Ignored(String),
}

/// Like `load_scene`, but first adds the furniture found in the scan
/// (`<scene>.detected.json` from `cargo run -p detect-furniture`) as ordinary
/// scene objects, so editing and the fire simulation see it. A detection file
/// that doesn't match the scene is ignored rather than failing the load.
pub fn load_scene_with_detection(
    json: &str,
    detection_json: Option<&str>,
) -> Result<(Scene, DetectionOutcome), String> {
    let mut scene: Scene = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if scene.assets.is_empty() {
        scene.assets = default_furniture_catalog();
    }
    let outcome = match detection_json {
        None => DetectionOutcome::None,
        Some(detection_json) => match serde_json::from_str::<Detection>(detection_json) {
            Err(error) => DetectionOutcome::Ignored(format!("unreadable: {error}")),
            Ok(detection) if detection.schema_version != DETECTION_SCHEMA_VERSION => {
                DetectionOutcome::Ignored(format!(
                    "schema v{} (this server reads v{DETECTION_SCHEMA_VERSION}); rerun detect-furniture",
                    detection.schema_version
                ))
            }
            Ok(detection) if detection.scene_id != scene.id => DetectionOutcome::Ignored(format!(
                "it is for scene {}, not {}",
                detection.scene_id, scene.id
            )),
            Ok(detection) => DetectionOutcome::Merged(merge_into_scene(&detection, &mut scene)),
        },
    };
    scene.validate().map_err(|errors| {
        errors
            .0
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    Ok((scene, outcome))
}

/// Builds the HTTP routes around one shared, in-memory `World`.
///
/// | Route | Method | world-core call |
/// | --- | --- | --- |
/// | `/health` | GET | none; returns `ok` |
/// | `/api/world/scene` | GET | `get_scene` |
/// | `/api/world/assets` | GET | `list_assets` |
/// | `/api/world/{add,move,rotate,remove}_object`, `set_object_transform` | POST | matching method |
/// | `/api/world/apply_edits` | POST | `apply_edits` |
/// | `/api/world/check_edits` | POST | `check_edits` (dry run; always 200 with `{valid, error}`) |
/// | `/api/world/validate_layout` | POST | `validate_layout` |
/// | `/api/fire/simulate` | POST | none; runs the surface-fire simulation on a scene snapshot |
/// | `/api/fire/simulate/hybrid` | POST | none; runs surface fire and CFAST, then merges both timelines |
/// | `/api/property` | GET | none; the scan's CoStar record and derived scenario assumptions (404 without one) |
///
/// Requests are serialized through a mutex, so edits apply one at a time.
pub fn router(world: World) -> Router {
    router_with_property(world, None)
}

/// Like `router`, with the scan's CoStar record (`load_property`) served at
/// `/api/property` and used as context for occupant placement.
pub fn router_with_property(world: World, property: Option<PropertyBrief>) -> Router {
    let state: SharedWorld = Arc::new(Mutex::new(world));
    let property: SharedProperty = Arc::new(property);
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/world/scene", get(get_scene))
        .route("/api/world/assets", get(list_assets))
        .route("/api/world/add_object", post(add_object))
        .route("/api/world/move_object", post(move_object))
        .route("/api/world/rotate_object", post(rotate_object))
        .route(
            "/api/world/set_object_transform",
            post(set_object_transform),
        )
        .route("/api/world/remove_object", post(remove_object))
        .route("/api/world/apply_edits", post(apply_edits))
        .route("/api/world/check_edits", post(check_edits))
        .route("/api/world/validate_layout", post(validate_layout))
        .route("/api/fire/simulate", post(fire::simulate))
        .route("/api/fire/simulate/hybrid", post(fire::simulate_hybrid))
        .route(
            "/api/agents/spawn_occupants",
            post(occupant_agent::spawn_occupants),
        )
        .route("/api/property", get(get_property))
        .layer(Extension(property))
        .with_state(state)
}

async fn get_property(Extension(property): Extension<SharedProperty>) -> Response {
    match property.as_ref() {
        // `gemini_placement` tells the UI whether it can use Gemini's
        // context-aware `spread` layout or must stay with `crowded`.
        Some(brief) => Json(json!({
            "record": brief.record,
            "profile": brief.profile,
            "gemini_placement": occupant_agent::gemini_configured(),
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "no_property_record",
                "message": "no CoStar record is loaded for this scene; run `cargo run -p import-property`"
            })),
        )
            .into_response(),
    }
}

async fn get_scene(State(world): State<SharedWorld>) -> Response {
    Json(lock(&world).get_scene()).into_response()
}

async fn list_assets(State(world): State<SharedWorld>) -> Response {
    Json(lock(&world).list_assets()).into_response()
}

async fn validate_layout(State(world): State<SharedWorld>) -> Response {
    respond(lock(&world).validate_layout())
}

async fn check_edits(
    State(world): State<SharedWorld>,
    body: Result<Json<CheckEditsRequest>, JsonRejection>,
) -> Response {
    match decode(body) {
        Ok(request) => Json(lock(&world).check_edits(request)).into_response(),
        Err(rejection) => rejection.into_response(),
    }
}

/// Generates a POST handler that decodes `$request` and calls `World::$method`.
macro_rules! mutation {
    ($name:ident, $request:ty, $method:ident) => {
        async fn $name(
            State(world): State<SharedWorld>,
            body: Result<Json<$request>, JsonRejection>,
        ) -> Response {
            match decode(body) {
                Ok(request) => respond(lock(&world).$method(request)),
                Err(rejection) => rejection.into_response(),
            }
        }
    };
}

mutation!(add_object, AddObjectRequest, add_object);
mutation!(move_object, MoveObjectRequest, move_object);
mutation!(rotate_object, RotateObjectRequest, rotate_object);
mutation!(
    set_object_transform,
    SetObjectTransformRequest,
    set_object_transform
);
mutation!(remove_object, RemoveObjectRequest, remove_object);
mutation!(apply_edits, ApplyEditsRequest, apply_edits);

/// Locks the world, recovering from a poisoned lock: `World` methods take
/// `&mut self` and leave no partially written state on panic.
pub(crate) fn lock(world: &SharedWorld) -> std::sync::MutexGuard<'_, World> {
    world.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Malformed request bodies become structured JSON so the agent can read them.
pub(crate) fn decode<T: DeserializeOwned>(
    body: Result<Json<T>, JsonRejection>,
) -> Result<T, (StatusCode, Json<serde_json::Value>)> {
    body.map(|Json(request)| request).map_err(|rejection| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "code": "invalid_request", "message": rejection.body_text() })),
        )
    })
}

/// Serializes a world result. Errors keep world-core's JSON shape
/// (`{"code": ..., ...}`); the HTTP status is chosen from `code` so that new
/// error variants added in world-core need no change here.
fn respond<T: Serialize>(result: WorldApiResult<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => {
            let body = serde_json::to_value(&error)
                .unwrap_or_else(|_| json!({ "code": "internal", "message": error.to_string() }));
            let status = match body.get("code").and_then(|code| code.as_str()) {
                Some("stale_revision" | "duplicate_object_id") => StatusCode::CONFLICT,
                Some("object_not_found" | "asset_not_found" | "floor_not_found") => {
                    StatusCode::NOT_FOUND
                }
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            };
            (status, Json(body)).into_response()
        }
    }
}
