use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, FurnitureAsset, FurnitureObject, Mobility,
    Opening, OpeningKind, Quaternion, Room, RoomClassification, RoomDimensions,
    SCENE_SCHEMA_VERSION, Scene, SceneSource, Transform3, Vec3,
};

#[test]
fn scene_json_round_trip_preserves_data() {
    let scene = valid_scene();

    let json = serde_json::to_string_pretty(&scene).expect("scene should serialize");
    let decoded: Scene = serde_json::from_str(&json).expect("scene should deserialize");

    assert_eq!(decoded, scene);
    assert!(decoded.validate().is_ok());
}

#[test]
fn validation_rejects_non_finite_coordinates() {
    let mut scene = valid_scene();
    scene.floors[0].rooms[0].boundary[0].start.x = f64::NAN;

    let errors = scene.validate().expect_err("NaN must be rejected");

    assert!(has_error(
        &errors.0,
        "floors[0].rooms[0].boundary[0].start.x",
        "finite"
    ));
}

#[test]
fn validation_rejects_negative_dimensions() {
    let mut scene = valid_scene();
    scene.floors[0].rooms[0]
        .dimensions
        .as_mut()
        .expect("fixture has dimensions")
        .height_m = Some(-1.0);

    let errors = scene
        .validate()
        .expect_err("negative dimensions must be rejected");

    assert!(has_error(
        &errors.0,
        "floors[0].rooms[0].dimensions.height_m",
        "negative"
    ));
}

#[test]
fn validation_rejects_degenerate_edges() {
    let mut scene = valid_scene();
    scene.floors[0].rooms[0].boundary[0].end = scene.floors[0].rooms[0].boundary[0].start;

    let errors = scene
        .validate()
        .expect_err("zero-length edges must be rejected");

    assert!(has_error(
        &errors.0,
        "floors[0].rooms[0].boundary[0]",
        "start and end"
    ));
}

#[test]
fn validation_rejects_openings_that_do_not_fit_their_edge() {
    let mut scene = valid_scene();
    scene.floors[0].rooms[0].boundary[0].openings[0].relative_center = 0.05;

    let errors = scene
        .validate()
        .expect_err("opening extending beyond its edge must be rejected");

    assert!(has_error(
        &errors.0,
        "floors[0].rooms[0].boundary[0].openings[0]",
        "fit completely"
    ));
}

#[test]
fn validation_rejects_objects_with_missing_assets() {
    let mut scene = valid_scene();
    scene.floors[0].objects[0].asset_id = "missing-asset".to_owned();

    let errors = scene
        .validate()
        .expect_err("unknown asset references must be rejected");

    assert!(has_error(
        &errors.0,
        "floors[0].objects[0].asset_id",
        "existing asset"
    ));
}

#[test]
fn validation_rejects_non_normalized_rotations() {
    let mut scene = valid_scene();
    scene.floors[0].objects[0].transform.rotation.w = 2.0;

    let errors = scene
        .validate()
        .expect_err("non-normalized rotations must be rejected");

    assert!(has_error(
        &errors.0,
        "floors[0].objects[0].transform.rotation",
        "normalized"
    ));
}

fn has_error(
    errors: &[tflop_world_core::SceneValidationError],
    path: &str,
    message_fragment: &str,
) -> bool {
    errors
        .iter()
        .any(|error| error.path == path && error.message.contains(message_fragment))
}

fn valid_scene() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "scene-1".to_owned(),
        name: Some("Example scene".to_owned()),
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "external-scene-1".to_owned(),
        },
        assets: vec![FurnitureAsset {
            id: "chair".to_owned(),
            category: "chair".to_owned(),
            model_uri: Some("assets/chair.glb".to_owned()),
            dimensions_m: Dimensions3 {
                width_m: 0.5,
                depth_m: 0.5,
                height_m: 0.9,
            },
        }],
        floors: vec![Floor {
            id: "floor-1".to_owned(),
            label: Some("Ground floor".to_owned()),
            elevation_m: Some(0.0),
            rooms: vec![Room {
                id: "room-1".to_owned(),
                label: Some("Example room".to_owned()),
                classifications: vec![RoomClassification {
                    label: "Office".to_owned(),
                    confidence: Some(0.95),
                }],
                dimensions: Some(RoomDimensions {
                    area_m2: Some(20.0),
                    width_m: Some(5.0),
                    depth_m: Some(4.0),
                    height_m: Some(2.7),
                    lower_elevation_m: Some(0.0),
                }),
                boundary: vec![BoundaryEdge {
                    id: "edge-1".to_owned(),
                    kind: BoundaryKind::Wall,
                    start: Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    end: Vec3 {
                        x: 5.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    thickness_m: Some(0.15),
                    center_line_bias: Some(0.5),
                    openings: vec![Opening {
                        id: "opening-1".to_owned(),
                        label: Some("Main door".to_owned()),
                        kind: OpeningKind::Door,
                        width_m: 1.0,
                        height_m: Some(2.1),
                        lower_elevation_m: Some(0.0),
                        relative_center: 0.5,
                    }],
                }],
            }],
            objects: vec![FurnitureObject {
                id: "chair-1".to_owned(),
                asset_id: "chair".to_owned(),
                transform: Transform3 {
                    position_m: Vec3 {
                        x: 1.0,
                        y: 1.0,
                        z: 0.0,
                    },
                    rotation: Quaternion::IDENTITY,
                    scale: Vec3 {
                        x: 1.0,
                        y: 1.0,
                        z: 1.0,
                    },
                },
                mobility: Mobility::Movable,
            }],
        }],
    }
}
