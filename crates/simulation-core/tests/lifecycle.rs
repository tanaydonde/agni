use tflop_simulation_core::{Simulation, SimulationConfig, SimulationError, SimulationStatus};
use tflop_world_core::{SCENE_SCHEMA_VERSION, Scene, SceneSource};

#[test]
fn starts_ready_with_an_empty_snapshot() {
    let simulation = simulation();

    assert_eq!(simulation.config().random_seed, 42);
    assert_eq!(
        simulation.snapshot(),
        tflop_simulation_core::SimulationSnapshot {
            scene_id: "scene-1".to_owned(),
            time_seconds: 0.0,
            status: SimulationStatus::Ready,
            fires: vec![],
            smoke: vec![],
            agents: vec![],
        }
    );
}

#[test]
fn steps_forward_and_returns_the_new_snapshot() {
    let mut simulation = simulation();

    let first = simulation.step(0.1).expect("positive step should work");
    let second = simulation.step(0.25).expect("positive step should work");

    assert_eq!(first.time_seconds, 0.1);
    assert_eq!(first.status, SimulationStatus::Running);
    assert_eq!(second.time_seconds, 0.35);
    assert_eq!(second, simulation.snapshot());
}

#[test]
fn invalid_steps_do_not_change_the_simulation() {
    for delta_seconds in [0.0, -0.1, f64::NAN, f64::INFINITY] {
        let mut simulation = simulation();
        assert!(matches!(
            simulation.step(delta_seconds),
            Err(SimulationError::InvalidDeltaSeconds { .. })
        ));
        assert_eq!(simulation.snapshot().time_seconds, 0.0);
        assert_eq!(simulation.snapshot().status, SimulationStatus::Ready);
    }
}

#[test]
fn reset_restores_the_initial_runtime_state() {
    let mut simulation = simulation();
    simulation.step(3.0).expect("positive step should work");

    let snapshot = simulation.reset();

    assert_eq!(snapshot.time_seconds, 0.0);
    assert_eq!(snapshot.status, SimulationStatus::Ready);
    assert_eq!(snapshot, simulation.snapshot());
}

#[test]
fn snapshot_serializes_for_the_frontend() {
    let snapshot = simulation().snapshot();

    assert_eq!(
        serde_json::to_value(snapshot).expect("snapshot should serialize"),
        serde_json::json!({
            "scene_id": "scene-1",
            "time_seconds": 0.0,
            "status": "ready",
            "fires": [],
            "smoke": [],
            "agents": []
        })
    );
}

#[test]
fn rejects_invalid_grid_config_even_without_floors() {
    let error = Simulation::new(
        scene_fixture(),
        SimulationConfig {
            grid_cell_size_m: 0.0,
            ..SimulationConfig::default()
        },
    )
    .expect_err("zero-sized cells must fail");

    assert!(matches!(error, SimulationError::GridBuild(_)));
}

fn simulation() -> Simulation {
    Simulation::new(
        scene_fixture(),
        SimulationConfig {
            random_seed: 42,
            ..SimulationConfig::default()
        },
    )
    .expect("test scene should be valid")
}

fn scene_fixture() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "scene-1".to_owned(),
        name: Some("Test scene".to_owned()),
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "scene-1".to_owned(),
        },
        assets: vec![],
        floors: vec![],
    }
}
