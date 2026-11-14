use std::collections::BTreeSet;

use tflop_simulation_core::{
    AgentFrame, AgentFrameRole, CombustionState, CrewReport, CrewSpawn, FIREFIGHTER_RADIUS_M,
    FireMaterialCatalog, FirefighterPhase, Firefighting, FirefightingConfig, FirefightingError,
    Movement, OCCUPANT_RADIUS_M, Rehearsal, RehearsalFrame, Simulation, SimulationConfig,
    SimulationGrid, SurfaceFireConfig, SurfaceFireNodeFrame, SurfaceFireWorld,
    reachable_exit_cells,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, FurnitureAsset, FurnitureObject, Mobility,
    Opening, OpeningKind, Quaternion, Room, SCENE_SCHEMA_VERSION, Scene, SceneSource, Transform3,
    Vec3,
};

const LEADER: &str = "engine-1/1";
const FOLLOWER: &str = "engine-1/2";

#[test]
fn fixture_rooms_connect_to_the_exit_except_the_closet() {
    let scene = scene_fixture(true);
    let reachable =
        reachable_exit_cells(&scene, SimulationConfig::default()).expect("exits resolve");
    let grid = grid(&scene);
    let reachable_rooms = grid
        .cells()
        .iter()
        .filter(|cell| reachable.contains(&("floor-1".to_owned(), cell.coordinate)))
        .flat_map(|cell| cell.room_ids.iter().cloned())
        .collect::<BTreeSet<_>>();

    assert_eq!(reachable_rooms, set(&["east", "hall", "west"]));
}

#[test]
fn crew_stages_at_the_entry_until_it_arrives() {
    let scene = scene_fixture(false);
    let crew = CrewSpawn {
        arrival_time_seconds: 10.0,
        ..crew(&[])
    };
    let (_, with_crew) = run(&scene, Some("chair-east"), Some(crew), 30.0);
    let (_, without_crew) = run(&scene, Some("chair-east"), None, 30.0);

    let start = firefighter(&with_crew[0], LEADER).position_m;
    for (frame, reference) in with_crew.iter().zip(&without_crew) {
        if frame.fire.time_seconds > 10.0 {
            break;
        }
        assert_eq!(frame.fire, reference.fire, "no water before arrival");
        for id in [LEADER, FOLLOWER] {
            let agent = firefighter(frame, id);
            assert_eq!(phase(agent), FirefighterPhase::Staging);
            assert_eq!(agent.position_m, start);
        }
    }
    assert_ne!(
        phase(firefighter(&with_crew[11], LEADER)),
        FirefighterPhase::Staging,
        "the crew acts from its arrival time"
    );
}

#[test]
fn firefighters_stay_on_walkable_ground_and_out_of_flames() {
    let scene = scene_fixture(false);
    let (_, frames) = run(&scene, Some("chair-east"), Some(crew(&[])), 300.0);
    let grid = grid(&scene);

    for (index, frame) in frames.iter().enumerate() {
        for id in [LEADER, FOLLOWER] {
            let agent = firefighter(frame, id);
            assert!(
                grid.circle_is_walkable(agent.position_m, OCCUPANT_RADIUS_M),
                "{id} off walkable ground at t={}: {:?}",
                frame.fire.time_seconds,
                agent.position_m
            );
            let Some(previous) = index.checked_sub(1).map(|index| &frames[index]) else {
                continue;
            };
            // A firefighter who moved this step must have stopped short of
            // every node that was burning when it moved. One who stood still
            // may have had the fire spread to it.
            if firefighter(previous, id).position_m == agent.position_m {
                continue;
            }
            for node in burning(previous) {
                let clearance = footprint_radius(node) + OCCUPANT_RADIUS_M - 0.05;
                assert!(
                    distance(agent.position_m, node.position_m) >= clearance,
                    "{id} walked into {} at t={}",
                    node.id,
                    frame.fire.time_seconds
                );
            }
        }
    }
}

#[test]
fn with_no_fire_the_crew_searches_every_room_in_order_then_leaves() {
    let scene = scene_fixture(false);
    let (rehearsal, frames) = run(&scene, None, Some(crew(&["west", "east"])), 400.0);

    let report = report(&rehearsal);
    assert_eq!(report.phase, FirefighterPhase::Exited);
    assert!(rehearsal.is_settled());
    assert!(frames.last().expect("frames").fire.time_seconds < 400.0);
    let searched = &report.memory.searched;
    assert_eq!(
        searched.keys().cloned().collect::<BTreeSet<_>>(),
        set(&["east", "hall", "west"])
    );
    assert!(searched["west"] < searched["east"], "{searched:?}");
    assert!(
        searched["east"] < searched["hall"],
        "unlisted rooms come after the search order: {searched:?}"
    );
    assert!(report.memory.unreachable.is_empty());
    assert!(
        frames
            .iter()
            .all(|frame| phase(firefighter(frame, LEADER)) != FirefighterPhase::Suppressing)
    );
}

#[test]
fn the_crew_learns_of_the_fire_only_by_observing_it() {
    let scene = scene_fixture(false);
    let config = FirefightingConfig::default();
    let mut rehearsal = rehearsal(&scene, Some(crew(&["west", "east"])));
    let mut previous = rehearsal.ignite_object("chair-east").expect("ignites");

    let mut first_known_at = None;
    while previous.fire.time_seconds < 300.0 && !rehearsal.is_settled() {
        let frame = rehearsal.step(1.0).expect("steps");
        let memory = report(&rehearsal).memory;
        if first_known_at.is_none() && !memory.known_fires.is_empty() {
            // The crew observed at the start of this step, from where the
            // leader stood in the previous frame, against that frame's fire.
            let leader = firefighter(&previous, LEADER).position_m;
            let saw_east = memory.first_observed.contains_key("east");
            let felt_heat = burning(&previous)
                .any(|node| distance(node.position_m, leader) <= config.heat_sense_radius_m);
            assert!(
                saw_east || felt_heat,
                "the crew knew of fire it could neither see nor feel"
            );
            first_known_at = Some(frame.fire.time_seconds);
        }
        previous = frame;
    }

    let memory = report(&rehearsal).memory;
    assert!(first_known_at.is_some(), "the crew found the fire");
    assert!(
        memory.searched["west"] < memory.first_observed["east"],
        "West is searched before the crew first enters East: {memory:?}"
    );
}

#[test]
fn the_crew_finds_the_fire_and_puts_it_out() {
    let scene = scene_fixture(false);
    let crew = CrewSpawn {
        arrival_time_seconds: 10.0,
        ..crew(&[])
    };
    let (rehearsal, frames) = run(&scene, Some("chair-east"), Some(crew), 300.0);
    let last = frames.last().expect("frames");

    assert!(rehearsal.is_settled(), "fire out and crew gone");
    assert_eq!(report(&rehearsal).phase, FirefighterPhase::Exited);
    assert_eq!(burning(last).count(), 0);
    assert!(
        last.fire
            .active_nodes
            .iter()
            .any(|node| node.state == CombustionState::Extinguished)
    );
    assert!(
        frames
            .iter()
            .any(|frame| phase(firefighter(frame, LEADER)) == FirefighterPhase::Suppressing)
    );

    let (_, unfought) = run(&scene, Some("chair-east"), None, last.fire.time_seconds);
    let unfought_last = unfought.last().expect("frames");
    assert_eq!(unfought_last.fire.time_seconds, last.fire.time_seconds);
    assert!(
        burning(unfought_last).count() > 0,
        "left alone, the fire is still burning"
    );
    assert!(burned_area(last) < burned_area(unfought_last));
}

#[test]
fn searching_the_fire_room_first_contains_it_sooner() {
    let scene = scene_fixture(false);
    let contained_at = |order: &[&str]| {
        let (_, frames) = run(&scene, Some("chair-east"), Some(crew(order)), 300.0);
        frames
            .iter()
            .skip(1)
            .find(|frame| burning(frame).count() == 0)
            .map(|frame| frame.fire.time_seconds)
            .expect("the fire is contained")
    };

    let east_first = contained_at(&["east", "west"]);
    let west_first = contained_at(&["west", "east"]);

    assert!(
        east_first < west_first,
        "east first: {east_first}s, west first: {west_first}s"
    );
}

#[test]
fn the_crew_sprays_only_at_fire_in_its_own_room_within_reach() {
    let scene = scene_fixture(false);
    let (_, frames) = run(&scene, Some("chair-east"), Some(crew(&[])), 300.0);

    assert!(assert_sprays_only_at_fire_in_reach(&scene, &frames) > 0);
}

#[test]
fn furniture_near_a_shared_wall_belongs_to_the_room_it_stands_in() {
    let mut scene = scene_fixture(false);
    scene.floors[0]
        .objects
        .push(chair("chair-east-wall", 4.4, 4.9));
    let fire = SurfaceFireWorld::from_scene(
        &scene,
        &FireMaterialCatalog::default(),
        SurfaceFireConfig::default(),
    )
    .expect("fire world builds");

    // The coarse fire cell under this chair straddles the West/East wall.
    assert_eq!(
        fire.node("object:chair-east-wall").expect("chair").room_id,
        "east"
    );
}

#[test]
fn fire_felt_through_a_wall_is_fought_from_inside_its_room() {
    let mut scene = scene_fixture(false);
    // Just inside East, 0.4 m from the wall it shares with West.
    scene.floors[0]
        .objects
        .push(chair("chair-east-wall", 4.4, 4.9));
    // Wide enough that the leader, heading into West, feels the fire behind
    // the wall before it has seen East.
    let config = FirefightingConfig {
        heat_sense_radius_m: 3.0,
        ..FirefightingConfig::default()
    };
    let mut rehearsal = rehearsal_with(&scene, Some(crew(&["west", "east"])), config);
    let mut frames = vec![rehearsal.ignite_object("chair-east-wall").expect("ignites")];
    let mut felt_before_seeing_east = false;
    while frames.last().expect("frames").fire.time_seconds < 300.0 && !rehearsal.is_settled() {
        frames.push(rehearsal.step(1.0).expect("steps"));
        let memory = report(&rehearsal).memory;
        if !memory.known_fires.is_empty() && !memory.first_observed.contains_key("east") {
            felt_before_seeing_east = true;
        }
    }

    assert!(
        felt_before_seeing_east,
        "precondition: heat felt through the wall"
    );
    assert!(assert_sprays_only_at_fire_in_reach(&scene, &frames) > 0);
    assert!(
        rehearsal.is_settled(),
        "the fire was put out and the crew left"
    );
}

/// Every frame where the leader is suppressing follows a frame with a
/// burning node in one of the rooms of the leader's cell, within nozzle
/// reach. Returns how many such frames there were.
fn assert_sprays_only_at_fire_in_reach(scene: &Scene, frames: &[RehearsalFrame]) -> usize {
    let grid = grid(scene);
    let reach = FirefightingConfig::default().nozzle_reach_m;
    let mut sprayed = 0;
    for pair in frames.windows(2) {
        let leader = firefighter(&pair[1], LEADER);
        if phase(leader) != FirefighterPhase::Suppressing {
            continue;
        }
        sprayed += 1;
        let cell = grid
            .world_to_cell(leader.position_m)
            .and_then(|coordinate| grid.cell(coordinate))
            .expect("leader stands on a cell");
        assert!(
            burning(&pair[0]).any(|node| cell.room_ids.contains(&node.room_id)
                && distance(node.position_m, leader.position_m) <= reach),
            "sprayed at t={} from {:?} (rooms {:?}) with no fire in reach in its room",
            pair[1].fire.time_seconds,
            leader.position_m,
            cell.room_ids
        );
    }
    sprayed
}

#[test]
fn the_crew_stays_together_without_overlapping() {
    let scene = scene_fixture(false);
    let (_, frames) = run(
        &scene,
        Some("chair-east"),
        Some(crew(&["west", "east"])),
        300.0,
    );

    let mut checked = 0;
    for frame in &frames {
        let leader = firefighter(frame, LEADER);
        if matches!(
            phase(leader),
            FirefighterPhase::Staging | FirefighterPhase::Exited
        ) {
            continue;
        }
        let follower = firefighter(frame, FOLLOWER);
        let gap = distance(leader.position_m, follower.position_m);
        // Crewmates may squeeze past each other in a doorway.
        let squeezing = [leader.movement, follower.movement].contains(&Movement::Squeezing);
        assert!(
            squeezing || gap >= 2.0 * FIREFIGHTER_RADIUS_M,
            "overlap {gap:.2} m at t={}",
            frame.fire.time_seconds
        );
        assert!(
            gap <= 3.0,
            "gap {gap:.2} m at t={}",
            frame.fire.time_seconds
        );
        assert_eq!(phase(firefighter(frame, FOLLOWER)), phase(leader));
        checked += 1;
    }
    assert!(checked > 20);
}

#[test]
fn a_room_with_no_route_is_reported_unreachable_and_skipped() {
    let scene = scene_fixture(true);
    let (rehearsal, _) = run(&scene, None, Some(crew(&["closet", "west"])), 400.0);

    let report = report(&rehearsal);
    assert_eq!(report.phase, FirefighterPhase::Exited);
    assert_eq!(report.memory.unreachable, set(&["closet"]));
    assert_eq!(
        report
            .memory
            .searched
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        set(&["east", "hall", "west"])
    );
}

#[test]
fn identical_inputs_produce_identical_runs() {
    let scene = scene_fixture(false);
    let first = run(&scene, Some("chair-east"), Some(crew(&[])), 200.0).1;
    let second = run(&scene, Some("chair-east"), Some(crew(&[])), 200.0).1;

    assert_eq!(first, second);
}

#[test]
fn rejects_invalid_crews_and_config() {
    let scene = scene_fixture(false);
    let build = |crew: CrewSpawn, config: FirefightingConfig| {
        Firefighting::new(&scene, &[crew], SimulationConfig::default(), config)
    };

    let Err(FirefightingError::InvalidCrews(errors)) = build(
        CrewSpawn {
            entry_opening_id: Some("nope".to_owned()),
            search_order: vec!["attic".to_owned(), "west".to_owned(), "west".to_owned()],
            ..crew(&[])
        },
        FirefightingConfig::default(),
    ) else {
        panic!("invalid crew should fail");
    };
    let paths = errors
        .0
        .iter()
        .map(|error| error.path.as_str())
        .collect::<BTreeSet<_>>();
    assert!(paths.contains("crews[0].entry_opening_id"), "{errors:?}");
    assert!(paths.contains("crews[0].search_order[2]"), "{errors:?}");

    let Err(FirefightingError::InvalidCrews(errors)) = build(
        CrewSpawn {
            search_order: vec!["attic".to_owned()],
            ..crew(&[])
        },
        FirefightingConfig::default(),
    ) else {
        panic!("unknown room should fail");
    };
    assert_eq!(errors.0[0].path, "crews[0].search_order[0]");

    assert!(matches!(
        build(
            crew(&[]),
            FirefightingConfig {
                approach_fraction: 1.5,
                ..FirefightingConfig::default()
            }
        ),
        Err(FirefightingError::InvalidConfig {
            field: "approach_fraction"
        })
    ));
}

#[test]
fn a_crew_without_an_entry_uses_the_front_door() {
    let scene = scene_fixture(false);
    let firefighting = Firefighting::new(
        &scene,
        &[crew(&[])],
        SimulationConfig::default(),
        FirefightingConfig::default(),
    )
    .expect("crew is valid");

    let leader = &firefighting.agents()[0];
    let tflop_simulation_core::AgentRole::Firefighter(state) = &leader.role else {
        panic!("expected a firefighter");
    };
    assert_eq!(state.entry_opening_id, "exit-front");
    assert!(distance(leader.position_m, point(1.0, 0.0)) <= 1.5);
}

fn run(
    scene: &Scene,
    ignite: Option<&str>,
    crew: Option<CrewSpawn>,
    duration_seconds: f64,
) -> (Rehearsal, Vec<RehearsalFrame>) {
    let mut rehearsal = rehearsal(scene, crew);
    let mut frames = Vec::new();
    if let Some(object_id) = ignite {
        frames.push(rehearsal.ignite_object(object_id).expect("ignites"));
    } else {
        frames.push(rehearsal.step(1.0).expect("steps"));
    }
    frames.extend(
        rehearsal
            .run_until(duration_seconds, 1.0)
            .expect("run succeeds"),
    );
    (rehearsal, frames)
}

fn rehearsal(scene: &Scene, crew: Option<CrewSpawn>) -> Rehearsal {
    rehearsal_with(scene, crew, FirefightingConfig::default())
}

fn rehearsal_with(scene: &Scene, crew: Option<CrewSpawn>, config: FirefightingConfig) -> Rehearsal {
    let fire = SurfaceFireWorld::from_scene(
        scene,
        &FireMaterialCatalog::default(),
        SurfaceFireConfig::default(),
    )
    .expect("fire world builds");
    let rehearsal = Rehearsal::new(fire, None);
    match crew {
        Some(crew) => rehearsal.with_firefighting(
            Firefighting::new(scene, &[crew], SimulationConfig::default(), config)
                .expect("crew is valid"),
        ),
        None => rehearsal,
    }
}

fn crew(search_order: &[&str]) -> CrewSpawn {
    CrewSpawn {
        id: "engine-1".to_owned(),
        entry_opening_id: None,
        search_order: search_order.iter().map(|&room| room.to_owned()).collect(),
        arrival_time_seconds: 0.0,
        member_count: 2,
        walking_speed_m_per_s: 1.0,
    }
}

fn report(rehearsal: &Rehearsal) -> CrewReport {
    rehearsal
        .firefighting()
        .expect("crew present")
        .crew_reports()
        .remove(0)
}

fn firefighter<'a>(frame: &'a RehearsalFrame, id: &str) -> &'a AgentFrame {
    frame
        .agents
        .iter()
        .find(|agent| agent.id == id)
        .expect("firefighter present in every frame")
}

fn phase(agent: &AgentFrame) -> FirefighterPhase {
    let AgentFrameRole::Firefighter { phase, .. } = agent.role else {
        panic!("expected a firefighter")
    };
    phase
}

fn burning(frame: &RehearsalFrame) -> impl Iterator<Item = &SurfaceFireNodeFrame> {
    frame
        .fire
        .active_nodes
        .iter()
        .filter(|node| node.state == CombustionState::Burning)
}

/// Floor area already burned, weighted by fuel consumed.
fn burned_area(frame: &RehearsalFrame) -> f64 {
    frame
        .fire
        .active_nodes
        .iter()
        .map(|node| {
            (1.0 - node.fuel_remaining_fraction)
                * node.dimensions_m.width_m
                * node.dimensions_m.depth_m
        })
        .sum()
}

fn footprint_radius(node: &SurfaceFireNodeFrame) -> f64 {
    node.dimensions_m.width_m.hypot(node.dimensions_m.depth_m) / 2.0
}

fn distance(left: Vec3, right: Vec3) -> f64 {
    (left.x - right.x).hypot(left.y - right.y)
}

fn grid(scene: &Scene) -> SimulationGrid {
    Simulation::new(scene.clone(), SimulationConfig::default())
        .expect("scene is valid")
        .grid("floor-1")
        .expect("floor grid")
        .clone()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|&item| item.to_owned()).collect()
}

/// An 8 x 3 m hall with a front door at x = 1, and two 4 x 4 m rooms behind
/// it (West, East) each reached by a 1 m door. Each side room has a chair in
/// its far corner, more than the heat-sense radius from its door. With
/// `closet`, a 2 x 3 m room east of the hall that only has a window.
fn scene_fixture(closet: bool) -> Scene {
    let mut rooms = vec![
        room(
            "hall",
            vec![
                edge(
                    "hall-south",
                    point(0.0, 0.0),
                    point(8.0, 0.0),
                    vec![door("exit-front", 1.0 / 8.0)],
                ),
                edge(
                    "hall-east",
                    point(8.0, 0.0),
                    point(8.0, 3.0),
                    if closet {
                        vec![window("closet-window-hall")]
                    } else {
                        vec![]
                    },
                ),
                edge(
                    "hall-north",
                    point(8.0, 3.0),
                    point(0.0, 3.0),
                    vec![
                        door("door-east-hall", 2.0 / 8.0),
                        door("door-west-hall", 6.0 / 8.0),
                    ],
                ),
                edge("hall-west", point(0.0, 3.0), point(0.0, 0.0), vec![]),
            ],
        ),
        room(
            "west",
            vec![
                edge(
                    "west-south",
                    point(0.0, 3.0),
                    point(4.0, 3.0),
                    vec![door("door-west-room", 0.5)],
                ),
                edge("west-east", point(4.0, 3.0), point(4.0, 7.0), vec![]),
                edge("west-north", point(4.0, 7.0), point(0.0, 7.0), vec![]),
                edge("west-west", point(0.0, 7.0), point(0.0, 3.0), vec![]),
            ],
        ),
        room(
            "east",
            vec![
                edge(
                    "east-south",
                    point(4.0, 3.0),
                    point(8.0, 3.0),
                    vec![door("door-east-room", 0.5)],
                ),
                edge("east-east", point(8.0, 3.0), point(8.0, 7.0), vec![]),
                edge("east-north", point(8.0, 7.0), point(4.0, 7.0), vec![]),
                edge("east-west", point(4.0, 7.0), point(4.0, 3.0), vec![]),
            ],
        ),
    ];
    if closet {
        rooms.push(room(
            "closet",
            vec![
                edge("closet-south", point(8.0, 0.0), point(10.0, 0.0), vec![]),
                edge("closet-east", point(10.0, 0.0), point(10.0, 3.0), vec![]),
                edge("closet-north", point(10.0, 3.0), point(8.0, 3.0), vec![]),
                edge(
                    "closet-west",
                    point(8.0, 3.0),
                    point(8.0, 0.0),
                    vec![window("closet-window-closet")],
                ),
            ],
        ));
    }

    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "firefighting-scene".to_owned(),
        name: None,
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "firefighting-scene".to_owned(),
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
            label: None,
            elevation_m: Some(0.0),
            rooms,
            objects: vec![chair("chair-west", 0.8, 6.2), chair("chair-east", 7.2, 6.2)],
        }],
    }
}

fn room(id: &str, boundary: Vec<BoundaryEdge>) -> Room {
    Room {
        id: id.to_owned(),
        label: None,
        classifications: Vec::new(),
        dimensions: None,
        boundary,
    }
}

fn chair(id: &str, x: f64, y: f64) -> FurnitureObject {
    FurnitureObject {
        id: id.to_owned(),
        asset_id: "chair-basic".to_owned(),
        transform: Transform3 {
            position_m: point(x, y),
            rotation: Quaternion::IDENTITY,
            scale: Vec3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        },
        mobility: Mobility::Movable,
    }
}

fn door(id: &str, relative_center: f64) -> Opening {
    Opening {
        id: id.to_owned(),
        label: None,
        kind: OpeningKind::Door,
        width_m: 1.0,
        height_m: Some(2.1),
        lower_elevation_m: Some(0.0),
        relative_center,
    }
}

fn window(id: &str) -> Opening {
    Opening {
        id: id.to_owned(),
        label: None,
        kind: OpeningKind::Window,
        width_m: 1.0,
        height_m: Some(1.0),
        lower_elevation_m: Some(1.0),
        relative_center: 0.5,
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
