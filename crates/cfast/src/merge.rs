use tflop_simulation_core::{
    FireConditions, GasLayerConditions, OpeningConditions, ResultsValidationErrors, RoomConditions,
    SIMULATION_RESULTS_SCHEMA_VERSION, SURFACE_FIRE_SCHEMA_VERSION, SimulationFrame,
    SimulationResults, SurfaceFireFrame,
};
use tflop_world_core::Scene;
use thiserror::Error;

const TIME_TOLERANCE_SECONDS: f64 = 1.0e-6;

/// Places spatial Rust fire cells and CFAST room conditions on one timeline.
///
/// Surface-fire timestamps are retained so the renderer does not lose spatial
/// detail when CFAST uses a coarser spreadsheet interval. Continuous CFAST
/// values are linearly interpolated between solver frames.
pub fn merge_cfast_with_surface_fire(
    scene: &Scene,
    cfast_results: &SimulationResults,
    surface_frames: &[SurfaceFireFrame],
) -> Result<SimulationResults, CfastMergeError> {
    cfast_results.validate_against_scene(scene)?;
    validate_surface_frames(scene, surface_frames)?;
    if cfast_results.frames.is_empty() {
        return Err(CfastMergeError::NoCfastFrames);
    }

    let frames = surface_frames
        .iter()
        .map(|surface| {
            let conditions = conditions_at_time(&cfast_results.frames, surface.time_seconds)?;
            Ok(SimulationFrame {
                time_seconds: surface.time_seconds,
                rooms: conditions.rooms,
                fires: conditions.fires,
                openings: conditions.openings,
                surface_fire: surface.active_nodes.clone(),
            })
        })
        .collect::<Result<Vec<_>, CfastMergeError>>()?;

    let merged = SimulationResults {
        schema_version: SIMULATION_RESULTS_SCHEMA_VERSION,
        scene_id: cfast_results.scene_id.clone(),
        scenario_id: cfast_results.scenario_id.clone(),
        engine: cfast_results.engine.clone(),
        frames,
    };
    merged.validate_against_scene(scene)?;
    Ok(merged)
}

fn validate_surface_frames(
    scene: &Scene,
    frames: &[SurfaceFireFrame],
) -> Result<(), CfastMergeError> {
    if frames.is_empty() {
        return Err(CfastMergeError::NoSurfaceFrames);
    }
    let mut previous_time = -1.0;
    for (index, frame) in frames.iter().enumerate() {
        if frame.schema_version != SURFACE_FIRE_SCHEMA_VERSION {
            return Err(CfastMergeError::UnsupportedSurfaceSchema {
                frame_index: index,
                schema_version: frame.schema_version,
            });
        }
        if frame.scene_id != scene.id {
            return Err(CfastMergeError::SurfaceSceneMismatch {
                frame_index: index,
                expected: scene.id.clone(),
                actual: frame.scene_id.clone(),
            });
        }
        if !frame.time_seconds.is_finite()
            || frame.time_seconds < 0.0
            || frame.time_seconds <= previous_time
        {
            return Err(CfastMergeError::InvalidSurfaceTime {
                frame_index: index,
                time_seconds: frame.time_seconds,
            });
        }
        previous_time = frame.time_seconds;
    }
    Ok(())
}

fn conditions_at_time(
    frames: &[SimulationFrame],
    time_seconds: f64,
) -> Result<SimulationFrame, CfastMergeError> {
    if let Some(exact) = frames
        .iter()
        .find(|frame| (frame.time_seconds - time_seconds).abs() <= TIME_TOLERANCE_SECONDS)
    {
        return Ok(exact.clone());
    }

    let upper_index = frames.partition_point(|frame| frame.time_seconds < time_seconds);
    if upper_index == 0 || upper_index == frames.len() {
        return Err(CfastMergeError::SurfaceTimeOutsideCfastRange {
            time_seconds,
            first_cfast_time: frames[0].time_seconds,
            last_cfast_time: frames[frames.len() - 1].time_seconds,
        });
    }
    let lower = &frames[upper_index - 1];
    let upper = &frames[upper_index];
    let fraction = (time_seconds - lower.time_seconds) / (upper.time_seconds - lower.time_seconds);

    Ok(SimulationFrame {
        time_seconds,
        rooms: interpolate_rooms(&lower.rooms, &upper.rooms, fraction, time_seconds)?,
        fires: interpolate_fires(&lower.fires, &upper.fires, fraction, time_seconds)?,
        openings: interpolate_openings(&lower.openings, &upper.openings, fraction, time_seconds)?,
        surface_fire: Vec::new(),
    })
}

fn interpolate_rooms(
    lower: &[RoomConditions],
    upper: &[RoomConditions],
    fraction: f64,
    time_seconds: f64,
) -> Result<Vec<RoomConditions>, CfastMergeError> {
    require_same_entity_count("room", lower.len(), upper.len(), time_seconds)?;
    lower
        .iter()
        .map(|left| {
            let right = upper
                .iter()
                .find(|right| right.room_id == left.room_id)
                .ok_or_else(|| CfastMergeError::MissingEntityAtInterpolationBoundary {
                    entity: "room",
                    id: left.room_id.clone(),
                    time_seconds,
                })?;
            Ok(RoomConditions {
                room_id: left.room_id.clone(),
                smoke_layer_height_m: lerp(
                    left.smoke_layer_height_m,
                    right.smoke_layer_height_m,
                    fraction,
                ),
                gauge_pressure_pa: lerp(left.gauge_pressure_pa, right.gauge_pressure_pa, fraction),
                absolute_pressure_pa: lerp(
                    left.absolute_pressure_pa,
                    right.absolute_pressure_pa,
                    fraction,
                ),
                upper_layer: interpolate_layer(left.upper_layer, right.upper_layer, fraction),
                lower_layer: interpolate_layer(left.lower_layer, right.lower_layer, fraction),
            })
        })
        .collect()
}

fn interpolate_layer(
    left: GasLayerConditions,
    right: GasLayerConditions,
    fraction: f64,
) -> GasLayerConditions {
    GasLayerConditions {
        temperature_c: lerp(left.temperature_c, right.temperature_c, fraction),
        oxygen_percent: lerp(left.oxygen_percent, right.oxygen_percent, fraction),
        carbon_monoxide_percent: lerp(
            left.carbon_monoxide_percent,
            right.carbon_monoxide_percent,
            fraction,
        ),
        hydrogen_cyanide_percent: lerp(
            left.hydrogen_cyanide_percent,
            right.hydrogen_cyanide_percent,
            fraction,
        ),
        optical_density_per_m: lerp(
            left.optical_density_per_m,
            right.optical_density_per_m,
            fraction,
        ),
    }
}

fn interpolate_fires(
    lower: &[FireConditions],
    upper: &[FireConditions],
    fraction: f64,
    time_seconds: f64,
) -> Result<Vec<FireConditions>, CfastMergeError> {
    require_same_entity_count("fire", lower.len(), upper.len(), time_seconds)?;
    lower
        .iter()
        .map(|left| {
            let right = upper
                .iter()
                .find(|right| right.fire_id == left.fire_id)
                .ok_or_else(|| CfastMergeError::MissingEntityAtInterpolationBoundary {
                    entity: "fire",
                    id: left.fire_id.clone(),
                    time_seconds,
                })?;
            if left.room_id != right.room_id {
                return Err(CfastMergeError::FireRoomChanged {
                    fire_id: left.fire_id.clone(),
                    lower_room_id: left.room_id.clone(),
                    upper_room_id: right.room_id.clone(),
                });
            }
            Ok(FireConditions {
                fire_id: left.fire_id.clone(),
                room_id: left.room_id.clone(),
                // Between solver samples, keep the last observed discrete state.
                ignited: left.ignited,
                expected_hrr_kw: lerp(left.expected_hrr_kw, right.expected_hrr_kw, fraction),
                actual_hrr_kw: lerp(left.actual_hrr_kw, right.actual_hrr_kw, fraction),
                convective_hrr_kw: lerp(left.convective_hrr_kw, right.convective_hrr_kw, fraction),
                flame_height_m: lerp(left.flame_height_m, right.flame_height_m, fraction),
                pyrolysis_rate_kg_per_s: lerp(
                    left.pyrolysis_rate_kg_per_s,
                    right.pyrolysis_rate_kg_per_s,
                    fraction,
                ),
            })
        })
        .collect()
}

fn interpolate_openings(
    lower: &[OpeningConditions],
    upper: &[OpeningConditions],
    fraction: f64,
    time_seconds: f64,
) -> Result<Vec<OpeningConditions>, CfastMergeError> {
    require_same_entity_count("opening", lower.len(), upper.len(), time_seconds)?;
    lower
        .iter()
        .map(|left| {
            let right = upper
                .iter()
                .find(|right| right.opening_id == left.opening_id)
                .ok_or_else(|| CfastMergeError::MissingEntityAtInterpolationBoundary {
                    entity: "opening",
                    id: left.opening_id.clone(),
                    time_seconds,
                })?;
            Ok(OpeningConditions {
                opening_id: left.opening_id.clone(),
                net_flow_kg_per_s: lerp(left.net_flow_kg_per_s, right.net_flow_kg_per_s, fraction),
                opening_fraction: lerp(left.opening_fraction, right.opening_fraction, fraction),
            })
        })
        .collect()
}

fn lerp(left: f64, right: f64, fraction: f64) -> f64 {
    left + (right - left) * fraction
}

fn require_same_entity_count(
    entity: &'static str,
    lower_count: usize,
    upper_count: usize,
    time_seconds: f64,
) -> Result<(), CfastMergeError> {
    if lower_count != upper_count {
        return Err(CfastMergeError::EntitySetChanged {
            entity,
            lower_count,
            upper_count,
            time_seconds,
        });
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum CfastMergeError {
    #[error(transparent)]
    InvalidResults(#[from] ResultsValidationErrors),
    #[error("CFAST returned no result frames")]
    NoCfastFrames,
    #[error("surface-fire timeline contains no frames")]
    NoSurfaceFrames,
    #[error("surface frame {frame_index} uses unsupported schema {schema_version}")]
    UnsupportedSurfaceSchema {
        frame_index: usize,
        schema_version: u32,
    },
    #[error("surface frame {frame_index} scene ID {actual} does not match {expected}")]
    SurfaceSceneMismatch {
        frame_index: usize,
        expected: String,
        actual: String,
    },
    #[error("surface frame {frame_index} has invalid or unordered time {time_seconds}")]
    InvalidSurfaceTime {
        frame_index: usize,
        time_seconds: f64,
    },
    #[error(
        "surface time {time_seconds} is outside CFAST range {first_cfast_time}..={last_cfast_time}"
    )]
    SurfaceTimeOutsideCfastRange {
        time_seconds: f64,
        first_cfast_time: f64,
        last_cfast_time: f64,
    },
    #[error("{entity} {id} is missing across the CFAST interpolation around {time_seconds}s")]
    MissingEntityAtInterpolationBoundary {
        entity: &'static str,
        id: String,
        time_seconds: f64,
    },
    #[error(
        "CFAST {entity} count changes from {lower_count} to {upper_count} around {time_seconds}s"
    )]
    EntitySetChanged {
        entity: &'static str,
        lower_count: usize,
        upper_count: usize,
        time_seconds: f64,
    },
    #[error(
        "fire {fire_id} changed rooms between CFAST frames: {lower_room_id} to {upper_room_id}"
    )]
    FireRoomChanged {
        fire_id: String,
        lower_room_id: String,
        upper_room_id: String,
    },
}
