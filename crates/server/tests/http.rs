//! In-process HTTP tests for the world server router, using the built-in
//! demo room.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tflop_server::{
    DEMO_SCENE_JSON, DetectionOutcome, load_property, load_scene, load_scene_with_detection,
    router, router_with_property,
};
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

#[tokio::test]
async fn occupant_agent_rejects_an_invalid_count_before_calling_gemini() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/agents/spawn_occupants",
        Some(json!({ "count": 0 })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_count");
}

#[tokio::test]
async fn a_crowded_layout_spawns_without_gemini_and_queues_at_the_door() {
    let (status, spawned) = call(
        app(),
        "POST",
        "/api/agents/spawn_occupants",
        Some(json!({ "count": 20, "layout": "crowded" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{spawned}");
    let occupants = spawned["occupants"].as_array().expect("occupants");
    assert_eq!(occupants.len(), 20);
    let (_, again) = call(
        app(),
        "POST",
        "/api/agents/spawn_occupants",
        Some(json!({ "count": 20, "layout": "crowded" })),
    )
    .await;
    assert_eq!(
        spawned["occupants"], again["occupants"],
        "crowded layouts repeat"
    );

    let (status, body) = call(
        app(),
        "POST",
        "/api/fire/simulate",
        Some(json!({
            "object_id": "chair-1",
            "duration_seconds": 60.0,
            "occupants": occupants,
            "alarm": "immediate",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let frames = body["results"]["frames"].as_array().expect("frames");
    assert_eq!(
        frames[1]["time_seconds"], 0.5,
        "half-second frames by default"
    );
    let movements = frames
        .iter()
        .flat_map(|frame| frame["agents"].as_array().expect("agents"))
        .map(|agent| agent["movement"].as_str().expect("movement").to_owned())
        .collect::<std::collections::HashSet<_>>();
    assert!(movements.contains("walking"), "{movements:?}");
    assert!(movements.contains("queued"), "{movements:?}");
}

#[tokio::test]
async fn hybrid_fire_route_rejects_a_malformed_request() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/fire/simulate/hybrid",
        Some(json!({ "duration_seconds": 30.0 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_request");
}

#[tokio::test]
async fn simulate_fire_returns_a_growing_timeline() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/fire/simulate",
        Some(json!({ "object_id": "chair-1", "duration_seconds": 30.0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revision"], 0);
    let results = &body["results"];
    assert_eq!(results["scene_id"], "demo-room");
    let frames = results["frames"].as_array().expect("frames");
    assert!(frames.len() > 1, "{body}");
    assert!(
        frames[0]["surface_fire"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|node| node["state"] == "burning")),
        "the ignited cell burns in frame 0"
    );
    let times: Vec<f64> = frames
        .iter()
        .map(|frame| frame["time_seconds"].as_f64().expect("time"))
        .collect();
    assert!(times.windows(2).all(|pair| pair[0] < pair[1]), "{times:?}");
    assert!(*times.last().expect("last") <= 30.0 + 1e-9);
}

#[tokio::test]
async fn a_crew_finds_the_fire_puts_it_out_and_leaves() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/fire/simulate",
        Some(json!({
            "object_id": "chair-1",
            "duration_seconds": 300.0,
            "crews": [{ "id": "engine-1", "member_count": 2, "arrival_time_seconds": 5.0 }]
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let frames = body["results"]["frames"].as_array().expect("frames");
    let phases = |id: &str| {
        frames
            .iter()
            .map(|frame| {
                let agent = frame["agents"]
                    .as_array()
                    .expect("agents")
                    .iter()
                    .find(|agent| agent["id"] == id)
                    .expect("firefighter in every frame");
                assert_eq!(agent["role"]["type"], "firefighter");
                assert_eq!(agent["role"]["crew_id"], "engine-1");
                agent["role"]["phase"].as_str().expect("phase").to_owned()
            })
            .collect::<Vec<_>>()
    };
    let leader = phases("engine-1/1");
    assert_eq!(phases("engine-1/2").len(), leader.len());
    assert_eq!(leader.first().map(String::as_str), Some("staging"));
    assert!(
        leader.iter().any(|phase| phase == "suppressing"),
        "{leader:?}"
    );
    assert_eq!(leader.last().map(String::as_str), Some("exited"));

    let last = frames.last().expect("last frame");
    assert!(
        last["time_seconds"].as_f64().expect("time") < 300.0,
        "the run settles early"
    );
    let states = last["surface_fire"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|node| node["state"].as_str().expect("state"))
        .collect::<Vec<_>>();
    assert!(states.contains(&"extinguished"), "{states:?}");
    assert!(!states.contains(&"burning"), "{states:?}");
}

#[tokio::test]
async fn an_invalid_crew_is_rejected_before_simulating() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/fire/simulate",
        Some(json!({
            "object_id": "chair-1",
            "crews": [{ "id": "engine-1", "member_count": 2, "search_order": ["attic"] }]
        })),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "firefighting_failed");
}

#[tokio::test]
async fn fire_timeline_moves_supplied_occupants() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/fire/simulate",
        Some(json!({
            "object_id": "chair-1",
            "duration_seconds": 5.0,
            "alarm": "immediate",
            "occupants": [{
                "id": "occupant-1",
                "floor_id": "floor-1",
                "position_m": { "x": 1.0, "y": 1.0, "z": 0.0 },
                "walking_speed_m_per_s": 1.2,
                "pre_movement_delay_seconds": 0.0
            }]
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let frames = body["results"]["frames"].as_array().expect("frames");
    assert_eq!(frames[0]["agents"][0]["role"]["phase"], "unaware");
    assert_ne!(
        frames[0]["agents"][0]["position_m"],
        frames.last().expect("last frame")["agents"][0]["position_m"]
    );
}

#[tokio::test]
async fn occupants_wait_for_the_alarm_by_default() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/fire/simulate",
        Some(json!({
            "object_id": "chair-1",
            "duration_seconds": 3.0,
            "occupants": [{
                "id": "occupant-1",
                "floor_id": "floor-1",
                "position_m": { "x": 1.0, "y": 1.0, "z": 0.0 },
            }]
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let frames = body["results"]["frames"].as_array().expect("frames");
    let last = &frames.last().expect("last frame")["agents"][0];
    assert_eq!(
        last["role"]["phase"], "unaware",
        "a just-lit chair isn't noticed yet"
    );
    assert_eq!(last["position_m"], frames[0]["agents"][0]["position_m"]);
}

#[tokio::test]
async fn a_firefighter_carries_injured_occupants_out() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/fire/simulate",
        Some(json!({
            "object_id": "chair-1",
            "duration_seconds": 300.0,
            "injured_fraction": 1.0,
            "seed": 3,
            "alarm": "immediate",
            "occupants": [{
                "id": "occupant-1",
                "floor_id": "floor-1",
                "position_m": { "x": 1.0, "y": 1.0, "z": 0.0 },
            }],
            "crews": [{ "id": "engine-1", "arrival_time_seconds": 10.0 }]
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let frames = body["results"]["frames"].as_array().expect("frames");
    let occupant_phases = frames
        .iter()
        .map(|frame| frame["agents"][0]["role"]["phase"].as_str().expect("phase"))
        .collect::<Vec<_>>();
    assert_eq!(occupant_phases[0], "injured");
    assert!(occupant_phases.contains(&"carried"), "{occupant_phases:?}");
    assert_eq!(occupant_phases.last(), Some(&"exited"));
    assert_eq!(frames[0]["agents"][0]["role"]["injured"], true);
    let crew_phases = frames
        .iter()
        .map(|frame| frame["agents"][1]["role"]["phase"].as_str().expect("phase"))
        .collect::<std::collections::HashSet<_>>();
    assert!(crew_phases.contains("carrying"), "{crew_phases:?}");
    assert!(
        frames
            .iter()
            .any(|frame| frame["agents"][1]["role"]["nozzle_target_m"].is_object()),
        "the nozzle is aimed while spraying"
    );
}

#[tokio::test]
async fn simulate_fire_rejects_unknown_objects() {
    let (status, body) = call(
        app(),
        "POST",
        "/api/fire/simulate",
        Some(json!({ "object_id": "piano-1" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "object_not_found");
}

fn demo_detection(scene_id: &str) -> String {
    json!({
        "schema_version": 3,
        "scene_id": scene_id,
        "params": tflop_furniture_detect::DetectParams::default(),
        "objects": [{
            "id": "detected-001",
            "floor_id": "floor-1",
            "room_id": "room-1",
            "room_type": null,
            "label": "sofa",
            "asset_id": "sofa-3s",
            "position_m": { "x": 1.5, "y": 1.5, "z": 0.01 },
            "yaw_degrees": 0.0,
            "size_m": { "width_m": 1.8, "depth_m": 0.8, "height_m": 0.8 },
            "footprint_area_m2": 1.44
        }]
    })
    .to_string()
}

#[tokio::test]
async fn scanned_furniture_becomes_scene_objects_that_can_burn() {
    let (scene, outcome) =
        load_scene_with_detection(DEMO_SCENE_JSON, Some(&demo_detection("demo-room")))
            .expect("scene with detection loads");
    assert!(
        matches!(outcome, DetectionOutcome::Merged(summary) if summary.merged == 1),
        "{outcome:?}"
    );
    let app = router(World::new(scene));

    let (_, body) = call(app.clone(), "GET", "/api/world/scene", None).await;
    let objects = body["scene"]["floors"][0]["objects"]
        .as_array()
        .expect("objects");
    assert!(
        objects
            .iter()
            .any(|object| object["id"] == "scan-detected-001")
    );

    let (status, body) = call(
        app,
        "POST",
        "/api/fire/simulate",
        Some(json!({ "object_id": "scan-detected-001", "duration_seconds": 5.0 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[test]
fn detection_for_another_scene_is_ignored() {
    let (scene, outcome) =
        load_scene_with_detection(DEMO_SCENE_JSON, Some(&demo_detection("other-scene")))
            .expect("scene still loads");
    assert!(
        matches!(outcome, DetectionOutcome::Ignored(_)),
        "{outcome:?}"
    );
    assert!(
        scene.floors[0]
            .objects
            .iter()
            .all(|object| !object.id.starts_with("scan-"))
    );
}

/// The bungalow's CoStar record from the costar crate's fixture kit,
/// relabelled for the demo room so it loads beside it.
fn demo_property_json() -> String {
    let kit = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../costar/fixtures/kit");
    let mut record = tflop_costar::find_by_model_id(&kit, "zvKHR9DHNjJ").expect("fixture joins");
    record.scan.model_id = "demo-room".to_owned();
    serde_json::to_string(&record).expect("serializes")
}

#[tokio::test]
async fn property_route_is_404_without_a_costar_record() {
    let (status, body) = call(app(), "GET", "/api/property", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "no_property_record");
}

#[tokio::test]
async fn property_route_serves_the_record_and_derived_profile() {
    let scene = load_scene(DEMO_SCENE_JSON).expect("demo scene is valid");
    let brief = load_property(&demo_property_json(), &scene).expect("record matches scene");
    let app = router_with_property(World::new(scene), Some(brief));
    let (status, body) = call(app, "GET", "/api/property", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["record"]["source"]["provider"], "costar");
    assert_eq!(body["record"]["year_built"], 1920);
    assert_eq!(body["profile"]["occupant_count"]["value"], 6);
    assert_eq!(
        body["profile"]["occupant_count"]["source_field"],
        "property.bedsMax"
    );
    assert_eq!(body["profile"]["floor_material"]["value"], "hardwood");
    assert!(
        body["profile"]["verification"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
    );
}

#[test]
fn a_property_record_for_another_scan_is_rejected() {
    let scene = load_scene(DEMO_SCENE_JSON).expect("demo scene is valid");
    let json = demo_property_json().replace("demo-room", "someOtherId");
    let error = load_property(&json, &scene).expect_err("wrong scan");
    assert!(error.contains("someOtherId"), "{error}");
}

#[tokio::test]
async fn custom_alarm_timings_change_when_occupants_react() {
    let run = |alarm_config: Value| {
        call(
            app(),
            "POST",
            "/api/fire/simulate",
            Some(json!({
                "object_id": "chair-1",
                "duration_seconds": 3.0,
                "alarm_config": alarm_config,
                "occupants": [{
                    "id": "occupant-1",
                    "floor_id": "floor-1",
                    "position_m": { "x": 1.0, "y": 1.0, "z": 0.0 },
                }]
            })),
        )
    };
    let (status, body) = run(json!({
        "notice_hrr_kw": 0.0, "same_room_seconds": 0.0, "seconds_per_metre": 0.0
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let frames = body["results"]["frames"].as_array().expect("frames");
    let last = &frames.last().expect("last frame")["agents"][0];
    assert_ne!(
        last["role"]["phase"], "unaware",
        "an instant alarm is heard"
    );
}

#[tokio::test]
async fn a_hardwood_floor_changes_the_fire() {
    let run = |floor: &str| {
        call(
            app(),
            "POST",
            "/api/fire/simulate",
            Some(
                json!({ "object_id": "chair-1", "duration_seconds": 240.0, "floor_material": floor }),
            ),
        )
    };
    let (carpet_status, carpet) = run("carpet").await;
    let (wood_status, wood) = run("hardwood").await;
    assert_eq!(carpet_status, StatusCode::OK, "{carpet}");
    assert_eq!(wood_status, StatusCode::OK, "{wood}");
    assert_ne!(carpet["results"]["frames"], wood["results"]["frames"]);
}
