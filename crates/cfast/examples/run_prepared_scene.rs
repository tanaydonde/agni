use std::{
    collections::{HashMap, VecDeque},
    env,
    error::Error,
    fs,
};

use tflop_cfast::{
    CfastFire, CfastFuel, CfastOpeningState, CfastRunner, CfastScenario, HeatReleasePoint,
    prepare_cfast_scene,
};
use tflop_world_core::{Scene, Vec3, resolve_opening_connections};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().collect::<Vec<_>>();
    let [_, scene_path, cfast_path, room_id, fire_x, fire_y] = arguments.as_slice() else {
        return Err(
            "usage: run_prepared_scene <scene.json> <cfast> <room-id> <fire-x> <fire-y>".into(),
        );
    };
    let source_scene: Scene = serde_json::from_str(&fs::read_to_string(scene_path)?)?;
    let prepared = prepare_cfast_scene(&source_scene)?;
    println!(
        "preparation original_rooms={} included_rooms={} warnings={}",
        prepared.report.original_room_count,
        prepared.report.included_room_count,
        prepared.report.warnings.len()
    );
    for warning in &prepared.report.warnings {
        println!("preparation_warning={warning:?}");
    }
    let scene = prepared.scene;
    let connections = resolve_opening_connections(&scene)?;
    let exterior_count = connections
        .iter()
        .filter(|connection| connection.room_b_id.is_none())
        .count();
    println!(
        "scene={} rooms={} openings={} interior={} exterior={}",
        scene.id,
        scene
            .floors
            .iter()
            .map(|floor| floor.rooms.len())
            .sum::<usize>(),
        connections.len(),
        connections.len() - exterior_count,
        exterior_count
    );
    let first_opening = connections
        .first()
        .ok_or("the scene has no resolved openings")?;
    let room_distances = graph_distances(room_id, &connections);
    let scenario = CfastScenario {
        id: "actual-building-trial".to_owned(),
        title: "Actual Matterport building trial".to_owned(),
        room_id: room_id.clone(),
        exterior_opening_id: first_opening.id.clone(),
        duration_seconds: 120.0,
        output_interval_seconds: 5.0,
        interior_temperature_c: 20.0,
        exterior_temperature_c: 20.0,
        relative_humidity_percent: 50.0,
        opening_states: connections
            .iter()
            .map(|connection| CfastOpeningState {
                opening_id: connection.id.clone(),
                open_fraction: 1.0,
            })
            .collect(),
        fire: CfastFire {
            id: "trial-fire".to_owned(),
            position_m: Vec3 {
                x: fire_x.parse()?,
                y: fire_y.parse()?,
                z: 0.0,
            },
            area_m2: 0.3,
            fuel: CfastFuel::default(),
            heat_release_curve: vec![
                HeatReleasePoint {
                    time_seconds: 0.0,
                    hrr_kw: 0.0,
                },
                HeatReleasePoint {
                    time_seconds: 60.0,
                    hrr_kw: 300.0,
                },
                HeatReleasePoint {
                    time_seconds: 120.0,
                    hrr_kw: 300.0,
                },
            ],
        },
    };

    let results = CfastRunner::new(cfast_path)?.run_all_rooms(&scene, &scenario)?;
    let final_frame = results.frames.last().ok_or("CFAST returned no frames")?;
    println!(
        "frames={} final_time={}s rooms={} vents={}",
        results.frames.len(),
        final_frame.time_seconds,
        final_frame.rooms.len(),
        final_frame.openings.len()
    );
    for room in &final_frame.rooms {
        println!(
            "room={} doors_from_fire={} upper_temp={:.1}C lower_temp={:.1}C smoke_layer={:.2}m oxygen={:.2}%",
            room.room_id,
            room_distances
                .get(room.room_id.as_str())
                .map_or_else(|| "unreachable".to_owned(), usize::to_string),
            room.upper_layer.temperature_c,
            room.lower_layer.temperature_c,
            room.smoke_layer_height_m,
            room.upper_layer.oxygen_percent
        );
    }
    Ok(())
}

fn graph_distances<'a>(
    fire_room_id: &'a str,
    connections: &'a [tflop_world_core::OpeningConnection],
) -> HashMap<&'a str, usize> {
    let mut distances = HashMap::from([(fire_room_id, 0)]);
    let mut queue = VecDeque::from([fire_room_id]);
    while let Some(room_id) = queue.pop_front() {
        let next_distance = distances[room_id] + 1;
        for connection in connections {
            let Some(room_b_id) = connection.room_b_id.as_deref() else {
                continue;
            };
            let neighbor = if connection.room_a_id == room_id {
                Some(room_b_id)
            } else if room_b_id == room_id {
                Some(connection.room_a_id.as_str())
            } else {
                None
            };
            if let Some(neighbor) = neighbor
                && !distances.contains_key(neighbor)
            {
                distances.insert(neighbor, next_distance);
                queue.push_back(neighbor);
            }
        }
    }
    distances
}
