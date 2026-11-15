use tflop_simulation_core::{
    AgentFrameRole, AlarmConfig, CROWD_GIVE_M, CombustionState, Evacuation, OccupantPhase,
    OccupantSpawn, SimulationConfig, SurfaceFireNodeFrame, SurfaceFireNodeSource,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, Opening, OpeningKind, Room,
    SCENE_SCHEMA_VERSION, Scene, SceneSource, Vec3,
};

#[test]
fn honors_pre_movement_delay_then_walks_to_an_exit() {
    let mut evacuation = Evacuation::new(
        &scene_fixture(),
        &[occupant("occupant-1", 2.0, 3.0, 1.0)],
        SimulationConfig::default(),
    )
    .expect("evacuation should build")
    .with_alarm(AlarmConfig::Immediate);

    let initial = evacuation.advance_to(0.0, &[]).expect("initial frame");
    let preparing = evacuation.advance_to(0.5, &[]).expect("preparing frame");
    assert_eq!(initial[0].position_m, preparing[0].position_m);
    assert_eq!(phase(&preparing[0]), OccupantPhase::Preparing);

    let moving = evacuation.advance_to(1.5, &[]).expect("moving frame");
    assert_ne!(moving[0].position_m, preparing[0].position_m);
    assert_eq!(phase(&moving[0]), OccupantPhase::MovingToExit);
    assert!(moving[0].velocity_m_per_s.x != 0.0 || moving[0].velocity_m_per_s.y != 0.0);

    let mut last = moving;
    for second in 2..=10 {
        last = evacuation
            .advance_to(f64::from(second), &[])
            .expect("evacuation frame");
        if phase(&last[0]) == OccupantPhase::Exited {
            break;
        }
    }
    assert_eq!(phase(&last[0]), OccupantPhase::Exited);
}

#[test]
fn converging_occupants_never_overlap() {
    let mut evacuation = Evacuation::new(
        &scene_fixture(),
        &[
            occupant("occupant-1", 2.0, 3.0, 0.0),
            occupant("occupant-2", 4.0, 3.0, 0.0),
        ],
        SimulationConfig::default(),
    )
    .expect("evacuation should build")
    .with_alarm(AlarmConfig::Immediate);

    for second in 0..=10 {
        let agents = evacuation
            .advance_to(f64::from(second), &[])
            .expect("evacuation frame");
        if phase(&agents[0]) == OccupantPhase::Exited || phase(&agents[1]) == OccupantPhase::Exited
        {
            continue;
        }
        let dx = agents[0].position_m.x - agents[1].position_m.x;
        let dy = agents[0].position_m.y - agents[1].position_m.y;
        // People converging in an open room may press together a little.
        assert!(
            dx.hypot(dy) >= agents[0].radius_m + agents[1].radius_m - 2.0 * CROWD_GIVE_M - 1.0e-9
        );
    }
}

#[test]
fn routes_around_a_burning_floor_cell() {
    let mut evacuation = Evacuation::new(
        &scene_fixture(),
        &[occupant("occupant-1", 3.0, 3.0, 0.0)],
        SimulationConfig::default(),
    )
    .expect("evacuation should build")
    .with_alarm(AlarmConfig::Immediate);
    let fire_position = point(3.125, 2.125);
    let fire = [SurfaceFireNodeFrame {
        id: "floor-fire".to_owned(),
        source: SurfaceFireNodeSource::FloorCell {
            floor_id: "floor-1".to_owned(),
            coordinate: tflop_simulation_core::GridCoordinate { column: 12, row: 8 },
        },
        room_id: "room-1".to_owned(),
        position_m: fire_position,
        dimensions_m: Dimensions3 {
            width_m: 0.5,
            depth_m: 0.5,
            height_m: 0.02,
        },
        furniture_cell: None,
        material_id: "carpet".to_owned(),
        state: CombustionState::Burning,
        ignition_time_seconds: Some(0.0),
        surface_temperature_c: 400.0,
        heat_release_rate_kw: 10.0,
        fuel_remaining_fraction: 1.0,
    }];

    evacuation.advance_to(0.0, &fire).expect("initial frame");
    let agents = evacuation.advance_to(1.0, &fire).expect("moving frame");

    assert!(
        (agents[0].position_m.x - 3.0).abs() > 0.25,
        "agent should turn sideways instead of walking through fire: {:?}",
        agents[0].position_m
    );
    assert!(
        (agents[0].position_m.x - fire_position.x).hypot(agents[0].position_m.y - fire_position.y)
            > 0.6
    );
}

#[test]
fn walks_straight_through_an_extinguished_floor_cell() {
    let mut evacuation = Evacuation::new(
        &scene_fixture(),
        &[occupant("occupant-1", 3.0, 3.0, 0.0)],
        SimulationConfig::default(),
    )
    .expect("evacuation should build")
    .with_alarm(AlarmConfig::Immediate);
    let fire_position = point(3.125, 2.125);
    let fire = [SurfaceFireNodeFrame {
        id: "floor-fire".to_owned(),
        source: SurfaceFireNodeSource::FloorCell {
            floor_id: "floor-1".to_owned(),
            coordinate: tflop_simulation_core::GridCoordinate { column: 12, row: 8 },
        },
        room_id: "room-1".to_owned(),
        position_m: fire_position,
        dimensions_m: Dimensions3 {
            width_m: 0.5,
            depth_m: 0.5,
            height_m: 0.02,
        },
        furniture_cell: None,
        material_id: "carpet".to_owned(),
        state: CombustionState::Extinguished,
        ignition_time_seconds: Some(0.0),
        surface_temperature_c: 400.0,
        heat_release_rate_kw: 0.0,
        fuel_remaining_fraction: 1.0,
    }];

    evacuation.advance_to(0.0, &fire).expect("initial frame");
    let agents = evacuation.advance_to(1.0, &fire).expect("moving frame");

    assert!(
        (agents[0].position_m.x - 3.0).abs() <= 0.25,
        "an extinguished cell is not a hazard, so the direct route stands: {:?}",
        agents[0].position_m
    );
}

#[test]
fn occupants_are_spread_across_exits_not_all_sent_to_the_nearest() {
    let people = row_of_occupants(12);
    let exits = exits_used(&people, SimulationConfig::default(), &[]);

    let west = exits.iter().filter(|&&west| west).count();
    assert!(
        west > 0 && west < people.len(),
        "both exits are used: {exits:?}"
    );
    // Someone standing nearer one exit walks to the other.
    assert!(
        people
            .iter()
            .zip(&exits)
            .any(|(person, &west)| (person.position_m.x < 6.0) != west),
        "{exits:?}"
    );
    assert_eq!(exits, exits_used(&people, SimulationConfig::default(), &[]));
}

#[test]
fn a_different_seed_assigns_different_exits() {
    let people = row_of_occupants(12);
    let seeded = |random_seed| {
        exits_used(
            &people,
            SimulationConfig {
                random_seed,
                ..SimulationConfig::default()
            },
            &[],
        )
    };
    assert_ne!(seeded(1), seeded(2));
}

#[test]
fn fire_at_an_exit_sends_everyone_to_the_other_one() {
    let people = row_of_occupants(12);
    let fire = [burning_floor_cell(point(0.6, 2.0))];
    let exits = exits_used(&people, SimulationConfig::default(), &fire);
    assert!(exits.iter().all(|&west| !west), "{exits:?}");
}

/// Twelve people across the middle of a 12 m room with exits at each end.
fn row_of_occupants(count: usize) -> Vec<OccupantSpawn> {
    (0..count)
        .map(|index| {
            occupant(
                &format!("occupant-{index}"),
                1.5 + 0.8 * index as f64,
                2.0,
                0.0,
            )
        })
        .collect()
}

/// Whether each person left by the west exit, after everyone is out.
fn exits_used(
    people: &[OccupantSpawn],
    config: SimulationConfig,
    fire: &[SurfaceFireNodeFrame],
) -> Vec<bool> {
    let mut evacuation = Evacuation::new(&two_exit_scene(), people, config)
        .expect("evacuation should build")
        .with_alarm(AlarmConfig::Immediate);
    let mut agents = Vec::new();
    for step in 0..=120 {
        agents = evacuation
            .advance_to(f64::from(step) * 0.5, fire)
            .expect("evacuation frame");
    }
    assert!(
        agents
            .iter()
            .all(|agent| phase(agent) == OccupantPhase::Exited)
    );
    agents
        .iter()
        .map(|agent| agent.position_m.x < 6.0)
        .collect()
}

fn burning_floor_cell(position_m: Vec3) -> SurfaceFireNodeFrame {
    SurfaceFireNodeFrame {
        id: "floor-fire".to_owned(),
        source: SurfaceFireNodeSource::FloorCell {
            floor_id: "floor-1".to_owned(),
            coordinate: tflop_simulation_core::GridCoordinate { column: 2, row: 8 },
        },
        room_id: "room-1".to_owned(),
        position_m,
        dimensions_m: Dimensions3 {
            width_m: 1.0,
            depth_m: 1.0,
            height_m: 0.02,
        },
        furniture_cell: None,
        material_id: "carpet".to_owned(),
        state: CombustionState::Burning,
        ignition_time_seconds: Some(0.0),
        surface_temperature_c: 400.0,
        heat_release_rate_kw: 10.0,
        fuel_remaining_fraction: 1.0,
    }
}

fn two_exit_scene() -> Scene {
    let exit = |id: &str| Opening {
        id: id.to_owned(),
        label: None,
        kind: OpeningKind::Door,
        width_m: 1.0,
        height_m: Some(2.1),
        lower_elevation_m: Some(0.0),
        relative_center: 0.5,
    };
    let mut scene = scene_fixture();
    scene.floors[0].rooms[0].boundary = vec![
        edge("south", point(0.0, 0.0), point(12.0, 0.0), Vec::new()),
        edge(
            "east",
            point(12.0, 0.0),
            point(12.0, 4.0),
            vec![exit("exit-east")],
        ),
        edge("north", point(12.0, 4.0), point(0.0, 4.0), Vec::new()),
        edge(
            "west",
            point(0.0, 4.0),
            point(0.0, 0.0),
            vec![exit("exit-west")],
        ),
    ];
    scene
}

#[test]
fn nobody_reacts_until_the_fire_is_noticeable() {
    let mut evacuation = Evacuation::new(
        &two_room_scene(),
        &[occupant("near", 1.0, 1.0, 0.0)],
        SimulationConfig::default(),
    )
    .expect("evacuation should build");
    let small = [fire_in("room-1", point(3.0, 2.0), 10.0)];
    let start = evacuation.advance_to(0.0, &small).expect("frame")[0].position_m;
    for second in 1..=5 {
        let agents = evacuation
            .advance_to(f64::from(second), &small)
            .expect("frame");
        assert_eq!(phase(&agents[0]), OccupantPhase::Unaware);
        assert_eq!(agents[0].position_m, start);
    }
    assert_eq!(evacuation.alarm_raised_at(), None);

    let big = [fire_in("room-1", point(3.0, 2.0), 80.0)];
    evacuation.advance_to(6.0, &big).expect("frame");
    assert_eq!(evacuation.alarm_raised_at(), Some(6.0));
    let agents = evacuation.advance_to(7.0, &big).expect("frame");
    assert_ne!(
        agents[0].position_m, start,
        "the one who noticed moves at once"
    );
}

#[test]
fn the_alarm_reaches_people_later_the_farther_they_are() {
    let mut evacuation = Evacuation::new(
        &two_room_scene(),
        &[
            occupant("in-fire-room", 1.0, 1.0, 0.0),
            occupant("next-door", 3.0, 5.0, 0.0),
            occupant("far-corner", 5.5, 11.5, 0.0),
        ],
        SimulationConfig::default(),
    )
    .expect("evacuation should build");
    evacuation
        .advance_to(0.0, &[fire_in("room-1", point(3.0, 2.0), 80.0)])
        .expect("frame");

    let alerted = evacuation
        .alerted_at()
        .iter()
        .map(|time| time.expect("everyone hears"))
        .collect::<Vec<_>>();
    assert_eq!(alerted[0], 0.0);
    assert!(alerted[1] > alerted[0] + 2.0, "{alerted:?}");
    assert!(alerted[2] > alerted[1] + 1.0, "{alerted:?}");
}

#[test]
fn a_fire_in_an_empty_room_alerts_nobody_until_it_spreads() {
    let mut evacuation = Evacuation::new(
        &two_room_scene(),
        &[occupant("upstairs", 3.0, 10.0, 0.0)],
        SimulationConfig::default(),
    )
    .expect("evacuation should build");
    let empty_room = [fire_in("room-1", point(3.0, 2.0), 500.0)];
    for second in 0..=5 {
        evacuation
            .advance_to(f64::from(second), &empty_room)
            .expect("frame");
    }
    assert_eq!(evacuation.alarm_raised_at(), None);

    let spread = [
        fire_in("room-1", point(3.0, 2.0), 500.0),
        fire_in("room-2", point(3.0, 6.0), 60.0),
    ];
    evacuation.advance_to(6.0, &spread).expect("frame");
    assert_eq!(evacuation.alarm_raised_at(), Some(6.0));
}

#[test]
fn the_pre_movement_delay_runs_from_the_alarm() {
    let mut evacuation = Evacuation::new(
        &two_room_scene(),
        &[occupant("near", 1.0, 1.0, 3.0)],
        SimulationConfig::default(),
    )
    .expect("evacuation should build");
    let fire = [fire_in("room-1", point(3.0, 2.0), 80.0)];
    let start = evacuation.advance_to(10.0, &fire).expect("frame")[0].position_m;
    let agents = evacuation.advance_to(12.0, &fire).expect("frame");
    assert_eq!(phase(&agents[0]), OccupantPhase::Preparing);
    assert_eq!(agents[0].position_m, start);
    let agents = evacuation.advance_to(14.0, &fire).expect("frame");
    assert_ne!(agents[0].position_m, start);
}

fn fire_in(room_id: &str, position_m: Vec3, hrr_kw: f64) -> SurfaceFireNodeFrame {
    SurfaceFireNodeFrame {
        id: format!("fire-{room_id}"),
        room_id: room_id.to_owned(),
        dimensions_m: Dimensions3 {
            width_m: 0.25,
            depth_m: 0.25,
            height_m: 0.02,
        },
        heat_release_rate_kw: hrr_kw,
        ..burning_floor_cell(position_m)
    }
}

/// A 6 m x 4 m room with the exit, and a 6 m x 8 m room above it through a
/// door.
fn two_room_scene() -> Scene {
    let door = |id: &str, relative_center: f64| Opening {
        id: id.to_owned(),
        label: None,
        kind: OpeningKind::Door,
        width_m: 1.0,
        height_m: Some(2.1),
        lower_elevation_m: Some(0.0),
        relative_center,
    };
    let room = |id: &str, boundary| Room {
        id: id.to_owned(),
        label: None,
        classifications: Vec::new(),
        dimensions: None,
        boundary,
    };
    let mut scene = scene_fixture();
    scene.floors[0].rooms = vec![
        room(
            "room-1",
            vec![
                edge(
                    "1-south",
                    point(0.0, 0.0),
                    point(6.0, 0.0),
                    vec![door("exit-1", 0.5)],
                ),
                edge("1-east", point(6.0, 0.0), point(6.0, 4.0), Vec::new()),
                edge(
                    "1-north",
                    point(6.0, 4.0),
                    point(0.0, 4.0),
                    vec![door("1-door", 0.5)],
                ),
                edge("1-west", point(0.0, 4.0), point(0.0, 0.0), Vec::new()),
            ],
        ),
        room(
            "room-2",
            vec![
                edge(
                    "2-south",
                    point(0.0, 4.0),
                    point(6.0, 4.0),
                    vec![door("2-door", 0.5)],
                ),
                edge("2-east", point(6.0, 4.0), point(6.0, 12.0), Vec::new()),
                edge("2-north", point(6.0, 12.0), point(0.0, 12.0), Vec::new()),
                edge("2-west", point(0.0, 12.0), point(0.0, 4.0), Vec::new()),
            ],
        ),
    ];
    scene
}

fn phase(agent: &tflop_simulation_core::AgentFrame) -> OccupantPhase {
    let AgentFrameRole::Occupant { phase, .. } = agent.role else {
        panic!("expected occupant")
    };
    phase
}

fn occupant(id: &str, x: f64, y: f64, delay: f64) -> OccupantSpawn {
    OccupantSpawn {
        id: id.to_owned(),
        floor_id: "floor-1".to_owned(),
        position_m: point(x, y),
        walking_speed_m_per_s: 1.0,
        pre_movement_delay_seconds: delay,
        injured: false,
    }
}

fn scene_fixture() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "evacuation-scene".to_owned(),
        name: None,
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "evacuation-scene".to_owned(),
        },
        assets: Vec::new(),
        floors: vec![Floor {
            id: "floor-1".to_owned(),
            label: None,
            elevation_m: Some(0.0),
            rooms: vec![Room {
                id: "room-1".to_owned(),
                label: None,
                classifications: Vec::new(),
                dimensions: None,
                boundary: vec![
                    edge(
                        "south",
                        point(0.0, 0.0),
                        point(6.0, 0.0),
                        vec![Opening {
                            id: "exit-1".to_owned(),
                            label: None,
                            kind: OpeningKind::Door,
                            width_m: 1.2,
                            height_m: Some(2.1),
                            lower_elevation_m: Some(0.0),
                            relative_center: 0.5,
                        }],
                    ),
                    edge("east", point(6.0, 0.0), point(6.0, 4.0), Vec::new()),
                    edge("north", point(6.0, 4.0), point(0.0, 4.0), Vec::new()),
                    edge("west", point(0.0, 4.0), point(0.0, 0.0), Vec::new()),
                ],
            }],
            objects: Vec::new(),
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

fn point(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
