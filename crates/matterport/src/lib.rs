//! Matterport API access and conversion into canonical tflop scenes.

mod client;
mod convert;
mod graphql;

pub use client::MatterportClient;
pub use convert::scene_from_graphql_json;

use tflop_world_core::SceneValidationErrors;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MatterportError {
    #[error("Matterport request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Matterport response was not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Matterport GraphQL returned errors: {0}")]
    GraphQl(String),
    #[error("Matterport response did not contain model {0}")]
    MissingModel(String),
    #[error("room {room_id} references unavailable floor {floor_id}")]
    UnknownFloor { room_id: String, floor_id: String },
    #[error("room {room_id} has no floor reference")]
    MissingFloor { room_id: String },
    #[error("edge {edge_id} must contain exactly two vertices, found {vertex_count}")]
    InvalidEdge {
        edge_id: String,
        vertex_count: usize,
    },
    #[error("the MatterPak for model {model_id} is {availability}, not unlocked")]
    MatterPakUnavailable {
        model_id: String,
        availability: String,
    },
    #[error("the MatterPak could not be read: {0}")]
    MatterPakArchive(#[from] zip::result::ZipError),
    #[error("the MatterPak contains no OBJ mesh")]
    MatterPakMissingMesh,
    #[error(transparent)]
    InvalidScene(#[from] SceneValidationErrors),
}
