use std::{collections::HashSet, error::Error, fmt};

use serde::{Deserialize, Serialize};
use tflop_world_core::Scene;

use crate::surface_fire::{SurfaceFireNodeFrame, SurfaceFireNodeSource};

pub const SIMULATION_RESULTS_SCHEMA_VERSION: u32 = 1;

/// Provider-independent, time-ordered results for one scene and scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulationResults {
    pub schema_version: u32,
    pub scene_id: String,
    pub scenario_id: String,
    pub engine: SimulationEngine,
    pub frames: Vec<SimulationFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationEngine {
    pub name: String,
    pub version: String,
}

/// All changing fire conditions at one simulation time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulationFrame {
    pub time_seconds: f64,
    pub rooms: Vec<RoomConditions>,
    pub fires: Vec<FireConditions>,
    pub openings: Vec<OpeningConditions>,
    /// Spatial floor and furniture combustion state for renderer placement.
    #[serde(default)]
    pub surface_fire: Vec<SurfaceFireNodeFrame>,
}

/// CFAST-style two-layer conditions mapped back to a `world-core` room ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomConditions {
    pub room_id: String,
    pub smoke_layer_height_m: f64,
    pub gauge_pressure_pa: f64,
    pub absolute_pressure_pa: f64,
    pub upper_layer: GasLayerConditions,
    pub lower_layer: GasLayerConditions,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GasLayerConditions {
    pub temperature_c: f64,
    pub oxygen_percent: f64,
    pub carbon_monoxide_percent: f64,
    pub hydrogen_cyanide_percent: f64,
    pub optical_density_per_m: f64,
}

/// Calculated state of one prescribed or triggered fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireConditions {
    pub fire_id: String,
    pub room_id: String,
    pub ignited: bool,
    pub expected_hrr_kw: f64,
    pub actual_hrr_kw: f64,
    pub convective_hrr_kw: f64,
    pub flame_height_m: f64,
    pub pyrolysis_rate_kg_per_s: f64,
}

/// Calculated flow through one door, window, or passage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningConditions {
    pub opening_id: String,
    /// Positive and negative values represent opposite flow directions.
    pub net_flow_kg_per_s: f64,
    pub opening_fraction: f64,
}

impl SimulationResults {
    /// Rejects malformed parser output before it reaches the backend.
    pub fn validate(&self) -> Result<(), ResultsValidationErrors> {
        let mut errors = Vec::new();
        if self.schema_version != SIMULATION_RESULTS_SCHEMA_VERSION {
            push_error(
                "schema_version",
                "unsupported simulation results schema version",
                &mut errors,
            );
        }
        require_nonempty(&self.scene_id, "scene_id", &mut errors);
        require_nonempty(&self.scenario_id, "scenario_id", &mut errors);
        require_nonempty(&self.engine.name, "engine.name", &mut errors);
        require_nonempty(&self.engine.version, "engine.version", &mut errors);

        let mut previous_time = None;
        for (frame_index, frame) in self.frames.iter().enumerate() {
            let frame_path = format!("frames[{frame_index}]");
            validate_nonnegative(
                frame.time_seconds,
                &format!("{frame_path}.time_seconds"),
                &mut errors,
            );
            if let Some(previous) = previous_time
                && frame.time_seconds.is_finite()
                && frame.time_seconds <= previous
            {
                push_error(
                    &format!("{frame_path}.time_seconds"),
                    "frame times must be strictly increasing",
                    &mut errors,
                );
            }
            if frame.time_seconds.is_finite() {
                previous_time = Some(frame.time_seconds);
            }
            validate_frame(frame, &frame_path, &mut errors);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ResultsValidationErrors(errors))
        }
    }

    /// Also verifies that every result ID points back to the physical scene.
    pub fn validate_against_scene(&self, scene: &Scene) -> Result<(), ResultsValidationErrors> {
        let mut errors = self
            .validate()
            .err()
            .map_or_else(Vec::new, |errors| errors.0);
        if self.scene_id != scene.id {
            push_error(
                "scene_id",
                "results scene ID does not match the physical scene",
                &mut errors,
            );
        }

        let room_ids = scene
            .floors
            .iter()
            .flat_map(|floor| floor.rooms.iter().map(|room| room.id.as_str()))
            .collect::<HashSet<_>>();
        let opening_ids = scene
            .floors
            .iter()
            .flat_map(|floor| floor.rooms.iter())
            .flat_map(|room| room.boundary.iter())
            .flat_map(|edge| edge.openings.iter().map(|opening| opening.id.as_str()))
            .collect::<HashSet<_>>();
        let floor_ids = scene
            .floors
            .iter()
            .map(|floor| floor.id.as_str())
            .collect::<HashSet<_>>();
        let object_ids = scene
            .floors
            .iter()
            .flat_map(|floor| floor.objects.iter().map(|object| object.id.as_str()))
            .collect::<HashSet<_>>();

        for (frame_index, frame) in self.frames.iter().enumerate() {
            for (room_index, room) in frame.rooms.iter().enumerate() {
                if !room_ids.contains(room.room_id.as_str()) {
                    push_error(
                        &format!("frames[{frame_index}].rooms[{room_index}].room_id"),
                        "room ID does not exist in the physical scene",
                        &mut errors,
                    );
                }
            }
            for (fire_index, fire) in frame.fires.iter().enumerate() {
                if !room_ids.contains(fire.room_id.as_str()) {
                    push_error(
                        &format!("frames[{frame_index}].fires[{fire_index}].room_id"),
                        "room ID does not exist in the physical scene",
                        &mut errors,
                    );
                }
            }
            for (opening_index, opening) in frame.openings.iter().enumerate() {
                if !opening_ids.contains(opening.opening_id.as_str()) {
                    push_error(
                        &format!("frames[{frame_index}].openings[{opening_index}].opening_id"),
                        "opening ID does not exist in the physical scene",
                        &mut errors,
                    );
                }
            }
            for (node_index, node) in frame.surface_fire.iter().enumerate() {
                let path = format!("frames[{frame_index}].surface_fire[{node_index}]");
                if !room_ids.contains(node.room_id.as_str()) {
                    push_error(
                        &format!("{path}.room_id"),
                        "room ID does not exist in the physical scene",
                        &mut errors,
                    );
                }
                match &node.source {
                    SurfaceFireNodeSource::FloorCell { floor_id, .. } => {
                        if !floor_ids.contains(floor_id.as_str()) {
                            push_error(
                                &format!("{path}.source.floor_id"),
                                "floor ID does not exist in the physical scene",
                                &mut errors,
                            );
                        }
                    }
                    SurfaceFireNodeSource::Furniture { object_id } => {
                        if !object_ids.contains(object_id.as_str()) {
                            push_error(
                                &format!("{path}.source.object_id"),
                                "object ID does not exist in the physical scene",
                                &mut errors,
                            );
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ResultsValidationErrors(errors))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultsValidationError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for ResultsValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultsValidationErrors(pub Vec<ResultsValidationError>);

impl fmt::Display for ResultsValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "simulation results validation failed with {} error(s)",
            self.0.len()
        )
    }
}

impl Error for ResultsValidationErrors {}

fn validate_frame(frame: &SimulationFrame, path: &str, errors: &mut Vec<ResultsValidationError>) {
    let mut room_ids = HashSet::new();
    for (index, room) in frame.rooms.iter().enumerate() {
        let room_path = format!("{path}.rooms[{index}]");
        require_unique_id(&room.room_id, &room_path, &mut room_ids, errors);
        validate_nonnegative(
            room.smoke_layer_height_m,
            &format!("{room_path}.smoke_layer_height_m"),
            errors,
        );
        validate_finite(
            room.gauge_pressure_pa,
            &format!("{room_path}.gauge_pressure_pa"),
            errors,
        );
        validate_nonnegative(
            room.absolute_pressure_pa,
            &format!("{room_path}.absolute_pressure_pa"),
            errors,
        );
        validate_layer(
            room.upper_layer,
            &format!("{room_path}.upper_layer"),
            errors,
        );
        validate_layer(
            room.lower_layer,
            &format!("{room_path}.lower_layer"),
            errors,
        );
    }

    let mut fire_ids = HashSet::new();
    for (index, fire) in frame.fires.iter().enumerate() {
        let fire_path = format!("{path}.fires[{index}]");
        require_unique_id(&fire.fire_id, &fire_path, &mut fire_ids, errors);
        require_nonempty(&fire.room_id, &format!("{fire_path}.room_id"), errors);
        for (field, value) in [
            ("expected_hrr_kw", fire.expected_hrr_kw),
            ("actual_hrr_kw", fire.actual_hrr_kw),
            ("convective_hrr_kw", fire.convective_hrr_kw),
            ("flame_height_m", fire.flame_height_m),
            ("pyrolysis_rate_kg_per_s", fire.pyrolysis_rate_kg_per_s),
        ] {
            validate_nonnegative(value, &format!("{fire_path}.{field}"), errors);
        }
    }

    let mut opening_ids = HashSet::new();
    for (index, opening) in frame.openings.iter().enumerate() {
        let opening_path = format!("{path}.openings[{index}]");
        require_unique_id(&opening.opening_id, &opening_path, &mut opening_ids, errors);
        validate_finite(
            opening.net_flow_kg_per_s,
            &format!("{opening_path}.net_flow_kg_per_s"),
            errors,
        );
        validate_percentage_fraction(
            opening.opening_fraction,
            &format!("{opening_path}.opening_fraction"),
            errors,
        );
    }

    let mut surface_node_ids = HashSet::new();
    for (index, node) in frame.surface_fire.iter().enumerate() {
        let node_path = format!("{path}.surface_fire[{index}]");
        require_unique_id(&node.id, &node_path, &mut surface_node_ids, errors);
        require_nonempty(&node.room_id, &format!("{node_path}.room_id"), errors);
        require_nonempty(
            &node.material_id,
            &format!("{node_path}.material_id"),
            errors,
        );
        for (field, value) in [
            ("position_m.x", node.position_m.x),
            ("position_m.y", node.position_m.y),
            ("position_m.z", node.position_m.z),
            ("surface_temperature_c", node.surface_temperature_c),
        ] {
            validate_finite(value, &format!("{node_path}.{field}"), errors);
        }
        for (field, value) in [
            ("dimensions_m.width_m", node.dimensions_m.width_m),
            ("dimensions_m.depth_m", node.dimensions_m.depth_m),
            ("dimensions_m.height_m", node.dimensions_m.height_m),
            ("heat_release_rate_kw", node.heat_release_rate_kw),
        ] {
            validate_nonnegative(value, &format!("{node_path}.{field}"), errors);
        }
        validate_percentage_fraction(
            node.fuel_remaining_fraction,
            &format!("{node_path}.fuel_remaining_fraction"),
            errors,
        );
        match (&node.source, node.furniture_cell) {
            (SurfaceFireNodeSource::FloorCell { .. }, Some(_)) => push_error(
                &format!("{node_path}.furniture_cell"),
                "floor nodes cannot carry furniture cell coordinates",
                errors,
            ),
            (SurfaceFireNodeSource::Furniture { .. }, None) => push_error(
                &format!("{node_path}.furniture_cell"),
                "furniture nodes must carry furniture cell coordinates",
                errors,
            ),
            _ => {}
        }
        if let Some(cell) = node.furniture_cell
            && (cell.columns == 0
                || cell.rows == 0
                || cell.layers == 0
                || cell.column >= cell.columns
                || cell.row >= cell.rows
                || cell.layer >= cell.layers)
        {
            push_error(
                &format!("{node_path}.furniture_cell"),
                "furniture cell coordinates must fit within nonzero grid dimensions",
                errors,
            );
        }
        if let Some(ignition_time_seconds) = node.ignition_time_seconds {
            validate_nonnegative(
                ignition_time_seconds,
                &format!("{node_path}.ignition_time_seconds"),
                errors,
            );
        }
    }
}

fn validate_layer(layer: GasLayerConditions, path: &str, errors: &mut Vec<ResultsValidationError>) {
    validate_finite(
        layer.temperature_c,
        &format!("{path}.temperature_c"),
        errors,
    );
    for (field, value) in [
        ("oxygen_percent", layer.oxygen_percent),
        ("carbon_monoxide_percent", layer.carbon_monoxide_percent),
        ("hydrogen_cyanide_percent", layer.hydrogen_cyanide_percent),
    ] {
        validate_percentage(value, &format!("{path}.{field}"), errors);
    }
    validate_nonnegative(
        layer.optical_density_per_m,
        &format!("{path}.optical_density_per_m"),
        errors,
    );
}

fn require_unique_id<'a>(
    id: &'a str,
    path: &str,
    ids: &mut HashSet<&'a str>,
    errors: &mut Vec<ResultsValidationError>,
) {
    require_nonempty(id, &format!("{path}.id"), errors);
    if !id.trim().is_empty() && !ids.insert(id) {
        push_error(&format!("{path}.id"), "ID must be unique in frame", errors);
    }
}

fn require_nonempty(value: &str, path: &str, errors: &mut Vec<ResultsValidationError>) {
    if value.trim().is_empty() {
        push_error(path, "value cannot be empty", errors);
    }
}

fn validate_finite(value: f64, path: &str, errors: &mut Vec<ResultsValidationError>) {
    if !value.is_finite() {
        push_error(path, "value must be finite", errors);
    }
}

fn validate_nonnegative(value: f64, path: &str, errors: &mut Vec<ResultsValidationError>) {
    if !value.is_finite() || value < 0.0 {
        push_error(path, "value must be finite and nonnegative", errors);
    }
}

fn validate_percentage(value: f64, path: &str, errors: &mut Vec<ResultsValidationError>) {
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        push_error(path, "percentage must be between zero and 100", errors);
    }
}

fn validate_percentage_fraction(value: f64, path: &str, errors: &mut Vec<ResultsValidationError>) {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        push_error(path, "fraction must be between zero and one", errors);
    }
}

fn push_error(path: &str, message: &str, errors: &mut Vec<ResultsValidationError>) {
    errors.push(ResultsValidationError {
        path: path.to_owned(),
        message: message.to_owned(),
    });
}
