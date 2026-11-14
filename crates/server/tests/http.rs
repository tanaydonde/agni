//! In-process HTTP tests for the world server router, using the built-in
//! demo room.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tflop_server::{DEMO_SCENE_JSON, load_scene, router};
use tflop_world_core::World;
use tower::ServiceExt;

fn app() -> Router {
    router(World::new(
        load_scene(DEMO_SCENE_JSON).expect("demo scene is valid"),
    ))
}

async fn call(app: Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("request builds");
    let response = app.oneshot(request).await.expect("router responds");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[test]
fn demo_scene_fixture_is_valid() {
    let scene = load_scene(DEMO_SCENE_JSON).expect("demo scene is valid");
    assert_eq!(scene.floors[0].id, "floor-1");
    assert!(scene.assets.iter().any(|asset| asset.id == "chair-basic"));
}

#[tokio::test]
async fn get_scene_returns_revision_and_scene() {
    let (status, body) = call(app(), "GET", "/api/world/scene", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["revision"], 0);
    assert_eq!(body["scene"]["id"], "demo-room");
}

#[tokio::test]
async fn list_assets_returns_catalog() {
    let (status, body) = call(app(), "GET", "/api/world/assets", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["assets"].as_array().map(Vec::len), Some(5));
}

#[tokio::test]
async fn add_then_move_snaps_to_floor_and_bumps_revision() {
    let app = app();
    let (status, body) = call(
        app.clone(),
        "POST",
        "/api/world/add_object",
        Some(json!({
            "expected_revision": 0, "object_id": "desk-9", "asset_id": "desk-basic",
            "floor_id": "floor-1", "position_m": { "x": 1.0, "y": 1.0, "z": 2.0 },
            "rotation_z_degrees": 0.0, "mobility": "movable"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revision"], 1);

    let (status, body) = call(
        app.clone(),
        "POST",
        "/api/world/move_object",
        Some(json!({
            "expected_revision": 1, "object_id": "desk-9",
            "position_m": { "x": 2.0, "y": 1.5, "z": -4.0 }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revision"], 2);

    let (_, body) = call(app, "GET", "/api/world/scene", None).await;
    let desk = body["scene"]["floors"][0]["objects"]
        .as_array()
        .and_then(|objects| objects.iter().find(|object| object["id"] == "desk-9"))
        .expect("desk was added")
        .clone();
    assert_eq!(
        desk["transform"]["position_m"],
        json!({ "x": 2.0, "y": 1.5, "z": 0.0 })
    );
}

#[tokio::test]
async fn stale_revision_returns_409_and_unknown_object_404() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/world/remove_object",
        Some(json!({ "expected_revision": 7, "object_id": "chair-1" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "stale_revision");

    let (status, body) = call(
        app(),
        "POST",
        "/api/world/remove_object",
        Some(json!({ "expected_revision": 0, "object_id": "missing" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "object_not_found");
}

#[tokio::test]
async fn check_edits_dry_runs_without_bumping_the_revision() {
    let app = app();
    let (status, body) = call(
        app.clone(),
        "POST",
        "/api/world/check_edits",
        Some(
            json!({ "edits": [{ "type": "move_object", "object_id": "missing",
                                  "position_m": { "x": 1.0, "y": 1.0, "z": 0.0 } }] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["valid"], false);
    assert_eq!(body["error"]["code"], "batch_edit_failed");
    assert_eq!(body["error"]["reason"]["code"], "object_not_found");

    let (_, body) = call(app, "GET", "/api/world/scene", None).await;
    assert_eq!(body["revision"], 0);
}

#[test]
fn scenes_without_assets_get_the_default_catalog() {
    let mut scene: Value = serde_json::from_str(DEMO_SCENE_JSON).expect("demo JSON parses");
    scene["assets"] = json!([]);
    scene["floors"][0]["objects"] = json!([]);
    let loaded = load_scene(&scene.to_string()).expect("scene loads");
    assert_eq!(loaded.assets.len(), 5);
}

#[tokio::test]
async fn malformed_body_returns_structured_400() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/world/move_object",
        Some(json!({ "object_id": "chair-1" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_request");
}
