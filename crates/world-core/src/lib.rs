//! Canonical scene types and world rules shared by every tflop component.

mod agent_api;
mod catalog;
mod model;
mod placement;
mod topology;
mod validation;

pub use agent_api::{
    AddObjectRequest, ApplyEditsRequest, CheckEditsRequest, CheckEditsResponse, EditCommand,
    EditResponse, GetSceneResponse, ListAssetsResponse, MoveObjectRequest, RemoveObjectRequest,
    RotateObjectRequest, SetObjectTransformRequest, ValidateLayoutResponse, ValidationIssue, World,
    WorldApiError, WorldApiResult,
};
pub use catalog::default_furniture_catalog;
pub use model::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, FurnitureAsset, FurnitureObject, Mobility,
    Opening, OpeningKind, Quaternion, Room, RoomClassification, RoomDimensions,
    SCENE_SCHEMA_VERSION, Scene, SceneSource, Transform3, Vec3,
};
pub use placement::PlacementRules;
pub use topology::{OpeningConnection, OpeningTopologyError, resolve_opening_connections};
pub use validation::{SceneValidationError, SceneValidationErrors};
