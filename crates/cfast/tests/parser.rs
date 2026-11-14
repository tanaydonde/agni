use tflop_cfast::{
    CfastFire, CfastFuel, CfastOpeningState, CfastParseError, CfastScenario, HeatReleasePoint,
    parse_cfast_results,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Floor, Opening, OpeningKind, Room, SCENE_SCHEMA_VERSION, Scene,
    SceneSource, Vec3,
};

const COMPARTMENTS_CSV: &str = "Time,ULT_1,LLT_1,HGT_1,PRS_1,APRS_1,ULO2_1,ULCO_1,ULHCN_1,ULOD_1,LLO2_1,LLCO_1,LLHCN_1,LLOD_1,IGN_1,PYROL_1,HRR_E1,HRR_1,HRRC_1,FLHGT_1
Simulation Time,Upper Temp,Lower Temp,Layer Height,Pressure,Absolute Pressure,Upper O2,Upper CO,Upper HCN,Upper OD,Lower O2,Lower CO,Lower HCN,Lower OD,Ignition,Pyrolysis,Expected HRR,Actual HRR,Convective HRR,Flame Height
Time,room-1,room-1,room-1,room-1,room-1,room-1,room-1,room-1,room-1,room-1,room-1,room-1,room-1,fire-1,fire-1,fire-1,fire-1,fire-1,fire-1
s,C,C,m,Pa,Pa,mol %,mol %,mol %,1/m,mol %,mol %,mol %,1/m,,kg/s,W,W,W,m
0,20,20,3,0,101325,20.5,0,0,0,20.5,0,0,0,0,0,0,0,0,0
1,140,25,1.9,-2,101323,19.2,0.01,0.001,0.3,20.4,0,0,0,1,0.004,75000,75000,48750,0.8
";

const VENTS_CSV: &str = "Time,W_1_Outside_1,WF_Outside_1_1
Simulation Time,Net Inflow,Opening Fraction
Time,door-1,door-1
s,kg/s,
0,0,1
1,-0.04,1
";

#[test]
fn parses_cfast_spreadsheets_into_canonical_results() {
    let scene = scene_fixture();
    let results = parse_cfast_results(
        &scene,
        &scenario_fixture(),
        "7.7.7",
        COMPARTMENTS_CSV,
        VENTS_CSV,
    )
    .expect("fixture CSV should parse");

    assert_eq!(results.scene_id, "scene-1");
    assert_eq!(results.scenario_id, "scenario-1");
    assert_eq!(results.frames.len(), 2);

    let frame = &results.frames[1];
    assert_eq!(frame.time_seconds, 1.0);
    assert_eq!(frame.rooms[0].upper_layer.temperature_c, 140.0);
    assert_eq!(frame.rooms[0].upper_layer.oxygen_percent, 19.2);
    assert_eq!(frame.rooms[0].smoke_layer_height_m, 1.9);
    assert_eq!(frame.fires[0].actual_hrr_kw, 75.0);
    assert_eq!(frame.fires[0].convective_hrr_kw, 48.75);
    assert!(frame.fires[0].ignited);
    assert_eq!(frame.openings[0].net_flow_kg_per_s, -0.04);
}

#[test]
fn rejects_missing_measurements() {
    let missing_flame_height = COMPARTMENTS_CSV.replace(",FLHGT_1", "");

    assert!(matches!(
        parse_cfast_results(
            &scene_fixture(),
            &scenario_fixture(),
            "7.7.7",
            &missing_flame_height,
            VENTS_CSV,
        ),
        Err(CfastParseError::MissingColumn {
            measurement: "flame height",
            ..
        })
    ));
}

#[test]
fn rejects_unsynchronized_output_timelines() {
    let mismatched_vents = VENTS_CSV.replace("1,-0.04,1", "2,-0.04,1");

    assert!(matches!(
        parse_cfast_results(
            &scene_fixture(),
            &scenario_fixture(),
            "7.7.7",
            COMPARTMENTS_CSV,
            &mismatched_vents,
        ),
        Err(CfastParseError::TimelineTimeMismatch { .. })
    ));
}

#[test]
fn parses_conditions_for_every_compartment_in_the_output() {
    let mut scene = scene_fixture();
    let mut second_room = scene.floors[0].rooms[0].clone();
    second_room.id = "room-2".to_owned();
    second_room.boundary[0].id = "edge-2".to_owned();
    second_room.boundary[0].openings.clear();
    scene.floors[0].rooms.push(second_room);

    let compartments = append_second_room(COMPARTMENTS_CSV);
    let results = parse_cfast_results(
        &scene,
        &scenario_fixture(),
        "7.7.7",
        &compartments,
        VENTS_CSV,
    )
    .expect("both compartments should parse");

    let frame = &results.frames[1];
    assert_eq!(frame.rooms.len(), 2);
    assert_eq!(frame.rooms[0].room_id, "room-1");
    assert_eq!(frame.rooms[1].room_id, "room-2");
    assert_eq!(frame.rooms[1].upper_layer.temperature_c, 21.0);
}

#[test]
fn parses_conditions_for_every_vent_in_the_output() {
    let mut scene = scene_fixture();
    scene.floors[0].rooms[0].boundary[0].openings.push(Opening {
        id: "door-2".to_owned(),
        label: None,
        kind: OpeningKind::Door,
        width_m: 1.0,
        height_m: Some(2.0),
        lower_elevation_m: Some(0.0),
        relative_center: 0.75,
    });
    let vents = VENTS_CSV
        .lines()
        .zip([
            "W_2_Outside_1,WF_Outside_2_1",
            "Net Inflow,Opening Fraction",
            "door-2,door-2",
            "kg/s,",
            "0,0",
            "0.02,0.5",
        ])
        .map(|(line, addition)| format!("{line},{addition}"))
        .collect::<Vec<_>>()
        .join("\n");

    let results = parse_cfast_results(
        &scene,
        &scenario_fixture(),
        "7.7.7",
        COMPARTMENTS_CSV,
        &vents,
    )
    .expect("both vents should parse");

    assert_eq!(results.frames[1].openings.len(), 2);
    assert_eq!(results.frames[1].openings[1].opening_id, "door-2");
    assert_eq!(results.frames[1].openings[1].opening_fraction, 0.5);
}

fn append_second_room(input: &str) -> String {
    let additions = [
        "ULT_2,LLT_2,HGT_2,PRS_2,APRS_2,ULO2_2,ULCO_2,ULHCN_2,ULOD_2,LLO2_2,LLCO_2,LLHCN_2,LLOD_2",
        "Upper Temp,Lower Temp,Layer Height,Pressure,Absolute Pressure,Upper O2,Upper CO,Upper HCN,Upper OD,Lower O2,Lower CO,Lower HCN,Lower OD",
        "room-2,room-2,room-2,room-2,room-2,room-2,room-2,room-2,room-2,room-2,room-2,room-2,room-2",
        "C,C,m,Pa,Pa,mol %,mol %,mol %,1/m,mol %,mol %,mol %,1/m",
        "20,20,3,0,101325,20.5,0,0,0,20.5,0,0,0",
        "21,20,3,0,101325,20.5,0,0,0,20.5,0,0,0",
    ];
    input
        .lines()
        .zip(additions)
        .map(|(line, addition)| format!("{line},{addition}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn scenario_fixture() -> CfastScenario {
    CfastScenario {
        id: "scenario-1".to_owned(),
        title: "Parser fixture".to_owned(),
        room_id: "room-1".to_owned(),
        exterior_opening_id: "door-1".to_owned(),
        duration_seconds: 1.0,
        output_interval_seconds: 1.0,
        interior_temperature_c: 20.0,
        exterior_temperature_c: 20.0,
        relative_humidity_percent: 50.0,
        opening_states: vec![CfastOpeningState {
            opening_id: "door-1".to_owned(),
            open_fraction: 1.0,
        }],
        fire: CfastFire {
            id: "fire-1".to_owned(),
            position_m: point(2.0, 2.0),
            area_m2: 0.3,
            fuel: CfastFuel::default(),
            heat_release_curve: vec![
                HeatReleasePoint {
                    time_seconds: 0.0,
                    hrr_kw: 0.0,
                },
                HeatReleasePoint {
                    time_seconds: 1.0,
                    hrr_kw: 75.0,
                },
            ],
        },
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
