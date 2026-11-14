use serde::{Deserialize, Serialize};

/// Current version of the serialized canonical scene format.
pub const SCENE_SCHEMA_VERSION: u32 = 1;

/// A complete, source-independent representation of one imported space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub schema_version: u32,
    pub id: String,
    pub name: Option<String>,
    pub source: SceneSource,
    pub assets: Vec<FurnitureAsset>,
    pub floors: Vec<Floor>,
}

/// Identifies where a canonical scene originated without coupling the model to
/// a provider-specific API response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneSource {
    pub provider: String,
    pub external_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Floor {
    pub id: String,
    pub label: Option<String>,
    pub elevation_m: Option<f64>,
    pub rooms: Vec<Room>,
    pub objects: Vec<FurnitureObject>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub label: Option<String>,
    pub classifications: Vec<RoomClassification>,
    pub dimensions: Option<RoomDimensions>,
    pub boundary: Vec<BoundaryEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomClassification {
    pub label: String,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomDimensions {
    pub area_m2: Option<f64>,
    pub width_m: Option<f64>,
    pub depth_m: Option<f64>,
    pub height_m: Option<f64>,
    pub lower_elevation_m: Option<f64>,
}

/// Metadata for a normalized GLB/GLTF asset.
///
/// Assets use metres and an origin at the bottom center. `Transform3` always
/// operates in the canonical Z-up world; renderers convert that basis once at
/// their boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FurnitureAsset {
    pub id: String,
    pub category: String,
    pub model_uri: Option<String>,
    pub dimensions_m: Dimensions3,
}

/// One placed instance of a reusable furniture asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FurnitureObject {
    pub id: String,
    pub asset_id: String,
    pub transform: Transform3,
    pub mobility: Mobility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mobility {
    Fixed,
    Movable,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dimensions3 {
    pub width_m: f64,
    pub depth_m: f64,
    pub height_m: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform3 {
    pub position_m: Vec3,
    pub rotation: Quaternion,
    pub scale: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Quaternion {
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    #[must_use]
    pub fn magnitude_squared(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryEdge {
    pub id: String,
    pub kind: BoundaryKind,
    pub start: Vec3,
    pub end: Vec3,
    pub thickness_m: Option<f64>,
    pub center_line_bias: Option<f64>,
    pub openings: Vec<Opening>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    Wall,
    Invisible,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    pub id: String,
    pub label: Option<String>,
    pub kind: OpeningKind,
    pub width_m: f64,
    pub height_m: Option<f64>,
    pub lower_elevation_m: Option<f64>,
    /// Position of the opening center along its edge, from 0.0 to 1.0.
    pub relative_center: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningKind {
    Door,
    Window,
    Passage,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    #[must_use]
    pub fn distance_to(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;

        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}
