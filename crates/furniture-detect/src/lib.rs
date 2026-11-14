//! Finds furniture in a Matterport scan by comparing the furnished mesh with
//! Matterport's defurnished mesh of the same space.
//!
//! Both meshes share one Z-up frame. Surfaces that appear in the furnished
//! mesh but lie well away from every defurnished surface are furniture. Those
//! are voxelized, projected to a top-down grid, split into connected pieces,
//! and each piece becomes an upright box: a measured footprint, rotation, and
//! height that can serve as a collision hitbox. Each box gets a label (what it
//! probably is, from size and room type) and the scene catalog asset closest
//! to it in size.

mod classify;
mod matching;
mod obj;
mod rect;
mod scene_objects;

pub use classify::{FurnitureLabel, classify};
pub use matching::nearest_asset;
pub use obj::{Mesh, ObjError, parse_obj};
pub use rect::{OrientedRect, min_area_rect};
pub use scene_objects::{
    MergeSummary, SCANNED_OBJECT_PREFIX, merge_into_scene, scene_object, yaw_quaternion,
};

use serde::{Deserialize, Serialize};
use tflop_world_core::{Dimensions3, Floor, FurnitureAsset, Room, Scene, Vec3};

/// Current version of the serialized detection output.
pub const DETECTION_SCHEMA_VERSION: u32 = 3;

/// Tuning knobs. The defaults suit Matterport's 50k defurnished mesh, whose
/// decimated walls and floors deviate from the full-resolution scan by a few
/// centimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DetectParams {
    /// Edge length of the voxels and of the top-down grid cells.
    pub voxel_m: f64,
    /// Furnished surfaces closer than this to a defurnished surface count as
    /// room shell (walls, floor, built-ins), not furniture.
    pub shell_tolerance_m: f64,
    /// Lowest height above the floor that counts, which drops rugs.
    pub min_height_m: f64,
    /// Highest height above the floor that counts, which drops ceiling
    /// fixtures.
    pub max_height_m: f64,
    /// Top-down cells need at least this many furniture voxels in their
    /// column. A flat bed or table top is only one voxel thick.
    pub min_column_voxels: u32,
    /// Smaller footprints are discarded as scan noise.
    pub min_footprint_m2: f64,
    /// Shorter objects are discarded as clutter.
    pub min_object_height_m: f64,
    /// Within one column, anything more than this far above the furniture
    /// below it is overhead (a chandelier over a table, a shelf over a desk)
    /// and is ignored.
    pub max_vertical_gap_m: f64,
    /// Columns whose lowest furniture starts higher than this are overhead
    /// fixtures, not obstacles.
    pub max_base_height_m: f64,
    /// Boxes that fill less of their rectangle than this (an L-shaped sofa)
    /// are split into smaller boxes.
    pub min_box_fill: f64,
    /// Fraction of a piece's cells that may lie outside its box on each side,
    /// so a few stray cells (a cable, a cushion corner) can't stretch it.
    pub edge_trim_fraction: f64,
    /// Box height is this percentile of the piece's column tops, so a lamp on
    /// a table doesn't set the table's height.
    pub height_percentile: f64,
}

impl Default for DetectParams {
    fn default() -> Self {
        Self {
            voxel_m: 0.04,
            shell_tolerance_m: 0.10,
            min_height_m: 0.08,
            max_height_m: 2.1,
            min_column_voxels: 1,
            min_footprint_m2: 0.08,
            min_object_height_m: 0.25,
            max_vertical_gap_m: 0.5,
            max_base_height_m: 1.0,
            min_box_fill: 0.8,
            edge_trim_fraction: 0.01,
            height_percentile: 0.95,
        }
    }
}

/// Everything found in one scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub schema_version: u32,
    pub scene_id: String,
    pub params: DetectParams,
    pub objects: Vec<DetectedObject>,
}

/// One piece of furniture as an upright box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedObject {
    pub id: String,
    pub floor_id: String,
    pub room_id: Option<String>,
    /// Matterport's room classification, e.g. "Living Room".
    pub room_type: Option<String>,
    /// What it probably is, from its size and room type.
    pub label: FurnitureLabel,
    /// Catalog asset closest in size, drawn in place of the real furniture.
    /// `None` only if the scene has no catalog.
    pub asset_id: Option<String>,
    /// Bottom centre of the box, on the floor.
    pub position_m: Vec3,
    /// Rotation about Z of the box's width (long) axis, from +X.
    pub yaw_degrees: f64,
    /// Measured size: width is the long side, depth the short side.
    pub size_m: Dimensions3,
    pub footprint_area_m2: f64,
}

/// Runs detection on every floor of `scene` that has rooms.
#[must_use]
pub fn detect(
    scene: &Scene,
    furnished: &Mesh,
    defurnished: &Mesh,
    params: DetectParams,
) -> Detection {
    let mut objects = Vec::new();
    for floor in &scene.floors {
        objects.extend(detect_floor(
            floor,
            &scene.assets,
            furnished,
            defurnished,
            params,
        ));
    }
    for (index, object) in objects.iter_mut().enumerate() {
        object.id = format!("detected-{:03}", index + 1);
    }

    Detection {
        schema_version: DETECTION_SCHEMA_VERSION,
        scene_id: scene.id.clone(),
        params,
        objects,
    }
}

/// The top-down furniture grid of one floor, for inspecting what the
/// detector saw. Row-major from `origin_m` (the south-west corner), `nx`
/// cells per row.
#[derive(Debug, Clone, PartialEq)]
pub struct FloorMask {
    pub floor_id: String,
    pub origin_m: [f64; 2],
    pub cell_m: f64,
    pub nx: usize,
    pub ny: usize,
    /// Furniture voxels counted in each column.
    pub counts: Vec<u32>,
    /// Whether each cell went into a box (after gap filling).
    pub occupied: Vec<bool>,
}

/// The intermediate top-down grids for every floor with rooms.
#[must_use]
pub fn debug_masks(
    scene: &Scene,
    furnished: &Mesh,
    defurnished: &Mesh,
    params: DetectParams,
) -> Vec<FloorMask> {
    scene
        .floors
        .iter()
        .filter_map(|floor| {
            let rooms = usable_rooms(floor);
            let columns = furniture_columns(floor, &rooms, furnished, defurnished, params)?;
            let occupied = columns.occupied(params);
            Some(FloorMask {
                floor_id: floor.id.clone(),
                origin_m: [columns.grid.origin[0], columns.grid.origin[1]],
                cell_m: columns.grid.voxel,
                nx: columns.grid.nx,
                ny: columns.grid.ny,
                counts: columns.counts,
                occupied,
            })
        })
        .collect()
}

fn usable_rooms(floor: &Floor) -> Vec<&Room> {
    floor
        .rooms
        .iter()
        .filter(|room| room.boundary.len() >= 3 && room_type(room) != Some("Void"))
        .collect()
}

/// Per top-down cell: how many furniture voxels its column holds, the top of
/// that furniture, and which room the cell lies in.
struct Columns {
    grid: GridSpec,
    counts: Vec<u32>,
    tops: Vec<f64>,
    room_of_cell: Vec<Option<usize>>,
}

impl Columns {
    fn occupied(&self, params: DetectParams) -> Vec<bool> {
        let occupied: Vec<bool> = self
            .counts
            .iter()
            .map(|&count| count >= params.min_column_voxels)
            .collect();
        // Opening drops isolated specks of scan noise; closing then fills
        // one-cell gaps inside real furniture.
        self.grid.close_2d(&self.grid.open_2d(&occupied))
    }
}

fn detect_floor(
    floor: &Floor,
    assets: &[FurnitureAsset],
    furnished: &Mesh,
    defurnished: &Mesh,
    params: DetectParams,
) -> Vec<DetectedObject> {
    let rooms = usable_rooms(floor);
    let Some(columns) = furniture_columns(floor, &rooms, furnished, defurnished, params) else {
        return Vec::new();
    };
    let closed = columns.occupied(params);
    let Columns {
        grid,
        tops,
        room_of_cell,
        ..
    } = columns;
    let floor_z = floor.floor_z();
    let voxel = params.voxel_m;

    let cell_corners = |cells: &[usize]| -> Vec<[f64; 2]> {
        let half = voxel / 2.0;
        cells
            .iter()
            .flat_map(|&cell| {
                let [x, y] = grid.cell_center(cell % grid.nx, cell / grid.nx);
                [
                    [x - half, y - half],
                    [x + half, y - half],
                    [x + half, y + half],
                    [x - half, y + half],
                ]
            })
            .collect()
    };

    let mut pieces = Vec::new();
    for component in grid.components_2d(&closed) {
        split_into_boxes(component, &grid, params, &mut pieces);
    }

    let mut objects = Vec::new();
    for piece in pieces {
        let area = piece.len() as f64 * voxel * voxel;
        let mut piece_tops: Vec<f64> = piece
            .iter()
            .map(|&cell| tops[cell])
            .filter(|top| top.is_finite())
            .collect();
        piece_tops.sort_by(f64::total_cmp);
        let top = percentile(&piece_tops, params.height_percentile);
        let height = top - floor_z;
        if area < params.min_footprint_m2
            || !height.is_finite()
            || height < params.min_object_height_m
        {
            continue;
        }
        let Some(rect) = min_area_rect(&cell_corners(&piece)).map(|rect| {
            trimmed_rect(
                &piece,
                rect,
                &grid,
                params.edge_trim_fraction,
                &cell_corners,
            )
        }) else {
            continue;
        };
        let component = piece;

        let room = rooms
            .iter()
            .find(|room| point_in_room(rect.center, room))
            .or_else(|| {
                // Fall back to the room most of the footprint lies in.
                let mut votes = vec![0_usize; rooms.len()];
                for &cell in &component {
                    if let Some(index) = room_of_cell[cell] {
                        votes[index] += 1;
                    }
                }
                votes
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, count)| **count)
                    .map(|(index, _)| &rooms[index])
            });
        let size = Dimensions3 {
            width_m: rect.length,
            depth_m: rect.width,
            height_m: height,
        };

        objects.push(DetectedObject {
            id: String::new(),
            floor_id: floor.id.clone(),
            room_id: room.map(|room| room.id.clone()),
            room_type: room.and_then(|room| room_type(room)).map(str::to_owned),
            label: classify(size, room.and_then(|room| room_type(room))),
            asset_id: nearest_asset(size, assets).map(|asset| asset.id.clone()),
            position_m: Vec3 {
                x: rect.center[0],
                y: rect.center[1],
                z: floor_z,
            },
            yaw_degrees: rect.angle_rad.to_degrees(),
            size_m: size,
            footprint_area_m2: area,
        });
    }

    // Stable, readable order: by room, then west to east.
    objects.sort_by(|a, b| {
        a.room_id
            .cmp(&b.room_id)
            .then(a.position_m.x.total_cmp(&b.position_m.x))
            .then(a.position_m.y.total_cmp(&b.position_m.y))
    });
    objects
}

/// Recursively cuts a footprint that fills its rectangle poorly (an L-shaped
/// sofa, a sofa beside a side table) into pieces that each fill theirs well,
/// so no box covers much open floor.
fn split_into_boxes(
    cells: Vec<usize>,
    grid: &GridSpec,
    params: DetectParams,
    out: &mut Vec<Vec<usize>>,
) {
    split_recursive(cells, grid, params, 0, out);
}

fn split_recursive(
    cells: Vec<usize>,
    grid: &GridSpec,
    params: DetectParams,
    depth: usize,
    out: &mut Vec<Vec<usize>>,
) {
    const MAX_DEPTH: usize = 4;
    /// A cut must shrink the total box area by at least this factor.
    const MIN_GAIN: f64 = 0.85;

    let voxel = grid.voxel;
    let cell_area = voxel * voxel;
    let min_cells = (params.min_footprint_m2 / cell_area).ceil() as usize;
    let centers: Vec<[f64; 2]> = cells
        .iter()
        .map(|&cell| grid.cell_center(cell % grid.nx, cell / grid.nx))
        .collect();
    let half = voxel / 2.0;
    let corners: Vec<[f64; 2]> = centers
        .iter()
        .flat_map(|&[x, y]| {
            [
                [x - half, y - half],
                [x + half, y - half],
                [x + half, y + half],
                [x - half, y + half],
            ]
        })
        .collect();
    let Some(rect) = min_area_rect(&corners) else {
        out.push(cells);
        return;
    };
    let rect_area = (rect.length * rect.width).max(cell_area);
    let fill = cells.len() as f64 * cell_area / rect_area;
    if depth >= MAX_DEPTH || fill >= params.min_box_fill || cells.len() < 2 * min_cells {
        out.push(cells);
        return;
    }

    // Try straight cuts across either axis of the rectangle; keep the one
    // whose two (rectangle-aligned) boxes have the least total area.
    let (sin, cos) = rect.angle_rad.sin_cos();
    let local: Vec<[f64; 2]> = centers
        .iter()
        .map(|&[x, y]| {
            let (dx, dy) = (x - rect.center[0], y - rect.center[1]);
            [dx * cos + dy * sin, -dx * sin + dy * cos]
        })
        .collect();
    let box_area = |points: &mut dyn Iterator<Item = &[f64; 2]>| {
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for point in points {
            bounds = [
                bounds[0].min(point[0]),
                bounds[1].min(point[1]),
                bounds[2].max(point[0]),
                bounds[3].max(point[1]),
            ];
        }
        (bounds[2] - bounds[0] + voxel) * (bounds[3] - bounds[1] + voxel)
    };

    let mut best: Option<(f64, usize, f64)> = None;
    for axis in 0..2 {
        let low = local
            .iter()
            .map(|point| point[axis])
            .fold(f64::INFINITY, f64::min);
        let high = local
            .iter()
            .map(|point| point[axis])
            .fold(f64::NEG_INFINITY, f64::max);
        let mut cut = low + voxel;
        while cut < high {
            let below = local.iter().filter(|point| point[axis] < cut).count();
            if below >= min_cells && local.len() - below >= min_cells {
                let total = box_area(&mut local.iter().filter(|point| point[axis] < cut))
                    + box_area(&mut local.iter().filter(|point| point[axis] >= cut));
                if best.is_none_or(|(best_total, _, _)| total < best_total) {
                    best = Some((total, axis, cut));
                }
            }
            cut += voxel;
        }
    }

    match best {
        Some((total, axis, cut)) if total <= MIN_GAIN * rect_area => {
            let (below, above): (Vec<_>, Vec<_>) = cells
                .iter()
                .zip(&local)
                .partition(|(_, point)| point[axis] < cut);
            split_recursive(
                below.into_iter().map(|(&cell, _)| cell).collect(),
                grid,
                params,
                depth + 1,
                out,
            );
            split_recursive(
                above.into_iter().map(|(&cell, _)| cell).collect(),
                grid,
                params,
                depth + 1,
                out,
            );
        }
        _ => out.push(cells),
    }
}

/// Voxelizes both meshes over the floor's rooms and counts, per top-down
/// cell, the furniture voxels below the first large vertical gap.
fn furniture_columns(
    floor: &Floor,
    rooms: &[&Room],
    furnished: &Mesh,
    defurnished: &Mesh,
    params: DetectParams,
) -> Option<Columns> {
    let bounds = rooms_bounds(rooms)?;
    let floor_z = floor.floor_z();
    let voxel = params.voxel_m;
    let shell_radius = (params.shell_tolerance_m / voxel).ceil() as usize;
    let pad = shell_radius as f64 * voxel;
    let grid = GridSpec::new(
        [
            bounds[0] - pad,
            bounds[1] - pad,
            floor_z + params.min_height_m - pad,
        ],
        [
            bounds[2] + pad,
            bounds[3] + pad,
            floor_z + params.max_height_m + pad,
        ],
        voxel,
    );

    let mut shell = grid.voxelize(defurnished);
    grid.dilate(&mut shell, shell_radius);
    let furnished = grid.voxelize(furnished);

    // Top-down columns: furniture voxel count and top height per cell.
    let columns = grid.nx * grid.ny;
    let mut counts = vec![0_u32; columns];
    let mut tops = vec![f64::NEG_INFINITY; columns];
    let room_of_cell: Vec<Option<usize>> = (0..columns)
        .map(|cell| {
            let center = grid.cell_center(cell % grid.nx, cell / grid.nx);
            rooms.iter().position(|room| point_in_room(center, room))
        })
        .collect();
    // Walk each column upward, stopping at the first large gap: whatever
    // hangs above it is overhead, not part of the obstacle below.
    let mut overhead = vec![false; columns];
    for iz in 0..grid.nz {
        let z = grid.origin[2] + (iz as f64 + 0.5) * voxel;
        if z < floor_z + params.min_height_m || z > floor_z + params.max_height_m {
            continue;
        }
        for cell in 0..columns {
            let index = iz * columns + cell;
            if overhead[cell] || !furnished[index] || shell[index] || room_of_cell[cell].is_none() {
                continue;
            }
            let base_too_high = counts[cell] == 0 && z - floor_z > params.max_base_height_m;
            let gap_too_large = counts[cell] > 0 && z - tops[cell] > params.max_vertical_gap_m;
            if base_too_high || gap_too_large {
                overhead[cell] = true;
                continue;
            }
            counts[cell] += 1;
            tops[cell] = tops[cell].max(z + voxel / 2.0);
        }
    }

    Some(Columns {
        grid,
        counts,
        tops,
        room_of_cell,
    })
}

/// Refits `rect` without the cells lying beyond the `fraction` quantiles
/// along either of its axes.
fn trimmed_rect(
    cells: &[usize],
    rect: OrientedRect,
    grid: &GridSpec,
    fraction: f64,
    cell_corners: &dyn Fn(&[usize]) -> Vec<[f64; 2]>,
) -> OrientedRect {
    if fraction <= 0.0 {
        return rect;
    }
    let (sin, cos) = rect.angle_rad.sin_cos();
    let local: Vec<[f64; 2]> = cells
        .iter()
        .map(|&cell| {
            let [x, y] = grid.cell_center(cell % grid.nx, cell / grid.nx);
            let (dx, dy) = (x - rect.center[0], y - rect.center[1]);
            [dx * cos + dy * sin, -dx * sin + dy * cos]
        })
        .collect();
    let bounds = |axis: usize| {
        let mut values: Vec<f64> = local.iter().map(|point| point[axis]).collect();
        values.sort_by(f64::total_cmp);
        (
            percentile(&values, fraction),
            percentile(&values, 1.0 - fraction),
        )
    };
    let (u, v) = (bounds(0), bounds(1));
    let kept: Vec<usize> = cells
        .iter()
        .zip(&local)
        .filter(|(_, point)| (u.0..=u.1).contains(&point[0]) && (v.0..=v.1).contains(&point[1]))
        .map(|(&cell, _)| cell)
        .collect();
    min_area_rect(&cell_corners(&kept)).unwrap_or(rect)
}

/// Nearest-rank percentile of sorted `values`; `-inf` if empty.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NEG_INFINITY;
    }
    let rank = (fraction.clamp(0.0, 1.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank]
}

fn room_type(room: &Room) -> Option<&str> {
    room.classifications
        .first()
        .map(|classification| classification.label.as_str())
}

/// `[min_x, min_y, max_x, max_y]` over every room boundary.
fn rooms_bounds(rooms: &[&Room]) -> Option<[f64; 4]> {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for edge in rooms.iter().flat_map(|room| &room.boundary) {
        for point in [edge.start, edge.end] {
            bounds = [
                bounds[0].min(point.x),
                bounds[1].min(point.y),
                bounds[2].max(point.x),
                bounds[3].max(point.y),
            ];
        }
    }
    bounds[0].is_finite().then_some(bounds)
}

/// Even-odd test against the room's boundary edges, which need not be ordered.
fn point_in_room(point: [f64; 2], room: &Room) -> bool {
    let mut inside = false;
    for edge in &room.boundary {
        let (start, end) = (edge.start, edge.end);
        if (start.y > point[1]) != (end.y > point[1]) {
            let crossing_x = (end.x - start.x) * (point[1] - start.y) / (end.y - start.y) + start.x;
            if point[0] < crossing_x {
                inside = !inside;
            }
        }
    }
    inside
}

/// A dense axis-aligned voxel grid. Index = (z * ny + y) * nx + x; the first
/// `nx * ny` indices double as the top-down grid.
struct GridSpec {
    origin: [f64; 3],
    voxel: f64,
    nx: usize,
    ny: usize,
    nz: usize,
}

impl GridSpec {
    fn new(min: [f64; 3], max: [f64; 3], voxel: f64) -> Self {
        let count = |axis: usize| ((max[axis] - min[axis]) / voxel).ceil().max(1.0) as usize;
        Self {
            origin: min,
            voxel,
            nx: count(0),
            ny: count(1),
            nz: count(2),
        }
    }

    fn cell_center(&self, ix: usize, iy: usize) -> [f64; 2] {
        [
            self.origin[0] + (ix as f64 + 0.5) * self.voxel,
            self.origin[1] + (iy as f64 + 0.5) * self.voxel,
        ]
    }

    fn voxel_index(&self, point: [f64; 3]) -> Option<usize> {
        let mut cell = [0_usize; 3];
        let dims = [self.nx, self.ny, self.nz];
        for axis in 0..3 {
            let offset = ((point[axis] - self.origin[axis]) / self.voxel).floor();
            if offset < 0.0 || offset >= dims[axis] as f64 {
                return None;
            }
            cell[axis] = offset as usize;
        }
        Some((cell[2] * self.ny + cell[1]) * self.nx + cell[0])
    }

    /// Marks every voxel a triangle passes through, by sampling each triangle
    /// at half-voxel spacing.
    fn voxelize(&self, mesh: &Mesh) -> Vec<bool> {
        let mut grid = vec![false; self.nx * self.ny * self.nz];
        let z_min = self.origin[2];
        let z_max = self.origin[2] + self.nz as f64 * self.voxel;
        let step = self.voxel / 2.0;

        for triangle in &mesh.triangles {
            let [a, b, c] = triangle.map(|index| mesh.positions[index]);
            let low = a[2].min(b[2]).min(c[2]);
            let high = a[2].max(b[2]).max(c[2]);
            if high < z_min || low > z_max {
                continue;
            }
            let longest = distance(a, b).max(distance(b, c)).max(distance(c, a));
            let samples = (longest / step).ceil().max(1.0) as usize;
            for i in 0..=samples {
                for j in 0..=samples - i {
                    let (u, v) = (i as f64 / samples as f64, j as f64 / samples as f64);
                    let point = [0, 1, 2]
                        .map(|axis| a[axis] + (b[axis] - a[axis]) * u + (c[axis] - a[axis]) * v);
                    if let Some(index) = self.voxel_index(point) {
                        grid[index] = true;
                    }
                }
            }
        }
        grid
    }

    /// Grows marked voxels by `radius` along each axis (a cube structuring
    /// element), one separable pass per axis.
    fn dilate(&self, grid: &mut [bool], radius: usize) {
        if radius == 0 {
            return;
        }
        let dims = [self.nx, self.ny, self.nz];
        let strides = [1, self.nx, self.nx * self.ny];
        for axis in 0..3 {
            let source = grid.to_vec();
            for (index, &set) in source.iter().enumerate() {
                if !set {
                    continue;
                }
                let position = (index / strides[axis]) % dims[axis];
                let start = position.saturating_sub(radius);
                let end = (position + radius).min(dims[axis] - 1);
                for neighbor in start..=end {
                    grid[index - position * strides[axis] + neighbor * strides[axis]] = true;
                }
            }
        }
    }

    /// Morphological closing with a 3×3 square: fills one-cell gaps and holes
    /// left by sparse scan surfaces.
    /// Morphological opening with a 3×3 square: removes specks and one-cell
    /// slivers, keeps everything wider.
    fn open_2d(&self, mask: &[bool]) -> Vec<bool> {
        let shrunk = self.neighborhood_2d(mask, false);
        self.neighborhood_2d(&shrunk, true)
    }

    fn close_2d(&self, mask: &[bool]) -> Vec<bool> {
        let grown = self.neighborhood_2d(mask, true);
        self.neighborhood_2d(&grown, false)
    }

    /// `any == true` dilates (any neighbor set); `false` erodes (all set).
    fn neighborhood_2d(&self, mask: &[bool], any: bool) -> Vec<bool> {
        let (nx, ny) = (self.nx as isize, self.ny as isize);
        (0..mask.len())
            .map(|cell| {
                let (x, y) = ((cell % self.nx) as isize, (cell / self.nx) as isize);
                let mut neighbors = (-1..=1)
                    .flat_map(|dy| (-1..=1).map(move |dx| (x + dx, y + dy)))
                    .map(|(nx_, ny_)| {
                        nx_ >= 0
                            && ny_ >= 0
                            && nx_ < nx
                            && ny_ < ny
                            && mask[(ny_ * nx + nx_) as usize]
                    });
                if any {
                    neighbors.any(|set| set)
                } else {
                    neighbors.all(|set| set)
                }
            })
            .collect()
    }

    /// 8-connected components of the top-down mask, as lists of cell indices.
    fn components_2d(&self, mask: &[bool]) -> Vec<Vec<usize>> {
        let mut seen = vec![false; mask.len()];
        let mut components = Vec::new();
        for start in 0..mask.len() {
            if !mask[start] || seen[start] {
                continue;
            }
            seen[start] = true;
            let mut stack = vec![start];
            let mut component = Vec::new();
            while let Some(cell) = stack.pop() {
                component.push(cell);
                let (x, y) = (cell % self.nx, cell / self.nx);
                for ny_ in y.saturating_sub(1)..=(y + 1).min(self.ny - 1) {
                    for nx_ in x.saturating_sub(1)..=(x + 1).min(self.nx - 1) {
                        let neighbor = ny_ * self.nx + nx_;
                        if mask[neighbor] && !seen[neighbor] {
                            seen[neighbor] = true;
                            stack.push(neighbor);
                        }
                    }
                }
            }
            components.push(component);
        }
        components
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tflop_world_core::{
        BoundaryEdge, BoundaryKind, RoomClassification, SCENE_SCHEMA_VERSION, SceneSource,
        default_furniture_catalog,
    };

    /// Adds an axis-aligned box's six faces to `mesh`.
    fn add_box(mesh: &mut Mesh, min: [f64; 3], max: [f64; 3]) {
        let base = mesh.positions.len();
        for corner in 0..8 {
            mesh.positions.push([
                if corner & 1 == 0 { min[0] } else { max[0] },
                if corner & 2 == 0 { min[1] } else { max[1] },
                if corner & 4 == 0 { min[2] } else { max[2] },
            ]);
        }
        for [a, b, c, d] in [
            [0, 1, 3, 2],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
            [0, 2, 6, 4],
            [1, 3, 7, 5],
        ] {
            mesh.triangles.push([base + a, base + b, base + c]);
            mesh.triangles.push([base + a, base + c, base + d]);
        }
    }

    fn room(width: f64, depth: f64) -> Room {
        let corners = [[0.0, 0.0], [width, 0.0], [width, depth], [0.0, depth]];
        Room {
            id: "room-1".into(),
            label: None,
            classifications: vec![RoomClassification {
                label: "Living Room".into(),
                confidence: None,
            }],
            dimensions: None,
            boundary: (0..4)
                .map(|index| {
                    let [sx, sy] = corners[index];
                    let [ex, ey] = corners[(index + 1) % 4];
                    BoundaryEdge {
                        id: format!("edge-{index}"),
                        kind: BoundaryKind::Wall,
                        start: Vec3 {
                            x: sx,
                            y: sy,
                            z: 0.0,
                        },
                        end: Vec3 {
                            x: ex,
                            y: ey,
                            z: 0.0,
                        },
                        thickness_m: None,
                        center_line_bias: None,
                        openings: Vec::new(),
                    }
                })
                .collect(),
        }
    }

    fn scene() -> Scene {
        Scene {
            schema_version: SCENE_SCHEMA_VERSION,
            id: "test".into(),
            name: None,
            source: SceneSource {
                provider: "test".into(),
                external_id: "test".into(),
            },
            assets: default_furniture_catalog(),
            floors: vec![Floor {
                id: "floor-1".into(),
                label: None,
                elevation_m: Some(0.0),
                rooms: vec![room(6.0, 5.0)],
                objects: Vec::new(),
            }],
        }
    }

    #[test]
    fn finds_a_sofa_that_only_the_furnished_mesh_contains() {
        let mut shell = Mesh::default();
        add_box(&mut shell, [0.0, 0.0, -0.05], [6.0, 5.0, 0.0]); // floor slab
        add_box(&mut shell, [-0.1, 0.0, 0.0], [0.0, 5.0, 2.6]); // west wall
        add_box(&mut shell, [0.0, 5.0, 0.0], [6.0, 5.1, 2.6]); // north wall

        let mut furnished = shell.clone();
        add_box(&mut furnished, [1.0, 1.0, 0.0], [3.0, 1.9, 0.85]);

        let detection = detect(&scene(), &furnished, &shell, DetectParams::default());
        assert_eq!(detection.objects.len(), 1, "{detection:#?}");
        let sofa = &detection.objects[0];
        assert_eq!(sofa.asset_id.as_deref(), Some("sofa-basic"));
        assert_eq!(sofa.room_type.as_deref(), Some("Living Room"));
        assert!((sofa.position_m.x - 2.0).abs() < 0.06, "{sofa:?}");
        assert!((sofa.position_m.y - 1.45).abs() < 0.06, "{sofa:?}");
        assert!((sofa.size_m.width_m - 2.0).abs() < 0.1, "{sofa:?}");
        assert!((sofa.size_m.depth_m - 0.9).abs() < 0.1, "{sofa:?}");
        assert!((sofa.size_m.height_m - 0.85).abs() < 0.06, "{sofa:?}");
        assert!(sofa.yaw_degrees.abs() < 1.0, "{sofa:?}");
    }

    fn shell() -> Mesh {
        let mut shell = Mesh::default();
        add_box(&mut shell, [0.0, 0.0, -0.05], [6.0, 5.0, 0.0]);
        shell
    }

    #[test]
    fn ignores_a_chandelier_hanging_over_a_table() {
        let mut furnished = shell();
        add_box(&mut furnished, [1.0, 1.0, 0.72], [3.0, 2.0, 0.76]); // table top
        add_box(&mut furnished, [1.8, 1.3, 1.6], [2.2, 1.7, 2.0]); // chandelier
        add_box(&mut furnished, [4.5, 3.5, 1.7], [4.8, 3.8, 2.0]); // pendant over open floor

        let detection = detect(&scene(), &furnished, &shell(), DetectParams::default());
        assert_eq!(detection.objects.len(), 1, "{detection:#?}");
        let table = &detection.objects[0];
        assert!((table.size_m.height_m - 0.76).abs() < 0.05, "{table:?}");
    }

    /// Adds one horizontal rectangle (two triangles) at height `z`.
    fn add_quad(mesh: &mut Mesh, min: [f64; 2], max: [f64; 2], z: f64) {
        let base = mesh.positions.len();
        mesh.positions.extend([
            [min[0], min[1], z],
            [max[0], min[1], z],
            [max[0], max[1], z],
            [min[0], max[1], z],
        ]);
        mesh.triangles
            .extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
    }

    #[test]
    fn counts_a_flat_top_with_no_sides_across_its_whole_footprint() {
        let mut furnished = shell();
        add_quad(&mut furnished, [1.0, 1.0], [3.0, 2.6], 0.55); // a bed's top surface

        let detection = detect(&scene(), &furnished, &shell(), DetectParams::default());
        assert_eq!(detection.objects.len(), 1, "{detection:#?}");
        let bed = &detection.objects[0];
        assert!((bed.size_m.width_m - 2.0).abs() < 0.1, "{bed:?}");
        assert!((bed.size_m.depth_m - 1.6).abs() < 0.1, "{bed:?}");
        let fill = bed.footprint_area_m2 / (bed.size_m.width_m * bed.size_m.depth_m);
        assert!(fill > 0.9, "only {fill:.2} of the bed's box is covered");
    }

    #[test]
    fn a_lamp_on_a_table_does_not_set_its_height() {
        let mut furnished = shell();
        add_box(&mut furnished, [1.0, 1.0, 0.0], [3.0, 2.0, 0.75]);
        add_box(&mut furnished, [2.7, 1.7, 0.75], [2.9, 1.9, 1.4]);

        let detection = detect(&scene(), &furnished, &shell(), DetectParams::default());
        assert_eq!(detection.objects.len(), 1, "{detection:#?}");
        let table = &detection.objects[0];
        assert!((table.size_m.height_m - 0.75).abs() < 0.05, "{table:?}");
    }

    #[test]
    fn a_thin_strip_does_not_stretch_the_box() {
        let mut furnished = shell();
        add_box(&mut furnished, [1.0, 1.0, 0.0], [3.0, 1.9, 0.85]);
        add_box(&mut furnished, [3.0, 1.4, 0.0], [3.6, 1.43, 0.3]); // a cable-thin strip

        let detection = detect(&scene(), &furnished, &shell(), DetectParams::default());
        assert_eq!(detection.objects.len(), 1, "{detection:#?}");
        let sofa = &detection.objects[0];
        assert!((sofa.size_m.width_m - 2.0).abs() < 0.1, "{sofa:?}");
    }

    #[test]
    fn splits_an_l_shaped_sofa_into_two_boxes() {
        let mut furnished = shell();
        add_box(&mut furnished, [1.0, 1.0, 0.0], [4.0, 1.9, 0.85]);
        add_box(&mut furnished, [3.1, 1.9, 0.0], [4.0, 4.0, 0.85]);

        let detection = detect(&scene(), &furnished, &shell(), DetectParams::default());
        assert_eq!(detection.objects.len(), 2, "{detection:#?}");
        let covered: f64 = detection
            .objects
            .iter()
            .map(|o| o.size_m.width_m * o.size_m.depth_m)
            .sum();
        let actual = 3.0 * 0.9 + 0.9 * 2.1;
        assert!(
            covered < actual * 1.2,
            "boxes cover {covered} m² for {actual} m² of sofa"
        );
    }

    #[test]
    fn ignores_the_room_shell_and_flat_rugs() {
        let mut shell = Mesh::default();
        add_box(&mut shell, [0.0, 0.0, -0.05], [6.0, 5.0, 0.0]);
        add_box(&mut shell, [-0.1, 0.0, 0.0], [0.0, 5.0, 2.6]);

        let mut furnished = shell.clone();
        add_box(&mut furnished, [2.0, 2.0, 0.0], [4.0, 3.5, 0.02]); // rug
        add_box(&mut furnished, [0.0, 1.0, 1.2], [0.03, 2.0, 1.8]); // picture on the wall

        let detection = detect(&scene(), &furnished, &shell, DetectParams::default());
        assert!(detection.objects.is_empty(), "{detection:#?}");
    }
}
