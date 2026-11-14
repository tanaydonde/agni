use serde::{Deserialize, Serialize};
use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Floor, FurnitureAsset, FurnitureObject, OpeningKind, Room, Scene,
    Vec3,
};
use thiserror::Error;

const MAX_GRID_CELLS: usize = 10_000_000;
const GEOMETRY_EPSILON: f64 = 1.0e-9;

/// Static, row-major representation of one floor used by the simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulationGrid {
    pub floor_id: String,
    pub cell_size_m: f64,
    pub columns: u32,
    pub rows: u32,
    pub bounds: GridBounds,
    cells: Vec<GridCell>,
}

impl SimulationGrid {
    pub fn from_scene(
        scene: &Scene,
        floor_id: &str,
        cell_size_m: f64,
    ) -> Result<Self, GridBuildError> {
        let floor = scene
            .floors
            .iter()
            .find(|floor| floor.id == floor_id)
            .ok_or_else(|| GridBuildError::FloorNotFound {
                floor_id: floor_id.to_owned(),
            })?;
        Self::from_floor(floor, &scene.assets, cell_size_m)
    }

    pub fn from_floor(
        floor: &Floor,
        assets: &[FurnitureAsset],
        cell_size_m: f64,
    ) -> Result<Self, GridBuildError> {
        if !cell_size_m.is_finite() || cell_size_m <= 0.0 {
            return Err(GridBuildError::InvalidCellSize { cell_size_m });
        }

        let source_bounds = geometry_bounds(floor).ok_or_else(|| GridBuildError::NoGeometry {
            floor_id: floor.id.clone(),
        })?;
        let columns = dimension(source_bounds.max_x - source_bounds.min_x, cell_size_m);
        let rows = dimension(source_bounds.max_y - source_bounds.min_y, cell_size_m);
        let cell_count =
            (columns as usize)
                .checked_mul(rows as usize)
                .ok_or(GridBuildError::GridTooLarge {
                    cell_count: usize::MAX,
                    maximum: MAX_GRID_CELLS,
                })?;
        if cell_count > MAX_GRID_CELLS {
            return Err(GridBuildError::GridTooLarge {
                cell_count,
                maximum: MAX_GRID_CELLS,
            });
        }

        let elevation_m = floor
            .elevation_m
            .or_else(|| {
                floor
                    .rooms
                    .iter()
                    .flat_map(|room| room.boundary.iter())
                    .next()
                    .map(|edge| edge.start.z)
            })
            .unwrap_or(0.0);
        let bounds = GridBounds {
            min_x: source_bounds.min_x,
            min_y: source_bounds.min_y,
            max_x: source_bounds.min_x + f64::from(columns) * cell_size_m,
            max_y: source_bounds.min_y + f64::from(rows) * cell_size_m,
            elevation_m,
        };

        let mut object_footprints = Vec::with_capacity(floor.objects.len());
        for object in &floor.objects {
            let asset = assets
                .iter()
                .find(|asset| asset.id == object.asset_id)
                .ok_or_else(|| GridBuildError::AssetNotFound {
                    object_id: object.id.clone(),
                    asset_id: object.asset_id.clone(),
                })?;
            object_footprints.push((object, furniture_footprint(object, asset)?));
        }

        let mut cells = Vec::with_capacity(cell_count);
        for row in 0..rows {
            for column in 0..columns {
                let coordinate = GridCoordinate { column, row };
                let center_m = cell_center(bounds, cell_size_m, coordinate);
                cells.push(classify_cell(
                    coordinate,
                    center_m,
                    cell_size_m,
                    floor,
                    &object_footprints,
                ));
            }
        }

        Ok(Self {
            floor_id: floor.id.clone(),
            cell_size_m,
            columns,
            rows,
            bounds,
            cells,
        })
    }

    #[must_use]
    pub fn cells(&self) -> &[GridCell] {
        &self.cells
    }

    #[must_use]
    pub fn cell(&self, coordinate: GridCoordinate) -> Option<&GridCell> {
        if coordinate.column >= self.columns || coordinate.row >= self.rows {
            return None;
        }
        let index = coordinate.row as usize * self.columns as usize + coordinate.column as usize;
        self.cells.get(index)
    }

    #[must_use]
    pub fn world_to_cell(&self, position_m: Vec3) -> Option<GridCoordinate> {
        if position_m.x < self.bounds.min_x
            || position_m.x >= self.bounds.max_x
            || position_m.y < self.bounds.min_y
            || position_m.y >= self.bounds.max_y
        {
            return None;
        }
        let coordinate = GridCoordinate {
            column: ((position_m.x - self.bounds.min_x) / self.cell_size_m).floor() as u32,
            row: ((position_m.y - self.bounds.min_y) / self.cell_size_m).floor() as u32,
        };
        self.cell(coordinate).map(|_| coordinate)
    }

    #[must_use]
    pub fn cell_center(&self, coordinate: GridCoordinate) -> Option<Vec3> {
        self.cell(coordinate)
            .map(|_| cell_center(self.bounds, self.cell_size_m, coordinate))
    }

    /// Whether a circular body fits entirely on walkable cells. Touching a
    /// blocked cell is allowed; overlapping one is not.
    #[must_use]
    pub fn circle_is_walkable(&self, center_m: Vec3, radius_m: f64) -> bool {
        if !center_m.x.is_finite()
            || !center_m.y.is_finite()
            || !radius_m.is_finite()
            || radius_m <= 0.0
            || center_m.x - radius_m < self.bounds.min_x
            || center_m.x + radius_m > self.bounds.max_x
            || center_m.y - radius_m < self.bounds.min_y
            || center_m.y + radius_m > self.bounds.max_y
        {
            return false;
        }

        let Some(center_coordinate) = self.world_to_cell(center_m) else {
            return false;
        };
        if !self
            .cell(center_coordinate)
            .is_some_and(GridCell::is_walkable)
        {
            return false;
        }

        let min_column = ((center_m.x - radius_m - self.bounds.min_x) / self.cell_size_m)
            .floor()
            .max(0.0) as u32;
        let max_column = ((center_m.x + radius_m - self.bounds.min_x) / self.cell_size_m)
            .floor()
            .min(f64::from(self.columns - 1)) as u32;
        let min_row = ((center_m.y - radius_m - self.bounds.min_y) / self.cell_size_m)
            .floor()
            .max(0.0) as u32;
        let max_row = ((center_m.y + radius_m - self.bounds.min_y) / self.cell_size_m)
            .floor()
            .min(f64::from(self.rows - 1)) as u32;
        let radius_squared = radius_m * radius_m;

        for row in min_row..=max_row {
            for column in min_column..=max_column {
                let Some(cell) = self.cell(GridCoordinate { column, row }) else {
                    return false;
                };
                if cell.is_walkable() {
                    continue;
                }

                let half_cell = self.cell_size_m / 2.0;
                let closest_x = center_m
                    .x
                    .clamp(cell.center_m.x - half_cell, cell.center_m.x + half_cell);
                let closest_y = center_m
                    .y
                    .clamp(cell.center_m.y - half_cell, cell.center_m.y + half_cell);
                let dx = center_m.x - closest_x;
                let dy = center_m.y - closest_y;
                if dx * dx + dy * dy < radius_squared - GEOMETRY_EPSILON {
                    return false;
                }
            }
        }

        true
    }

    #[must_use]
    pub fn summary(&self) -> GridSummary {
        let mut summary = GridSummary {
            total_cells: self.cells.len(),
            ..GridSummary::default()
        };
        for cell in &self.cells {
            match cell.kind {
                GridCellKind::Outside => summary.outside_cells += 1,
                GridCellKind::Floor => summary.floor_cells += 1,
                GridCellKind::Wall => summary.wall_cells += 1,
                GridCellKind::Opening => summary.opening_cells += 1,
                GridCellKind::Furniture => summary.furniture_cells += 1,
            }
        }
        summary
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GridBounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub elevation_m: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GridCoordinate {
    pub column: u32,
    pub row: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridCell {
    pub coordinate: GridCoordinate,
    pub center_m: Vec3,
    pub kind: GridCellKind,
    pub room_ids: Vec<String>,
    pub boundary_edge_id: Option<String>,
    pub opening_id: Option<String>,
    pub opening_kind: Option<OpeningKind>,
    pub object_id: Option<String>,
}

impl GridCell {
    /// Whether a person can stand in or walk through this cell: open floor,
    /// or a door or passage. Windows and unclassified openings are not
    /// walkable.
    #[must_use]
    pub fn is_walkable(&self) -> bool {
        match self.kind {
            GridCellKind::Floor => true,
            GridCellKind::Opening => matches!(
                self.opening_kind,
                Some(OpeningKind::Door | OpeningKind::Passage)
            ),
            GridCellKind::Outside | GridCellKind::Wall | GridCellKind::Furniture => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridCellKind {
    Outside,
    Floor,
    Wall,
    Opening,
    Furniture,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridSummary {
    pub total_cells: usize,
    pub outside_cells: usize,
    pub floor_cells: usize,
    pub wall_cells: usize,
    pub opening_cells: usize,
    pub furniture_cells: usize,
}

#[derive(Debug, Error)]
pub enum GridBuildError {
    #[error("grid cell size must be finite and greater than zero, got {cell_size_m}")]
    InvalidCellSize { cell_size_m: f64 },
    #[error("floor {floor_id} was not found")]
    FloorNotFound { floor_id: String },
    #[error("floor {floor_id} has no boundary geometry")]
    NoGeometry { floor_id: String },
    #[error("grid would contain {cell_count} cells, exceeding the maximum of {maximum}")]
    GridTooLarge { cell_count: usize, maximum: usize },
    #[error("object {object_id} references missing asset {asset_id}")]
    AssetNotFound { object_id: String, asset_id: String },
    #[error("object {object_id} must remain upright and may only rotate around the Z axis")]
    UnsupportedFurnitureRotation { object_id: String },
}

#[derive(Clone, Copy)]
struct Bounds2 {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

#[derive(Clone, Copy)]
struct Point2 {
    x: f64,
    y: f64,
}

fn geometry_bounds(floor: &Floor) -> Option<Bounds2> {
    let mut bounds: Option<Bounds2> = None;
    for edge in floor.rooms.iter().flat_map(|room| room.boundary.iter()) {
        for point in [edge.start, edge.end] {
            bounds = Some(match bounds {
                Some(current) => Bounds2 {
                    min_x: current.min_x.min(point.x),
                    min_y: current.min_y.min(point.y),
                    max_x: current.max_x.max(point.x),
                    max_y: current.max_y.max(point.y),
                },
                None => Bounds2 {
                    min_x: point.x,
                    min_y: point.y,
                    max_x: point.x,
                    max_y: point.y,
                },
            });
        }
    }
    bounds.filter(|bounds| bounds.max_x > bounds.min_x && bounds.max_y > bounds.min_y)
}

fn dimension(length_m: f64, cell_size_m: f64) -> u32 {
    (length_m / cell_size_m).ceil().max(1.0) as u32
}

fn cell_center(bounds: GridBounds, cell_size_m: f64, coordinate: GridCoordinate) -> Vec3 {
    Vec3 {
        x: bounds.min_x + (f64::from(coordinate.column) + 0.5) * cell_size_m,
        y: bounds.min_y + (f64::from(coordinate.row) + 0.5) * cell_size_m,
        z: bounds.elevation_m,
    }
}

fn classify_cell(
    coordinate: GridCoordinate,
    center_m: Vec3,
    cell_size_m: f64,
    floor: &Floor,
    object_footprints: &[(&FurnitureObject, [Point2; 4])],
) -> GridCell {
    let center = point(center_m.x, center_m.y);
    let half_diagonal = cell_size_m * 2.0_f64.sqrt() / 2.0;
    let mut room_ids = Vec::new();
    let mut wall = None;
    let mut opening = None;

    for room in &floor.rooms {
        if point_in_room(center, room) {
            push_unique(&mut room_ids, &room.id);
        }
        for edge in &room.boundary {
            let half_wall_thickness = edge.thickness_m.unwrap_or(0.0) / 2.0;
            if edge.kind == BoundaryKind::Wall
                && point_segment_distance(
                    center,
                    point(edge.start.x, edge.start.y),
                    point(edge.end.x, edge.end.y),
                ) <= half_diagonal + half_wall_thickness
            {
                push_unique(&mut room_ids, &room.id);
                wall.get_or_insert_with(|| edge.id.clone());
            }

            for edge_opening in &edge.openings {
                let (start, end) =
                    opening_segment(edge, edge_opening.relative_center, edge_opening.width_m);
                if point_segment_distance(center, start, end) <= half_diagonal + half_wall_thickness
                {
                    push_unique(&mut room_ids, &room.id);
                    opening.get_or_insert_with(|| {
                        (edge.id.clone(), edge_opening.id.clone(), edge_opening.kind)
                    });
                }
            }
        }
    }

    let mut kind = if room_ids.is_empty() {
        GridCellKind::Outside
    } else {
        GridCellKind::Floor
    };
    let mut boundary_edge_id = None;
    let mut opening_id = None;
    let mut opening_kind = None;
    if let Some(edge_id) = wall {
        kind = GridCellKind::Wall;
        boundary_edge_id = Some(edge_id);
    }
    if let Some((edge_id, id, opening_type)) = opening {
        kind = GridCellKind::Opening;
        boundary_edge_id = Some(edge_id);
        opening_id = Some(id);
        opening_kind = Some(opening_type);
    }

    let cell_polygon = square(center, cell_size_m / 2.0);
    let mut object_id = None;
    for (object, footprint) in object_footprints {
        if convex_polygons_intersect(&cell_polygon, footprint) {
            kind = GridCellKind::Furniture;
            object_id = Some(object.id.clone());
            break;
        }
    }

    GridCell {
        coordinate,
        center_m,
        kind,
        room_ids,
        boundary_edge_id,
        opening_id,
        opening_kind,
        object_id,
    }
}

/// Whether `position_m` lies inside `room`'s boundary (or on it), ignoring
/// height.
pub(crate) fn room_contains(room: &Room, position_m: Vec3) -> bool {
    point_in_room(point(position_m.x, position_m.y), room)
}

fn point_in_room(point: Point2, room: &Room) -> bool {
    let mut inside = false;
    for edge in &room.boundary {
        let start = Point2 {
            x: edge.start.x,
            y: edge.start.y,
        };
        let end = Point2 {
            x: edge.end.x,
            y: edge.end.y,
        };
        if point_on_segment(point, start, end) {
            return true;
        }
        if (start.y > point.y) != (end.y > point.y) {
            let crossing_x = (end.x - start.x) * (point.y - start.y) / (end.y - start.y) + start.x;
            if point.x < crossing_x {
                inside = !inside;
            }
        }
    }
    inside
}

fn opening_segment(edge: &BoundaryEdge, relative_center: f64, width_m: f64) -> (Point2, Point2) {
    let start = point(edge.start.x, edge.start.y);
    let end = point(edge.end.x, edge.end.y);
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = dx.hypot(dy);
    let direction = point(dx / length, dy / length);
    let center = point(
        start.x + dx * relative_center,
        start.y + dy * relative_center,
    );
    let half_width = width_m / 2.0;
    (
        point(
            center.x - direction.x * half_width,
            center.y - direction.y * half_width,
        ),
        point(
            center.x + direction.x * half_width,
            center.y + direction.y * half_width,
        ),
    )
}

fn furniture_footprint(
    object: &FurnitureObject,
    asset: &FurnitureAsset,
) -> Result<[Point2; 4], GridBuildError> {
    ensure_upright(object)?;
    let rotation = object.transform.rotation;
    let yaw = (2.0 * (rotation.w * rotation.z + rotation.x * rotation.y))
        .atan2(1.0 - 2.0 * (rotation.y * rotation.y + rotation.z * rotation.z));
    let half_width = asset.dimensions_m.width_m * object.transform.scale.x / 2.0;
    let half_depth = asset.dimensions_m.depth_m * object.transform.scale.y / 2.0;
    let center = point(object.transform.position_m.x, object.transform.position_m.y);
    let cosine = yaw.cos();
    let sine = yaw.sin();
    Ok([
        rotate_corner(center, -half_width, -half_depth, cosine, sine),
        rotate_corner(center, half_width, -half_depth, cosine, sine),
        rotate_corner(center, half_width, half_depth, cosine, sine),
        rotate_corner(center, -half_width, half_depth, cosine, sine),
    ])
}

fn ensure_upright(object: &FurnitureObject) -> Result<(), GridBuildError> {
    let rotation = object.transform.rotation;
    let vertical_x = 2.0 * (rotation.x * rotation.z + rotation.w * rotation.y);
    let vertical_y = 2.0 * (rotation.y * rotation.z - rotation.w * rotation.x);
    let vertical_z = 1.0 - 2.0 * (rotation.x * rotation.x + rotation.y * rotation.y);
    if vertical_x.abs() > 1.0e-6 || vertical_y.abs() > 1.0e-6 || vertical_z < 1.0 - 1.0e-6 {
        Err(GridBuildError::UnsupportedFurnitureRotation {
            object_id: object.id.clone(),
        })
    } else {
        Ok(())
    }
}

fn rotate_corner(center: Point2, local_x: f64, local_y: f64, cosine: f64, sine: f64) -> Point2 {
    point(
        center.x + local_x * cosine - local_y * sine,
        center.y + local_x * sine + local_y * cosine,
    )
}

fn square(center: Point2, half_size: f64) -> [Point2; 4] {
    [
        point(center.x - half_size, center.y - half_size),
        point(center.x + half_size, center.y - half_size),
        point(center.x + half_size, center.y + half_size),
        point(center.x - half_size, center.y + half_size),
    ]
}

fn convex_polygons_intersect(a: &[Point2], b: &[Point2]) -> bool {
    for polygon in [a, b] {
        for index in 0..polygon.len() {
            let current = polygon[index];
            let next = polygon[(index + 1) % polygon.len()];
            let axis = point(-(next.y - current.y), next.x - current.x);
            let (a_min, a_max) = project(a, axis);
            let (b_min, b_max) = project(b, axis);
            if a_max < b_min - GEOMETRY_EPSILON || b_max < a_min - GEOMETRY_EPSILON {
                return false;
            }
        }
    }
    true
}

fn project(polygon: &[Point2], axis: Point2) -> (f64, f64) {
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for point in polygon {
        let projection = point.x * axis.x + point.y * axis.y;
        minimum = minimum.min(projection);
        maximum = maximum.max(projection);
    }
    (minimum, maximum)
}

fn point_segment_distance(target: Point2, start: Point2, end: Point2) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= GEOMETRY_EPSILON {
        return (target.x - start.x).hypot(target.y - start.y);
    }
    let t =
        (((target.x - start.x) * dx + (target.y - start.y) * dy) / length_squared).clamp(0.0, 1.0);
    let closest = point(start.x + t * dx, start.y + t * dy);
    (target.x - closest.x).hypot(target.y - closest.y)
}

fn point_on_segment(point: Point2, start: Point2, end: Point2) -> bool {
    orientation(start, end, point).abs() <= GEOMETRY_EPSILON
        && point.x >= start.x.min(end.x) - GEOMETRY_EPSILON
        && point.x <= start.x.max(end.x) + GEOMETRY_EPSILON
        && point.y >= start.y.min(end.y) - GEOMETRY_EPSILON
        && point.y <= start.y.max(end.y) + GEOMETRY_EPSILON
}

fn orientation(a: Point2, b: Point2, c: Point2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

const fn point(x: f64, y: f64) -> Point2 {
    Point2 { x, y }
}
