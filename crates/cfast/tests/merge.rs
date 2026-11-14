use tflop_cfast::{CfastMergeError, merge_cfast_with_surface_fire};
use tflop_simulation_core::{
    CombustionState, FireConditions, GasLayerConditions, GridCoordinate, OpeningConditions,
    RoomConditions, RoomHeatRelease, SIMULATION_RESULTS_SCHEMA_VERSION,
    SURFACE_FIRE_SCHEMA_VERSION, SimulationEngine, SimulationFrame, SimulationResults,
    SurfaceFireFrame, SurfaceFireNodeFrame, SurfaceFireNodeSource,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, Opening, OpeningKind, Room,
    SCENE_SCHEMA_VERSION, Scene, SceneSource, Vec3,
};

#[test]
fn merges_spatial_cells_onto_their_full_timeline() {
    let scene = scene_fixture();
    let merged = merge_cfast_with_surface_fire(
        &scene,
        &cfast_results(),
        &[
            surface_frame(0.0, vec![], 0.0),
            surface_frame(5.0, vec![surface_node(50.0)], 50.0),
            surface_frame(10.0, vec![surface_node(100.0)], 100.0),
        ],
    )
    .expect("timelines should merge");

    assert_eq!(merged.frames.len(), 3);
    let middle = &merged.frames[1];
    assert_eq!(middle.time_seconds, 5.0);
    assert_eq!(middle.surface_fire.len(), 1);
    assert_eq!(middle.surface_fire[0].id, "floor:floor-1:1:1");
    assert_eq!(middle.rooms[0].upper_layer.temperature_c, 60.0);
    assert_eq!(middle.rooms[0].smoke_layer_height_m, 2.0);
    assert_eq!(middle.fires[0].actual_hrr_kw, 50.0);
    assert!(!middle.fires[0].ignited);
    assert_eq!(middle.openings[0].net_flow_kg_per_s, 1.0);
    assert!(merged.frames[2].fires[0].ignited);
}

#[test]
fn rejects_surface_frames_outside_the_cfast_time_range() {
    let scene = scene_fixture();
    let frames = [
        surface_frame(0.0, vec![], 0.0),
        surface_frame(11.0, vec![surface_node(100.0)], 100.0),
    ];

    assert!(matches!(
        merge_cfast_with_surface_fire(&scene, &cfast_results(), &frames),
        Err(CfastMergeError::SurfaceTimeOutsideCfastRange {
            time_seconds: 11.0,
            ..
        })
    ));
}

#[test]
fn rejects_entity_sets_that_change_between_cfast_frames() {
    let scene = scene_fixture();
    let mut cfast = cfast_results();
    cfast.frames[1].rooms.clear();
    let frames = [
        surface_frame(0.0, vec![], 0.0),
        surface_frame(5.0, vec![surface_node(50.0)], 50.0),
    ];

    assert!(matches!(
        merge_cfast_with_surface_fire(&scene, &cfast, &frames),
        Err(CfastMergeError::EntitySetChanged { entity: "room", .. })
    ));
}

fn cfast_results() -> SimulationResults {
    SimulationResults {
        schema_version: SIMULATION_RESULTS_SCHEMA_VERSION,
        scene_id: "scene-1".to_owned(),
        scenario_id: "scenario-1".to_owned(),
        engine: SimulationEngine {
            name: "cfast".to_owned(),
            version: "7.7.7".to_owned(),
        },
        frames: vec![
            cfast_frame(0.0, 20.0, 3.0, 0.0, false),
            cfast_frame(10.0, 100.0, 1.0, 100.0, true),
        ],
    }
}

fn cfast_frame(
    time_seconds: f64,
    upper_temperature_c: f64,
    smoke_layer_height_m: f64,
    hrr_kw: f64,
    ignited: bool,
) -> SimulationFrame {
    SimulationFrame {
        time_seconds,
        rooms: vec![RoomConditions {
            room_id: "room-1".to_owned(),
            smoke_layer_height_m,
            gauge_pressure_pa: hrr_kw / 10.0,
            absolute_pressure_pa: 101_325.0 + hrr_kw / 10.0,
            upper_layer: layer(upper_temperature_c, 20.0 - hrr_kw / 100.0),
            lower_layer: layer(20.0 + hrr_kw / 20.0, 20.0),
        }],
        fires: vec![FireConditions {
            fire_id: "surface-fire:room-1".to_owned(),
            room_id: "room-1".to_owned(),
            ignited,
            expected_hrr_kw: hrr_kw,
            actual_hrr_kw: hrr_kw,
            convective_hrr_kw: hrr_kw * 0.65,
            flame_height_m: hrr_kw / 100.0,
            pyrolysis_rate_kg_per_s: hrr_kw / 18_000.0,
        }],
        openings: vec![OpeningConditions {
            opening_id: "door-1".to_owned(),
            net_flow_kg_per_s: hrr_kw / 50.0,
            opening_fraction: 1.0,
        }],
        surface_fire: vec![],
    }
}

fn layer(temperature_c: f64, oxygen_percent: f64) -> GasLayerConditions {
    GasLayerConditions {
        temperature_c,
        oxygen_percent,
        carbon_monoxide_percent: 0.0,
        hydrogen_cyanide_percent: 0.0,
        optical_density_per_m: (temperature_c - 20.0).max(0.0) / 100.0,
    }
}

fn surface_frame(
    time_seconds: f64,
    active_nodes: Vec<SurfaceFireNodeFrame>,
    hrr_kw: f64,
) -> SurfaceFireFrame {
    SurfaceFireFrame {
        schema_version: SURFACE_FIRE_SCHEMA_VERSION,
        scene_id: "scene-1".to_owned(),
        time_seconds,
        active_nodes,
        room_heat_release: vec![RoomHeatRelease {
            room_id: "room-1".to_owned(),
            heat_release_rate_kw: hrr_kw,
        }],
    }
}

fn surface_node(hrr_kw: f64) -> SurfaceFireNodeFrame {
    SurfaceFireNodeFrame {
        id: "floor:floor-1:1:1".to_owned(),
        source: SurfaceFireNodeSource::FloorCell {
            floor_id: "floor-1".to_owned(),
            coordinate: GridCoordinate { column: 1, row: 1 },
        },
        room_id: "room-1".to_owned(),
        position_m: point(1.0, 1.0),
        dimensions_m: Dimensions3 {
            width_m: 0.25,
            depth_m: 0.25,
            height_m: 0.02,
        },
        furniture_cell: None,
        material_id: "generic_carpet".to_owned(),
        state: CombustionState::Burning,
        ignition_time_seconds: Some(1.0),
        surface_temperature_c: 500.0,
        heat_release_rate_kw: hrr_kw,
        fuel_remaining_fraction: 0.5,
    }
}

fn scene_fixture() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "scene-1".to_owned(),
        name: None,
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
                boundary: vec![
                    edge("south", point(0.0, 0.0), point(4.0, 0.0), true),
                    edge("east", point(4.0, 0.0), point(4.0, 4.0), false),
                    edge("north", point(4.0, 4.0), point(0.0, 4.0), false),
                    edge("west", point(0.0, 4.0), point(0.0, 0.0), false),
                ],
            }],
            objects: vec![],
        }],
    }
}

fn edge(id: &str, start: Vec3, end: Vec3, with_door: bool) -> BoundaryEdge {
    BoundaryEdge {
        id: id.to_owned(),
        kind: BoundaryKind::Wall,
        start,
        end,
        thickness_m: Some(0.1),
        center_line_bias: Some(0.5),
        openings: if with_door {
            vec![Opening {
                id: "door-1".to_owned(),
                label: None,
                kind: OpeningKind::Door,
                width_m: 1.0,
                height_m: Some(2.0),
                lower_elevation_m: Some(0.0),
                relative_center: 0.5,
            }]
        } else {
            vec![]
        },
    }
}

fn point(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
