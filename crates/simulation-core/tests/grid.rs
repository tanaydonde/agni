use tflop_simulation_core::{
    GridBuildError, GridCellKind, Simulation, SimulationConfig, SimulationGrid,
};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, FurnitureAsset, FurnitureObject, Mobility,
    Opening, OpeningKind, Quaternion, Room, SCENE_SCHEMA_VERSION, Scene, SceneSource, Transform3,
    Vec3,
};

#[test]
fn builds_and_classifies_an_l_shaped_floor() {
    let scene = scene_fixture();
    let grid = SimulationGrid::from_scene(&scene, "floor-1", 0.5).expect("grid should build");

    assert_eq!(grid.columns, 8);
    assert_eq!(grid.rows, 8);
    assert_eq!(grid.cells().len(), 64);
    assert_eq!(kind_at(&grid, 3.0, 3.0), GridCellKind::Outside);
    assert_eq!(kind_at(&grid, 1.0, 3.0), GridCellKind::Floor);
    assert_eq!(kind_at(&grid, 0.1, 3.0), GridCellKind::Wall);
    assert_eq!(kind_at(&grid, 2.0, 0.1), GridCellKind::Opening);
    assert_eq!(kind_at(&grid, 1.0, 1.0), GridCellKind::Furniture);

    let opening = cell_at(&grid, 2.0, 0.1);
    assert_eq!(opening.boundary_edge_id.as_deref(), Some("south"));
    assert_eq!(opening.opening_id.as_deref(), Some("door-1"));
    assert_eq!(opening.opening_kind, Some(OpeningKind::Door));

    let furniture = cell_at(&grid, 1.0, 1.0);
    assert_eq!(furniture.object_id.as_deref(), Some("chair-1"));
    assert!(furniture.room_ids.iter().any(|id| id == "room-1"));
}

#[test]
fn converts_between_world_positions_and_grid_coordinates() {
    let scene = scene_fixture();
    let grid = SimulationGrid::from_scene(&scene, "floor-1", 0.5).expect("grid should build");
    let world_position = position(1.2, 2.7);

    let coordinate = grid
        .world_to_cell(world_position)
        .expect("position is in bounds");
    let center = grid.cell_center(coordinate).expect("cell exists");

    assert_eq!(coordinate.column, 2);
    assert_eq!(coordinate.row, 5);
    assert_eq!(center, position(1.25, 2.75));
    assert_eq!(grid.world_to_cell(position(4.0, 2.0)), None);
}

#[test]
fn reports_counts_for_every_cell_kind() {
    let scene = scene_fixture();
    let grid = SimulationGrid::from_scene(&scene, "floor-1", 0.5).expect("grid should build");
    let summary = grid.summary();

    assert_eq!(
        summary.total_cells,
        summary.outside_cells
            + summary.floor_cells
            + summary.wall_cells
            + summary.opening_cells
            + summary.furniture_cells
    );
    assert!(summary.outside_cells > 0);
    assert!(summary.floor_cells > 0);
    assert!(summary.wall_cells > 0);
    assert!(summary.opening_cells > 0);
    assert!(summary.furniture_cells > 0);
}

#[test]
fn simulation_builds_and_exposes_static_grids() {
    let simulation = Simulation::new(
        scene_fixture(),
        SimulationConfig {
            random_seed: 7,
            grid_cell_size_m: 0.5,
        },
    )
    .expect("simulation should build its grids");

    assert_eq!(simulation.grids().len(), 1);
    assert_eq!(simulation.grid("floor-1").expect("grid exists").columns, 8);
    assert!(simulation.grid("missing").is_none());
    assert!(simulation.snapshot().fires.is_empty());
}

#[test]
fn rejects_missing_floors_and_invalid_cell_sizes() {
    let scene = scene_fixture();
    assert!(matches!(
        SimulationGrid::from_scene(&scene, "missing", 0.5),
        Err(GridBuildError::FloorNotFound { .. })
    ));
    assert!(matches!(
        SimulationGrid::from_scene(&scene, "floor-1", 0.0),
        Err(GridBuildError::InvalidCellSize { .. })
    ));
}

fn kind_at(grid: &SimulationGrid, x: f64, y: f64) -> GridCellKind {
    cell_at(grid, x, y).kind
}

fn cell_at(grid: &SimulationGrid, x: f64, y: f64) -> &tflop_simulation_core::GridCell {
    let coordinate = grid
        .world_to_cell(position(x, y))
        .expect("test position should be in grid bounds");
    grid.cell(coordinate).expect("coordinate should exist")
}

fn scene_fixture() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "scene-1".to_owned(),
        name: Some("Grid fixture".to_owned()),
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "scene-1".to_owned(),
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
            label: Some("Ground floor".to_owned()),
            elevation_m: Some(0.0),
            rooms: vec![Room {
                id: "room-1".to_owned(),
                label: Some("L room".to_owned()),
                classifications: vec![],
                dimensions: None,
                boundary: vec![
                    edge(
                        "south",
                        position(0.0, 0.0),
                        position(4.0, 0.0),
                        vec![Opening {
                            id: "door-1".to_owned(),
                            label: Some("Door".to_owned()),
                            kind: OpeningKind::Door,
                            width_m: 1.0,
                            height_m: Some(2.1),
                            lower_elevation_m: Some(0.0),
                            relative_center: 0.5,
                        }],
                    ),
                    edge("east-low", position(4.0, 0.0), position(4.0, 2.0), vec![]),
                    edge("notch-low", position(4.0, 2.0), position(2.0, 2.0), vec![]),
                    edge("notch-high", position(2.0, 2.0), position(2.0, 4.0), vec![]),
                    edge("north", position(2.0, 4.0), position(0.0, 4.0), vec![]),
                    edge("west", position(0.0, 4.0), position(0.0, 0.0), vec![]),
                ],
            }],
            objects: vec![FurnitureObject {
                id: "chair-1".to_owned(),
                asset_id: "chair-basic".to_owned(),
                transform: Transform3 {
                    position_m: position(1.0, 1.0),
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

fn position(x: f64, y: f64) -> Vec3 {
    Vec3 { x, y, z: 0.0 }
}
