use std::collections::{BTreeMap, HashMap, HashSet};

use tflop_simulation_core::{SURFACE_FIRE_SCHEMA_VERSION, SurfaceFireFrame, SurfaceFireNodeFrame};
use tflop_world_core::Vec3;
use thiserror::Error;

use crate::{CFAST_MAX_CURVE_POINTS, CfastFire, CfastFuel, CfastRoomFire, HeatReleasePoint};

const HRR_EPSILON_KW: f64 = 1.0e-6;
const DEFAULT_MINIMUM_FIRE_AREA_M2: f64 = 0.01;

/// Controls assumptions that cannot be inferred from spatial fire frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceFireAdapterConfig {
    pub fuel: CfastFuel,
    /// Keeps a small fire valid even when its source cell has near-zero area.
    pub minimum_fire_area_m2: f64,
}

impl Default for SurfaceFireAdapterConfig {
    fn default() -> Self {
        Self {
            fuel: CfastFuel::default(),
            minimum_fire_area_m2: DEFAULT_MINIMUM_FIRE_AREA_M2,
        }
    }
}

/// Room-level CFAST inputs derived from a complete Rust surface-fire run.
#[derive(Debug, Clone, PartialEq)]
pub struct CfastSurfaceFireTimeline {
    pub scene_id: String,
    pub duration_seconds: f64,
    pub room_fires: Vec<CfastRoomFire>,
}

/// Aggregates spatial Rust fire cells into one prescribed CFAST fire per room.
///
/// CFAST fire locations are fixed, so each room uses the HRR-weighted center
/// of all burning cells over the timeline. Its curve still preserves every
/// input frame's changing room HRR.
pub fn adapt_surface_fire_for_cfast(
    frames: &[SurfaceFireFrame],
    config: SurfaceFireAdapterConfig,
) -> Result<CfastSurfaceFireTimeline, SurfaceFireAdapterError> {
    validate_config(config)?;
    let (scene_id, duration_seconds) = validate_frames(frames)?;

    let room_ids = frames
        .iter()
        .flat_map(|frame| frame.active_nodes.iter())
        .filter(|node| node.heat_release_rate_kw > HRR_EPSILON_KW)
        .map(|node| node.room_id.as_str())
        .collect::<HashSet<_>>();
    if room_ids.is_empty() {
        return Err(SurfaceFireAdapterError::NoBurningCells);
    }

    let mut room_fires = room_ids
        .into_iter()
        .map(|room_id| build_room_fire(room_id, frames, config))
        .collect::<Vec<_>>();
    room_fires.sort_by(|left, right| left.room_id.cmp(&right.room_id));

    Ok(CfastSurfaceFireTimeline {
        scene_id: scene_id.to_owned(),
        duration_seconds,
        room_fires,
    })
}

fn build_room_fire(
    room_id: &str,
    frames: &[SurfaceFireFrame],
    config: SurfaceFireAdapterConfig,
) -> CfastRoomFire {
    let mut weighted_position = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let mut total_position_weight = 0.0;
    let mut peak_area_m2: f64 = 0.0;
    let mut curve = Vec::with_capacity(frames.len());

    for frame in frames {
        let burning_nodes = frame
            .active_nodes
            .iter()
            .filter(|node| node.room_id == room_id && node.heat_release_rate_kw > HRR_EPSILON_KW)
            .collect::<Vec<_>>();
        let hrr_kw = burning_nodes
            .iter()
            .map(|node| node.heat_release_rate_kw)
            .sum::<f64>();
        let area_m2 = burning_nodes
            .iter()
            .map(|node| node.dimensions_m.width_m * node.dimensions_m.depth_m)
            .sum::<f64>();
        peak_area_m2 = peak_area_m2.max(area_m2);

        for node in burning_nodes {
            let weight = node.heat_release_rate_kw;
            weighted_position.x += node.position_m.x * weight;
            weighted_position.y += node.position_m.y * weight;
            weighted_position.z += node.position_m.z * weight;
            total_position_weight += weight;
        }
        curve.push(HeatReleasePoint {
            time_seconds: frame.time_seconds,
            hrr_kw,
        });
    }

    debug_assert!(total_position_weight > 0.0);
    weighted_position.x /= total_position_weight;
    weighted_position.y /= total_position_weight;
    weighted_position.z /= total_position_weight;

    CfastRoomFire {
        room_id: room_id.to_owned(),
        fire: CfastFire {
            id: format!("surface-fire:{room_id}"),
            position_m: weighted_position,
            area_m2: peak_area_m2.max(config.minimum_fire_area_m2),
            fuel: config.fuel,
            heat_release_curve: thin_curve(curve, CFAST_MAX_CURVE_POINTS),
        },
    }
}

/// Keeps a heat-release curve within `max_points`, as CFAST requires. The
/// first and last points stay; the rest are split into equal runs, and each
/// run keeps its hottest point, so peaks survive thinning.
fn thin_curve(curve: Vec<HeatReleasePoint>, max_points: usize) -> Vec<HeatReleasePoint> {
    if curve.len() <= max_points || max_points < 3 {
        return curve;
    }
    let interior = &curve[1..curve.len() - 1];
    let runs = max_points - 2;
    let mut thinned = Vec::with_capacity(max_points);
    thinned.push(curve[0]);
    for run in 0..runs {
        let start = run * interior.len() / runs;
        let end = (run + 1) * interior.len() / runs;
        if let Some(hottest) = interior[start..end]
            .iter()
            .copied()
            .reduce(|hottest, point| {
                if point.hrr_kw > hottest.hrr_kw {
                    point
                } else {
                    hottest
                }
            })
        {
            thinned.push(hottest);
        }
    }
    thinned.push(curve[curve.len() - 1]);
    thinned
}

fn validate_config(config: SurfaceFireAdapterConfig) -> Result<(), SurfaceFireAdapterError> {
    if !config.minimum_fire_area_m2.is_finite() || config.minimum_fire_area_m2 <= 0.0 {
        return Err(SurfaceFireAdapterError::InvalidMinimumArea {
            area_m2: config.minimum_fire_area_m2,
        });
    }
    Ok(())
}

fn validate_frames(frames: &[SurfaceFireFrame]) -> Result<(&str, f64), SurfaceFireAdapterError> {
    if frames.len() < 2 {
        return Err(SurfaceFireAdapterError::TooFewFrames {
            frame_count: frames.len(),
        });
    }
    let first = &frames[0];
    if first.time_seconds != 0.0 {
        return Err(SurfaceFireAdapterError::TimelineMustStartAtZero {
            time_seconds: first.time_seconds,
        });
    }
    if first.scene_id.trim().is_empty() {
        return Err(SurfaceFireAdapterError::EmptySceneId);
    }

    let mut previous_time = -1.0;
    for (index, frame) in frames.iter().enumerate() {
        if frame.schema_version != SURFACE_FIRE_SCHEMA_VERSION {
            return Err(SurfaceFireAdapterError::UnsupportedSchemaVersion {
                frame_index: index,
                schema_version: frame.schema_version,
            });
        }
        if frame.scene_id != first.scene_id {
            return Err(SurfaceFireAdapterError::SceneIdMismatch {
                frame_index: index,
                expected: first.scene_id.clone(),
                actual: frame.scene_id.clone(),
            });
        }
        if !frame.time_seconds.is_finite()
            || frame.time_seconds < 0.0
            || frame.time_seconds <= previous_time
        {
            return Err(SurfaceFireAdapterError::InvalidFrameTime {
                frame_index: index,
                time_seconds: frame.time_seconds,
            });
        }
        previous_time = frame.time_seconds;
        validate_frame(index, frame)?;
    }

    Ok((first.scene_id.as_str(), previous_time))
}

fn validate_frame(
    frame_index: usize,
    frame: &SurfaceFireFrame,
) -> Result<(), SurfaceFireAdapterError> {
    let mut node_ids = HashSet::new();
    let mut hrr_by_room = HashMap::<&str, f64>::new();
    for node in &frame.active_nodes {
        validate_node(frame_index, node)?;
        if !node_ids.insert(node.id.as_str()) {
            return Err(SurfaceFireAdapterError::DuplicateNodeId {
                frame_index,
                node_id: node.id.clone(),
            });
        }
        *hrr_by_room.entry(node.room_id.as_str()).or_default() += node.heat_release_rate_kw;
    }

    let mut reported_hrr = BTreeMap::<&str, f64>::new();
    for room in &frame.room_heat_release {
        if room.room_id.trim().is_empty()
            || !room.heat_release_rate_kw.is_finite()
            || room.heat_release_rate_kw < 0.0
        {
            return Err(SurfaceFireAdapterError::InvalidRoomHeatRelease {
                frame_index,
                room_id: room.room_id.clone(),
            });
        }
        if reported_hrr
            .insert(room.room_id.as_str(), room.heat_release_rate_kw)
            .is_some()
        {
            return Err(SurfaceFireAdapterError::DuplicateRoomHeatRelease {
                frame_index,
                room_id: room.room_id.clone(),
            });
        }
    }
    for (room_id, calculated_hrr) in hrr_by_room {
        let reported = reported_hrr.get(room_id).copied().unwrap_or(0.0);
        if (reported - calculated_hrr).abs() > HRR_EPSILON_KW {
            return Err(SurfaceFireAdapterError::RoomHeatReleaseMismatch {
                frame_index,
                room_id: room_id.to_owned(),
                calculated_hrr_kw: calculated_hrr,
                reported_hrr_kw: reported,
            });
        }
    }
    Ok(())
}

fn validate_node(
    frame_index: usize,
    node: &SurfaceFireNodeFrame,
) -> Result<(), SurfaceFireAdapterError> {
    let valid = !node.id.trim().is_empty()
        && !node.room_id.trim().is_empty()
        && node.position_m.x.is_finite()
        && node.position_m.y.is_finite()
        && node.position_m.z.is_finite()
        && node.dimensions_m.width_m.is_finite()
        && node.dimensions_m.width_m >= 0.0
        && node.dimensions_m.depth_m.is_finite()
        && node.dimensions_m.depth_m >= 0.0
        && node.heat_release_rate_kw.is_finite()
        && node.heat_release_rate_kw >= 0.0;
    if !valid {
        return Err(SurfaceFireAdapterError::InvalidNode {
            frame_index,
            node_id: node.id.clone(),
        });
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq)]
pub enum SurfaceFireAdapterError {
    #[error("surface-fire adapter requires at least two frames, got {frame_count}")]
    TooFewFrames { frame_count: usize },
    #[error("surface-fire timeline must start at zero seconds, got {time_seconds}")]
    TimelineMustStartAtZero { time_seconds: f64 },
    #[error("surface-fire scene ID cannot be empty")]
    EmptySceneId,
    #[error("frame {frame_index} uses unsupported surface-fire schema {schema_version}")]
    UnsupportedSchemaVersion {
        frame_index: usize,
        schema_version: u32,
    },
    #[error("frame {frame_index} scene ID {actual} does not match {expected}")]
    SceneIdMismatch {
        frame_index: usize,
        expected: String,
        actual: String,
    },
    #[error("frame {frame_index} has invalid or unordered time {time_seconds}")]
    InvalidFrameTime {
        frame_index: usize,
        time_seconds: f64,
    },
    #[error("frame {frame_index} has invalid fire node {node_id}")]
    InvalidNode { frame_index: usize, node_id: String },
    #[error("frame {frame_index} contains duplicate fire node {node_id}")]
    DuplicateNodeId { frame_index: usize, node_id: String },
    #[error("frame {frame_index} has invalid room HRR for {room_id}")]
    InvalidRoomHeatRelease { frame_index: usize, room_id: String },
    #[error("frame {frame_index} contains duplicate room HRR for {room_id}")]
    DuplicateRoomHeatRelease { frame_index: usize, room_id: String },
    #[error(
        "frame {frame_index} room {room_id} reports {reported_hrr_kw} kW but its nodes total {calculated_hrr_kw} kW"
    )]
    RoomHeatReleaseMismatch {
        frame_index: usize,
        room_id: String,
        calculated_hrr_kw: f64,
        reported_hrr_kw: f64,
    },
    #[error("surface-fire timeline contains no burning cells")]
    NoBurningCells,
    #[error("minimum CFAST fire area must be finite and positive, got {area_m2}")]
    InvalidMinimumArea { area_m2: f64 },
}
