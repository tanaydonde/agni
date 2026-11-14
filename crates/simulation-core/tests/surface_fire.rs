use tflop_simulation_core::{
    CombustionState, FireMaterialCatalog, FireMaterialProfile, FurnitureFireMaterial,
    SurfaceFireConfig, SurfaceFireError, SurfaceFireNodeSource, SurfaceFireWorld,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, FurnitureAsset, FurnitureObject, Mobility,
    Quaternion, Room, SCENE_SCHEMA_VERSION, Scene, SceneSource, Transform3, Vec3,
};

#[test]
fn builds_floor_cells_and_three_dimensional_cells_for_furniture() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(true), SurfaceFireConfig::default())
            .expect("surface world should build");

    assert!(
        world
            .nodes()
            .iter()
            .any(|node| matches!(node.source, SurfaceFireNodeSource::FloorCell { .. }))
    );
    let chair = world.node("object:chair-1").expect("chair node exists");
    assert!(matches!(
        chair.source,
        SurfaceFireNodeSource::Furniture { ref object_id } if object_id == "chair-1"
    ));
    assert_eq!(chair.room_id, "room-1");
    assert_eq!(chair.dimensions_m.width_m, 0.2);
    let cell = chair.furniture_cell.expect("furniture cell");
    assert_eq!((cell.column, cell.row, cell.layer), (1, 1, 0));
    assert_eq!((cell.columns, cell.rows, cell.layers), (2, 2, 4));
    assert_eq!(
        world
            .nodes()
            .iter()
            .filter(|node| matches!(
                node.source,
                SurfaceFireNodeSource::Furniture { ref object_id } if object_id == "chair-1"
            ))
            .count(),
        16
    );
}

#[test]
fn fire_climbs_furniture_one_section_at_a_time() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");

    let initial = world
        .ignite_object("chair-1")
        .expect("bottom section should ignite");
    assert_eq!(initial.active_nodes.len(), 1);
    assert_eq!(initial.active_nodes[0].id, "object:chair-1");
    assert_eq!(
        initial.active_nodes[0]
            .furniture_cell
            .expect("cell metadata")
            .layer,
        0
    );

    world.step(1.0).expect("first upward step");
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:1")
            .expect("cell above ignition")
            .state,
        CombustionState::Burning
    );
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:2")
            .expect("cell two layers above")
            .state,
        CombustionState::Unignited
    );

    world.step(1.0).expect("second upward step");
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:2")
            .expect("cell two layers above")
            .state,
        CombustionState::Burning
    );
}

#[test]
fn fire_spreads_from_floor_into_bottom_of_furniture_then_upward() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(true), SurfaceFireConfig::default())
            .expect("surface world should build");
    let ignition_cell_position = world
        .node("object:chair-1")
        .expect("default chair ignition cell")
        .position_m;
    let floor_beneath_chair = world
        .nodes()
        .iter()
        .filter(|node| matches!(node.source, SurfaceFireNodeSource::FloorCell { .. }))
        .min_by(|left, right| {
            let left_distance = (left.position_m.x - ignition_cell_position.x)
                .hypot(left.position_m.y - ignition_cell_position.y);
            let right_distance = (right.position_m.x - ignition_cell_position.x)
                .hypot(right.position_m.y - ignition_cell_position.y);
            left_distance.total_cmp(&right_distance)
        })
        .expect("floor beneath chair")
        .id
        .clone();

    world
        .ignite_node(&floor_beneath_chair)
        .expect("floor should ignite");
    world.step(1.0).expect("floor should heat chair base");
    assert_eq!(
        world.node("object:chair-1").expect("chair bottom").state,
        CombustionState::Burning
    );
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:1")
            .expect("chair cell above base")
            .state,
        CombustionState::Unignited
    );

    world.step(1.0).expect("chair fire should climb");
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:1")
            .expect("chair cell above base")
            .state,
        CombustionState::Burning
    );
}

#[test]
fn wide_furniture_burns_across_its_base_cell_by_cell() {
    let mut scene = scene_with_chairs(&[("bench-1", 1.5, 2.0)]);
    scene.assets[0].dimensions_m = Dimensions3 {
        width_m: 1.0,
        depth_m: 0.25,
        height_m: 0.25,
    };
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");

    world
        .ignite_object_cell("bench-1", 0, 0, 0)
        .expect("leftmost cell should ignite");
    assert_eq!(
        world
            .node("object:bench-1:cell:3:0:0")
            .expect("rightmost cell")
            .state,
        CombustionState::Unignited
    );

    world.step(1.0).expect("fire should move one cell");
    assert_eq!(
        world
            .node("object:bench-1:cell:1:0:0")
            .expect("neighbor cell")
            .state,
        CombustionState::Burning
    );
    assert_eq!(
        world
            .node("object:bench-1:cell:3:0:0")
            .expect("rightmost cell")
            .state,
        CombustionState::Unignited
    );
}

#[test]
fn furniture_cell_positions_follow_the_object_rotation() {
    let mut scene = scene_with_chairs(&[("bench-1", 1.5, 2.0)]);
    scene.assets[0].dimensions_m = Dimensions3 {
        width_m: 1.0,
        depth_m: 0.25,
        height_m: 0.25,
    };
    let half_sqrt_two = std::f64::consts::FRAC_1_SQRT_2;
    scene.floors[0].objects[0].transform.rotation = Quaternion {
        x: 0.0,
        y: 0.0,
        z: half_sqrt_two,
        w: half_sqrt_two,
    };
    let world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("rotated surface world should build");
    let left = world
        .node("object:bench-1:cell:0:0:0")
        .expect("first width cell");
    let right = world
        .node("object:bench-1:cell:3:0:0")
        .expect("last width cell");

    assert!((left.position_m.x - right.position_m.x).abs() < 1.0e-9);
    assert!((left.position_m.y - right.position_m.y).abs() > 0.7);
}

#[test]
fn spreads_between_nearby_furniture_hitboxes() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0), ("chair-2", 2.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");

    world
        .ignite_object("chair-1")
        .expect("first chair should ignite");
    let frame = world.step(1.0).expect("step should succeed");

    assert_eq!(
        world.node("object:chair-2").expect("second chair").state,
        CombustionState::Burning
    );
    assert!(frame.active_nodes.iter().any(|node| {
        matches!(
            node.source,
            SurfaceFireNodeSource::Furniture { ref object_id } if object_id == "chair-2"
        ) && node.ignition_time_seconds == Some(1.0)
    }));
}

#[test]
fn spreads_across_neighboring_floor_cells() {
    let scene = scene_with_chairs(&[]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(true), SurfaceFireConfig::default())
            .expect("surface world should build");
    let initial_floor_id = world
        .nodes()
        .iter()
        .find(|node| matches!(node.source, SurfaceFireNodeSource::FloorCell { .. }))
        .expect("floor node exists")
        .id
        .clone();

    world
        .ignite_node(&initial_floor_id)
        .expect("floor should ignite");
    world.step(1.0).expect("step should succeed");

    assert!(
        world
            .nodes()
            .iter()
            .filter(|node| node.state == CombustionState::Burning)
            .count()
            > 1
    );
}

#[test]
fn burning_nodes_consume_fuel_and_burn_out() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut catalog = fast_catalog(false);
    catalog.default_furniture.fuel_energy_kj_per_m2 = 1.0;
    catalog.furniture[0].material.fuel_energy_kj_per_m2 = 1.0;
    let mut world = SurfaceFireWorld::from_scene(&scene, &catalog, SurfaceFireConfig::default())
        .expect("surface world should build");

    world.ignite_object("chair-1").expect("chair ignites");
    let frame = world.step(1.0).expect("step should succeed");

    let chair = frame
        .active_nodes
        .iter()
        .find(|node| node.id == "object:chair-1")
        .expect("chair remains in active timeline");
    assert_eq!(chair.state, CombustionState::BurnedOut);
    assert_eq!(chair.heat_release_rate_kw, 0.0);
    assert_eq!(chair.fuel_remaining_fraction, 0.0);
}

#[test]
fn rejects_invalid_steps_without_advancing_time() {
    let scene = scene_with_chairs(&[]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(true), SurfaceFireConfig::default())
            .expect("surface world should build");

    assert!(matches!(
        world.step(0.0),
        Err(SurfaceFireError::InvalidDeltaSeconds { .. })
    ));
    assert_eq!(world.snapshot().time_seconds, 0.0);
}

#[test]
fn frame_json_preserves_the_furniture_object_id() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    let frame = world.ignite_object("chair-1").expect("chair ignites");

    let json = serde_json::to_value(frame).expect("frame should serialize");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["active_nodes"][0]["source"]["type"], "furniture");
    assert_eq!(json["active_nodes"][0]["source"]["object_id"], "chair-1");
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["column"], 1);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["row"], 1);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["layer"], 0);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["columns"], 2);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["rows"], 2);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["layers"], 4);
    assert_eq!(json["active_nodes"][0]["state"], "burning");
    assert!(json["active_nodes"][0]["heat_release_rate_kw"].is_number());
}

#[test]
fn furniture_heat_does_not_jump_through_a_room_boundary() {
    let mut scene = scene_with_chairs(&[("chair-1", 3.5, 2.0), ("chair-2", 4.5, 2.0)]);
    scene.floors[0].rooms.push(Room {
        id: "room-2".to_owned(),
        label: None,
        classifications: vec![],
        dimensions: None,
        boundary: vec![
            edge("south-2", point(4.0, 0.0), point(8.0, 0.0)),
            edge("east-2", point(8.0, 0.0), point(8.0, 4.0)),
            edge("north-2", point(8.0, 4.0), point(4.0, 4.0)),
            edge("west-2", point(4.0, 4.0), point(4.0, 0.0)),
        ],
    });

    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair ignites");
    world.step(1.0).expect("step should succeed");

    assert_eq!(
        world.node("object:chair-2").expect("second chair").state,
        CombustionState::Unignited
    );
}

fn fast_catalog(combustible_floor: bool) -> FireMaterialCatalog {
    let furniture = fast_material("test_furniture");
    FireMaterialCatalog {
        floor: if combustible_floor {
            fast_material("test_floor")
        } else {
            FireMaterialProfile::noncombustible("concrete")
        },
        default_furniture: furniture.clone(),
        furniture: vec![FurnitureFireMaterial {
            category: "chair".to_owned(),
            material: furniture,
        }],
    }
}

fn fast_material(id: &str) -> FireMaterialProfile {
    FireMaterialProfile {
        id: id.to_owned(),
        combustible: true,
        ignition_temperature_c: 200.0,
        ignition_energy_kj_per_m2: 1.0,
        peak_hrr_kw_per_m2: 200.0,
        fuel_energy_kj_per_m2: 1_000.0,
        heat_transfer_fraction: 0.8,
        cooling_kw_per_m2: 0.0,
    }
}

fn scene_with_chairs(chairs: &[(&str, f64, f64)]) -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "fire-scene".to_owned(),
        name: None,
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "fire-scene".to_owned(),
        },
        assets: vec![FurnitureAsset {
            id: "chair-basic".to_owned(),
            category: "chair".to_owned(),
            model_uri: None,
            dimensions_m: Dimensions3 {
                width_m: 0.4,
                depth_m: 0.4,
                height_m: 0.9,
            },
        }],
        floors: vec![Floor {
            id: "floor-1".to_owned(),
            label: None,
            elevation_m: Some(0.0),
            rooms: vec![Room {
                id: "room-1".to_owned(),
                label: None,
                classifications: vec![],
                dimensions: None,
                boundary: vec![
                    edge("south", point(0.0, 0.0), point(4.0, 0.0)),
                    edge("east", point(4.0, 0.0), point(4.0, 4.0)),
                    edge("north", point(4.0, 4.0), point(0.0, 4.0)),
                    edge("west", point(0.0, 4.0), point(0.0, 0.0)),
                ],
            }],
            objects: chairs
                .iter()
                .map(|(id, x, y)| FurnitureObject {
                    id: (*id).to_owned(),
                    asset_id: "chair-basic".to_owned(),
                    transform: Transform3 {
                        position_m: point(*x, *y),
                        rotation: Quaternion::IDENTITY,
                        scale: Vec3 {
                            x: 1.0,
                            y: 1.0,
                            z: 1.0,
                        },
                    },
                    mobility: Mobility::Movable,
                })
                .collect(),
        }],
    }
}

fn edge(id: &str, start: Vec3, end: Vec3) -> BoundaryEdge {
    BoundaryEdge {
        id: id.to_owned(),
        kind: BoundaryKind::Wall,
        start,
        end,
        thickness_m: Some(0.1),
        center_line_bias: Some(0.5),
        openings: vec![],
    }
}

fn point(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
