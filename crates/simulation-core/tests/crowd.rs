//! Crowds: occupants and firefighters share one space, queue single file in
//! doors and hallways, and squeeze past each other.

use tflop_simulation_core::{
    AgentFrame, AgentFrameRole, AlarmConfig, CROWD_GIVE_M, CrewSpawn, CrowdConfig, Evacuation,
    FireMaterialCatalog, FirefighterPhase, Firefighting, FirefightingConfig, Movement,
    OccupantPhase, OccupantSpawn, Rehearsal, RehearsalFrame, SimulationConfig, SurfaceFireConfig,
    SurfaceFireWorld,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Floor, Opening, OpeningKind, Room, SCENE_SCHEMA_VERSION, Scene,
    SceneSource, Vec3,
};

const STEP_SECONDS: f64 = 0.5;
const HALL_WIDTH_M: f64 = 1.2;

#[test]
fn bodies_never_press_into_each_other_unless_squeezing_past() {
    let frames = run(&occupants(30), None, 240.0);
    for frame in &frames {
        let inside = inside(frame);
        for (index, left) in inside.iter().enumerate() {
            for right in &inside[index + 1..] {
                if squeezing(left) || squeezing(right) {
                    continue;
                }
                // Pressed together in an open crowd, people give a little.
                let gap = distance(left.position_m, right.position_m);
                assert!(
                    gap >= left.radius_m + right.radius_m - 2.0 * CROWD_GIVE_M - 1.0e-6,
                    "{} and {} overlap by {:.3} m at t={}",
                    left.id,
                    right.id,
                    left.radius_m + right.radius_m - gap,
                    frame.fire.time_seconds
                );
            }
        }
    }
}

#[test]
fn nobody_walks_abreast_in_the_hallway() {
    let frames = run(&occupants(30), None, 240.0);
    let mut hall_frames = 0;
    for frame in &frames {
        let in_hall = inside(frame)
            .into_iter()
            .filter(|agent| in_hallway(agent.position_m))
            .collect::<Vec<_>>();
        if in_hall.len() > 1 {
            hall_frames += 1;
        }
        for (index, left) in in_hall.iter().enumerate() {
            for right in &in_hall[index + 1..] {
                if squeezing(left) || squeezing(right) {
                    continue;
                }
                let along = (left.position_m.x - right.position_m.x).abs();
                assert!(
                    along >= left.radius_m + right.radius_m - 1.0e-6,
                    "{} and {} side by side ({along:.2} m apart along the hall) at t={}",
                    left.id,
                    right.id,
                    frame.fire.time_seconds
                );
            }
        }
    }
    assert!(hall_frames > 10, "the hallway was never shared");
}

#[test]
fn a_crowd_queues_at_the_door_and_everyone_still_gets_out() {
    let frames = run(&occupants(30), None, 300.0);
    let last = frames.last().expect("frames");
    assert!(
        occupant_frames(last).all(|agent| phase(agent) == OccupantPhase::Exited),
        "everyone should get out by t={}",
        last.fire.time_seconds
    );
    let most_queued = frames
        .iter()
        .map(|frame| {
            occupant_frames(frame)
                .filter(|agent| agent.movement == Movement::Queued)
                .count()
        })
        .max()
        .unwrap_or(0);
    assert!(most_queued >= 5, "only {most_queued} queued at once");
}

#[test]
fn more_people_take_longer_to_get_out() {
    let few = last_exit_seconds(&run(&occupants(5), None, 400.0));
    let many = last_exit_seconds(&run(&occupants(40), None, 400.0));
    assert!(many > few + 10.0, "5 people out by {few} s, 40 by {many} s");
}

#[test]
fn single_file_limits_flow_through_the_exit() {
    let frames = run(&occupants(40), None, 400.0);
    // Shoulder to shoulder at walking speed, one body width apart.
    let max_per_second = 1.4 / 0.5;
    let window_seconds = 5.0;
    let exits = exit_times(&frames);
    for (index, &start) in exits.iter().enumerate() {
        let in_window = exits[index..]
            .iter()
            .take_while(|&&time| time < start + window_seconds)
            .count();
        assert!(
            in_window as f64 <= max_per_second * window_seconds + 1.0,
            "{in_window} people out within {window_seconds} s of t={start}"
        );
    }
}

#[test]
fn a_crew_entering_against_the_crowd_is_slowed_but_gets_through() {
    let alone = office_reached_seconds(&run(&[], Some(crew(0.0)), 120.0));
    let against = office_reached_seconds(&run(&occupants(30), Some(crew(0.0)), 300.0));
    let alone = alone.expect("an unopposed crew reaches the office");
    let against = against.expect("the crew pushes through the crowd");
    assert!(
        against > alone + 5.0,
        "crew reached the office at {alone} s alone, {against} s against the crowd"
    );
}

#[test]
fn firefighters_squeeze_past_occupants() {
    let frames = run(&occupants(30), Some(crew(0.0)), 300.0);
    assert!(frames.iter().any(|frame| {
        frame.agents.iter().any(|agent| {
            matches!(agent.role, AgentFrameRole::Firefighter { .. })
                && agent.movement == Movement::Squeezing
        })
    }));
}

#[test]
fn a_crew_staging_at_the_exit_does_not_block_it() {
    let frames = run(&occupants(10), Some(crew(1_000.0)), 200.0);
    let last = frames.last().expect("frames");
    assert!(occupant_frames(last).all(|agent| phase(agent) == OccupantPhase::Exited));
    assert!(last.agents.iter().all(|agent| match &agent.role {
        AgentFrameRole::Firefighter { phase, .. } => *phase == FirefighterPhase::Staging,
        AgentFrameRole::Occupant { .. } => true,
    }));
}

#[test]
fn a_packed_office_and_a_crew_never_gridlock() {
    let scene = scene_fixture();
    for seed in 0..4 {
        let config = SimulationConfig {
            random_seed: seed,
            ..SimulationConfig::default()
        };
        let evacuation = Evacuation::new(&scene, &occupants(45), config)
            .expect("evacuation builds")
            .with_alarm(AlarmConfig::Immediate);
        let crew = Firefighting::new(&scene, &[crew(5.0)], config, FirefightingConfig::default())
            .expect("crew is valid");
        let mut rehearsal =
            Rehearsal::new(fire_world(&scene), Some(evacuation)).with_firefighting(crew);
        let frames = rehearsal
            .run_until(600.0, STEP_SECONDS)
            .expect("run succeeds");
        let last = frames.last().expect("frames");
        assert!(
            rehearsal.is_settled(),
            "seed {seed}: {} still inside at t={}",
            inside(last).len(),
            last.fire.time_seconds
        );
    }
}

#[test]
fn identical_inputs_produce_identical_frames() {
    let first = run(&occupants(20), Some(crew(0.0)), 120.0);
    let second = run(&occupants(20), Some(crew(0.0)), 120.0);
    assert_eq!(first, second);
}

#[test]
fn a_different_seed_changes_who_goes_first() {
    let frames = |seed| {
        let scene = scene_fixture();
        let evacuation = Evacuation::new(
            &scene,
            &occupants(20),
            SimulationConfig {
                random_seed: seed,
                ..SimulationConfig::default()
            },
        )
        .expect("evacuation builds")
        .with_alarm(AlarmConfig::Immediate);
        let mut rehearsal = Rehearsal::new(fire_world(&scene), Some(evacuation));
        rehearsal
            .run_until(60.0, STEP_SECONDS)
            .expect("run succeeds")
    };
    assert_ne!(frames(1), frames(2));
}

#[test]
fn rejects_an_invalid_crowd_config() {
    let scene = scene_fixture();
    let rehearsal = Rehearsal::new(fire_world(&scene), None);
    let config = CrowdConfig {
        occupant_squeeze_speed_factor: 0.0,
        ..CrowdConfig::default()
    };
    assert!(rehearsal.with_crowd_config(config).is_err());
}

fn run(occupants: &[OccupantSpawn], crew: Option<CrewSpawn>, until: f64) -> Vec<RehearsalFrame> {
    let scene = scene_fixture();
    let evacuation = (!occupants.is_empty()).then(|| {
        Evacuation::new(&scene, occupants, SimulationConfig::default())
            .expect("evacuation builds")
            .with_alarm(AlarmConfig::Immediate)
    });
    let mut rehearsal = Rehearsal::new(fire_world(&scene), evacuation);
    if let Some(crew) = crew {
        rehearsal = rehearsal.with_firefighting(
            Firefighting::new(
                &scene,
                &[crew],
                SimulationConfig::default(),
                FirefightingConfig::default(),
            )
            .expect("crew is valid"),
        );
    }
    let mut frames = vec![rehearsal.step(STEP_SECONDS).expect("steps")];
    frames.extend(
        rehearsal
            .run_until(until, STEP_SECONDS)
            .expect("run succeeds"),
    );
    frames
}

fn fire_world(scene: &Scene) -> SurfaceFireWorld {
    SurfaceFireWorld::from_scene(
        scene,
        &FireMaterialCatalog::default(),
        SurfaceFireConfig::default(),
    )
    .expect("fire world builds")
}

/// Up to 49 people on a grid across the office, nearest the door first.
fn occupants(count: usize) -> Vec<OccupantSpawn> {
    let mut spots = Vec::new();
    for row in 0..7 {
        for column in 0..7 {
            spots.push(point(
                0.6 + 0.8 * f64::from(column),
                HALL_WIDTH_M + 0.6 + 0.8 * f64::from(row),
            ));
        }
    }
    spots.sort_by(|left, right| {
        distance(*left, point(3.0, HALL_WIDTH_M))
            .total_cmp(&distance(*right, point(3.0, HALL_WIDTH_M)))
    });
    spots
        .into_iter()
        .take(count)
        .enumerate()
        .map(|(index, position_m)| OccupantSpawn {
            id: format!("occupant-{index}"),
            floor_id: "floor-1".to_owned(),
            position_m,
            walking_speed_m_per_s: 1.4,
            pre_movement_delay_seconds: 0.0,
            injured: false,
        })
        .collect()
}

fn crew(arrival_time_seconds: f64) -> CrewSpawn {
    CrewSpawn {
        id: "engine-1".to_owned(),
        entry_opening_id: Some("exit-east".to_owned()),
        route: vec!["office".to_owned()],
        arrival_time_seconds,
        member_count: 2,
        walking_speed_m_per_s: 1.0,
    }
}

fn inside(frame: &RehearsalFrame) -> Vec<&AgentFrame> {
    frame
        .agents
        .iter()
        .filter(|agent| match &agent.role {
            AgentFrameRole::Occupant { phase, .. } => *phase != OccupantPhase::Exited,
            AgentFrameRole::Firefighter { phase, .. } => {
                !matches!(phase, FirefighterPhase::Staging | FirefighterPhase::Exited)
            }
        })
        .collect()
}

fn occupant_frames(frame: &RehearsalFrame) -> impl Iterator<Item = &AgentFrame> {
    frame
        .agents
        .iter()
        .filter(|agent| matches!(agent.role, AgentFrameRole::Occupant { .. }))
}

fn phase(agent: &AgentFrame) -> OccupantPhase {
    match agent.role {
        AgentFrameRole::Occupant { phase, .. } => phase,
        AgentFrameRole::Firefighter { .. } => panic!("{} is not an occupant", agent.id),
    }
}

fn squeezing(agent: &AgentFrame) -> bool {
    agent.movement == Movement::Squeezing
}

/// The hallway past the office door, where it is one passage; at the door
/// itself people still merge in from the side.
fn in_hallway(position: Vec3) -> bool {
    position.y < HALL_WIDTH_M && (4.0..9.5).contains(&position.x)
}

/// When each occupant first shows as exited, in order.
fn exit_times(frames: &[RehearsalFrame]) -> Vec<f64> {
    let mut times = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for frame in frames {
        for agent in occupant_frames(frame) {
            if phase(agent) == OccupantPhase::Exited && seen.insert(agent.id.clone()) {
                times.push(frame.fire.time_seconds);
            }
        }
    }
    times
}

fn last_exit_seconds(frames: &[RehearsalFrame]) -> f64 {
    let last = frames.last().expect("frames");
    assert!(occupant_frames(last).all(|agent| phase(agent) == OccupantPhase::Exited));
    exit_times(frames).last().copied().expect("someone exited")
}

/// When the crew leader first stood in the office.
fn office_reached_seconds(frames: &[RehearsalFrame]) -> Option<f64> {
    frames.iter().find_map(|frame| {
        let leader = frame.agents.iter().find(|agent| agent.id == "engine-1/1")?;
        (leader.position_m.y > HALL_WIDTH_M + 0.3).then_some(frame.fire.time_seconds)
    })
}

/// A 6 m square office opening through a 0.9 m door onto a 1.2 m by 10 m
/// hallway, with the only exit at the hallway's far end.
fn scene_fixture() -> Scene {
    let hall = Room {
        id: "hall".to_owned(),
        label: None,
        classifications: Vec::new(),
        dimensions: None,
        boundary: vec![
            edge("hall-south", point(0.0, 0.0), point(10.0, 0.0), vec![]),
            edge(
                "hall-east",
                point(10.0, 0.0),
                point(10.0, HALL_WIDTH_M),
                vec![door("exit-east", 0.5)],
            ),
            edge(
                "hall-north",
                point(10.0, HALL_WIDTH_M),
                point(0.0, HALL_WIDTH_M),
                vec![door("door-office-hall", 0.7)],
            ),
            edge(
                "hall-west",
                point(0.0, HALL_WIDTH_M),
                point(0.0, 0.0),
                vec![],
            ),
        ],
    };
    let top = HALL_WIDTH_M + 6.0;
    let office = Room {
        id: "office".to_owned(),
        label: None,
        classifications: Vec::new(),
        dimensions: None,
        boundary: vec![
            edge(
                "office-south",
                point(0.0, HALL_WIDTH_M),
                point(6.0, HALL_WIDTH_M),
                vec![door("door-office", 0.5)],
            ),
            edge(
                "office-east",
                point(6.0, HALL_WIDTH_M),
                point(6.0, top),
                vec![],
            ),
            edge("office-north", point(6.0, top), point(0.0, top), vec![]),
            edge(
                "office-west",
                point(0.0, top),
                point(0.0, HALL_WIDTH_M),
                vec![],
            ),
        ],
    };
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "crowd-scene".to_owned(),
        name: None,
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "crowd-scene".to_owned(),
        },
        assets: Vec::new(),
        floors: vec![Floor {
            id: "floor-1".to_owned(),
            label: None,
            elevation_m: Some(0.0),
            rooms: vec![hall, office],
            objects: Vec::new(),
        }],
    }
}

fn door(id: &str, relative_center: f64) -> Opening {
    Opening {
        id: id.to_owned(),
        label: None,
        kind: OpeningKind::Door,
        width_m: 0.9,
        height_m: Some(2.1),
        lower_elevation_m: Some(0.0),
        relative_center,
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

fn distance(left: Vec3, right: Vec3) -> f64 {
    (left.x - right.x).hypot(left.y - right.y)
}
