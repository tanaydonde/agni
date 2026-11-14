use std::{env, error::Error, fs, path::Path};

use tflop_cfast::{
    CfastOpeningState, CfastRunner, CfastScenario, SurfaceFireAdapterConfig,
    adapt_surface_fire_for_cfast, merge_cfast_with_surface_fire, prepare_cfast_scene,
};
use tflop_simulation_core::{
    CombustionState, FireMaterialCatalog, SurfaceFireConfig, SurfaceFireNodeSource,
    SurfaceFireWorld,
};
use tflop_world_core::{Scene, resolve_opening_connections};

const DURATION_SECONDS: usize = 120;
const SURFACE_STEP_SECONDS: f64 = 1.0;
const CFAST_OUTPUT_INTERVAL_SECONDS: f64 = 5.0;

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().collect::<Vec<_>>();
    let [_, scene_path, cfast_path, output_path] = arguments.as_slice() else {
        return Err("usage: run_hybrid_scene <scene.json> <cfast> <output-results.json>".into());
    };

    let source_scene: Scene = serde_json::from_str(&fs::read_to_string(scene_path)?)?;
    let prepared = prepare_cfast_scene(&source_scene)?;
    let scene = prepared.scene;
    let room_count = scene
        .floors
        .iter()
        .map(|floor| floor.rooms.len())
        .sum::<usize>();
    let object_count = scene
        .floors
        .iter()
        .map(|floor| floor.objects.len())
        .sum::<usize>();
    let openings = resolve_opening_connections(&scene)?;

    println!(
        "prepared scene={} source_rooms={} included_rooms={} objects={} openings={} assumptions={}",
        scene.id,
        prepared.report.original_room_count,
        room_count,
        object_count,
        openings.len(),
        prepared.report.warnings.len()
    );

    let mut surface = SurfaceFireWorld::from_scene(
        &scene,
        &FireMaterialCatalog::default(),
        SurfaceFireConfig::default(),
    )?;
    let ignition = surface
        .nodes()
        .iter()
        .filter(|node| matches!(node.source, SurfaceFireNodeSource::FloorCell { .. }))
        .max_by(|left, right| {
            room_area(&scene, &left.room_id)
                .total_cmp(&room_area(&scene, &right.room_id))
                .then_with(|| left.id.cmp(&right.id))
        })
        .ok_or("the prepared scene has no combustible floor cells")?;
    let ignition_id = ignition.id.clone();
    let ignition_room_id = ignition.room_id.clone();
    let ignition_position = ignition.position_m;

    let mut surface_frames = vec![surface.ignite_node(&ignition_id)?];
    for _ in 0..DURATION_SECONDS {
        surface_frames.push(surface.step(SURFACE_STEP_SECONDS)?);
    }

    let cfast_timeline =
        adapt_surface_fire_for_cfast(&surface_frames, SurfaceFireAdapterConfig::default())?;
    let scenario = CfastScenario {
        id: "matterport-hybrid-trial".to_owned(),
        title: "Matterport hybrid surface fire and CFAST trial".to_owned(),
        room_id: ignition_room_id.clone(),
        // Required only by the legacy single-room exporter. All-room export
        // uses the resolved opening states below.
        exterior_opening_id: "unused-in-all-room-run".to_owned(),
        // CFAST writes spreadsheet rows on [0, duration), so run one output
        // interval past the desired surface timeline to receive its endpoint.
        duration_seconds: cfast_timeline.duration_seconds + CFAST_OUTPUT_INTERVAL_SECONDS,
        output_interval_seconds: CFAST_OUTPUT_INTERVAL_SECONDS,
        interior_temperature_c: 20.0,
        exterior_temperature_c: 20.0,
        relative_humidity_percent: 50.0,
        opening_states: openings
            .iter()
            .map(|opening| CfastOpeningState {
                opening_id: opening.id.clone(),
                open_fraction: 1.0,
            })
            .collect(),
        fires: cfast_timeline.room_fires,
    };

    println!(
        "ignition room={} node={} position=({:.2}, {:.2}, {:.2}) surface_nodes={} heat_connections={} cfast_fires={}",
        ignition_room_id,
        ignition_id,
        ignition_position.x,
        ignition_position.y,
        ignition_position.z,
        surface.nodes().len(),
        surface.connection_count(),
        scenario.fires.len()
    );
    print_surface_summary(&surface_frames, 0.0);
    print_surface_summary(&surface_frames, 30.0);
    print_surface_summary(&surface_frames, 60.0);
    print_surface_summary(&surface_frames, 120.0);

    let cfast_results = CfastRunner::new(cfast_path)?.run_all_rooms(&scene, &scenario)?;
    let results = merge_cfast_with_surface_fire(&scene, &cfast_results, &surface_frames)?;
    if let Some(parent) = Path::new(output_path).parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_string_pretty(&results)?)?;

    println!(
        "merged frames={} rooms_per_frame={} output={}",
        results.frames.len(),
        results.frames.first().map_or(0, |frame| frame.rooms.len()),
        output_path
    );
    print_combined_summary(&results, &ignition_room_id, 30.0);
    print_combined_summary(&results, &ignition_room_id, 60.0);
    print_combined_summary(&results, &ignition_room_id, 120.0);
    Ok(())
}

fn room_area(scene: &Scene, room_id: &str) -> f64 {
    scene
        .floors
        .iter()
        .flat_map(|floor| &floor.rooms)
        .find(|room| room.id == room_id)
        .and_then(|room| room.dimensions.as_ref())
        .and_then(|dimensions| dimensions.area_m2)
        .unwrap_or(0.0)
}

fn print_surface_summary(frames: &[tflop_simulation_core::SurfaceFireFrame], time_seconds: f64) {
    let frame = frames
        .iter()
        .find(|frame| frame.time_seconds == time_seconds)
        .expect("requested summary time is present");
    let burning = frame
        .active_nodes
        .iter()
        .filter(|node| node.state == CombustionState::Burning)
        .count();
    let heating = frame
        .active_nodes
        .iter()
        .filter(|node| node.state == CombustionState::Heating)
        .count();
    let burned_out = frame
        .active_nodes
        .iter()
        .filter(|node| node.state == CombustionState::BurnedOut)
        .count();
    let hrr_kw = frame
        .room_heat_release
        .iter()
        .map(|room| room.heat_release_rate_kw)
        .sum::<f64>();
    println!(
        "surface t={time_seconds:.0}s burning={burning} heating={heating} burned_out={burned_out} total_hrr={hrr_kw:.1}kW"
    );
}

fn print_combined_summary(
    results: &tflop_simulation_core::SimulationResults,
    ignition_room_id: &str,
    time_seconds: f64,
) {
    let frame = results
        .frames
        .iter()
        .find(|frame| frame.time_seconds == time_seconds)
        .expect("requested summary time is present");
    let room = frame
        .rooms
        .iter()
        .find(|room| room.room_id == ignition_room_id)
        .expect("ignition room is present in CFAST output");
    let fire_hrr_kw = frame
        .fires
        .iter()
        .map(|fire| fire.actual_hrr_kw)
        .sum::<f64>();
    println!(
        "combined t={time_seconds:.0}s fire_hrr={fire_hrr_kw:.1}kW upper_temp={:.1}C lower_temp={:.1}C smoke_layer={:.2}m upper_o2={:.2}% visible_cells={}",
        room.upper_layer.temperature_c,
        room.lower_layer.temperature_c,
        room.smoke_layer_height_m,
        room.upper_layer.oxygen_percent,
        frame.surface_fire.len()
    );
}
