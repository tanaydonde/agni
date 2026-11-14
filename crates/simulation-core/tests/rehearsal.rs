use tflop_simulation_core::{
    AgentFrameRole, AlarmConfig, CombustionState, DEFAULT_NOZZLE_COOLING_KW,
    DEFAULT_NOZZLE_REACH_M, Evacuation, FireMaterialCatalog, FireMaterialProfile,
    FurnitureFireMaterial, OccupantPhase, OccupantSpawn, Rehearsal, RehearsalError, RehearsalFrame,
    SimulationConfig, Suppression, SurfaceFireConfig, SurfaceFireError, SurfaceFireWorld,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, FurnitureAsset, FurnitureObject, Mobility,
    Opening, OpeningKind, Quaternion, Room, SCENE_SCHEMA_VERSION, Scene, SceneSource, Transform3,
    Vec3,
};

/// While agents can't affect the fire, stepping everything together must give
/// exactly what replaying occupants over a finished fire timeline gave.
#[test]
fn matches_replaying_occupants_over_a_finished_fire() {
    let scene = scene_fixture();
    let catalog = FireMaterialCatalog::default();
    let occupants = [occupant("occupant-1", 1.0, 3.0, 2.0)];

    let mut rehearsal = rehearsal(&scene, &catalog, &occupants);
    let mut frames = vec![rehearsal.ignite_object("chair-1").expect("chair ignites")];
    frames.extend(rehearsal.run_until(8.0, 1.0).expect("run succeeds"));

    let mut fire = fire_world(&scene, &catalog);
    let mut evacuation = Evacuation::new(&scene, &occupants, SimulationConfig::default())
        .expect("evacuation builds")
        .with_alarm(AlarmConfig::Immediate);
    let mut fire_frames = vec![fire.ignite_object("chair-1").expect("chair ignites")];
    for _ in 0..8 {
        fire_frames.push(fire.step(1.0).expect("fire steps"));
    }
    let replayed = fire_frames
        .into_iter()
        .map(|fire| RehearsalFrame {
            agents: evacuation
                .advance_to(fire.time_seconds, &fire.active_nodes)
                .expect("occupants advance"),
            fire,
        })
        .collect::<Vec<_>>();

    assert_eq!(frames.len(), 9);
    assert_eq!(frames, replayed);
}

#[test]
fn keeps_running_after_the_fire_goes_out_until_occupants_leave() {
    let scene = scene_fixture();
    let catalog = burns_out_quickly();
    let mut rehearsal = rehearsal(&scene, &catalog, &[occupant("occupant-1", 1.0, 3.0, 10.0)]);
    let mut frames = vec![rehearsal.ignite_object("chair-1").expect("chair ignites")];
    frames.extend(rehearsal.run_until(60.0, 1.0).expect("run succeeds"));

    let fire_out_at = frames
        .iter()
        .position(|frame| !fire_active(frame))
        .expect("the fire goes out");
    let last = frames.last().expect("frames");
    assert_eq!(phase(&frames[fire_out_at]), OccupantPhase::Preparing);
    assert_eq!(phase(last), OccupantPhase::Exited);
    assert!(
        last.fire.time_seconds < 60.0,
        "should stop once everyone is out"
    );
    assert!(rehearsal.is_settled());
}

#[test]
fn fire_only_run_stops_when_the_fire_is_out() {
    let scene = scene_fixture();
    let catalog = burns_out_quickly();
    let mut rehearsal = Rehearsal::new(fire_world(&scene, &catalog), None);
    rehearsal.ignite_object("chair-1").expect("chair ignites");

    let frames = rehearsal.run_until(60.0, 1.0).expect("run succeeds");

    let last = frames.last().expect("frames");
    assert!(!fire_active(last));
    assert!(last.fire.time_seconds < 60.0);
    assert!(frames.iter().all(|frame| frame.agents.is_empty()));
}

#[test]
fn identical_inputs_produce_identical_frames() {
    let scene = scene_fixture();
    let catalog = FireMaterialCatalog::default();
    let occupants = [
        occupant("occupant-1", 1.0, 3.0, 0.0),
        occupant("occupant-2", 3.0, 3.0, 1.0),
    ];
    let run = || {
        let mut rehearsal = rehearsal(&scene, &catalog, &occupants);
        let mut frames = vec![rehearsal.ignite_object("chair-1").expect("chair ignites")];
        frames.extend(rehearsal.run_until(20.0, 1.0).expect("run succeeds"));
        frames
    };

    assert_eq!(run(), run());
}

#[test]
fn rejects_invalid_steps() {
    let scene = scene_fixture();
    let mut rehearsal = Rehearsal::new(fire_world(&scene, &FireMaterialCatalog::default()), None);
    rehearsal.ignite_object("chair-1").expect("chair ignites");

    for step in [0.0, -1.0, f64::NAN] {
        assert!(matches!(
            rehearsal.run_until(10.0, step),
            Err(RehearsalError::InvalidStep { .. })
        ));
        assert!(matches!(
            rehearsal.step(step),
            Err(RehearsalError::InvalidStep { .. })
        ));
    }
}

#[test]
fn suppression_during_the_run_puts_the_fire_out() {
    let scene = scene_fixture();
    let catalog = FireMaterialCatalog::default();
    let mut unsuppressed = Rehearsal::new(fire_world(&scene, &catalog), None);
    let mut suppressed = Rehearsal::new(fire_world(&scene, &catalog), None);
    unsuppressed
        .ignite_object("chair-1")
        .expect("chair ignites");
    suppressed.ignite_object("chair-1").expect("chair ignites");
    // Let the fire grow before the crew arrives.
    for _ in 0..5 {
        assert_eq!(
            unsuppressed.step(1.0).expect("steps"),
            suppressed.step(1.0).expect("steps")
        );
    }

    let mut last = None;
    for _ in 0..10 {
        unsuppressed.step(1.0).expect("steps");
        last = Some(
            suppressed
                .step_with_suppression(1.0, &[room_nozzle()])
                .expect("steps"),
        );
    }

    let last = last.expect("suppressed frames");
    assert!(!fire_active(&last));
    assert!(
        last.fire
            .active_nodes
            .iter()
            .any(|node| node.state == CombustionState::Extinguished)
    );
    assert!(
        last.fire
            .room_heat_release
            .iter()
            .all(|room| room.heat_release_rate_kw == 0.0)
    );
    assert!(suppressed.is_settled());
    assert!(
        !unsuppressed.is_settled(),
        "without water the chair is still burning"
    );
}

#[test]
fn an_invalid_nozzle_applies_no_water_at_all() {
    let scene = scene_fixture();
    let mut rehearsal = Rehearsal::new(fire_world(&scene, &FireMaterialCatalog::default()), None);
    rehearsal.ignite_object("chair-1").expect("chair ignites");
    let broken = Suppression {
        reach_m: 0.0,
        ..room_nozzle()
    };

    assert!(matches!(
        rehearsal.step_with_suppression(1.0, &[room_nozzle(), broken]),
        Err(RehearsalError::Fire(SurfaceFireError::InvalidSuppression {
            field: "reach_m"
        }))
    ));
    let frame = rehearsal.step(1.0).expect("steps");
    assert!(
        frame
            .fire
            .active_nodes
            .iter()
            .all(|node| node.state != CombustionState::Extinguished)
    );
}

/// Covers the whole 6 x 4 m fixture room.
fn room_nozzle() -> Suppression {
    Suppression {
        room_id: "room-1".to_owned(),
        position_m: point(3.0, 2.0),
        reach_m: DEFAULT_NOZZLE_REACH_M,
        cooling_kw: DEFAULT_NOZZLE_COOLING_KW,
    }
}

fn rehearsal(
    scene: &Scene,
    catalog: &FireMaterialCatalog,
    occupants: &[OccupantSpawn],
) -> Rehearsal {
    let evacuation = Evacuation::new(scene, occupants, SimulationConfig::default())
        .expect("evacuation builds")
        .with_alarm(AlarmConfig::Immediate);
    Rehearsal::new(fire_world(scene, catalog), Some(evacuation))
}

fn fire_world(scene: &Scene, catalog: &FireMaterialCatalog) -> SurfaceFireWorld {
    SurfaceFireWorld::from_scene(scene, catalog, SurfaceFireConfig::default())
        .expect("fire world builds")
}

/// Matches `Rehearsal`: only burning nodes keep a fire going.
fn fire_active(frame: &RehearsalFrame) -> bool {
    frame
        .fire
        .active_nodes
        .iter()
        .any(|node| node.state == CombustionState::Burning)
}

fn phase(frame: &RehearsalFrame) -> OccupantPhase {
    let AgentFrameRole::Occupant { phase } = frame.agents[0].role else {
        panic!("expected occupant")
    };
    phase
}

/// A chair with almost no fuel on a floor that can't burn.
fn burns_out_quickly() -> FireMaterialCatalog {
    let chair = FireMaterialProfile {
        id: "test_chair".to_owned(),
        combustible: true,
        ignition_temperature_c: 200.0,
        ignition_energy_kj_per_m2: 1.0,
        peak_hrr_kw_per_m2: 200.0,
        fuel_energy_kj_per_m2: 1.0,
        heat_transfer_fraction: 0.8,
        cooling_kw_per_m2: 1.0,
    };
    FireMaterialCatalog {
        floor: FireMaterialProfile::noncombustible("concrete"),
        default_furniture: chair.clone(),
        furniture: vec![FurnitureFireMaterial {
            category: "chair".to_owned(),
            material: chair,
        }],
    }
}

fn occupant(id: &str, x: f64, y: f64, delay: f64) -> OccupantSpawn {
    OccupantSpawn {
        id: id.to_owned(),
        floor_id: "floor-1".to_owned(),
        position_m: point(x, y),
        walking_speed_m_per_s: 1.2,
        pre_movement_delay_seconds: delay,
    }
}

/// A 6 x 4 m room with an exterior door centered on the south wall and a
/// chair in the far corner.
fn scene_fixture() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "rehearsal-scene".to_owned(),
        name: None,
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "rehearsal-scene".to_owned(),
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
            objects: vec![FurnitureObject {
                id: "chair-1".to_owned(),
                asset_id: "chair-basic".to_owned(),
                transform: Transform3 {
                    position_m: point(5.0, 3.0),
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

fn point(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
