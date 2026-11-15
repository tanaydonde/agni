//! Empirical fire frequency and representative-run selection for a batch of
//! simulations. This analyzes completed timelines; it never changes a run.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CombustionState, SimulationResults};

/// Spatial and temporal resolution used to compare fire timelines.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FireEnsembleConfig {
    /// World-space X/Y positions are projected onto square cells this wide.
    pub cell_size_m: f64,
    /// Frames within this interval belong to the same time bin.
    pub time_bin_seconds: f64,
    /// Dilates each fire observation by this many bins only for similarity,
    /// so a small timing shift does not make otherwise similar runs unrelated.
    pub time_tolerance_bins: usize,
}

impl Default for FireEnsembleConfig {
    fn default() -> Self {
        Self {
            cell_size_m: 0.25,
            time_bin_seconds: 1.0,
            time_tolerance_bins: 1,
        }
    }
}

/// One empirical `l / N` observation. It is a frequency within the supplied
/// ensemble, not a calibrated prediction of a real fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireFrequencyCell {
    pub room_id: String,
    pub column: i64,
    pub row: i64,
    pub time_bin: usize,
    pub time_seconds: f64,
    pub simulation_count: usize,
    pub frequency: f64,
}

/// One floor-plan cell of the ensemble's fire, with time and height summed
/// away: where fire tended to be over the whole batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireHeatmapCell {
    pub column: i64,
    pub row: i64,
    /// Seconds this cell spent burning, averaged over the simulations.
    pub expected_burning_seconds: f64,
    /// Share of the simulations in which this cell burned at any time.
    pub burned_fraction: f64,
}

/// One selected run and the simulations for which it is the nearest selected
/// representative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireRepresentative {
    pub simulation_index: usize,
    pub support_count: usize,
    pub support_fraction: f64,
}

/// The nearest selected representative for one input simulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FireRepresentativeAssignment {
    pub simulation_index: usize,
    pub representative_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireEnsembleAnalysis {
    pub config: FireEnsembleConfig,
    pub frequencies: Vec<FireFrequencyCell>,
    /// `frequencies` summed over time, one entry per X/Y cell that ever burned.
    pub heatmap: Vec<FireHeatmapCell>,
    pub representatives: Vec<FireRepresentative>,
    pub assignments: Vec<FireRepresentativeAssignment>,
}

/// Builds the empirical `(cell, time)` fire-frequency field and greedily
/// chooses up to `top_k` medoids. At each selection step, the chosen run is
/// the one that most reduces every run's distance to its nearest selected
/// representative. Distance is time-aware Jaccard distance between burning
/// cell masks.
pub fn analyze_fire_ensemble(
    results: &[&SimulationResults],
    top_k: usize,
    config: FireEnsembleConfig,
) -> Result<FireEnsembleAnalysis, FireEnsembleError> {
    validate_inputs(results, top_k, config)?;

    let raw_masks = results
        .iter()
        .map(|results| fire_mask(results, config))
        .collect::<Vec<_>>();
    let frequencies = frequencies(&raw_masks, config);
    let heatmap = heatmap(results, config);
    let comparison_masks = raw_masks
        .iter()
        .map(|mask| dilate_time(mask, config.time_tolerance_bins))
        .collect::<Vec<_>>();
    let distances = pairwise_distances(&comparison_masks);
    let selected = select_medoids(&distances, top_k.min(results.len()));
    let (assignments, support) = assign_to_representatives(&distances, &selected);
    let denominator = results.len() as f64;
    let representatives = selected
        .into_iter()
        .map(|simulation_index| FireRepresentative {
            simulation_index,
            support_count: support[simulation_index],
            support_fraction: support[simulation_index] as f64 / denominator,
        })
        .collect();

    Ok(FireEnsembleAnalysis {
        config,
        frequencies,
        heatmap,
        representatives,
        assignments,
    })
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum FireEnsembleError {
    #[error("at least one completed simulation is required")]
    NoSimulations,
    #[error("top_k must be greater than zero")]
    InvalidTopK,
    #[error("cell_size_m must be finite and greater than zero")]
    InvalidCellSize,
    #[error("time_bin_seconds must be finite and greater than zero")]
    InvalidTimeBin,
    #[error("all simulation results must belong to scene {expected}, got {actual}")]
    MixedScenes { expected: String, actual: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct FireCellTime {
    room_id: String,
    column: i64,
    row: i64,
    time_bin: usize,
}

fn validate_inputs(
    results: &[&SimulationResults],
    top_k: usize,
    config: FireEnsembleConfig,
) -> Result<(), FireEnsembleError> {
    let Some(first) = results.first() else {
        return Err(FireEnsembleError::NoSimulations);
    };
    if top_k == 0 {
        return Err(FireEnsembleError::InvalidTopK);
    }
    if !config.cell_size_m.is_finite() || config.cell_size_m <= 0.0 {
        return Err(FireEnsembleError::InvalidCellSize);
    }
    if !config.time_bin_seconds.is_finite() || config.time_bin_seconds <= 0.0 {
        return Err(FireEnsembleError::InvalidTimeBin);
    }
    for result in &results[1..] {
        if result.scene_id != first.scene_id {
            return Err(FireEnsembleError::MixedScenes {
                expected: first.scene_id.clone(),
                actual: result.scene_id.clone(),
            });
        }
    }
    Ok(())
}

fn fire_mask(results: &SimulationResults, config: FireEnsembleConfig) -> HashSet<FireCellTime> {
    let mut mask = HashSet::new();
    for frame in &results.frames {
        let time_bin = (frame.time_seconds / config.time_bin_seconds).floor() as usize;
        for node in &frame.surface_fire {
            if node.state != CombustionState::Burning {
                continue;
            }
            mask.insert(FireCellTime {
                room_id: node.room_id.clone(),
                column: (node.position_m.x / config.cell_size_m).floor() as i64,
                row: (node.position_m.y / config.cell_size_m).floor() as i64,
                time_bin,
            });
        }
    }
    mask
}

fn frequencies(
    masks: &[HashSet<FireCellTime>],
    config: FireEnsembleConfig,
) -> Vec<FireFrequencyCell> {
    let mut counts = HashMap::<FireCellTime, usize>::new();
    for mask in masks {
        for cell in mask {
            *counts.entry(cell.clone()).or_default() += 1;
        }
    }
    let denominator = masks.len() as f64;
    let mut cells = counts.into_iter().collect::<Vec<_>>();
    cells.sort_by(|(left, _), (right, _)| left.cmp(right));
    cells
        .into_iter()
        .map(|(cell, simulation_count)| FireFrequencyCell {
            room_id: cell.room_id,
            column: cell.column,
            row: cell.row,
            time_bin: cell.time_bin,
            time_seconds: cell.time_bin as f64 * config.time_bin_seconds,
            simulation_count,
            frequency: simulation_count as f64 / denominator,
        })
        .collect()
}

/// Collapses each run's burning cells onto the floor plan. Unlike the
/// similarity masks, a node covers every heatmap cell under its footprint
/// (floor cells are wider than `cell_size_m`), and nodes stacked in height
/// count once per time bin.
fn heatmap(results: &[&SimulationResults], config: FireEnsembleConfig) -> Vec<FireHeatmapCell> {
    // Per cell: burning time bins summed over runs, and runs it burned in.
    let mut totals = HashMap::<(i64, i64), (usize, usize)>::new();
    for results in results {
        let mut bins = HashMap::<(i64, i64), HashSet<usize>>::new();
        for frame in &results.frames {
            let time_bin = (frame.time_seconds / config.time_bin_seconds).floor() as usize;
            for node in &frame.surface_fire {
                if node.state != CombustionState::Burning {
                    continue;
                }
                for cell in footprint_cells(node, config.cell_size_m) {
                    bins.entry(cell).or_default().insert(time_bin);
                }
            }
        }
        for (cell, bins) in bins {
            let total = totals.entry(cell).or_default();
            total.0 += bins.len();
            total.1 += 1;
        }
    }
    let denominator = results.len() as f64;
    let mut cells = totals.into_iter().collect::<Vec<_>>();
    cells.sort_by_key(|(cell, _)| *cell);
    cells
        .into_iter()
        .map(|((column, row), (bins, runs))| FireHeatmapCell {
            column,
            row,
            expected_burning_seconds: bins as f64 * config.time_bin_seconds / denominator,
            burned_fraction: runs as f64 / denominator,
        })
        .collect()
}

/// The heatmap cells whose centers lie under a node's footprint, and always
/// the cell holding the node's own center. Furniture nodes are sized in
/// their object's axes; they are about one cell wide, so its rotation is
/// ignored here.
fn footprint_cells(
    node: &crate::SurfaceFireNodeFrame,
    cell_size_m: f64,
) -> impl Iterator<Item = (i64, i64)> {
    let span = |center: f64, size: f64| {
        let own = (center / cell_size_m).floor() as i64;
        // Cell `i` has its center at `(i + 0.5) * cell_size_m`.
        let first = ((center - size / 2.0) / cell_size_m - 0.5).ceil() as i64;
        let last = ((center + size / 2.0) / cell_size_m - 0.5).floor() as i64;
        first.min(own)..=last.max(own)
    };
    let columns = span(node.position_m.x, node.dimensions_m.width_m);
    let rows = span(node.position_m.y, node.dimensions_m.depth_m);
    columns.flat_map(move |column| rows.clone().map(move |row| (column, row)))
}

fn dilate_time(mask: &HashSet<FireCellTime>, tolerance: usize) -> HashSet<FireCellTime> {
    if tolerance == 0 {
        return mask.clone();
    }
    let mut dilated = HashSet::new();
    for cell in mask {
        let first = cell.time_bin.saturating_sub(tolerance);
        let last = cell.time_bin.saturating_add(tolerance);
        for time_bin in first..=last {
            let mut nearby = cell.clone();
            nearby.time_bin = time_bin;
            dilated.insert(nearby);
        }
    }
    dilated
}

fn pairwise_distances(masks: &[HashSet<FireCellTime>]) -> Vec<Vec<f64>> {
    let mut distances = vec![vec![0.0; masks.len()]; masks.len()];
    for left in 0..masks.len() {
        for right in (left + 1)..masks.len() {
            let intersection = masks[left].intersection(&masks[right]).count();
            let union = masks[left].len() + masks[right].len() - intersection;
            let distance = if union == 0 {
                0.0
            } else {
                1.0 - intersection as f64 / union as f64
            };
            distances[left][right] = distance;
            distances[right][left] = distance;
        }
    }
    distances
}

fn select_medoids(distances: &[Vec<f64>], top_k: usize) -> Vec<usize> {
    let count = distances.len();
    let first = (0..count)
        .min_by(|&left, &right| {
            distances[left]
                .iter()
                .sum::<f64>()
                .total_cmp(&distances[right].iter().sum::<f64>())
                .then(left.cmp(&right))
        })
        .expect("validated nonempty ensemble");
    let mut selected = vec![first];
    let mut nearest = distances[first].clone();

    while selected.len() < top_k {
        let candidate = (0..count)
            .filter(|candidate| !selected.contains(candidate))
            .max_by(|&left, &right| {
                coverage_gain(&nearest, &distances[left])
                    .total_cmp(&coverage_gain(&nearest, &distances[right]))
                    // Prefer the earlier input when gains tie.
                    .then_with(|| right.cmp(&left))
            })
            .expect("top_k does not exceed simulation count");
        for (current, candidate_distance) in nearest.iter_mut().zip(&distances[candidate]) {
            *current = current.min(*candidate_distance);
        }
        selected.push(candidate);
    }
    selected
}

fn coverage_gain(nearest: &[f64], candidate: &[f64]) -> f64 {
    nearest
        .iter()
        .zip(candidate)
        .map(|(current, candidate)| current - current.min(*candidate))
        .sum()
}

fn assign_to_representatives(
    distances: &[Vec<f64>],
    representatives: &[usize],
) -> (Vec<FireRepresentativeAssignment>, Vec<usize>) {
    let mut support = vec![0; distances.len()];
    let assignments = (0..distances.len())
        .map(|simulation_index| {
            let representative_index = *representatives
                .iter()
                .min_by(|&&left, &&right| {
                    distances[simulation_index][left]
                        .total_cmp(&distances[simulation_index][right])
                        .then(left.cmp(&right))
                })
                .expect("at least one representative");
            support[representative_index] += 1;
            FireRepresentativeAssignment {
                simulation_index,
                representative_index,
            }
        })
        .collect();
    (assignments, support)
}
