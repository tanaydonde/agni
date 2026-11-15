use tflop_simulation_core::{
    CombustionState, FireEnsembleConfig, SIMULATION_RESULTS_SCHEMA_VERSION, SimulationEngine,
    SimulationFrame, SimulationResults, SurfaceFireNodeFrame, SurfaceFireNodeSource,
    analyze_fire_ensemble,
};
use tflop_world_core::{Dimensions3, Vec3};

#[test]
fn computes_empirical_frequency_and_selects_diverse_representatives() {
    let runs = [
        run("a", &[(0.0, 0), (1.0, 0)]),
        run("b", &[(0.0, 0), (1.0, 0), (2.0, 1)]),
        run("c", &[(0.0, 10), (1.0, 10)]),
    ];
    let refs = runs.iter().collect::<Vec<_>>();

    let analysis = analyze_fire_ensemble(
        &refs,
        2,
        FireEnsembleConfig {
            cell_size_m: 0.25,
            time_bin_seconds: 1.0,
            time_tolerance_bins: 0,
        },
    )
    .expect("valid ensemble");

    assert_eq!(
        analysis
            .representatives
            .iter()
            .map(|representative| representative.simulation_index)
            .collect::<Vec<_>>(),
        vec![0, 2],
        "the common run is selected first and the outlier adds coverage"
    );
    assert_eq!(analysis.representatives[0].support_count, 2);
    assert_eq!(analysis.representatives[0].support_fraction, 2.0 / 3.0);
    assert_eq!(analysis.representatives[1].support_count, 1);
    assert_eq!(analysis.assignments[1].representative_index, 0);
    assert_eq!(analysis.assignments[2].representative_index, 2);

    let common = analysis
        .frequencies
        .iter()
        .find(|cell| cell.column == 0 && cell.time_bin == 0)
        .expect("common cell");
    assert_eq!(common.simulation_count, 2);
    assert_eq!(common.frequency, 2.0 / 3.0);
    let uncommon = analysis
        .frequencies
        .iter()
        .find(|cell| cell.column == 1 && cell.time_bin == 2)
        .expect("uncommon cell");
    assert_eq!(uncommon.simulation_count, 1);
    assert_eq!(uncommon.frequency, 1.0 / 3.0);

    // Summed over time: runs a and b burn column 0 for two seconds each.
    let heat = |column: i64| {
        analysis
            .heatmap
            .iter()
            .find(|cell| cell.column == column)
            .expect("heatmap cell")
    };
    assert_eq!(analysis.heatmap.len(), 3);
    assert_eq!(heat(0).expected_burning_seconds, 4.0 / 3.0);
    assert_eq!(heat(0).burned_fraction, 2.0 / 3.0);
    assert_eq!(heat(1).expected_burning_seconds, 1.0 / 3.0);
    assert_eq!(heat(10).expected_burning_seconds, 2.0 / 3.0);
    assert_eq!(heat(10).burned_fraction, 1.0 / 3.0);
}

#[test]
fn timing_tolerance_makes_a_one_bin_shift_similar() {
    let runs = [run("early", &[(1.0, 0)]), run("late", &[(2.0, 0)])];
    let refs = runs.iter().collect::<Vec<_>>();
    let analysis = analyze_fire_ensemble(
        &refs,
        1,
        FireEnsembleConfig {
            time_tolerance_bins: 1,
            ..FireEnsembleConfig::default()
        },
    )
    .expect("valid ensemble");

    assert_eq!(analysis.representatives.len(), 1);
    assert_eq!(analysis.representatives[0].support_count, 2);
    assert_eq!(
        analysis.frequencies.len(),
        2,
        "tolerance affects similarity, not the raw frequency field"
    );
}

#[test]
fn heatmap_covers_every_cell_under_a_wide_node_once_per_time_bin() {
    // A 0.5 m floor cell under a furniture cell: four heatmap cells, and the
    // stacked pair counts once.
    let mut floor = node("floor".to_owned(), 0);
    floor.position_m.x = 0.25;
    floor.position_m.y = 0.25;
    floor.dimensions_m.width_m = 0.5;
    floor.dimensions_m.depth_m = 0.5;
    let mut results = run("wide", &[(0.0, 0)]);
    results.frames[0].surface_fire.push(floor);

    let analysis = analyze_fire_ensemble(&[&results], 1, FireEnsembleConfig::default())
        .expect("valid ensemble");

    let cells = analysis
        .heatmap
        .iter()
        .map(|cell| (cell.column, cell.row, cell.expected_burning_seconds))
        .collect::<Vec<_>>();
    assert_eq!(
        cells,
        vec![(0, 0, 1.0), (0, 1, 1.0), (1, 0, 1.0), (1, 1, 1.0)]
    );
}

fn run(id: &str, observations: &[(f64, i64)]) -> SimulationResults {
    SimulationResults {
        schema_version: SIMULATION_RESULTS_SCHEMA_VERSION,
        scene_id: "scene-1".to_owned(),
        scenario_id: id.to_owned(),
        engine: SimulationEngine {
            name: "fixture".to_owned(),
            version: "1".to_owned(),
        },
        frames: observations
            .iter()
            .enumerate()
            .map(|(index, &(time_seconds, column))| SimulationFrame {
                time_seconds,
                rooms: vec![],
                fires: vec![],
                openings: vec![],
                surface_fire: vec![node(format!("{id}-{index}"), column)],
                agents: vec![],
            })
            .collect(),
    }
}

fn node(id: String, column: i64) -> SurfaceFireNodeFrame {
    SurfaceFireNodeFrame {
        id,
        source: SurfaceFireNodeSource::Furniture {
            object_id: "object-1".to_owned(),
        },
        room_id: "room-1".to_owned(),
        position_m: Vec3 {
            x: column as f64 * 0.25 + 0.125,
            y: 0.125,
            z: 0.125,
        },
        dimensions_m: Dimensions3 {
            width_m: 0.25,
            depth_m: 0.25,
            height_m: 0.25,
        },
        furniture_cell: None,
        material_id: "test".to_owned(),
        state: CombustionState::Burning,
        ignition_time_seconds: Some(0.0),
        surface_temperature_c: 500.0,
        heat_release_rate_kw: 10.0,
        fuel_remaining_fraction: 1.0,
    }
}
