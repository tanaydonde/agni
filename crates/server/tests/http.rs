//! In-process HTTP tests for the world server router, using the built-in
//! demo room. Mutation routes are expected to return world-core's
//! `not_implemented` error until those methods are implemented.

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
async fn stubbed_mutation_returns_501_with_world_error_body() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/world/apply_edits",
        Some(json!({
            "expected_revision": 0,
            "edits": [{ "type": "move_object", "object_id": "chair-1",
                        "position_m": { "x": 1.0, "y": 1.0, "z": 0.0 } }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(
        body,
        json!({ "code": "not_implemented", "operation": "apply_edits" })
    );
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
