use serde::{Deserialize, Serialize};

pub(crate) const MODEL_QUERY: &str = r#"
query ImportModel($id: ID!) {
  model(id: $id) {
    id
    name
    floors { id label }
    rooms {
      id
      label
      floor { id }
      classifications { label confidence }
      dimensions(units: metric) { areaFloor width depth height }
      dimensionEstimates(units: metric) { area width depth height lowerElevation }
      boundary {
        edges {
          id
          type
          thickness
          centerLineBias
          vertices { id position { x y } }
          openings { id label type width height lowerElevation relativeCenter }
        }
      }
    }
  }
}
"#;

pub(crate) const MESH_ASSETS_QUERY: &str = r#"
query MeshAssets($id: ID!) {
  model(id: $id) {
    assets {
      meshes { filename url }
    }
  }
}
"#;

/// The MatterPak: the furnished, textured OBJ (plus point cloud and plans) as
/// one signed zip.
pub(crate) const MATTERPAK_QUERY: &str = r#"
query MatterPak($id: ID!) {
  model(id: $id) {
    bundle(id: "mp:matterpak") {
      availability
      assets { url }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
pub(crate) struct MatterPakResponse {
    pub data: Option<MatterPakData>,
    #[serde(default)]
    pub errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MatterPakData {
    pub model: Option<MatterPakModel>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MatterPakModel {
    pub bundle: Option<MatterPakBundle>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MatterPakBundle {
    pub availability: Option<String>,
    #[serde(default)]
    pub assets: Vec<BundleAsset>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BundleAsset {
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MeshAssetsResponse {
    pub data: Option<MeshAssetsData>,
    #[serde(default)]
    pub errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MeshAssetsData {
    pub model: Option<MeshAssetsModel>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MeshAssetsModel {
    pub assets: Option<ModelAssets>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ModelAssets {
    #[serde(default)]
    pub meshes: Vec<MeshAsset>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MeshAsset {
    pub filename: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GraphQlRequest<'a> {
    pub query: &'a str,
    pub variables: Variables<'a>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Variables<'a> {
    pub id: &'a str,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GraphQlResponse {
    pub data: Option<GraphQlData>,
    #[serde(default)]
    pub errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GraphQlError {
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GraphQlData {
    pub model: Option<Model>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Model {
    pub id: String,
    pub name: Option<String>,
    #[serde(default)]
    pub floors: Vec<ModelFloor>,
    #[serde(default)]
    pub rooms: Vec<ModelRoom>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ModelFloor {
    pub id: String,
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ModelRoom {
    pub id: String,
    pub label: Option<String>,
    pub floor: Option<IdReference>,
    #[serde(default)]
    pub classifications: Vec<Classification>,
    pub dimensions: Option<LegacyDimensions>,
    #[serde(rename = "dimensionEstimates")]
    pub dimension_estimates: Option<DimensionEstimates>,
    pub boundary: Option<BoundedRegion>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IdReference {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Classification {
    pub label: String,
    pub confidence: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LegacyDimensions {
    #[serde(rename = "areaFloor")]
    pub area_floor: Option<f64>,
    pub width: Option<f64>,
    pub depth: Option<f64>,
    pub height: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DimensionEstimates {
    pub area: Option<f64>,
    pub width: Option<f64>,
    pub depth: Option<f64>,
    pub height: Option<f64>,
    #[serde(rename = "lowerElevation")]
    pub lower_elevation: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BoundedRegion {
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Edge {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub thickness: Option<f64>,
    #[serde(rename = "centerLineBias")]
    pub center_line_bias: Option<f64>,
    #[serde(default)]
    pub vertices: Vec<Vertex>,
    #[serde(default)]
    pub openings: Vec<ModelOpening>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Vertex {
    #[allow(dead_code)]
    pub id: String,
    pub position: Point2,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Point2 {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ModelOpening {
    pub id: String,
    pub label: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub width: f64,
    pub height: Option<f64>,
    #[serde(rename = "lowerElevation")]
    pub lower_elevation: Option<f64>,
    #[serde(rename = "relativeCenter")]
    pub relative_center: f64,
}
