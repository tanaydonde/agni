use tflop_simulation_core::{
    AgentFrame, AgentFrameRole, CombustionState, Exposure, FireConditions, GasLayerConditions,
    GridCoordinate, Movement, OccupantPhase, OpeningConditions, RoomConditions,
    SIMULATION_RESULTS_SCHEMA_VERSION, SimulationEngine, SimulationFrame, SimulationResults,
    SurfaceFireNodeFrame, SurfaceFireNodeSource,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, Opening, OpeningKind, Room,
    SCENE_SCHEMA_VERSION, Scene, SceneSource, Vec3,
};

#[test]
fn valid_results_round_trip_through_json() {
    let results = results_fixture();

    results.validate().expect("fixture should be valid");
    let json = serde_json::to_string_pretty(&results).expect("results should serialize");
    let decoded: SimulationResults =
        serde_json::from_str(&json).expect("results should deserialize");

    assert_eq!(decoded, results);
    assert_eq!(decoded.frames[0].rooms[0].room_id, "room-1");
    assert_eq!(decoded.frames[0].fires[0].actual_hrr_kw, 75.0);
    assert_eq!(decoded.frames[0].openings[0].opening_id, "door-1");
    assert_eq!(decoded.frames[0].surface_fire[0].id, "floor:floor-1:2:3");
}

#[test]
fn rejects_frames_that_are_not_strictly_ordered() {
    let mut results = results_fixture();
    results.frames.push(results.frames[0].clone());

    let errors = results.validate().expect_err("duplicate time should fail");

    assert!(errors.0.iter().any(|error| {
        error.path == "frames[1].time_seconds" && error.message.contains("strictly increasing")
    }));
}

#[test]
fn rejects_duplicate_ids_and_invalid_physical_values() {
    let mut results = results_fixture();
    let duplicate = results.frames[0].rooms[0].clone();
    results.frames[0].rooms.push(duplicate);
    results.frames[0].rooms[0].upper_layer.oxygen_percent = 120.0;
    results.frames[0].fires[0].actual_hrr_kw = -1.0;
    results.frames[0].openings[0].opening_fraction = 1.5;

    let errors = results.validate().expect_err("invalid values should fail");

    assert!(has_error(&errors.0, "frames[0].rooms[1].id", "unique"));
    assert!(has_error(
        &errors.0,
        "frames[0].rooms[0].upper_layer.oxygen_percent",
        "zero and 100"
    ));
    assert!(has_error(
        &errors.0,
        "frames[0].fires[0].actual_hrr_kw",
        "nonnegative"
    ));
    assert!(has_error(
        &errors.0,
        "frames[0].openings[0].opening_fraction",
        "zero and one"
    ));
}

#[test]
fn verifies_result_ids_against_the_physical_scene() {
    let results = results_fixture();
    results
        .validate_against_scene(&scene_fixture())
        .expect("all IDs should map to the scene");

    let mut bad_results = results;
    bad_results.frames[0].openings[0].opening_id = "missing".to_owned();
    let errors = bad_results
        .validate_against_scene(&scene_fixture())
        .expect_err("unknown opening should fail");

    assert!(has_error(
        &errors.0,
        "frames[0].openings[0].opening_id",
        "physical scene"
    ));
}

#[test]
fn validates_agent_frames() {
    let mut results = results_fixture();
    let agent = AgentFrame {
        id: "occupant-1".to_owned(),
        floor_id: "floor-9".to_owned(),
        position_m: point(f64::NAN, 1.0),
        velocity_m_per_s: point(0.0, 0.0),
        radius_m: 0.25,
        exposure: Exposure {
            heat: -0.1,
            toxic_gas: 0.0,
        },
        movement: Movement::Walking,
        role: AgentFrameRole::Occupant {
            phase: OccupantPhase::MovingToExit,
            injured: false,
        },
    };
    results.frames[0].agents = vec![agent.clone(), agent];

    let errors = results
        .validate_against_scene(&scene_fixture())
        .expect_err("invalid agents should fail");

    assert!(has_error(&errors.0, "frames[0].agents[1].id", "unique"));
    assert!(has_error(
        &errors.0,
        "frames[0].agents[0].position_m.x",
        "finite"
    ));
    assert!(has_error(
        &errors.0,
        "frames[0].agents[0].exposure.heat",
        "nonnegative"
    ));
    assert!(has_error(
        &errors.0,
        "frames[0].agents[0].floor_id",
        "physical scene"
    ));
}

fn has_error(
    errors: &[tflop_simulation_core::ResultsValidationError],
    path: &str,
    message: &str,
) -> bool {
    errors
        .iter()
        .any(|error| error.path == path && error.message.contains(message))
}

fn results_fixture() -> SimulationResults {
    SimulationResults {
        schema_version: SIMULATION_RESULTS_SCHEMA_VERSION,
        scene_id: "scene-1".to_owned(),
        scenario_id: "scenario-1".to_owned(),
        engine: SimulationEngine {
            name: "cfast".to_owned(),
            version: "7.7.7".to_owned(),
        },
        frames: vec![SimulationFrame {
            time_seconds: 30.0,
            rooms: vec![RoomConditions {
                room_id: "room-1".to_owned(),
                smoke_layer_height_m: 1.9,
                gauge_pressure_pa: -2.0,
                absolute_pressure_pa: 101_323.0,
                upper_layer: GasLayerConditions {
                    temperature_c: 140.0,
                    oxygen_percent: 19.2,
                    carbon_monoxide_percent: 0.01,
                    hydrogen_cyanide_percent: 0.0,
                    optical_density_per_m: 0.3,
                },
                lower_layer: GasLayerConditions {
                    temperature_c: 25.0,
                    oxygen_percent: 20.4,
                    carbon_monoxide_percent: 0.0,
                    hydrogen_cyanide_percent: 0.0,
                    optical_density_per_m: 0.0,
                },
            }],
            fires: vec![FireConditions {
                fire_id: "fire-1".to_owned(),
                room_id: "room-1".to_owned(),
                ignited: true,
                expected_hrr_kw: 75.0,
                actual_hrr_kw: 75.0,
                convective_hrr_kw: 48.75,
                flame_height_m: 0.8,
                pyrolysis_rate_kg_per_s: 0.004,
            }],
            openings: vec![OpeningConditions {
                opening_id: "door-1".to_owned(),
                net_flow_kg_per_s: -0.04,
                opening_fraction: 1.0,
            }],
            surface_fire: vec![SurfaceFireNodeFrame {
                id: "floor:floor-1:2:3".to_owned(),
                source: SurfaceFireNodeSource::FloorCell {
                    floor_id: "floor-1".to_owned(),
                    coordinate: GridCoordinate { column: 2, row: 3 },
                },
                room_id: "room-1".to_owned(),
                position_m: point(1.25, 1.75),
                dimensions_m: Dimensions3 {
                    width_m: 0.5,
                    depth_m: 0.5,
                    height_m: 0.02,
                },
                furniture_cell: None,
                material_id: "generic_carpet".to_owned(),
                state: CombustionState::Burning,
                ignition_time_seconds: Some(20.0),
                surface_temperature_c: 600.0,
                heat_release_rate_kw: 45.0,
                fuel_remaining_fraction: 0.8,
            }],
            agents: vec![],
        }],
    }
}

fn scene_fixture() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "scene-1".to_owned(),
        name: Some("Results fixture".to_owned()),
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
                id: "room-1".to_owned(),
                label: None,
                classifications: vec![],
                dimensions: None,
                boundary: vec![BoundaryEdge {
                    id: "edge-1".to_owned(),
                    kind: BoundaryKind::Wall,
                    start: point(0.0, 0.0),
                    end: point(4.0, 0.0),
                    thickness_m: Some(0.1),
                    center_line_bias: Some(0.5),
                    openings: vec![Opening {
                        id: "door-1".to_owned(),
                        label: None,
                        kind: OpeningKind::Door,
                        width_m: 1.0,
                        height_m: Some(2.0),
                        lower_elevation_m: Some(0.0),
                        relative_center: 0.5,
                    }],
                }],
            }],
            objects: vec![],
        }],
    }
}

fn point(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
