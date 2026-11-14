use tflop_cfast::{
    CfastExportError, CfastFire, CfastFuel, CfastOpeningState, CfastScenario, HeatReleasePoint,
    export_all_rooms, export_single_room,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Floor, Opening, OpeningKind, Room, RoomDimensions,
    SCENE_SCHEMA_VERSION, Scene, SceneSource, Vec3,
};

#[test]
fn exports_one_room_one_exterior_vent_and_one_fire() {
    let input =
        export_single_room(&scene_fixture(), &scenario_fixture()).expect("fixture should export");

    assert!(input.contains("&HEAD VERSION = 7700"));
    assert!(input.contains("&COMP ID = 'Demo Room'"));
    assert!(input.contains("DEPTH = 4 HEIGHT = 3 WIDTH = 5"));
    assert!(input.contains("ID = 'Exterior Door' COMP_IDS = 'Demo Room', 'OUTSIDE'"));
    assert!(input.contains("BOTTOM = 0 HEIGHT = 2 WIDTH = 1"));
    assert!(input.contains("F = 1, 1 FACE = 'FRONT' OFFSET = 2"));
    assert!(input.contains("ID = 'Demo Fire' COMP_ID = 'Demo Room'"));
    assert!(input.contains("LOCATION = 2.5, 2"));
    assert!(input.contains("DATA = 60, 300, 0, 0.3, 0.006, 0.015, 0, 0, 0"));
    assert!(input.ends_with("&TAIL /\n"));
}

#[test]
fn normalizes_translated_world_coordinates_into_the_compartment() {
    let mut scene = scene_fixture();
    for edge in &mut scene.floors[0].rooms[0].boundary {
        edge.start.x += 10.0;
        edge.end.x += 10.0;
        edge.start.y += 20.0;
        edge.end.y += 20.0;
    }
    let mut scenario = scenario_fixture();
    scenario.fire.position_m = point(12.5, 22.0);

    let input = export_single_room(&scene, &scenario).expect("translated room should export");

    assert!(input.contains("LOCATION = 2.5, 2"));
    assert!(input.contains("FACE = 'FRONT' OFFSET = 2"));
    assert!(input.contains("ORIGIN = 0, 0, 0"));
}

#[test]
fn rejects_fires_outside_the_selected_room() {
    let mut scenario = scenario_fixture();
    scenario.fire.position_m = point(20.0, 20.0);

    assert!(matches!(
        export_single_room(&scene_fixture(), &scenario),
        Err(CfastExportError::FireOutsideRoom { .. })
    ));
}

#[test]
fn rejects_missing_openings_and_invalid_fire_curves() {
    let mut missing_opening = scenario_fixture();
    missing_opening.exterior_opening_id = "missing".to_owned();
    assert!(matches!(
        export_single_room(&scene_fixture(), &missing_opening),
        Err(CfastExportError::OpeningNotFound { .. })
    ));

    let mut invalid_curve = scenario_fixture();
    invalid_curve.fire.heat_release_curve[1].time_seconds = 0.0;
    assert!(matches!(
        export_single_room(&scene_fixture(), &invalid_curve),
        Err(CfastExportError::InvalidScenario { .. })
    ));
}

#[test]
fn exports_every_room_as_a_positioned_compartment() {
    let scene = two_room_scene();
    let input = export_all_rooms(&scene, &scenario_fixture()).expect("both rooms should export");

    assert_eq!(input.matches("&COMP ID =").count(), 2);
    assert!(input.contains("&COMP ID = 'Demo Room'"));
    assert!(input.contains("&COMP ID = 'Second Room'"));
    assert!(input.contains("ORIGIN = 6, 0, 0"));
    assert_eq!(input.matches("&VENT TYPE = 'WALL'").count(), 1);
    assert_eq!(input.matches("&FIRE ID =").count(), 1);
}

#[test]
fn rejects_duplicate_room_ids_in_the_all_room_export() {
    let mut scene = two_room_scene();
    scene.floors[0].rooms[1].id = "Demo Room".to_owned();

    assert!(matches!(
        export_all_rooms(&scene, &scenario_fixture()),
        Err(CfastExportError::DuplicateRoomId { .. })
    ));
}

#[test]
fn exports_resolved_room_connections_with_their_open_fraction() {
    let scene = connected_two_room_scene();
    let mut scenario = scenario_fixture();
    scenario.opening_states.push(CfastOpeningState {
        opening_id: "Interior Door".to_owned(),
        open_fraction: 0.5,
    });

    let input = export_all_rooms(&scene, &scenario).expect("connected rooms should export");

    assert!(input.contains("ID = 'Interior Door' COMP_IDS = 'Demo Room', 'Second Room'"));
    assert!(input.contains("F = 0.5, 0.5 FACE = 'RIGHT'"));
    assert_eq!(input.matches("&VENT TYPE = 'WALL'").count(), 2);
}

#[test]
fn requires_an_explicit_state_for_every_resolved_opening() {
    assert!(matches!(
        export_all_rooms(&connected_two_room_scene(), &scenario_fixture()),
        Err(CfastExportError::MissingOpeningState { opening_id })
            if opening_id == "Interior Door"
    ));
}

fn scenario_fixture() -> CfastScenario {
    CfastScenario {
        id: "feasibility".to_owned(),
        title: "tflop CFAST feasibility scenario".to_owned(),
        room_id: "Demo Room".to_owned(),
        exterior_opening_id: "Exterior Door".to_owned(),
        duration_seconds: 120.0,
        output_interval_seconds: 1.0,
        interior_temperature_c: 20.0,
        exterior_temperature_c: 20.0,
        relative_humidity_percent: 50.0,
        opening_states: vec![CfastOpeningState {
            opening_id: "Exterior Door".to_owned(),
            open_fraction: 1.0,
        }],
        fire: CfastFire {
            id: "Demo Fire".to_owned(),
            position_m: point(2.5, 2.0),
            area_m2: 0.3,
            fuel: CfastFuel::default(),
            heat_release_curve: vec![
                HeatReleasePoint {
                    time_seconds: 0.0,
                    hrr_kw: 0.0,
                },
                HeatReleasePoint {
                    time_seconds: 30.0,
                    hrr_kw: 75.0,
                },
                HeatReleasePoint {
                    time_seconds: 60.0,
                    hrr_kw: 300.0,
                },
                HeatReleasePoint {
                    time_seconds: 120.0,
                    hrr_kw: 300.0,
                },
            ],
        },
    }
}

fn scene_fixture() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "scene-1".to_owned(),
        name: Some("CFAST fixture".to_owned()),
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "scene-1".to_owned(),
        },
        assets: vec![],
        floors: vec![Floor {
            id: "floor-1".to_owned(),
            label: Some("Ground floor".to_owned()),
            elevation_m: Some(0.0),
            rooms: vec![Room {
                id: "Demo Room".to_owned(),
                label: Some("Demo Room".to_owned()),
                classifications: vec![],
                dimensions: Some(RoomDimensions {
                    area_m2: Some(20.0),
                    width_m: Some(5.0),
                    depth_m: Some(4.0),
                    height_m: Some(3.0),
                    lower_elevation_m: Some(0.0),
                }),
                boundary: vec![
                    edge(
                        "south",
                        point(0.0, 0.0),
                        point(5.0, 0.0),
                        vec![Opening {
                            id: "Exterior Door".to_owned(),
                            label: Some("Exterior Door".to_owned()),
                            kind: OpeningKind::Door,
                            width_m: 1.0,
                            height_m: Some(2.0),
                            lower_elevation_m: Some(0.0),
                            relative_center: 0.5,
                        }],
                    ),
                    edge("east", point(5.0, 0.0), point(5.0, 4.0), vec![]),
                    edge("north", point(5.0, 4.0), point(0.0, 4.0), vec![]),
                    edge("west", point(0.0, 4.0), point(0.0, 0.0), vec![]),
                ],
            }],
            objects: vec![],
        }],
    }
}

fn two_room_scene() -> Scene {
    let mut scene = scene_fixture();
    let mut room = scene.floors[0].rooms[0].clone();
    room.id = "Second Room".to_owned();
    room.label = Some("Second Room".to_owned());
    for edge in &mut room.boundary {
        edge.start.x += 6.0;
        edge.end.x += 6.0;
        edge.openings.clear();
    }
    scene.floors[0].rooms.push(room);
    scene
}

fn connected_two_room_scene() -> Scene {
    let mut scene = scene_fixture();
    scene.floors[0].rooms[0].boundary[1].openings = vec![Opening {
        id: "Interior Door".to_owned(),
        label: None,
        kind: OpeningKind::Door,
        width_m: 1.0,
        height_m: Some(2.0),
        lower_elevation_m: Some(0.0),
        relative_center: 0.5,
    }];
    let mut room = scene.floors[0].rooms[0].clone();
    room.id = "Second Room".to_owned();
    room.label = Some("Second Room".to_owned());
    for edge in &mut room.boundary {
        edge.start.x += 5.0;
        edge.end.x += 5.0;
        edge.openings.clear();
    }
    scene.floors[0].rooms.push(room);
    scene
}

fn edge(id: &str, start: Vec3, end: Vec3, openings: Vec<Opening>) -> BoundaryEdge {
    BoundaryEdge {
        id: id.to_owned(),
        kind: BoundaryKind::Wall,
        start,
        end,
        thickness_m: Some(0.1),
        center_line_bias: Some(0.5),
        openings,
    }
}

fn point(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
