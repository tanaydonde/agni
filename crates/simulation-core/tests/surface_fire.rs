use tflop_simulation_core::{
    CombustionState, DEFAULT_NOZZLE_COOLING_KW, DEFAULT_NOZZLE_HALF_ANGLE_DEGREES,
    DEFAULT_NOZZLE_REACH_M, FireMaterialCatalog, FireMaterialProfile, FurnitureFireMaterial,
    Suppression, SurfaceFireConfig, SurfaceFireError, SurfaceFireNodeSource, SurfaceFireWorld,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, FurnitureAsset, FurnitureObject, Mobility,
    Quaternion, Room, SCENE_SCHEMA_VERSION, Scene, SceneSource, Transform3, Vec3,
};

#[test]
fn builds_floor_cells_and_three_dimensional_cells_for_furniture() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(true), SurfaceFireConfig::default())
            .expect("surface world should build");

    assert!(
        world
            .nodes()
            .iter()
            .any(|node| matches!(node.source, SurfaceFireNodeSource::FloorCell { .. }))
    );
    let chair = world.node("object:chair-1").expect("chair node exists");
    assert!(matches!(
        chair.source,
        SurfaceFireNodeSource::Furniture { ref object_id } if object_id == "chair-1"
    ));
    assert_eq!(chair.room_id, "room-1");
    assert_eq!(chair.dimensions_m.width_m, 0.2);
    let cell = chair.furniture_cell.expect("furniture cell");
    assert_eq!((cell.column, cell.row, cell.layer), (1, 1, 0));
    assert_eq!((cell.columns, cell.rows, cell.layers), (2, 2, 4));
    assert_eq!(
        world
            .nodes()
            .iter()
            .filter(|node| matches!(
                node.source,
                SurfaceFireNodeSource::Furniture { ref object_id } if object_id == "chair-1"
            ))
            .count(),
        16
    );
}

#[test]
fn fire_climbs_furniture_one_section_at_a_time() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world = SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), neighbours_only())
        .expect("surface world should build");

    let initial = world
        .ignite_object("chair-1")
        .expect("bottom section should ignite");
    assert_eq!(initial.active_nodes.len(), 1);
    assert_eq!(initial.active_nodes[0].id, "object:chair-1");
    assert_eq!(
        initial.active_nodes[0]
            .furniture_cell
            .expect("cell metadata")
            .layer,
        0
    );

    world.step(1.0).expect("first upward step");
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:1")
            .expect("cell above ignition")
            .state,
        CombustionState::Burning
    );
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:2")
            .expect("cell two layers above")
            .state,
        CombustionState::Unignited
    );

    world.step(1.0).expect("second upward step");
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:2")
            .expect("cell two layers above")
            .state,
        CombustionState::Burning
    );
}

#[test]
fn fire_spreads_from_floor_into_bottom_of_furniture_then_upward() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world = SurfaceFireWorld::from_scene(&scene, &fast_catalog(true), neighbours_only())
        .expect("surface world should build");
    let ignition_cell_position = world
        .node("object:chair-1")
        .expect("default chair ignition cell")
        .position_m;
    let floor_beneath_chair = world
        .nodes()
        .iter()
        .filter(|node| matches!(node.source, SurfaceFireNodeSource::FloorCell { .. }))
        .min_by(|left, right| {
            let left_distance = (left.position_m.x - ignition_cell_position.x)
                .hypot(left.position_m.y - ignition_cell_position.y);
            let right_distance = (right.position_m.x - ignition_cell_position.x)
                .hypot(right.position_m.y - ignition_cell_position.y);
            left_distance.total_cmp(&right_distance)
        })
        .expect("floor beneath chair")
        .id
        .clone();

    world
        .ignite_node(&floor_beneath_chair)
        .expect("floor should ignite");
    world.step(1.0).expect("floor should heat chair base");
    assert_eq!(
        world.node("object:chair-1").expect("chair bottom").state,
        CombustionState::Burning
    );
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:1")
            .expect("chair cell above base")
            .state,
        CombustionState::Unignited
    );

    world.step(1.0).expect("chair fire should climb");
    assert_eq!(
        world
            .node("object:chair-1:cell:1:1:1")
            .expect("chair cell above base")
            .state,
        CombustionState::Burning
    );
}

#[test]
fn wide_furniture_burns_across_its_base_cell_by_cell() {
    let mut scene = scene_with_chairs(&[("bench-1", 1.5, 2.0)]);
    scene.assets[0].dimensions_m = Dimensions3 {
        width_m: 1.0,
        depth_m: 0.25,
        height_m: 0.25,
    };
    let mut world = SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), neighbours_only())
        .expect("surface world should build");

    world
        .ignite_object_cell("bench-1", 0, 0, 0)
        .expect("leftmost cell should ignite");
    assert_eq!(
        world
            .node("object:bench-1:cell:3:0:0")
            .expect("rightmost cell")
            .state,
        CombustionState::Unignited
    );

    world.step(1.0).expect("fire should move one cell");
    assert_eq!(
        world
            .node("object:bench-1:cell:1:0:0")
            .expect("neighbor cell")
            .state,
        CombustionState::Burning
    );
    assert_eq!(
        world
            .node("object:bench-1:cell:3:0:0")
            .expect("rightmost cell")
            .state,
        CombustionState::Unignited
    );
}

#[test]
fn furniture_cell_positions_follow_the_object_rotation() {
    let mut scene = scene_with_chairs(&[("bench-1", 1.5, 2.0)]);
    scene.assets[0].dimensions_m = Dimensions3 {
        width_m: 1.0,
        depth_m: 0.25,
        height_m: 0.25,
    };
    let half_sqrt_two = std::f64::consts::FRAC_1_SQRT_2;
    scene.floors[0].objects[0].transform.rotation = Quaternion {
        x: 0.0,
        y: 0.0,
        z: half_sqrt_two,
        w: half_sqrt_two,
    };
    let world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("rotated surface world should build");
    let left = world
        .node("object:bench-1:cell:0:0:0")
        .expect("first width cell");
    let right = world
        .node("object:bench-1:cell:3:0:0")
        .expect("last width cell");

    assert!((left.position_m.x - right.position_m.x).abs() < 1.0e-9);
    assert!((left.position_m.y - right.position_m.y).abs() > 0.7);
}

#[test]
fn spreads_between_nearby_furniture_hitboxes() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0), ("chair-2", 2.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");

    world
        .ignite_object("chair-1")
        .expect("first chair should ignite");
    let frame = world.step(1.0).expect("step should succeed");

    assert_eq!(
        world.node("object:chair-2").expect("second chair").state,
        CombustionState::Burning
    );
    assert!(frame.active_nodes.iter().any(|node| {
        matches!(
            node.source,
            SurfaceFireNodeSource::Furniture { ref object_id } if object_id == "chair-2"
        ) && node.ignition_time_seconds == Some(1.0)
    }));
}

#[test]
fn spreads_across_neighboring_floor_cells() {
    let scene = scene_with_chairs(&[]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(true), SurfaceFireConfig::default())
            .expect("surface world should build");
    let initial_floor_id = world
        .nodes()
        .iter()
        .find(|node| matches!(node.source, SurfaceFireNodeSource::FloorCell { .. }))
        .expect("floor node exists")
        .id
        .clone();

    world
        .ignite_node(&initial_floor_id)
        .expect("floor should ignite");
    world.step(1.0).expect("step should succeed");

    assert!(
        world
            .nodes()
            .iter()
            .filter(|node| node.state == CombustionState::Burning)
            .count()
            > 1
    );
}

#[test]
fn burning_nodes_consume_fuel_and_burn_out() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut catalog = fast_catalog(false);
    catalog.default_furniture.fuel_energy_kj_per_m2 = 1.0;
    catalog.furniture[0].material.fuel_energy_kj_per_m2 = 1.0;
    let mut world = SurfaceFireWorld::from_scene(&scene, &catalog, SurfaceFireConfig::default())
        .expect("surface world should build");

    world.ignite_object("chair-1").expect("chair ignites");
    let frame = world.step(1.0).expect("step should succeed");

    let chair = frame
        .active_nodes
        .iter()
        .find(|node| node.id == "object:chair-1")
        .expect("chair remains in active timeline");
    assert_eq!(chair.state, CombustionState::BurnedOut);
    assert_eq!(chair.heat_release_rate_kw, 0.0);
    assert_eq!(chair.fuel_remaining_fraction, 0.0);
}

#[test]
fn rejects_invalid_steps_without_advancing_time() {
    let scene = scene_with_chairs(&[]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(true), SurfaceFireConfig::default())
            .expect("surface world should build");

    assert!(matches!(
        world.step(0.0),
        Err(SurfaceFireError::InvalidDeltaSeconds { .. })
    ));
    assert_eq!(world.snapshot().time_seconds, 0.0);
}

#[test]
fn frame_json_preserves_the_furniture_object_id() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    let frame = world.ignite_object("chair-1").expect("chair ignites");

    let json = serde_json::to_value(frame).expect("frame should serialize");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["active_nodes"][0]["source"]["type"], "furniture");
    assert_eq!(json["active_nodes"][0]["source"]["object_id"], "chair-1");
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["column"], 1);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["row"], 1);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["layer"], 0);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["columns"], 2);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["rows"], 2);
    assert_eq!(json["active_nodes"][0]["furniture_cell"]["layers"], 4);
    assert_eq!(json["active_nodes"][0]["state"], "burning");
    assert!(json["active_nodes"][0]["heat_release_rate_kw"].is_number());
}

#[test]
fn furniture_heat_does_not_jump_through_a_room_boundary() {
    let mut scene = scene_with_chairs(&[("chair-1", 3.5, 2.0), ("chair-2", 4.5, 2.0)]);
    scene.floors[0].rooms.push(Room {
        id: "room-2".to_owned(),
        label: None,
        classifications: vec![],
        dimensions: None,
        boundary: vec![
            edge("south-2", point(4.0, 0.0), point(8.0, 0.0)),
            edge("east-2", point(8.0, 0.0), point(8.0, 4.0)),
            edge("north-2", point(8.0, 4.0), point(4.0, 4.0)),
            edge("west-2", point(4.0, 4.0), point(4.0, 0.0)),
        ],
    });

    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair ignites");
    world.step(1.0).expect("step should succeed");

    assert_eq!(
        world.node("object:chair-2").expect("second chair").state,
        CombustionState::Unignited
    );
}

#[test]
fn suppression_puts_out_burning_nodes_in_reach() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair ignites");

    let outcome = world
        .suppress(&nozzle("room-1", 1.0, 2.0, 2.0, 2_000.0), 1.0)
        .expect("suppression applies");

    assert_eq!(outcome.burning_reached, 1);
    assert_eq!(outcome.extinguished_node_ids, ["object:chair-1"]);
    let frame = world.snapshot();
    let chair = frame
        .active_nodes
        .iter()
        .find(|node| node.id == "object:chair-1")
        .expect("extinguished chair stays visible");
    assert_eq!(chair.state, CombustionState::Extinguished);
    assert_eq!(chair.heat_release_rate_kw, 0.0);
    assert!(chair.fuel_remaining_fraction > 0.0);
    assert!(
        frame
            .room_heat_release
            .iter()
            .all(|room| room.heat_release_rate_kw == 0.0)
    );
}

#[test]
fn knockdown_takes_longer_than_one_step_when_cooling_barely_beats_the_fire() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    let hrr_kw = world
        .ignite_object("chair-1")
        .expect("chair ignites")
        .active_nodes[0]
        .heat_release_rate_kw;
    // Knockdown needs the configured seconds' worth of the fire's output.
    let seconds = SurfaceFireConfig::default().suppression_knockdown_seconds;
    let barely_enough = nozzle("room-1", 1.5, 1.5, 1.0, hrr_kw * 1.01);

    for _ in 0..(seconds as usize - 1) {
        let outcome = world.suppress(&barely_enough, 1.0).expect("applies");
        assert!(outcome.extinguished_node_ids.is_empty());
    }
    let outcome = world.suppress(&barely_enough, 1.0).expect("applies");
    assert_eq!(outcome.extinguished_node_ids, ["object:chair-1"]);
}

#[test]
fn stream_stops_at_walls_and_at_its_reach() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair ignites");

    for suppression in [
        nozzle("room-2", 1.5, 2.0, 5.0, 2_000.0),
        nozzle("room-1", 3.5, 3.5, 1.0, 2_000.0),
    ] {
        let outcome = world.suppress(&suppression, 1.0).expect("applies");
        assert_eq!(outcome.burning_reached, 0);
    }
    assert_eq!(
        world.node("object:chair-1").expect("chair").state,
        CombustionState::Burning
    );
}

#[test]
fn extinguished_furniture_reignites_from_a_neighboring_fire() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0), ("chair-2", 2.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair-1 ignites");
    world.ignite_object("chair-2").expect("chair-2 ignites");

    world
        .suppress(&nozzle("room-1", 1.5, 2.0, 0.3, 2_000.0), 1.0)
        .expect("only chair-1 is in reach");
    assert_eq!(
        world.node("object:chair-2").expect("chair-2").state,
        CombustionState::Burning
    );
    world.step(1.0).expect("step succeeds");

    let chair = world.node("object:chair-1").expect("chair-1");
    assert_eq!(chair.state, CombustionState::Burning);
    assert_eq!(chair.ignition_time_seconds, Some(1.0));
}

#[test]
fn extinguished_furniture_stays_out_without_heat() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair ignites");
    world
        .suppress(&nozzle("room-1", 1.5, 2.0, 1.0, 2_000.0), 1.0)
        .expect("applies");

    for _ in 0..5 {
        world.step(1.0).expect("step succeeds");
    }

    assert_eq!(
        world.node("object:chair-1").expect("chair").state,
        CombustionState::Extinguished
    );
}

#[test]
fn stream_cools_heating_furniture_when_no_flames_are_in_reach() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0), ("chair-2", 2.5, 2.0)]);
    let mut catalog = fast_catalog(false);
    catalog.furniture[0].material.ignition_energy_kj_per_m2 = 1.0e6;
    let mut world = SurfaceFireWorld::from_scene(&scene, &catalog, SurfaceFireConfig::default())
        .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair ignites");
    world.step(1.0).expect("step succeeds");
    assert_eq!(
        world.node("object:chair-2").expect("chair-2").state,
        CombustionState::Heating
    );

    let outcome = world
        .suppress(&nozzle("room-1", 2.5, 2.0, 0.3, 2_000.0), 1.0)
        .expect("applies");

    assert_eq!(outcome.burning_reached, 0);
    let chair = world.node("object:chair-2").expect("chair-2");
    assert_eq!(chair.state, CombustionState::Unignited);
    assert_eq!(chair.absorbed_heat_kj, 0.0);
}

#[test]
fn rejects_invalid_suppression_without_changing_the_fire() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair ignites");

    for (suppression, field) in [
        (nozzle("", 1.5, 2.0, 1.0, 2_000.0), "room_id"),
        (nozzle("room-1", f64::NAN, 2.0, 1.0, 2_000.0), "position_m"),
        (nozzle("room-1", 1.5, 2.0, 0.0, 2_000.0), "reach_m"),
        (nozzle("room-1", 1.5, 2.0, 1.0, -1.0), "cooling_kw"),
    ] {
        assert!(matches!(
            world.suppress(&suppression, 1.0),
            Err(SurfaceFireError::InvalidSuppression { field: actual }) if actual == field
        ));
    }
    assert!(matches!(
        world.suppress(&nozzle("room-1", 1.5, 2.0, 1.0, 2_000.0), 0.0),
        Err(SurfaceFireError::InvalidDeltaSeconds { .. })
    ));
    assert_eq!(
        world.node("object:chair-1").expect("chair").state,
        CombustionState::Burning
    );
}

#[test]
fn an_aimed_stream_wets_only_what_it_points_at() {
    let scene = scene_with_chairs(&[("left", 1.0, 2.0), ("right", 3.0, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    world.ignite_object("left").expect("left ignites");
    world.ignite_object("right").expect("right ignites");

    // From the middle, pointing right.
    let aimed = Suppression {
        direction: Some((1.0, 0.0)),
        ..nozzle("room-1", 2.0, 2.0, 3.0, 1.0e9)
    };
    let outcome = world.suppress(&aimed, 1.0).expect("applies");

    assert_eq!(outcome.burning_reached, 1);
    assert_eq!(outcome.extinguished_node_ids, ["object:right"]);
    assert_eq!(
        world.node("object:left").expect("left").state,
        CombustionState::Burning
    );
}

#[test]
fn an_aimed_stream_wets_whatever_is_right_at_the_nozzle() {
    let scene = scene_with_chairs(&[("chair-1", 1.5, 2.0)]);
    let mut world =
        SurfaceFireWorld::from_scene(&scene, &fast_catalog(false), SurfaceFireConfig::default())
            .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair ignites");
    let behind = Suppression {
        direction: Some((1.0, 0.0)),
        ..nozzle("room-1", 1.8, 2.0, 3.0, 1.0e9)
    };
    let outcome = world.suppress(&behind, 1.0).expect("applies");
    assert_eq!(
        outcome.burning_reached, 1,
        "0.3 m behind the nozzle still gets wet"
    );
}

#[test]
fn a_burning_sofa_takes_tens_of_seconds_to_knock_down() {
    let mut scene = scene_with_chairs(&[("sofa-1", 2.0, 2.0)]);
    scene.assets[0].category = "sofa".to_owned();
    scene.assets[0].dimensions_m = Dimensions3 {
        width_m: 2.0,
        depth_m: 0.9,
        height_m: 0.8,
    };
    let mut world = SurfaceFireWorld::from_scene(
        &scene,
        &FireMaterialCatalog::default(),
        SurfaceFireConfig::default(),
    )
    .expect("surface world should build");
    world.ignite_object("sofa-1").expect("sofa ignites");
    // Let it grow until it's fully involved.
    let mut peak_kw = 0.0;
    for _ in 0..600 {
        let frame = world.step(1.0).expect("steps");
        let total = frame
            .active_nodes
            .iter()
            .map(|node| node.heat_release_rate_kw)
            .sum::<f64>();
        if total < peak_kw * 0.98 {
            break;
        }
        peak_kw = f64::max(peak_kw, total);
    }

    // Aim at the nearest burning cell each second, as a firefighter does.
    let nozzle_at = point(0.5, 2.0);
    let mut seconds = 0;
    while let Some(target) = world
        .snapshot()
        .active_nodes
        .iter()
        .filter(|node| node.state == CombustionState::Burning)
        .map(|node| node.position_m)
        .min_by(|left, right| {
            let distance = |p: &Vec3| (p.x - nozzle_at.x).hypot(p.y - nozzle_at.y);
            distance(left).total_cmp(&distance(right))
        })
    {
        let offset = (target.x - nozzle_at.x, target.y - nozzle_at.y);
        let length = offset.0.hypot(offset.1).max(1.0e-9);
        let hose = Suppression {
            direction: Some((offset.0 / length, offset.1 / length)),
            ..nozzle(
                "room-1",
                nozzle_at.x,
                nozzle_at.y,
                DEFAULT_NOZZLE_REACH_M,
                DEFAULT_NOZZLE_COOLING_KW,
            )
        };
        world.suppress(&hose, 1.0).expect("applies");
        world.step(1.0).expect("steps");
        seconds += 1;
        assert!(seconds < 120, "a {peak_kw:.0} kW sofa never went out");
    }
    // The carpet around the sofa burns too, so this is a room fire of
    // over a megawatt.
    assert!(
        (10..=60).contains(&seconds),
        "a {peak_kw:.0} kW sofa fire took {seconds} s to knock down"
    );
}

#[test]
fn a_sofa_fire_takes_over_its_room_in_about_a_minute() {
    let mut scene = scene_with_chairs(&[("chair-near", 2.0, 2.45), ("chair-far", 3.3, 3.3)]);
    scene.assets.push(FurnitureAsset {
        id: "sofa-basic".to_owned(),
        category: "sofa".to_owned(),
        model_uri: None,
        dimensions_m: Dimensions3 {
            width_m: 2.0,
            depth_m: 0.9,
            height_m: 0.8,
        },
    });
    let mut sofa = scene.floors[0].objects[0].clone();
    sofa.id = "sofa-1".to_owned();
    sofa.asset_id = "sofa-basic".to_owned();
    sofa.transform.position_m = point(2.0, 0.8);
    scene.floors[0].objects.push(sofa);
    let mut world = SurfaceFireWorld::from_scene(
        &scene,
        &FireMaterialCatalog::default(),
        SurfaceFireConfig::default(),
    )
    .expect("surface world should build");
    let floor_cells = world
        .nodes()
        .iter()
        .filter(|node| matches!(node.source, SurfaceFireNodeSource::FloorCell { .. }))
        .count() as f64;
    world.start_fire("sofa-1").expect("sofa ignites");

    let mut caught_at = std::collections::BTreeMap::new();
    let mut chair_lit_at = None;
    for step in 1..=150 {
        let frame = world.step(0.5).expect("steps");
        let seconds = f64::from(step) * 0.5;
        let caught = frame
            .active_nodes
            .iter()
            .filter(|node| {
                matches!(node.source, SurfaceFireNodeSource::FloorCell { .. })
                    && matches!(
                        node.state,
                        CombustionState::Burning | CombustionState::BurnedOut
                    )
            })
            .count() as f64;
        caught_at.insert(step, caught / floor_cells);
        let chair_burning = frame.active_nodes.iter().any(|node| {
            node.state == CombustionState::Burning
                && matches!(&node.source, SurfaceFireNodeSource::Furniture { object_id } if object_id == "chair-near")
        });
        if chair_lit_at.is_none() && chair_burning {
            chair_lit_at = Some(seconds);
        }
    }

    let at = |seconds: u32| caught_at[&(seconds * 2)];
    assert!(
        at(15) < 0.5,
        "{:.0}% of the floor caught by 15 s",
        at(15) * 100.0
    );
    let chair = chair_lit_at.expect("the nearby chair catches");
    assert!(chair <= 30.0, "the chair 1.5 m away caught at {chair} s");
    assert!(
        at(75) >= 0.8,
        "only {:.0}% of the floor caught by 75 s",
        at(75) * 100.0
    );
    assert!(at(45) < 1.0, "the whole room flashed over by 45 s");
}

#[test]
fn room_heat_does_not_pass_through_walls() {
    // Two rooms side by side; carpet only, so heat can only cross by the
    // room-wide term if it leaks.
    let mut scene = scene_with_chairs(&[("chair-1", 1.0, 2.0)]);
    let room = scene.floors[0].rooms[0].clone();
    let mut next_door = room.clone();
    next_door.id = "room-2".to_owned();
    for edge in &mut next_door.boundary {
        edge.id = format!("2-{}", edge.id);
        edge.start.x += 4.0;
        edge.end.x += 4.0;
    }
    scene.floors[0].rooms.push(next_door);
    let mut world = SurfaceFireWorld::from_scene(
        &scene,
        &FireMaterialCatalog::default(),
        SurfaceFireConfig {
            room_heat_fraction: 1.0,
            ..SurfaceFireConfig::default()
        },
    )
    .expect("surface world should build");
    world.ignite_object("chair-1").expect("chair ignites");
    for _ in 0..20 {
        world.step(0.5).expect("steps");
    }
    let warmed_next_door = world
        .nodes()
        .iter()
        .filter(|node| node.room_id == "room-2")
        .any(|node| node.absorbed_heat_kj > 0.0);
    assert!(!warmed_next_door);
}

#[test]
fn burning_cells_burn_out_instead_of_smouldering_forever() {
    let scene = scene_with_chairs(&[("chair-1", 2.0, 2.0)]);
    let mut world = SurfaceFireWorld::from_scene(
        &scene,
        &FireMaterialCatalog::default(),
        SurfaceFireConfig::default(),
    )
    .expect("surface world should build");
    world.start_fire("chair-1").expect("chair ignites");
    let mut frame = world.snapshot();
    for _ in 0..1_200 {
        frame = world.step(0.5).expect("steps");
    }
    let still_burning = frame
        .active_nodes
        .iter()
        .filter(|node| node.state == CombustionState::Burning)
        .map(|node| node.heat_release_rate_kw)
        .collect::<Vec<_>>();
    assert!(
        still_burning.is_empty(),
        "still 'burning' at {still_burning:?} kW"
    );
}

#[test]
fn a_desk_fire_grows_into_a_noticeable_one() {
    // Wood is harder to light than upholstery; a desk fire must still grow
    // rather than die out on the few cells it started on.
    let mut scene = scene_with_chairs(&[("desk-1", 2.0, 2.0)]);
    scene.assets[0].category = "desk".to_owned();
    scene.assets[0].dimensions_m = Dimensions3 {
        width_m: 1.2,
        depth_m: 0.6,
        height_m: 0.75,
    };
    let mut world = SurfaceFireWorld::from_scene(
        &scene,
        &FireMaterialCatalog::default(),
        SurfaceFireConfig::default(),
    )
    .expect("surface world should build");
    world.start_fire("desk-1").expect("desk ignites");
    let mut peak_kw = 0.0;
    let mut noticeable_at = None;
    for step in 1..=240 {
        let frame = world.step(0.5).expect("steps");
        let total = frame
            .active_nodes
            .iter()
            .map(|node| node.heat_release_rate_kw)
            .sum::<f64>();
        peak_kw = f64::max(peak_kw, total);
        if noticeable_at.is_none() && total >= 50.0 {
            noticeable_at = Some(f64::from(step) * 0.5);
        }
    }
    let noticeable_at = noticeable_at.expect("the desk fire gets noticeable");
    assert!(noticeable_at <= 45.0, "50 kW only at {noticeable_at} s");
    assert!(
        peak_kw >= 200.0,
        "the desk fire peaked at {peak_kw:.0} kW in two minutes"
    );
}

/// Spread only between touching neighbours: no room-wide heat and no
/// furniture radiating onto the floor around it, so tests of how fire moves
/// cell by cell aren't swamped by them.
fn neighbours_only() -> SurfaceFireConfig {
    SurfaceFireConfig {
        room_heat_fraction: 0.0,
        furniture_floor_radiation_m: 0.0,
        ..SurfaceFireConfig::default()
    }
}

fn nozzle(room_id: &str, x: f64, y: f64, reach_m: f64, cooling_kw: f64) -> Suppression {
    Suppression {
        room_id: room_id.to_owned(),
        position_m: point(x, y),
        reach_m,
        cooling_kw,
        direction: None,
        half_angle_degrees: DEFAULT_NOZZLE_HALF_ANGLE_DEGREES,
    }
}

fn fast_catalog(combustible_floor: bool) -> FireMaterialCatalog {
    let furniture = fast_material("test_furniture");
    FireMaterialCatalog {
        floor: if combustible_floor {
            fast_material("test_floor")
        } else {
            FireMaterialProfile::noncombustible("concrete")
        },
        default_furniture: furniture.clone(),
        furniture: vec![FurnitureFireMaterial {
            category: "chair".to_owned(),
            material: furniture,
        }],
    }
}

fn fast_material(id: &str) -> FireMaterialProfile {
    FireMaterialProfile {
        id: id.to_owned(),
        combustible: true,
        ignition_temperature_c: 200.0,
        ignition_energy_kj_per_m2: 1.0,
        peak_hrr_kw_per_m2: 200.0,
        fuel_energy_kj_per_m2: 1_000.0,
        heat_transfer_fraction: 0.8,
        cooling_kw_per_m2: 0.0,
    }
}

fn scene_with_chairs(chairs: &[(&str, f64, f64)]) -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "fire-scene".to_owned(),
        name: None,
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "fire-scene".to_owned(),
        },
        assets: vec![FurnitureAsset {
            id: "chair-basic".to_owned(),
            category: "chair".to_owned(),
            model_uri: None,
            dimensions_m: Dimensions3 {
                width_m: 0.4,
                depth_m: 0.4,
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
                classifications: vec![],
                dimensions: None,
                boundary: vec![
                    edge("south", point(0.0, 0.0), point(4.0, 0.0)),
                    edge("east", point(4.0, 0.0), point(4.0, 4.0)),
                    edge("north", point(4.0, 4.0), point(0.0, 4.0)),
                    edge("west", point(0.0, 4.0), point(0.0, 0.0)),
                ],
            }],
            objects: chairs
                .iter()
                .map(|(id, x, y)| FurnitureObject {
                    id: (*id).to_owned(),
                    asset_id: "chair-basic".to_owned(),
                    transform: Transform3 {
                        position_m: point(*x, *y),
                        rotation: Quaternion::IDENTITY,
                        scale: Vec3 {
                            x: 1.0,
                            y: 1.0,
                            z: 1.0,
                        },
                    },
                    mobility: Mobility::Movable,
                })
                .collect(),
        }],
    }
}

fn edge(id: &str, start: Vec3, end: Vec3) -> BoundaryEdge {
    BoundaryEdge {
        id: id.to_owned(),
        kind: BoundaryKind::Wall,
        start,
        end,
        thickness_m: Some(0.1),
        center_line_bias: Some(0.5),
        openings: vec![],
    }
}

fn point(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
