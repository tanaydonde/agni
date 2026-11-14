//! Canonical scene types and world rules shared by every tflop component.

mod model;
mod validation;

pub use model::{
    BoundaryEdge, BoundaryKind, Dimensions3, Floor, FurnitureAsset, FurnitureObject, Mobility,
    Opening, OpeningKind, Quaternion, Room, RoomClassification, RoomDimensions,
    SCENE_SCHEMA_VERSION, Scene, SceneSource, Transform3, Vec3,
};
pub use validation::{SceneValidationError, SceneValidationErrors};
