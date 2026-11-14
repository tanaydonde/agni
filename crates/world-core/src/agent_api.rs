use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    FurnitureAsset, FurnitureObject, Mobility, PlacementRules, Quaternion, Scene, Transform3, Vec3,
    placement::validate_candidate_placement,
};

const QUATERNION_NORMALIZATION_TOLERANCE: f64 = 1.0e-6;

/// In-memory entry point shared by future Gemini tools and editor commands.
#[derive(Clone)]
pub struct World {
    scene: Scene,
    revision: u64,
    object_index: HashMap<String, ObjectLocation>,
    placement_rules: PlacementRules,
}

#[derive(Clone, Copy)]
struct ObjectLocation {
    floor_index: usize,
    object_index: usize,
}

impl World {
    #[must_use]
    pub fn new(scene: Scene) -> Self {
        let object_index = build_object_index(&scene);
        Self {
            scene,
            revision: 0,
            object_index,
            placement_rules: PlacementRules::default(),
        }
    }

    pub fn with_placement_rules(
        scene: Scene,
        placement_rules: PlacementRules,
    ) -> WorldApiResult<Self> {
        validate_placement_rules(placement_rules)?;
        let mut world = Self::new(scene);
        world.placement_rules = placement_rules;
        Ok(world)
    }

    #[must_use]
    pub fn get_scene(&self) -> GetSceneResponse {
        GetSceneResponse {
            revision: self.revision,
            scene: self.scene.clone(),
        }
    }

    #[must_use]
    pub fn list_assets(&self) -> ListAssetsResponse {
        ListAssetsResponse {
            assets: self.scene.assets.clone(),
        }
    }

    pub fn add_object(&mut self, request: AddObjectRequest) -> WorldApiResult<EditResponse> {
        self.require_revision(request.expected_revision)?;
        let object_id = self.add_object_unversioned(
            request.object_id,
            request.asset_id,
            request.floor_id,
            request.position_m,
            request.rotation_z_degrees,
            request.mobility,
        )?;
        Ok(self.finish_edit(vec![object_id]))
    }

    pub fn move_object(&mut self, request: MoveObjectRequest) -> WorldApiResult<EditResponse> {
        self.require_revision(request.expected_revision)?;
        let object_id = self.move_object_unversioned(request.object_id, request.position_m)?;
        Ok(self.finish_edit(vec![object_id]))
    }

    pub fn rotate_object(&mut self, request: RotateObjectRequest) -> WorldApiResult<EditResponse> {
        self.require_revision(request.expected_revision)?;
        let object_id =
            self.rotate_object_unversioned(request.object_id, request.rotation_z_degrees)?;
        Ok(self.finish_edit(vec![object_id]))
    }

    pub fn set_object_transform(
        &mut self,
        request: SetObjectTransformRequest,
    ) -> WorldApiResult<EditResponse> {
        self.require_revision(request.expected_revision)?;
        let object_id =
            self.set_object_transform_unversioned(request.object_id, request.transform)?;
        Ok(self.finish_edit(vec![object_id]))
    }

    pub fn remove_object(&mut self, request: RemoveObjectRequest) -> WorldApiResult<EditResponse> {
        self.require_revision(request.expected_revision)?;
        let object_id = self.remove_object_unversioned(request.object_id)?;
        Ok(self.finish_edit(vec![object_id]))
    }

    pub fn apply_edits(&mut self, request: ApplyEditsRequest) -> WorldApiResult<EditResponse> {
        self.require_revision(request.expected_revision)?;
        if request.edits.is_empty() {
            return Ok(EditResponse {
                revision: self.revision,
                changed_object_ids: Vec::new(),
            });
        }

        let mut staged = self.clone();
        let mut changed_object_ids = Vec::new();
        for (edit_index, edit) in request.edits.into_iter().enumerate() {
            match staged.apply_edit_unversioned(edit) {
                Ok(object_id) => changed_object_ids.push(object_id),
                Err(reason) => {
                    return Err(WorldApiError::BatchEditFailed {
                        edit_index,
                        reason: Box::new(reason),
                    });
                }
            }
        }

        deduplicate(&mut changed_object_ids);
        staged.revision = self.revision + 1;
        *self = staged;

        Ok(EditResponse {
            revision: self.revision,
            changed_object_ids,
        })
    }

    pub fn validate_layout(&self) -> WorldApiResult<ValidateLayoutResponse> {
        let mut issues = self.scene.validate().map_or_else(
            |errors| {
                errors
                    .0
                    .into_iter()
                    .map(|error| ValidationIssue {
                        path: error.path,
                        message: error.message,
                    })
                    .collect()
            },
            |()| Vec::new(),
        );

        for (floor_index, floor) in self.scene.floors.iter().enumerate() {
            for (object_index, object) in floor.objects.iter().enumerate() {
                if let Err(error) = self.validate_placement(floor_index, object) {
                    issues.push(ValidationIssue {
                        path: format!("floors[{floor_index}].objects[{object_index}]"),
                        message: error.to_string(),
                    });
                }
            }
        }

        Ok(ValidateLayoutResponse {
            valid: issues.is_empty(),
            issues,
        })
    }

    fn apply_edit_unversioned(&mut self, edit: EditCommand) -> WorldApiResult<String> {
        match edit {
            EditCommand::AddObject {
                object_id,
                asset_id,
                floor_id,
                position_m,
                rotation_z_degrees,
                mobility,
            } => self.add_object_unversioned(
                object_id,
                asset_id,
                floor_id,
                position_m,
                rotation_z_degrees,
                mobility,
            ),
            EditCommand::MoveObject {
                object_id,
                position_m,
            } => self.move_object_unversioned(object_id, position_m),
            EditCommand::RotateObject {
                object_id,
                rotation_z_degrees,
            } => self.rotate_object_unversioned(object_id, rotation_z_degrees),
            EditCommand::SetObjectTransform {
                object_id,
                transform,
            } => self.set_object_transform_unversioned(object_id, transform),
            EditCommand::RemoveObject { object_id } => self.remove_object_unversioned(object_id),
        }
    }

    fn add_object_unversioned(
        &mut self,
        object_id: String,
        asset_id: String,
        floor_id: String,
        position_m: Vec3,
        rotation_z_degrees: f64,
        mobility: Mobility,
    ) -> WorldApiResult<String> {
        if object_id.trim().is_empty() {
            return Err(invalid_request("object_id", "value cannot be empty"));
        }
        if self.object_index.contains_key(&object_id) {
            return Err(WorldApiError::DuplicateObjectId { object_id });
        }
        if !self.scene.assets.iter().any(|asset| asset.id == asset_id) {
            return Err(WorldApiError::AssetNotFound { asset_id });
        }
        let floor_index = self
            .scene
            .floors
            .iter()
            .position(|floor| floor.id == floor_id)
            .ok_or(WorldApiError::FloorNotFound { floor_id })?;
        let rotation = z_rotation(rotation_z_degrees)?;
        let transform = Transform3 {
            position_m,
            rotation,
            scale: Vec3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        };
        validate_transform(&transform)?;

        let object = FurnitureObject {
            id: object_id.clone(),
            asset_id,
            transform,
            mobility,
        };
        self.validate_placement(floor_index, &object)?;
        let object_index = self.scene.floors[floor_index].objects.len();
        self.scene.floors[floor_index].objects.push(object);
        self.object_index.insert(
            object_id.clone(),
            ObjectLocation {
                floor_index,
                object_index,
            },
        );

        Ok(object_id)
    }

    fn move_object_unversioned(
        &mut self,
        object_id: String,
        position_m: Vec3,
    ) -> WorldApiResult<String> {
        let mut transform = self.object(&object_id)?.transform;
        transform.position_m = position_m;
        self.set_object_transform_unversioned(object_id, transform)
    }

    fn rotate_object_unversioned(
        &mut self,
        object_id: String,
        rotation_z_degrees: f64,
    ) -> WorldApiResult<String> {
        let mut transform = self.object(&object_id)?.transform;
        transform.rotation = z_rotation(rotation_z_degrees)?;
        self.set_object_transform_unversioned(object_id, transform)
    }

    fn set_object_transform_unversioned(
        &mut self,
        object_id: String,
        transform: Transform3,
    ) -> WorldApiResult<String> {
        validate_transform(&transform)?;
        let location = self.location(&object_id)?;
        let object = &mut self.scene.floors[location.floor_index].objects[location.object_index];
        if object.mobility == Mobility::Fixed {
            return Err(WorldApiError::ObjectFixed { object_id });
        }
        let mut candidate = object.clone();
        candidate.transform = transform;
        self.validate_placement(location.floor_index, &candidate)?;
        self.scene.floors[location.floor_index].objects[location.object_index].transform =
            transform;

        Ok(object_id)
    }

    fn remove_object_unversioned(&mut self, object_id: String) -> WorldApiResult<String> {
        let location = self.location(&object_id)?;
        let object = &self.scene.floors[location.floor_index].objects[location.object_index];
        if object.mobility == Mobility::Fixed {
            return Err(WorldApiError::ObjectFixed { object_id });
        }

        self.scene.floors[location.floor_index]
            .objects
            .remove(location.object_index);
        self.object_index = build_object_index(&self.scene);

        Ok(object_id)
    }

    fn location(&self, object_id: &str) -> WorldApiResult<ObjectLocation> {
        self.object_index
            .get(object_id)
            .copied()
            .ok_or_else(|| WorldApiError::ObjectNotFound {
                object_id: object_id.to_owned(),
            })
    }

    fn object(&self, object_id: &str) -> WorldApiResult<&FurnitureObject> {
        let location = self.location(object_id)?;
        Ok(&self.scene.floors[location.floor_index].objects[location.object_index])
    }

    fn validate_placement(
        &self,
        floor_index: usize,
        object: &FurnitureObject,
    ) -> WorldApiResult<()> {
        let asset = self
            .scene
            .assets
            .iter()
            .find(|asset| asset.id == object.asset_id)
            .ok_or_else(|| WorldApiError::AssetNotFound {
                asset_id: object.asset_id.clone(),
            })?;
        let floor = &self.scene.floors[floor_index];
        validate_candidate_placement(
            object,
            asset,
            &floor.id,
            &floor.rooms,
            &floor.objects,
            &self.scene.assets,
            self.placement_rules,
        )
    }

    fn require_revision(&self, expected_revision: u64) -> WorldApiResult<()> {
        if expected_revision == self.revision {
            Ok(())
        } else {
            Err(WorldApiError::StaleRevision {
                expected_revision,
                current_revision: self.revision,
            })
        }
    }

    fn finish_edit(&mut self, changed_object_ids: Vec<String>) -> EditResponse {
        self.revision += 1;
        EditResponse {
            revision: self.revision,
            changed_object_ids,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetSceneResponse {
    pub revision: u64,
    pub scene: Scene,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListAssetsResponse {
    pub assets: Vec<FurnitureAsset>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddObjectRequest {
    pub expected_revision: u64,
    pub object_id: String,
    pub asset_id: String,
    pub floor_id: String,
    pub position_m: Vec3,
    pub rotation_z_degrees: f64,
    pub mobility: Mobility,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveObjectRequest {
    pub expected_revision: u64,
    pub object_id: String,
    pub position_m: Vec3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RotateObjectRequest {
    pub expected_revision: u64,
    pub object_id: String,
    pub rotation_z_degrees: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetObjectTransformRequest {
    pub expected_revision: u64,
    pub object_id: String,
    pub transform: Transform3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveObjectRequest {
    pub expected_revision: u64,
    pub object_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyEditsRequest {
    pub expected_revision: u64,
    pub edits: Vec<EditCommand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditCommand {
    AddObject {
        object_id: String,
        asset_id: String,
        floor_id: String,
        position_m: Vec3,
        rotation_z_degrees: f64,
        mobility: Mobility,
    },
    MoveObject {
        object_id: String,
        position_m: Vec3,
    },
    RotateObject {
        object_id: String,
        rotation_z_degrees: f64,
    },
    SetObjectTransform {
        object_id: String,
        transform: Transform3,
    },
    RemoveObject {
        object_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditResponse {
    pub revision: u64,
    pub changed_object_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateLayoutResponse {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

pub type WorldApiResult<T> = Result<T, WorldApiError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum WorldApiError {
    #[error("expected revision {expected_revision}, but current revision is {current_revision}")]
    StaleRevision {
        expected_revision: u64,
        current_revision: u64,
    },
    #[error("floor {floor_id} was not found")]
    FloorNotFound { floor_id: String },
    #[error("asset {asset_id} was not found")]
    AssetNotFound { asset_id: String },
    #[error("object {object_id} was not found")]
    ObjectNotFound { object_id: String },
    #[error("object ID {object_id} already exists")]
    DuplicateObjectId { object_id: String },
    #[error("object {object_id} is fixed")]
    ObjectFixed { object_id: String },
    #[error("object {object_id} is not centered inside a room on floor {floor_id}")]
    ObjectOutsideRooms { object_id: String, floor_id: String },
    #[error("object {object_id} does not fit completely inside room {room_id}")]
    ObjectOutsideRoom { object_id: String, room_id: String },
    #[error("object {object_id} overlaps object {conflicting_object_id}")]
    FurnitureOverlap {
        object_id: String,
        conflicting_object_id: String,
    },
    #[error("object {object_id} collides with edge {edge_id} in room {room_id}")]
    WallCollision {
        object_id: String,
        room_id: String,
        edge_id: String,
    },
    #[error("object {object_id} blocks opening {opening_id} on edge {edge_id} in room {room_id}")]
    OpeningClearanceBlocked {
        object_id: String,
        room_id: String,
        edge_id: String,
        opening_id: String,
    },
    #[error("object {object_id} must remain upright and may only rotate around the Z axis")]
    UnsupportedFurnitureRotation { object_id: String },
    #[error("request validation failed")]
    InvalidRequest { issues: Vec<ValidationIssue> },
    #[error("batch edit {edit_index} failed: {reason}")]
    BatchEditFailed {
        edit_index: usize,
        reason: Box<WorldApiError>,
    },
}

fn build_object_index(scene: &Scene) -> HashMap<String, ObjectLocation> {
    let mut index = HashMap::new();
    for (floor_index, floor) in scene.floors.iter().enumerate() {
        for (object_index, object) in floor.objects.iter().enumerate() {
            index.insert(
                object.id.clone(),
                ObjectLocation {
                    floor_index,
                    object_index,
                },
            );
        }
    }
    index
}

fn z_rotation(degrees: f64) -> WorldApiResult<Quaternion> {
    if !degrees.is_finite() {
        return Err(invalid_request(
            "rotation_z_degrees",
            "value must be finite",
        ));
    }
    let half_radians = degrees.to_radians() / 2.0;
    Ok(Quaternion {
        x: 0.0,
        y: 0.0,
        z: half_radians.sin(),
        w: half_radians.cos(),
    })
}

fn validate_transform(transform: &Transform3) -> WorldApiResult<()> {
    let mut issues = Vec::new();
    for (path, value) in [
        ("transform.position_m.x", transform.position_m.x),
        ("transform.position_m.y", transform.position_m.y),
        ("transform.position_m.z", transform.position_m.z),
        ("transform.rotation.x", transform.rotation.x),
        ("transform.rotation.y", transform.rotation.y),
        ("transform.rotation.z", transform.rotation.z),
        ("transform.rotation.w", transform.rotation.w),
    ] {
        if !value.is_finite() {
            issues.push(ValidationIssue {
                path: path.to_owned(),
                message: "value must be finite".to_owned(),
            });
        }
    }
    for (path, value) in [
        ("transform.scale.x", transform.scale.x),
        ("transform.scale.y", transform.scale.y),
        ("transform.scale.z", transform.scale.z),
    ] {
        if !value.is_finite() || value <= 0.0 {
            issues.push(ValidationIssue {
                path: path.to_owned(),
                message: "value must be finite and greater than zero".to_owned(),
            });
        }
    }

    let magnitude_squared = transform.rotation.magnitude_squared();
    if magnitude_squared.is_finite()
        && (magnitude_squared - 1.0).abs() > QUATERNION_NORMALIZATION_TOLERANCE
    {
        issues.push(ValidationIssue {
            path: "transform.rotation".to_owned(),
            message: "quaternion must be normalized".to_owned(),
        });
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(WorldApiError::InvalidRequest { issues })
    }
}

fn validate_placement_rules(rules: PlacementRules) -> WorldApiResult<()> {
    let mut issues = Vec::new();
    for (path, value) in [
        (
            "placement_rules.opening_clearance_depth_m",
            rules.opening_clearance_depth_m,
        ),
        (
            "placement_rules.opening_clearance_side_m",
            rules.opening_clearance_side_m,
        ),
    ] {
        if !value.is_finite() || value < 0.0 {
            issues.push(ValidationIssue {
                path: path.to_owned(),
                message: "value must be finite and nonnegative".to_owned(),
            });
        }
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(WorldApiError::InvalidRequest { issues })
    }
}

fn invalid_request(path: &str, message: &str) -> WorldApiError {
    WorldApiError::InvalidRequest {
        issues: vec![ValidationIssue {
            path: path.to_owned(),
            message: message.to_owned(),
        }],
    }
}

fn deduplicate(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}
