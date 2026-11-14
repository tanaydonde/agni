use tflop_world_core::{
    AddObjectRequest, ApplyEditsRequest, BoundaryEdge, BoundaryKind, Dimensions3, EditCommand,
    Floor, FurnitureAsset, Mobility, MoveObjectRequest, Opening, OpeningKind, Quaternion,
    RemoveObjectRequest, Room, RotateObjectRequest, SCENE_SCHEMA_VERSION, Scene, SceneSource,
    SetObjectTransformRequest, Transform3, Vec3, World, WorldApiError,
};

#[test]
fn adds_moves_rotates_and_removes_an_object() {
    let mut world = test_world();

    let added = world
        .add_object(add_request(0, "chair-1", Mobility::Movable))
        .expect("valid object should be added");
    assert_eq!(added.revision, 1);

    let moved = world
        .move_object(MoveObjectRequest {
            expected_revision: 1,
            object_id: "chair-1".to_owned(),
            position_m: Vec3 {
                x: 3.0,
                y: 4.0,
                z: 0.0,
            },
        })
        .expect("movable object should move");
    assert_eq!(moved.revision, 2);

    let rotated = world
        .rotate_object(RotateObjectRequest {
            expected_revision: 2,
            object_id: "chair-1".to_owned(),
            rotation_z_degrees: 90.0,
        })
        .expect("movable object should rotate");
    assert_eq!(rotated.revision, 3);

    let snapshot = world.get_scene();
    let object = &snapshot.scene.floors[0].objects[0];
    assert_eq!(object.transform.position_m.x, 3.0);
    assert!((object.transform.rotation.z - 2.0_f64.sqrt() / 2.0).abs() < 1.0e-12);

    let removed = world
        .remove_object(RemoveObjectRequest {
            expected_revision: 3,
            object_id: "chair-1".to_owned(),
        })
        .expect("movable object should be removed");
    assert_eq!(removed.revision, 4);
    assert!(world.get_scene().scene.floors[0].objects.is_empty());
}

#[test]
fn rejects_stale_revisions_duplicate_ids_and_missing_assets() {
    let mut world = test_world();
    world
        .add_object(add_request(0, "chair-1", Mobility::Movable))
        .expect("first add should work");

    assert_eq!(
        world
            .add_object(add_request(0, "chair-2", Mobility::Movable))
            .expect_err("revision zero is stale"),
        WorldApiError::StaleRevision {
            expected_revision: 0,
            current_revision: 1,
        }
    );
    assert_eq!(
        world
            .add_object(add_request(1, "chair-1", Mobility::Movable))
            .expect_err("duplicate ID should fail"),
        WorldApiError::DuplicateObjectId {
            object_id: "chair-1".to_owned(),
        }
    );

    let mut missing_asset = add_request(1, "chair-2", Mobility::Movable);
    missing_asset.asset_id = "missing".to_owned();
    assert_eq!(
        world
            .add_object(missing_asset)
            .expect_err("missing asset should fail"),
        WorldApiError::AssetNotFound {
            asset_id: "missing".to_owned(),
        }
    );
}

#[test]
fn rejects_invalid_transforms_and_changes_to_fixed_objects() {
    let mut world = test_world();
    world
        .add_object(add_request(0, "fixed-1", Mobility::Fixed))
        .expect("fixed object may be created");

    assert_eq!(
        world
            .remove_object(RemoveObjectRequest {
                expected_revision: 1,
                object_id: "fixed-1".to_owned(),
            })
            .expect_err("fixed object cannot be removed"),
        WorldApiError::ObjectFixed {
            object_id: "fixed-1".to_owned(),
        }
    );

    let error = world
        .set_object_transform(SetObjectTransformRequest {
            expected_revision: 1,
            object_id: "fixed-1".to_owned(),
            transform: Transform3 {
                position_m: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: Quaternion::IDENTITY,
                scale: Vec3 {
                    x: -1.0,
                    y: 1.0,
                    z: 1.0,
                },
            },
        })
        .expect_err("negative scale must fail");
    assert!(matches!(error, WorldApiError::InvalidRequest { .. }));
}

#[test]
fn failed_batches_are_atomic() {
    let mut world = test_world();

    let error = world
        .apply_edits(ApplyEditsRequest {
            expected_revision: 0,
            edits: vec![
                EditCommand::AddObject {
                    object_id: "chair-1".to_owned(),
                    asset_id: "chair-basic".to_owned(),
                    floor_id: "floor-1".to_owned(),
                    position_m: origin(),
                    rotation_z_degrees: 0.0,
                    mobility: Mobility::Movable,
                },
                EditCommand::MoveObject {
                    object_id: "missing".to_owned(),
                    position_m: origin(),
                },
            ],
        })
        .expect_err("batch should fail on its second edit");

    assert!(matches!(
        error,
        WorldApiError::BatchEditFailed { edit_index: 1, .. }
    ));
    let snapshot = world.get_scene();
    assert_eq!(snapshot.revision, 0);
    assert!(snapshot.scene.floors[0].objects.is_empty());
}

#[test]
fn successful_batches_increment_revision_once() {
    let mut world = test_world();

    let response = world
        .apply_edits(ApplyEditsRequest {
            expected_revision: 0,
            edits: vec![
                EditCommand::AddObject {
                    object_id: "chair-1".to_owned(),
                    asset_id: "chair-basic".to_owned(),
                    floor_id: "floor-1".to_owned(),
                    position_m: origin(),
                    rotation_z_degrees: 0.0,
                    mobility: Mobility::Movable,
                },
                EditCommand::MoveObject {
                    object_id: "chair-1".to_owned(),
                    position_m: Vec3 {
                        x: 4.0,
                        y: 2.0,
                        z: 0.0,
                    },
                },
            ],
        })
        .expect("batch should succeed");

    assert_eq!(response.revision, 1);
    assert_eq!(response.changed_object_ids, vec!["chair-1"]);
    assert_eq!(
        world.get_scene().scene.floors[0].objects[0]
            .transform
            .position_m
            .x,
        4.0
    );
}

#[test]
fn rejects_furniture_outside_rooms_and_preserves_revision() {
    let mut world = test_world();
    let mut request = add_request(0, "outside", Mobility::Movable);
    request.position_m = position(12.0, 2.0);

    assert_eq!(
        world.add_object(request).expect_err("object is outside"),
        WorldApiError::ObjectOutsideRooms {
            object_id: "outside".to_owned(),
            floor_id: "floor-1".to_owned(),
        }
    );
    assert_eq!(world.get_scene().revision, 0);
    assert!(world.get_scene().scene.floors[0].objects.is_empty());
}

#[test]
fn rejects_furniture_overlap_but_allows_touching_edges() {
    let mut world = test_world();
    world
        .add_object(add_request(0, "chair-1", Mobility::Movable))
        .expect("first chair fits");

    let mut overlapping = add_request(1, "chair-2", Mobility::Movable);
    overlapping.position_m = position(2.4, 2.0);
    assert_eq!(
        world
            .add_object(overlapping)
            .expect_err("chairs overlap by 0.1 m"),
        WorldApiError::FurnitureOverlap {
            object_id: "chair-2".to_owned(),
            conflicting_object_id: "chair-1".to_owned(),
        }
    );

    let mut rotated_overlap = add_request(1, "chair-rotated", Mobility::Movable);
    rotated_overlap.position_m = position(2.6, 2.0);
    rotated_overlap.rotation_z_degrees = 90.0;
    assert!(matches!(
        world.add_object(rotated_overlap),
        Err(WorldApiError::FurnitureOverlap { object_id, .. }) if object_id == "chair-rotated"
    ));

    let mut touching = add_request(1, "chair-3", Mobility::Movable);
    touching.position_m = position(2.5, 2.0);
    world
        .add_object(touching)
        .expect("chairs may touch without overlapping");
}

#[test]
fn rejects_wall_collisions_and_door_clearance_obstructions() {
    let mut world = test_world();

    let mut against_wall = add_request(0, "wall-chair", Mobility::Movable);
    against_wall.position_m = position(0.2, 8.0);
    assert!(matches!(
        world.add_object(against_wall),
        Err(WorldApiError::WallCollision { edge_id, .. }) if edge_id == "west"
    ));

    let mut blocks_door = add_request(0, "door-chair", Mobility::Movable);
    blocks_door.position_m = position(5.0, 0.6);
    assert!(matches!(
        world.add_object(blocks_door),
        Err(WorldApiError::OpeningClearanceBlocked { opening_id, .. }) if opening_id == "door-1"
    ));
}

#[test]
fn failed_move_keeps_the_previous_transform_and_revision() {
    let mut world = test_world();
    world
        .add_object(add_request(0, "chair-1", Mobility::Movable))
        .expect("chair fits");

    let error = world
        .move_object(MoveObjectRequest {
            expected_revision: 1,
            object_id: "chair-1".to_owned(),
            position_m: position(20.0, 20.0),
        })
        .expect_err("invalid move should fail");
    assert!(matches!(error, WorldApiError::ObjectOutsideRooms { .. }));
    let snapshot = world.get_scene();
    assert_eq!(snapshot.revision, 1);
    assert_eq!(
        snapshot.scene.floors[0].objects[0].transform.position_m,
        origin()
    );
}

#[test]
fn validates_layout_and_serializes_tagged_commands() {
    let world = test_world();
    assert!(world.validate_layout().expect("validation runs").valid);

    let command = EditCommand::RotateObject {
        object_id: "chair-1".to_owned(),
        rotation_z_degrees: 45.0,
    };
    assert_eq!(
        serde_json::to_value(command).expect("command should serialize"),
        serde_json::json!({
            "type": "rotate_object",
            "object_id": "chair-1",
            "rotation_z_degrees": 45.0
        })
    );
}

fn add_request(revision: u64, object_id: &str, mobility: Mobility) -> AddObjectRequest {
    AddObjectRequest {
        expected_revision: revision,
        object_id: object_id.to_owned(),
        asset_id: "chair-basic".to_owned(),
        floor_id: "floor-1".to_owned(),
        position_m: origin(),
        rotation_z_degrees: 0.0,
        mobility,
    }
}

fn test_world() -> World {
    World::new(Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "scene-1".to_owned(),
        name: Some("Test scene".to_owned()),
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "scene-1".to_owned(),
        },
        assets: vec![FurnitureAsset {
            id: "chair-basic".to_owned(),
            category: "chair".to_owned(),
            model_uri: Some("assets/chair.glb".to_owned()),
            dimensions_m: Dimensions3 {
                width_m: 0.5,
                depth_m: 0.8,
                height_m: 0.9,
            },
        }],
        floors: vec![Floor {
            id: "floor-1".to_owned(),
            label: Some("Floor 1".to_owned()),
            elevation_m: Some(0.0),
            rooms: vec![test_room()],
            objects: vec![],
        }],
    })
}

fn origin() -> Vec3 {
    position(2.0, 2.0)
}

fn position(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}

fn test_room() -> Room {
    Room {
        id: "room-1".to_owned(),
        label: Some("Test room".to_owned()),
        classifications: vec![],
        dimensions: None,
        boundary: vec![
            edge(
                "south",
                position(0.0, 0.0),
                position(10.0, 0.0),
                vec![Opening {
                    id: "door-1".to_owned(),
                    label: Some("Door".to_owned()),
                    kind: OpeningKind::Door,
                    width_m: 1.0,
                    height_m: Some(2.1),
                    lower_elevation_m: Some(0.0),
                    relative_center: 0.5,
                }],
            ),
            edge("east", position(10.0, 0.0), position(10.0, 10.0), vec![]),
            edge("north", position(10.0, 10.0), position(0.0, 10.0), vec![]),
            edge("west", position(0.0, 10.0), position(0.0, 0.0), vec![]),
        ],
    }
}

fn edge(id: &str, start: Vec3, end: Vec3, openings: Vec<Opening>) -> BoundaryEdge {
    BoundaryEdge {
        id: id.to_owned(),
        kind: BoundaryKind::Wall,
        start,
        end,
        thickness_m: Some(0.2),
        center_line_bias: Some(0.5),
        openings,
    }
}
