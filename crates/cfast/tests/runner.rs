use tflop_cfast::{
    CfastFire, CfastFuel, CfastOpeningState, CfastRunner, CfastScenario, HeatReleasePoint,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Floor, Opening, OpeningKind, Room, RoomDimensions,
    SCENE_SCHEMA_VERSION, Scene, SceneSource, Vec3,
};

/// Run manually with:
/// `CFAST_BIN=/path/to/cfast7 cargo test -p tflop-cfast --test runner -- --ignored`
#[test]
#[ignore = "requires an external CFAST 7.7.7 executable"]
fn runs_the_real_solver_and_parses_its_results() {
    let executable = std::env::var("CFAST_BIN").expect("CFAST_BIN must point to CFAST 7.7.7");
    let runner = CfastRunner::new(executable).expect("CFAST executable should resolve");

    let results = runner
        .run_single_room(&scene_fixture(), &scenario_fixture())
        .expect("CFAST should run and parse");

    assert_eq!(results.engine.name, "cfast");
    assert_eq!(results.engine.version, "7.7.7");
    assert_eq!(
        results.frames.first().expect("frames exist").time_seconds,
        0.0
    );
    let last = results.frames.last().expect("frames exist");
    assert!(last.time_seconds >= 119.0);
    assert!((last.fires[0].actual_hrr_kw - 300.0).abs() < 0.01);
    assert!(last.rooms[0].upper_layer.temperature_c > 100.0);
    assert_eq!(last.openings[0].opening_fraction, 1.0);
}

#[test]
#[ignore = "requires an external CFAST 7.7.7 executable"]
fn runs_and_parses_multiple_compartments() {
    let executable = std::env::var("CFAST_BIN").expect("CFAST_BIN must point to CFAST 7.7.7");
    let runner = CfastRunner::new(executable).expect("CFAST executable should resolve");

    let results = runner
        .run_all_rooms(&two_room_scene(), &scenario_fixture())
        .expect("multi-room CFAST should run and parse");

    let last = results.frames.last().expect("frames exist");
    assert_eq!(last.rooms.len(), 2);
    assert_eq!(last.rooms[0].room_id, "Demo Room");
    assert_eq!(last.rooms[1].room_id, "Second Room");
    assert!(last.rooms[0].upper_layer.temperature_c > 100.0);
    assert!(last.rooms[1].upper_layer.temperature_c < 30.0);
}

#[test]
#[ignore = "requires an external CFAST 7.7.7 executable"]
fn spreads_fire_conditions_through_an_open_interior_door() {
    let executable = std::env::var("CFAST_BIN").expect("CFAST_BIN must point to CFAST 7.7.7");
    let runner = CfastRunner::new(executable).expect("CFAST executable should resolve");
    let mut scenario = scenario_fixture();
    scenario.opening_states.push(CfastOpeningState {
        opening_id: "Interior Door".to_owned(),
        open_fraction: 1.0,
    });

    let results = runner
        .run_all_rooms(&connected_two_room_scene(), &scenario)
        .expect("connected multi-room CFAST should run and parse");

    let last = results.frames.last().expect("frames exist");
    assert_eq!(last.rooms.len(), 2);
    assert_eq!(last.openings.len(), 2);
    assert!(last.rooms[1].upper_layer.temperature_c > 20.0);
    assert_eq!(last.openings[1].opening_fraction, 1.0);
}

fn scenario_fixture() -> CfastScenario {
    CfastScenario {
        id: "runner-feasibility".to_owned(),
        title: "Runner feasibility".to_owned(),
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
        name: Some("Runner fixture".to_owned()),
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "scene-1".to_owned(),
        },
        assets: vec![],
        floors: vec![Floor {
            id: "floor-1".to_owned(),
            label: None,
            elevation_m: Some(0.0),
            rooms: vec![Room {
                id: "Demo Room".to_owned(),
                label: None,
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
                            label: None,
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
    scene.floors[0].rooms[0].boundary[1].openings.push(Opening {
        id: "Interior Door".to_owned(),
        label: None,
        kind: OpeningKind::Door,
        width_m: 1.0,
        height_m: Some(2.0),
        lower_elevation_m: Some(0.0),
        relative_center: 0.5,
    });
    let mut room = scene.floors[0].rooms[0].clone();
    room.id = "Second Room".to_owned();
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
