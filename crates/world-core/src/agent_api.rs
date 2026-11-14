use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{FurnitureAsset, Mobility, Scene, Transform3, Vec3};

/// In-memory entry point shared by future Gemini tools and editor commands.
pub struct World {
    scene: Scene,
    revision: u64,
}

impl World {
    #[must_use]
    pub fn new(scene: Scene) -> Self {
        Self { scene, revision: 0 }
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

    pub fn add_object(&mut self, _request: AddObjectRequest) -> WorldApiResult<EditResponse> {
        not_implemented("add_object")
    }

    pub fn move_object(&mut self, _request: MoveObjectRequest) -> WorldApiResult<EditResponse> {
        not_implemented("move_object")
    }

    pub fn rotate_object(&mut self, _request: RotateObjectRequest) -> WorldApiResult<EditResponse> {
        not_implemented("rotate_object")
    }

    pub fn set_object_transform(
        &mut self,
        _request: SetObjectTransformRequest,
    ) -> WorldApiResult<EditResponse> {
        not_implemented("set_object_transform")
    }

    pub fn remove_object(&mut self, _request: RemoveObjectRequest) -> WorldApiResult<EditResponse> {
        not_implemented("remove_object")
    }

    pub fn apply_edits(&mut self, _request: ApplyEditsRequest) -> WorldApiResult<EditResponse> {
        not_implemented("apply_edits")
    }

    pub fn validate_layout(&self) -> WorldApiResult<ValidateLayoutResponse> {
        not_implemented("validate_layout")
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
    #[error("{operation} is not implemented")]
    NotImplemented { operation: String },
}

fn not_implemented<T>(operation: &str) -> WorldApiResult<T> {
    Err(WorldApiError::NotImplemented {
        operation: operation.to_owned(),
    })
}
