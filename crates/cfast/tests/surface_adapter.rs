use tflop_cfast::{
    CFAST_MAX_CURVE_POINTS, SurfaceFireAdapterConfig, SurfaceFireAdapterError,
    adapt_surface_fire_for_cfast,
};
use tflop_simulation_core::{
    CombustionState, GridCoordinate, RoomHeatRelease, SURFACE_FIRE_SCHEMA_VERSION,
    SurfaceFireFrame, SurfaceFireNodeFrame, SurfaceFireNodeSource,
};
use tflop_world_core::{Dimensions3, Vec3};

#[test]
fn aggregates_spatial_cells_into_one_curve_per_room() {
    let frames = vec![
        frame(0.0, vec![], &[("room-a", 0.0), ("room-b", 0.0)]),
        frame(
            5.0,
            vec![
                node("a-left", "room-a", 1.0, 10.0),
                node("a-right", "room-a", 3.0, 30.0),
                node("b", "room-b", 10.0, 20.0),
            ],
            &[("room-a", 40.0), ("room-b", 20.0)],
        ),
        frame(
            10.0,
            vec![node("a-spread", "room-a", 5.0, 60.0)],
            &[("room-a", 60.0), ("room-b", 0.0)],
        ),
    ];

    let timeline = adapt_surface_fire_for_cfast(&frames, SurfaceFireAdapterConfig::default())
        .expect("valid timeline should adapt");

    assert_eq!(timeline.scene_id, "scene-1");
    assert_eq!(timeline.duration_seconds, 10.0);
    assert_eq!(timeline.room_fires.len(), 2);
    let room_a = &timeline.room_fires[0];
    assert_eq!(room_a.room_id, "room-a");
    assert_eq!(room_a.fire.id, "surface-fire:room-a");
    assert_eq!(
        room_a
            .fire
            .heat_release_curve
            .iter()
            .map(|point| point.hrr_kw)
            .collect::<Vec<_>>(),
        vec![0.0, 40.0, 60.0]
    );
    assert!((room_a.fire.position_m.x - 4.0).abs() < 1.0e-9);
    assert!((room_a.fire.area_m2 - 0.125).abs() < 1.0e-9);

    let room_b = &timeline.room_fires[1];
    assert_eq!(room_b.room_id, "room-b");
    assert_eq!(
        room_b
            .fire
            .heat_release_curve
            .iter()
            .map(|point| point.hrr_kw)
            .collect::<Vec<_>>(),
        vec![0.0, 20.0, 0.0]
    );
    assert!((room_b.fire.position_m.x - 10.0).abs() < 1.0e-9);
}

#[test]
fn long_timelines_are_thinned_to_what_cfast_can_read_keeping_the_peak() {
    // Five minutes of half-second frames: 601 points, three times CFAST's limit.
    let peak_frame = 377;
    let frames = (0..=600)
        .map(|index| {
            let time = f64::from(index) * 0.5;
            let hrr = if index == peak_frame {
                900.0
            } else {
                f64::from(index % 7) * 10.0
            };
            let nodes = if index == 0 {
                vec![]
            } else {
                vec![node("a", "room-a", 1.0, hrr)]
            };
            let total = if index == 0 { 0.0 } else { hrr };
            frame(time, nodes, &[("room-a", total)])
        })
        .collect::<Vec<_>>();

    let timeline = adapt_surface_fire_for_cfast(&frames, SurfaceFireAdapterConfig::default())
        .expect("valid timeline should adapt");

    let curve = &timeline.room_fires[0].fire.heat_release_curve;
    assert_eq!(curve.len(), CFAST_MAX_CURVE_POINTS);
    assert_eq!(curve[0].time_seconds, 0.0);
    assert_eq!(curve.last().map(|point| point.time_seconds), Some(300.0));
    assert!(
        curve
            .windows(2)
            .all(|pair| pair[0].time_seconds < pair[1].time_seconds)
    );
    assert!(
        curve.iter().any(|point| point.hrr_kw == 900.0),
        "the peak survives"
    );
}

#[test]
fn applies_the_configured_minimum_fire_area() {
    let frames = vec![
        frame(0.0, vec![], &[("room-a", 0.0)]),
        frame(
            1.0,
            vec![node("tiny", "room-a", 1.0, 1.0)],
            &[("room-a", 1.0)],
        ),
    ];
    let config = SurfaceFireAdapterConfig {
        minimum_fire_area_m2: 0.5,
        ..SurfaceFireAdapterConfig::default()
    };

    let timeline = adapt_surface_fire_for_cfast(&frames, config).expect("timeline should adapt");

    assert_eq!(timeline.room_fires[0].fire.area_m2, 0.5);
}

#[test]
fn rejects_a_room_total_that_disagrees_with_its_cells() {
    let frames = vec![
        frame(0.0, vec![], &[("room-a", 0.0)]),
        frame(
            1.0,
            vec![node("fire", "room-a", 1.0, 20.0)],
            &[("room-a", 10.0)],
        ),
    ];

    assert!(matches!(
        adapt_surface_fire_for_cfast(&frames, SurfaceFireAdapterConfig::default()),
        Err(SurfaceFireAdapterError::RoomHeatReleaseMismatch {
            frame_index: 1,
            room_id,
            calculated_hrr_kw: 20.0,
            reported_hrr_kw: 10.0,
        }) if room_id == "room-a"
    ));
}

#[test]
fn rejects_timelines_without_a_burning_cell() {
    let frames = vec![
        frame(0.0, vec![], &[("room-a", 0.0)]),
        frame(1.0, vec![], &[("room-a", 0.0)]),
    ];

    assert_eq!(
        adapt_surface_fire_for_cfast(&frames, SurfaceFireAdapterConfig::default()),
        Err(SurfaceFireAdapterError::NoBurningCells)
    );
}

#[test]
fn rejects_unordered_or_nonzero_start_times() {
    let nonzero_start = vec![
        frame(1.0, vec![], &[("room-a", 0.0)]),
        frame(2.0, vec![], &[("room-a", 0.0)]),
    ];
    assert!(matches!(
        adapt_surface_fire_for_cfast(&nonzero_start, SurfaceFireAdapterConfig::default()),
        Err(SurfaceFireAdapterError::TimelineMustStartAtZero { .. })
    ));

    let unordered = vec![
        frame(0.0, vec![], &[("room-a", 0.0)]),
        frame(0.0, vec![], &[("room-a", 0.0)]),
    ];
    assert!(matches!(
        adapt_surface_fire_for_cfast(&unordered, SurfaceFireAdapterConfig::default()),
        Err(SurfaceFireAdapterError::InvalidFrameTime { frame_index: 1, .. })
    ));
}

fn frame(
    time_seconds: f64,
    active_nodes: Vec<SurfaceFireNodeFrame>,
    room_hrr: &[(&str, f64)],
) -> SurfaceFireFrame {
    SurfaceFireFrame {
        schema_version: SURFACE_FIRE_SCHEMA_VERSION,
        scene_id: "scene-1".to_owned(),
        time_seconds,
        active_nodes,
        room_heat_release: room_hrr
            .iter()
            .map(|(room_id, heat_release_rate_kw)| RoomHeatRelease {
                room_id: (*room_id).to_owned(),
                heat_release_rate_kw: *heat_release_rate_kw,
            })
            .collect(),
    }
}

fn node(id: &str, room_id: &str, x: f64, hrr_kw: f64) -> SurfaceFireNodeFrame {
    SurfaceFireNodeFrame {
        id: id.to_owned(),
        source: SurfaceFireNodeSource::FloorCell {
            floor_id: "floor-1".to_owned(),
            coordinate: GridCoordinate { column: 0, row: 0 },
        },
        room_id: room_id.to_owned(),
        position_m: Vec3 { x, y: 2.0, z: 0.0 },
        dimensions_m: Dimensions3 {
            width_m: 0.25,
            depth_m: 0.25,
            height_m: 0.02,
        },
        furniture_cell: None,
        material_id: "test".to_owned(),
        state: CombustionState::Burning,
        ignition_time_seconds: Some(0.0),
        surface_temperature_c: 500.0,
        heat_release_rate_kw: hrr_kw,
        fuel_remaining_fraction: 0.5,
    }
}
