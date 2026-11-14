use serde::{Deserialize, Serialize};
use tflop_world_core::{RoomDimensions, Scene, SceneValidationErrors};
use thiserror::Error;

const DEFAULT_ROOM_HEIGHT_M: f64 = 2.7;
const MIN_GEOMETRY_EXTENT_M: f64 = 1.0e-6;

/// A CFAST-compatible copy of a canonical scene plus every approximation made
/// while preparing incomplete scan data. The source scene is never mutated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedCfastScene {
    pub scene: Scene,
    pub report: CfastPreparationReport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CfastPreparationReport {
    pub original_room_count: usize,
    pub included_room_count: usize,
    pub warnings: Vec<CfastPreparationWarning>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CfastPreparationWarning {
    RoomExcluded {
        floor_id: String,
        room_id: String,
        reason: RoomExclusionReason,
    },
    RoomHeightInferred {
        floor_id: String,
        room_id: String,
        height_m: f64,
        source: HeightInferenceSource,
    },
    RoomWidthInferredFromBoundary {
        floor_id: String,
        room_id: String,
        width_m: f64,
    },
    RoomDepthInferredFromBoundary {
        floor_id: String,
        room_id: String,
        depth_m: f64,
    },
    RoomLowerElevationInferred {
        floor_id: String,
        room_id: String,
        lower_elevation_m: f64,
    },
    OpeningBottomAdjusted {
        room_id: String,
        opening_id: String,
        original_bottom_m: f64,
        adjusted_bottom_m: f64,
    },
    OpeningBottomInferred {
        room_id: String,
        opening_id: String,
        bottom_m: f64,
    },
    OpeningHeightClamped {
        room_id: String,
        opening_id: String,
        original_height_m: f64,
        adjusted_height_m: f64,
    },
    OpeningHeightInferred {
        room_id: String,
        opening_id: String,
        height_m: f64,
    },
    OpeningExcluded {
        room_id: String,
        opening_id: String,
        reason: OpeningExclusionReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomExclusionReason {
    MissingBoundary,
    InsufficientBoundaryGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningExclusionReason {
    MissingWidth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeightInferenceSource {
    FloorMedian,
    SceneMedian,
    GenericDefault,
}

#[derive(Debug, Error)]
pub enum CfastPreparationError {
    #[error(transparent)]
    InvalidScene(#[from] SceneValidationErrors),
    #[error("the Matterport scene has no rooms with usable boundary geometry")]
    NoUsableRooms,
}

/// Makes a best-effort CFAST scene while retaining a machine-readable audit
/// trail of every inferred, adjusted, or excluded piece of scan data.
pub fn prepare_cfast_scene(scene: &Scene) -> Result<PreparedCfastScene, CfastPreparationError> {
    scene.validate()?;
    let original_room_count = room_count(scene);
    let scene_height = median(measured_heights(scene));
    let mut prepared = scene.clone();
    let mut warnings = Vec::new();

    for floor in &mut prepared.floors {
        let floor_height = median(
            floor
                .rooms
                .iter()
                .filter_map(|room| positive_height(room.dimensions.as_ref()?.height_m)),
        );
        floor.rooms.retain(|room| {
            let reason = unusable_boundary_reason(room);
            if let Some(reason) = reason {
                warnings.push(CfastPreparationWarning::RoomExcluded {
                    floor_id: floor.id.clone(),
                    room_id: room.id.clone(),
                    reason,
                });
                false
            } else {
                true
            }
        });

        for room in &mut floor.rooms {
            let (boundary_width_m, boundary_depth_m, boundary_elevation_m) =
                boundary_dimensions(room);
            let (height_m, source) = if let Some(height_m) = room
                .dimensions
                .as_ref()
                .and_then(|dimensions| positive_height(dimensions.height_m))
            {
                (height_m, None)
            } else if let Some(height_m) = floor_height {
                (height_m, Some(HeightInferenceSource::FloorMedian))
            } else if let Some(height_m) = scene_height {
                (height_m, Some(HeightInferenceSource::SceneMedian))
            } else {
                (
                    DEFAULT_ROOM_HEIGHT_M,
                    Some(HeightInferenceSource::GenericDefault),
                )
            };

            let dimensions = room.dimensions.get_or_insert(RoomDimensions {
                area_m2: None,
                width_m: None,
                depth_m: None,
                height_m: None,
                lower_elevation_m: floor.elevation_m,
            });
            dimensions.height_m = Some(height_m);
            if let Some(source) = source {
                warnings.push(CfastPreparationWarning::RoomHeightInferred {
                    floor_id: floor.id.clone(),
                    room_id: room.id.clone(),
                    height_m,
                    source,
                });
            }

            if positive_length(dimensions.width_m).is_none() {
                dimensions.width_m = Some(boundary_width_m);
                warnings.push(CfastPreparationWarning::RoomWidthInferredFromBoundary {
                    floor_id: floor.id.clone(),
                    room_id: room.id.clone(),
                    width_m: boundary_width_m,
                });
            }
            if positive_length(dimensions.depth_m).is_none() {
                dimensions.depth_m = Some(boundary_depth_m);
                warnings.push(CfastPreparationWarning::RoomDepthInferredFromBoundary {
                    floor_id: floor.id.clone(),
                    room_id: room.id.clone(),
                    depth_m: boundary_depth_m,
                });
            }
            if dimensions.lower_elevation_m.is_none() {
                let lower_elevation_m = floor.elevation_m.unwrap_or(boundary_elevation_m);
                dimensions.lower_elevation_m = Some(lower_elevation_m);
                warnings.push(CfastPreparationWarning::RoomLowerElevationInferred {
                    floor_id: floor.id.clone(),
                    room_id: room.id.clone(),
                    lower_elevation_m,
                });
            }

            let floor_elevation_m = dimensions
                .lower_elevation_m
                .or(floor.elevation_m)
                .or_else(|| room.boundary.first().map(|edge| edge.start.z))
                .unwrap_or(0.0);
            normalize_openings(room, floor_elevation_m, height_m, &mut warnings);
        }
    }

    let included_room_count = room_count(&prepared);
    if included_room_count == 0 {
        return Err(CfastPreparationError::NoUsableRooms);
    }
    prepared.validate()?;

    Ok(PreparedCfastScene {
        scene: prepared,
        report: CfastPreparationReport {
            original_room_count,
            included_room_count,
            warnings,
        },
    })
}

fn normalize_openings(
    room: &mut tflop_world_core::Room,
    floor_elevation_m: f64,
    room_height_m: f64,
    warnings: &mut Vec<CfastPreparationWarning>,
) {
    for edge in &mut room.boundary {
        edge.openings.retain(|opening| {
            if opening.width_m > 0.0 {
                true
            } else {
                warnings.push(CfastPreparationWarning::OpeningExcluded {
                    room_id: room.id.clone(),
                    opening_id: opening.id.clone(),
                    reason: OpeningExclusionReason::MissingWidth,
                });
                false
            }
        });
        for opening in &mut edge.openings {
            let original_bottom_m = opening
                .lower_elevation_m
                .map_or(0.0, |elevation| elevation - floor_elevation_m);
            let adjusted_bottom_m = if original_bottom_m < 0.0 || original_bottom_m >= room_height_m
            {
                0.0
            } else {
                original_bottom_m
            };
            if opening.lower_elevation_m.is_none() {
                warnings.push(CfastPreparationWarning::OpeningBottomInferred {
                    room_id: room.id.clone(),
                    opening_id: opening.id.clone(),
                    bottom_m: adjusted_bottom_m,
                });
                opening.lower_elevation_m = Some(floor_elevation_m + adjusted_bottom_m);
            } else if adjusted_bottom_m != original_bottom_m {
                warnings.push(CfastPreparationWarning::OpeningBottomAdjusted {
                    room_id: room.id.clone(),
                    opening_id: opening.id.clone(),
                    original_bottom_m,
                    adjusted_bottom_m,
                });
                opening.lower_elevation_m = Some(floor_elevation_m + adjusted_bottom_m);
            }

            let available_height_m = (room_height_m - adjusted_bottom_m).max(0.0);
            if let Some(original_height_m) = opening.height_m.filter(|height| *height > 0.0) {
                let adjusted_height_m = original_height_m.min(available_height_m);
                if adjusted_height_m != original_height_m {
                    warnings.push(CfastPreparationWarning::OpeningHeightClamped {
                        room_id: room.id.clone(),
                        opening_id: opening.id.clone(),
                        original_height_m,
                        adjusted_height_m,
                    });
                    opening.height_m = Some(adjusted_height_m);
                }
            } else {
                warnings.push(CfastPreparationWarning::OpeningHeightInferred {
                    room_id: room.id.clone(),
                    opening_id: opening.id.clone(),
                    height_m: available_height_m,
                });
                opening.height_m = Some(available_height_m);
            }
        }
    }
}

fn unusable_boundary_reason(room: &tflop_world_core::Room) -> Option<RoomExclusionReason> {
    if room.boundary.is_empty() {
        return Some(RoomExclusionReason::MissingBoundary);
    }
    if room.boundary.len() < 3 {
        return Some(RoomExclusionReason::InsufficientBoundaryGeometry);
    }
    let min_x = room
        .boundary
        .iter()
        .flat_map(|edge| [edge.start.x, edge.end.x])
        .reduce(f64::min)?;
    let max_x = room
        .boundary
        .iter()
        .flat_map(|edge| [edge.start.x, edge.end.x])
        .reduce(f64::max)?;
    let min_y = room
        .boundary
        .iter()
        .flat_map(|edge| [edge.start.y, edge.end.y])
        .reduce(f64::min)?;
    let max_y = room
        .boundary
        .iter()
        .flat_map(|edge| [edge.start.y, edge.end.y])
        .reduce(f64::max)?;
    ((max_x - min_x) <= MIN_GEOMETRY_EXTENT_M || (max_y - min_y) <= MIN_GEOMETRY_EXTENT_M)
        .then_some(RoomExclusionReason::InsufficientBoundaryGeometry)
}

fn measured_heights(scene: &Scene) -> impl Iterator<Item = f64> + '_ {
    scene
        .floors
        .iter()
        .flat_map(|floor| &floor.rooms)
        .filter_map(|room| positive_height(room.dimensions.as_ref()?.height_m))
}

fn positive_height(height_m: Option<f64>) -> Option<f64> {
    height_m.filter(|height| height.is_finite() && *height > 0.0)
}

fn positive_length(length_m: Option<f64>) -> Option<f64> {
    length_m.filter(|length| length.is_finite() && *length > 0.0)
}

fn boundary_dimensions(room: &tflop_world_core::Room) -> (f64, f64, f64) {
    let points = room
        .boundary
        .iter()
        .flat_map(|edge| [edge.start, edge.end])
        .collect::<Vec<_>>();
    let min_x = points.iter().map(|point| point.x).reduce(f64::min).unwrap();
    let max_x = points.iter().map(|point| point.x).reduce(f64::max).unwrap();
    let min_y = points.iter().map(|point| point.y).reduce(f64::min).unwrap();
    let max_y = points.iter().map(|point| point.y).reduce(f64::max).unwrap();
    let min_z = points.iter().map(|point| point.z).reduce(f64::min).unwrap();
    (max_x - min_x, max_y - min_y, min_z)
}

fn median(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values = values.collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    let midpoint = values.len() / 2;
    match values.len() {
        0 => None,
        length if length % 2 == 0 => Some((values[midpoint - 1] + values[midpoint]) / 2.0),
        _ => Some(values[midpoint]),
    }
}

fn room_count(scene: &Scene) -> usize {
    scene.floors.iter().map(|floor| floor.rooms.len()).sum()
}
