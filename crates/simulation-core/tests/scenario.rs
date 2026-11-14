use tflop_simulation_core::{
    AgentRole, CrewSpawn, DEFAULT_OCCUPANT_WALKING_SPEED_M_PER_S, FirefighterPhase, OccupantPhase,
    OccupantSpawn, SCENARIO_SCHEMA_VERSION, Scenario, ScenarioValidationError, Simulation,
    SimulationConfig, SimulationError, SimulationGrid, exterior_exits,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, FurnitureAsset, FurnitureObject, Mobility,
    Opening, OpeningKind, Quaternion, Room, SCENE_SCHEMA_VERSION, Scene, SceneSource, Transform3,
    Vec3,
};

#[test]
fn spawns_occupants_and_crew_members() {
    let simulation = Simulation::from_scenario(
        scene_fixture(),
        &scenario_fixture(),
        SimulationConfig::default(),
    )
    .expect("scenario should be valid");

    let agents = simulation.agents();
    assert_eq!(
        agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>(),
        ["occupant-1", "engine-1/1", "engine-1/2"]
    );

    let occupant = &agents[0];
    assert_eq!(occupant.position_m, position(1.0, 3.0));
    let AgentRole::Occupant(state) = &occupant.role else {
        panic!("first agent should be an occupant");
    };
    assert_eq!(state.phase, OccupantPhase::Unaware);
    assert_eq!(
        state.walking_speed_m_per_s,
        DEFAULT_OCCUPANT_WALKING_SPEED_M_PER_S
    );

    let firefighter = &agents[1];
    assert_eq!(firefighter.floor_id, "floor-1");
    assert_eq!(firefighter.position_m, position(2.0, 0.0));
    let AgentRole::Firefighter(state) = &firefighter.role else {
        panic!("second agent should be a firefighter");
    };
    assert_eq!(state.crew_id, "engine-1");
    assert_eq!(state.phase, FirefighterPhase::Staging);
    assert_eq!(state.entry_opening_id, "door-1");
    assert_eq!(state.arrival_time_seconds, 30.0);
    assert!(agents.iter().all(|agent| agent.is_active()));
}

#[test]
fn snapshot_serializes_agents_for_the_frontend() {
    let simulation = Simulation::from_scenario(
        scene_fixture(),
        &scenario_fixture(),
        SimulationConfig::default(),
    )
    .expect("scenario should be valid");

    let json = serde_json::to_value(simulation.snapshot()).expect("snapshot should serialize");

    assert_eq!(
        json["agents"][0],
        serde_json::json!({
            "id": "occupant-1",
            "floor_id": "floor-1",
            "position_m": { "x": 1.0, "y": 3.0, "z": 0.0 },
            "velocity_m_per_s": { "x": 0.0, "y": 0.0, "z": 0.0 },
            "radius_m": 0.25,
            "exposure": { "heat": 0.0, "toxic_gas": 0.0 },
            "role": { "type": "occupant", "phase": "unaware" }
        })
    );
    assert_eq!(
        json["agents"][1]["role"],
        serde_json::json!({ "type": "firefighter", "crew_id": "engine-1", "phase": "staging" })
    );
}

#[test]
fn reset_restores_the_spawned_agents() {
    let mut simulation = Simulation::from_scenario(
        scene_fixture(),
        &scenario_fixture(),
        SimulationConfig::default(),
    )
    .expect("scenario should be valid");
    let initial = simulation.snapshot();

    simulation.step(1.0).expect("positive step should work");
    let reset = simulation.reset();

    assert_eq!(reset, initial);
    assert_eq!(reset.agents.len(), 3);
}

#[test]
fn deserializes_with_defaults() {
    let scenario: Scenario = serde_json::from_value(serde_json::json!({
        "schema_version": SCENARIO_SCHEMA_VERSION,
        "id": "scenario-1",
        "scene_id": "scene-1",
        "ignition_object_id": "chair-1",
        "occupants": [
            { "id": "occupant-1", "floor_id": "floor-1", "position_m": { "x": 1.0, "y": 3.0, "z": 0.0 } }
        ]
    }))
    .expect("minimal scenario should deserialize");

    assert_eq!(
        scenario.occupants[0].walking_speed_m_per_s,
        DEFAULT_OCCUPANT_WALKING_SPEED_M_PER_S
    );
    assert_eq!(scenario.occupants[0].pre_movement_delay_seconds, 0.0);
    assert!(scenario.crews.is_empty());
    scenario
        .validate()
        .expect("minimal scenario should be valid");
}

#[test]
fn reports_every_scenario_problem() {
    let mut scenario = scenario_fixture();
    scenario.scene_id = "other-scene".to_owned();
    scenario.ignition_object_id = "missing-object".to_owned();
    scenario.occupants = vec![
        occupant("on-chair", "floor-1", 1.0, 1.0),
        occupant("outside", "floor-1", 3.0, 3.0),
        occupant("no-floor", "floor-9", 1.0, 3.0),
        occupant("no-floor", "floor-1", 1.0, 3.0),
    ];
    scenario.occupants[0].walking_speed_m_per_s = 0.0;
    scenario.crews = vec![
        crew("not-an-exit", "door-9", 2),
        crew("empty-crew", "door-1", 0),
    ];

    let Err(SimulationError::InvalidScenario(errors)) =
        Simulation::from_scenario(scene_fixture(), &scenario, SimulationConfig::default())
    else {
        panic!("invalid scenario should fail");
    };

    for (path, message) in [
        ("scene_id", "does not match"),
        ("ignition_object_id", "does not exist"),
        ("occupants[0].walking_speed_m_per_s", "greater than zero"),
        ("occupants[0].position_m", "walkable"),
        ("occupants[1].position_m", "walkable"),
        ("occupants[2].floor_id", "does not exist"),
        ("occupants[3].id", "unique"),
        ("crews[0].entry_opening_id", "exterior door"),
        ("crews[1].member_count", "between 1 and"),
    ] {
        assert!(
            has_error(&errors.0, path, message),
            "missing {path}: {message} in {errors:?}"
        );
    }
}

#[test]
fn finds_exterior_doors_and_walkable_cells() {
    let scene = scene_fixture();
    let exits = exterior_exits(&scene).expect("topology should resolve");
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0].id, "door-1");

    let grid = SimulationGrid::from_scene(&scene, "floor-1", 0.5).expect("grid should build");
    let walkable = |x, y| {
        let coordinate = grid.world_to_cell(position(x, y)).expect("in bounds");
        grid.cell(coordinate).expect("cell exists").is_walkable()
    };
    assert!(walkable(1.0, 3.0), "open floor");
    assert!(walkable(2.0, 0.1), "door");
    assert!(!walkable(1.0, 1.0), "furniture");
    assert!(!walkable(0.1, 3.0), "wall");
    assert!(!walkable(3.0, 3.0), "outside");
}

fn has_error(errors: &[ScenarioValidationError], path: &str, message: &str) -> bool {
    errors
        .iter()
        .any(|error| error.path == path && error.message.contains(message))
}

fn scenario_fixture() -> Scenario {
    Scenario {
        schema_version: SCENARIO_SCHEMA_VERSION,
        id: "scenario-1".to_owned(),
        scene_id: "scene-1".to_owned(),
        ignition_object_id: "chair-1".to_owned(),
        occupants: vec![occupant("occupant-1", "floor-1", 1.0, 3.0)],
        crews: vec![crew("engine-1", "door-1", 2)],
    }
}

fn occupant(id: &str, floor_id: &str, x: f64, y: f64) -> OccupantSpawn {
    OccupantSpawn {
        id: id.to_owned(),
        floor_id: floor_id.to_owned(),
        position_m: position(x, y),
        walking_speed_m_per_s: DEFAULT_OCCUPANT_WALKING_SPEED_M_PER_S,
        pre_movement_delay_seconds: 0.0,
    }
}

fn crew(id: &str, entry_opening_id: &str, member_count: u32) -> CrewSpawn {
    CrewSpawn {
        id: id.to_owned(),
        entry_opening_id: entry_opening_id.to_owned(),
        arrival_time_seconds: 30.0,
        member_count,
        walking_speed_m_per_s: 1.0,
    }
}

/// L-shaped room with an exterior door on the south wall and a chair.
fn scene_fixture() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "scene-1".to_owned(),
        name: Some("Scenario fixture".to_owned()),
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "scene-1".to_owned(),
        },
        assets: vec![FurnitureAsset {
            id: "chair-basic".to_owned(),
            category: "chair".to_owned(),
            model_uri: None,
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
                label: Some("L room".to_owned()),
                classifications: vec![],
                dimensions: None,
                boundary: vec![
                    edge(
                        "south",
                        position(0.0, 0.0),
                        position(4.0, 0.0),
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
                    edge("east-low", position(4.0, 0.0), position(4.0, 2.0), vec![]),
                    edge("notch-low", position(4.0, 2.0), position(2.0, 2.0), vec![]),
                    edge("notch-high", position(2.0, 2.0), position(2.0, 4.0), vec![]),
                    edge("north", position(2.0, 4.0), position(0.0, 4.0), vec![]),
                    edge("west", position(0.0, 4.0), position(0.0, 0.0), vec![]),
                ],
            }],
            objects: vec![FurnitureObject {
                id: "chair-1".to_owned(),
                asset_id: "chair-basic".to_owned(),
                transform: Transform3 {
                    position_m: position(1.0, 1.0),
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

fn position(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
