use std::collections::HashMap;

use tflop_world_core::{
    BoundaryEdge, BoundaryKind, Floor, Opening, OpeningKind, Room, RoomClassification,
    RoomDimensions, SCENE_SCHEMA_VERSION, Scene, SceneSource, Vec3, default_furniture_catalog,
};

use crate::{
    MatterportError,
    graphql::{Edge, GraphQlResponse, LegacyDimensions, Model, ModelOpening, ModelRoom},
};

/// Converts a saved Matterport GraphQL response into a validated canonical scene.
/// Useful for fixtures and offline debugging; production imports use
/// [`crate::MatterportClient`].
pub fn scene_from_graphql_json(json: &str) -> Result<Scene, MatterportError> {
    let response: GraphQlResponse = serde_json::from_str(json)?;
    let requested_id = response
        .data
        .as_ref()
        .and_then(|data| data.model.as_ref())
        .map_or_else(|| "unknown".to_owned(), |model| model.id.clone());
    scene_from_response(response, &requested_id)
}

pub(crate) fn scene_from_response(
    response: GraphQlResponse,
    requested_id: &str,
) -> Result<Scene, MatterportError> {
    if !response.errors.is_empty() {
        let messages = response
            .errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(MatterportError::GraphQl(messages));
    }

    let model = response
        .data
        .and_then(|data| data.model)
        .ok_or_else(|| MatterportError::MissingModel(requested_id.to_owned()))?;

    convert_model(model)
}

fn convert_model(model: Model) -> Result<Scene, MatterportError> {
    let mut rooms_by_floor: HashMap<&str, Vec<&ModelRoom>> = HashMap::new();
    for room in &model.rooms {
        let floor_id = room
            .floor
            .as_ref()
            .ok_or_else(|| MatterportError::MissingFloor {
                room_id: room.id.clone(),
            })?
            .id
            .as_str();
        rooms_by_floor.entry(floor_id).or_default().push(room);
    }

    let known_floors = model
        .floors
        .iter()
        .map(|floor| floor.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for (floor_id, rooms) in &rooms_by_floor {
        if !known_floors.contains(floor_id) {
            return Err(MatterportError::UnknownFloor {
                room_id: rooms[0].id.clone(),
                floor_id: (*floor_id).to_owned(),
            });
        }
    }

    let mut floors = Vec::with_capacity(model.floors.len());
    for source_floor in &model.floors {
        let source_rooms = rooms_by_floor
            .get(source_floor.id.as_str())
            .map_or(&[][..], Vec::as_slice);
        let elevation_m = median(
            source_rooms
                .iter()
                .filter_map(|room| room.dimension_estimates.as_ref()?.lower_elevation),
        );
        // Matterport vertices are 2D. Use one robust floor elevation for all
        // room boundaries so shared walls do not develop tiny vertical seams
        // from per-room scan noise.
        let boundary_z = elevation_m.unwrap_or(0.0);
        let rooms = source_rooms
            .iter()
            .map(|room| convert_room(room, boundary_z))
            .collect::<Result<Vec<_>, _>>()?;

        floors.push(Floor {
            id: source_floor.id.clone(),
            label: nonempty(source_floor.label.clone()),
            elevation_m,
            rooms,
            objects: Vec::new(),
        });
    }

    let scene = Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: model.id.clone(),
        name: nonempty(model.name),
        source: SceneSource {
            provider: "matterport".to_owned(),
            external_id: model.id,
        },
        assets: default_furniture_catalog(),
        floors,
    };
    scene.validate()?;

    Ok(scene)
}

fn convert_room(room: &ModelRoom, boundary_z: f64) -> Result<Room, MatterportError> {
    let boundary = room
        .boundary
        .as_ref()
        .map_or(&[][..], |boundary| boundary.edges.as_slice())
        .iter()
        .map(|edge| convert_edge(edge, boundary_z))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Room {
        id: room.id.clone(),
        label: nonempty(room.label.clone()),
        classifications: room
            .classifications
            .iter()
            .map(|classification| RoomClassification {
                label: classification.label.clone(),
                confidence: classification.confidence,
            })
            .collect(),
        dimensions: convert_dimensions(room),
        boundary,
    })
}

fn convert_dimensions(room: &ModelRoom) -> Option<RoomDimensions> {
    match (&room.dimension_estimates, &room.dimensions) {
        (None, None) => None,
        (estimates, legacy) => Some(RoomDimensions {
            area_m2: estimates
                .as_ref()
                .and_then(|value| value.area)
                .or_else(|| legacy.as_ref().and_then(|value| value.area_floor)),
            width_m: preferred(
                estimates.as_ref().and_then(|value| value.width),
                legacy,
                |v| v.width,
            ),
            depth_m: preferred(
                estimates.as_ref().and_then(|value| value.depth),
                legacy,
                |v| v.depth,
            ),
            height_m: preferred(
                estimates.as_ref().and_then(|value| value.height),
                legacy,
                |v| v.height,
            ),
            lower_elevation_m: estimates.as_ref().and_then(|value| value.lower_elevation),
        }),
    }
}

fn preferred(
    estimate: Option<f64>,
    legacy: &Option<LegacyDimensions>,
    select: impl FnOnce(&LegacyDimensions) -> Option<f64>,
) -> Option<f64> {
    estimate.or_else(|| legacy.as_ref().and_then(select))
}

fn convert_edge(edge: &Edge, boundary_z: f64) -> Result<BoundaryEdge, MatterportError> {
    let [start, end] = edge.vertices.as_slice() else {
        return Err(MatterportError::InvalidEdge {
            edge_id: edge.id.clone(),
            vertex_count: edge.vertices.len(),
        });
    };

    Ok(BoundaryEdge {
        id: edge.id.clone(),
        kind: match edge.kind.as_str() {
            "wall" => BoundaryKind::Wall,
            "invisible" => BoundaryKind::Invisible,
            _ => BoundaryKind::Unknown,
        },
        start: Vec3 {
            x: start.position.x,
            y: start.position.y,
            z: boundary_z,
        },
        end: Vec3 {
            x: end.position.x,
            y: end.position.y,
            z: boundary_z,
        },
        thickness_m: edge.thickness,
        center_line_bias: edge.center_line_bias,
        openings: edge.openings.iter().map(convert_opening).collect(),
    })
}

fn convert_opening(opening: &ModelOpening) -> Opening {
    Opening {
        id: opening.id.clone(),
        label: nonempty(opening.label.clone()),
        kind: match opening.kind.as_str() {
            "doorway" => OpeningKind::Door,
            "window" => OpeningKind::Window,
            "passage" => OpeningKind::Passage,
            _ => OpeningKind::Unknown,
        },
        width_m: opening.width,
        height_m: opening.height,
        lower_elevation_m: opening.lower_elevation,
        relative_center: opening.relative_center,
    }
}

fn median(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let midpoint = values.len() / 2;
    if values.len() % 2 == 0 {
        Some((values[midpoint - 1] + values[midpoint]) / 2.0)
    } else {
        Some(values[midpoint])
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}
