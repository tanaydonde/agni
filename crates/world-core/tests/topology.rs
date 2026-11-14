use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Floor, Opening, OpeningKind, OpeningTopologyError, Room,
    SCENE_SCHEMA_VERSION, Scene, SceneSource, Vec3, resolve_opening_connections,
};

#[test]
fn resolves_an_explicit_opening_between_adjacent_rooms() {
    let scene = adjacent_rooms(true, false);

    let connections = resolve_opening_connections(&scene).expect("topology should resolve");

    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].id, "door-a");
    assert_eq!(connections[0].room_a_id, "room-a");
    assert_eq!(connections[0].room_b_id.as_deref(), Some("room-b"));
}

#[test]
fn does_not_connect_rooms_that_only_share_a_solid_wall() {
    let scene = adjacent_rooms(false, false);

    let connections = resolve_opening_connections(&scene).expect("topology should resolve");

    assert!(connections.is_empty());
}

#[test]
fn classifies_an_opening_without_a_neighbor_as_outside() {
    let mut scene = adjacent_rooms(false, false);
    scene.floors[0].rooms.truncate(1);
    scene.floors[0].rooms[0].boundary[3].openings = vec![opening("outside-door")];

    let connections = resolve_opening_connections(&scene).expect("topology should resolve");

    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].room_a_id, "room-a");
    assert_eq!(connections[0].room_b_id, None);
}

#[test]
fn merges_two_boundary_records_for_the_same_physical_door() {
    let scene = adjacent_rooms(true, true);

    let connections = resolve_opening_connections(&scene).expect("topology should resolve");

    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].source_opening_ids, ["door-a", "door-b"]);
    assert_eq!(connections[0].room_b_id.as_deref(), Some("room-b"));
}

#[test]
fn rejects_ambiguous_neighbor_geometry() {
    let mut scene = adjacent_rooms(true, false);
    let mut overlapping = scene.floors[0].rooms[1].clone();
    overlapping.id = "room-c".to_owned();
    scene.floors[0].rooms.push(overlapping);

    assert!(matches!(
        resolve_opening_connections(&scene),
        Err(OpeningTopologyError::AmbiguousNeighbor { .. })
    ));
}

fn adjacent_rooms(opening_on_a: bool, duplicate_on_b: bool) -> Scene {
    let a_east_openings = if opening_on_a {
        vec![opening("door-a")]
    } else {
        Vec::new()
    };
    let b_west_openings = if duplicate_on_b {
        vec![opening("door-b")]
    } else {
        Vec::new()
    };

    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "topology-scene".to_owned(),
        name: None,
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "topology-scene".to_owned(),
        },
        assets: vec![],
        floors: vec![Floor {
            id: "floor-1".to_owned(),
            label: None,
            elevation_m: Some(0.0),
            rooms: vec![
                room(
                    "room-a",
                    0.0,
                    4.0,
                    [vec![], a_east_openings, vec![], vec![]],
                ),
                room(
                    "room-b",
                    4.0,
                    8.0,
                    [vec![], vec![], vec![], b_west_openings],
                ),
            ],
            objects: vec![],
        }],
    }
}

fn room(id: &str, min_x: f64, max_x: f64, openings: [Vec<Opening>; 4]) -> Room {
    Room {
        id: id.to_owned(),
        label: None,
        classifications: vec![],
        dimensions: None,
        boundary: vec![
            edge(
                "south",
                point(min_x, 0.0),
                point(max_x, 0.0),
                openings[0].clone(),
            ),
            edge(
                "east",
                point(max_x, 0.0),
                point(max_x, 4.0),
                openings[1].clone(),
            ),
            edge(
                "north",
                point(max_x, 4.0),
                point(min_x, 4.0),
                openings[2].clone(),
            ),
            edge(
                "west",
                point(min_x, 4.0),
                point(min_x, 0.0),
                openings[3].clone(),
            ),
        ],
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

fn opening(id: &str) -> Opening {
    Opening {
        id: id.to_owned(),
        label: None,
        kind: OpeningKind::Door,
        width_m: 1.0,
        height_m: Some(2.0),
        lower_elevation_m: Some(0.0),
        relative_center: 0.5,
    }
}

fn point(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
