use std::{collections::HashSet, error::Error, fmt};

use crate::{BoundaryEdge, FurnitureAsset, FurnitureObject, Opening, RoomDimensions, Scene, Vec3};

const MIN_EDGE_LENGTH_M: f64 = 1.0e-9;
const QUATERNION_NORMALIZATION_TOLERANCE: f64 = 1.0e-6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneValidationError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for SceneValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneValidationErrors(pub Vec<SceneValidationError>);

impl fmt::Display for SceneValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "scene validation failed with {} error(s)",
            self.0.len()
        )
    }
}

impl Error for SceneValidationErrors {}

impl Scene {
    /// Validates all scene geometry and returns every detected problem.
    pub fn validate(&self) -> Result<(), SceneValidationErrors> {
        let mut errors = Vec::new();

        require_nonempty(&self.id, "id", &mut errors);
        if let Some(name) = &self.name {
            require_nonempty(name, "name", &mut errors);
        }
        require_nonempty(&self.source.provider, "source.provider", &mut errors);
        require_nonempty(&self.source.external_id, "source.external_id", &mut errors);

        let mut asset_ids = HashSet::new();
        for (asset_index, asset) in self.assets.iter().enumerate() {
            let asset_path = format!("assets[{asset_index}]");
            validate_asset(asset, &asset_path, &mut errors);
            if !asset_ids.insert(asset.id.as_str()) {
                push_error(
                    &format!("{asset_path}.id"),
                    "asset ID must be unique",
                    &mut errors,
                );
            }
        }

        let mut object_ids = HashSet::new();

        for (floor_index, floor) in self.floors.iter().enumerate() {
            let floor_path = format!("floors[{floor_index}]");
            require_nonempty(&floor.id, &format!("{floor_path}.id"), &mut errors);
            validate_optional_finite(
                floor.elevation_m,
                &format!("{floor_path}.elevation_m"),
                &mut errors,
            );

            for (room_index, room) in floor.rooms.iter().enumerate() {
                let room_path = format!("{floor_path}.rooms[{room_index}]");
                require_nonempty(&room.id, &format!("{room_path}.id"), &mut errors);

                for (classification_index, classification) in
                    room.classifications.iter().enumerate()
                {
                    let classification_path =
                        format!("{room_path}.classifications[{classification_index}]");
                    require_nonempty(
                        &classification.label,
                        &format!("{classification_path}.label"),
                        &mut errors,
                    );
                    validate_confidence(
                        classification.confidence,
                        &format!("{classification_path}.confidence"),
                        &mut errors,
                    );
                }

                if let Some(dimensions) = &room.dimensions {
                    validate_dimensions(dimensions, &room_path, &mut errors);
                }

                for (edge_index, edge) in room.boundary.iter().enumerate() {
                    validate_edge(
                        edge,
                        &format!("{room_path}.boundary[{edge_index}]"),
                        &mut errors,
                    );
                }
            }

            for (object_index, object) in floor.objects.iter().enumerate() {
                let object_path = format!("{floor_path}.objects[{object_index}]");
                validate_object(object, &object_path, &asset_ids, &mut errors);
                validate_on_floor(object, floor.floor_z(), &object_path, &mut errors);
                if !object_ids.insert(object.id.as_str()) {
                    push_error(
                        &format!("{object_path}.id"),
                        "object ID must be unique across the scene",
                        &mut errors,
                    );
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(SceneValidationErrors(errors))
        }
    }
}

fn validate_dimensions(
    dimensions: &RoomDimensions,
    room_path: &str,
    errors: &mut Vec<SceneValidationError>,
) {
    for (field, value) in [
        ("area_m2", dimensions.area_m2),
        ("width_m", dimensions.width_m),
        ("depth_m", dimensions.depth_m),
        ("height_m", dimensions.height_m),
        ("lower_elevation_m", dimensions.lower_elevation_m),
    ] {
        let path = format!("{room_path}.dimensions.{field}");
        if field == "lower_elevation_m" {
            validate_optional_finite(value, &path, errors);
        } else {
            validate_optional_nonnegative(value, &path, errors);
        }
    }
}

fn validate_asset(asset: &FurnitureAsset, path: &str, errors: &mut Vec<SceneValidationError>) {
    require_nonempty(&asset.id, &format!("{path}.id"), errors);
    require_nonempty(&asset.category, &format!("{path}.category"), errors);
    if let Some(model_uri) = &asset.model_uri {
        require_nonempty(model_uri, &format!("{path}.model_uri"), errors);
    }
    validate_positive(
        asset.dimensions_m.width_m,
        &format!("{path}.dimensions_m.width_m"),
        errors,
    );
    validate_positive(
        asset.dimensions_m.depth_m,
        &format!("{path}.dimensions_m.depth_m"),
        errors,
    );
    validate_positive(
        asset.dimensions_m.height_m,
        &format!("{path}.dimensions_m.height_m"),
        errors,
    );
}

fn validate_object(
    object: &FurnitureObject,
    path: &str,
    asset_ids: &HashSet<&str>,
    errors: &mut Vec<SceneValidationError>,
) {
    require_nonempty(&object.id, &format!("{path}.id"), errors);
    require_nonempty(&object.asset_id, &format!("{path}.asset_id"), errors);
    if !object.asset_id.trim().is_empty() && !asset_ids.contains(object.asset_id.as_str()) {
        push_error(
            &format!("{path}.asset_id"),
            "object must reference an existing asset",
            errors,
        );
    }

    validate_vec3(
        object.transform.position_m,
        &format!("{path}.transform.position_m"),
        errors,
    );
    for (axis, value) in [
        ("x", object.transform.scale.x),
        ("y", object.transform.scale.y),
        ("z", object.transform.scale.z),
    ] {
        validate_positive(value, &format!("{path}.transform.scale.{axis}"), errors);
    }

    let rotation = object.transform.rotation;
    for (component, value) in [
        ("x", rotation.x),
        ("y", rotation.y),
        ("z", rotation.z),
        ("w", rotation.w),
    ] {
        validate_finite(
            value,
            &format!("{path}.transform.rotation.{component}"),
            errors,
        );
    }

    let magnitude_squared = rotation.magnitude_squared();
    if magnitude_squared.is_finite()
        && (magnitude_squared - 1.0).abs() > QUATERNION_NORMALIZATION_TOLERANCE
    {
        push_error(
            &format!("{path}.transform.rotation"),
            "quaternion must be normalized",
            errors,
        );
    }
}

/// Furniture must stand upright with its bottom (the asset origin) on the
/// floor surface.
fn validate_on_floor(
    object: &FurnitureObject,
    floor_z: f64,
    path: &str,
    errors: &mut Vec<SceneValidationError>,
) {
    const FLOOR_TOLERANCE_M: f64 = 1.0e-6;
    let z = object.transform.position_m.z;
    if z.is_finite() && floor_z.is_finite() && (z - floor_z).abs() > FLOOR_TOLERANCE_M {
        push_error(
            &format!("{path}.transform.position_m.z"),
            "object must stand on the floor (z must equal the floor elevation)",
            errors,
        );
    }
    if !object.transform.rotation.is_yaw_only() {
        push_error(
            &format!("{path}.transform.rotation"),
            "object must stay upright (rotation about Z only)",
            errors,
        );
    }
}

fn validate_edge(edge: &BoundaryEdge, path: &str, errors: &mut Vec<SceneValidationError>) {
    require_nonempty(&edge.id, &format!("{path}.id"), errors);
    validate_vec3(edge.start, &format!("{path}.start"), errors);
    validate_vec3(edge.end, &format!("{path}.end"), errors);
    validate_optional_nonnegative(edge.thickness_m, &format!("{path}.thickness_m"), errors);
    validate_optional_unit_interval(
        edge.center_line_bias,
        &format!("{path}.center_line_bias"),
        errors,
    );

    let edge_length = edge.start.distance_to(edge.end);
    if edge_length.is_finite() && edge_length <= MIN_EDGE_LENGTH_M {
        push_error(path, "edge start and end must be different", errors);
    }

    for (opening_index, opening) in edge.openings.iter().enumerate() {
        validate_opening(
            opening,
            edge_length,
            &format!("{path}.openings[{opening_index}]"),
            errors,
        );
    }
}

fn validate_opening(
    opening: &Opening,
    edge_length: f64,
    path: &str,
    errors: &mut Vec<SceneValidationError>,
) {
    require_nonempty(&opening.id, &format!("{path}.id"), errors);
    if let Some(label) = &opening.label {
        require_nonempty(label, &format!("{path}.label"), errors);
    }
    validate_nonnegative(opening.width_m, &format!("{path}.width_m"), errors);
    validate_optional_nonnegative(opening.height_m, &format!("{path}.height_m"), errors);
    validate_optional_finite(
        opening.lower_elevation_m,
        &format!("{path}.lower_elevation_m"),
        errors,
    );
    validate_unit_interval(
        opening.relative_center,
        &format!("{path}.relative_center"),
        errors,
    );

    if edge_length.is_finite()
        && edge_length > MIN_EDGE_LENGTH_M
        && opening.width_m.is_finite()
        && opening.width_m >= 0.0
        && opening.relative_center.is_finite()
        && (0.0..=1.0).contains(&opening.relative_center)
    {
        let center_m = opening.relative_center * edge_length;
        let half_width_m = opening.width_m / 2.0;
        if center_m - half_width_m < 0.0 || center_m + half_width_m > edge_length {
            push_error(path, "opening must fit completely along its edge", errors);
        }
    }
}

fn validate_vec3(value: Vec3, path: &str, errors: &mut Vec<SceneValidationError>) {
    for (axis, coordinate) in [("x", value.x), ("y", value.y), ("z", value.z)] {
        validate_finite(coordinate, &format!("{path}.{axis}"), errors);
    }
}

fn validate_confidence(value: Option<f64>, path: &str, errors: &mut Vec<SceneValidationError>) {
    validate_optional_unit_interval(value, path, errors);
}

fn validate_optional_finite(
    value: Option<f64>,
    path: &str,
    errors: &mut Vec<SceneValidationError>,
) {
    if let Some(value) = value {
        validate_finite(value, path, errors);
    }
}

fn validate_optional_nonnegative(
    value: Option<f64>,
    path: &str,
    errors: &mut Vec<SceneValidationError>,
) {
    if let Some(value) = value {
        validate_nonnegative(value, path, errors);
    }
}

fn validate_optional_unit_interval(
    value: Option<f64>,
    path: &str,
    errors: &mut Vec<SceneValidationError>,
) {
    if let Some(value) = value {
        validate_unit_interval(value, path, errors);
    }
}

fn validate_nonnegative(value: f64, path: &str, errors: &mut Vec<SceneValidationError>) {
    if !value.is_finite() {
        push_error(path, "value must be finite", errors);
    } else if value < 0.0 {
        push_error(path, "value cannot be negative", errors);
    }
}

fn validate_positive(value: f64, path: &str, errors: &mut Vec<SceneValidationError>) {
    if !value.is_finite() {
        push_error(path, "value must be finite", errors);
    } else if value <= 0.0 {
        push_error(path, "value must be greater than zero", errors);
    }
}

fn validate_unit_interval(value: f64, path: &str, errors: &mut Vec<SceneValidationError>) {
    if !value.is_finite() {
        push_error(path, "value must be finite", errors);
    } else if !(0.0..=1.0).contains(&value) {
        push_error(path, "value must be between 0.0 and 1.0", errors);
    }
}

fn validate_finite(value: f64, path: &str, errors: &mut Vec<SceneValidationError>) {
    if !value.is_finite() {
        push_error(path, "value must be finite", errors);
    }
}

fn require_nonempty(value: &str, path: &str, errors: &mut Vec<SceneValidationError>) {
    if value.trim().is_empty() {
        push_error(path, "value cannot be empty", errors);
    }
}

fn push_error(path: &str, message: &str, errors: &mut Vec<SceneValidationError>) {
    errors.push(SceneValidationError {
        path: path.to_owned(),
        message: message.to_owned(),
    });
}
