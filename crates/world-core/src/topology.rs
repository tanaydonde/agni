use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{BoundaryEdge, Opening, OpeningKind, Room, Scene, SceneValidationErrors, Vec3};

const MIN_PROBE_DISTANCE_M: f64 = 0.05;
const WALL_CLEARANCE_M: f64 = 0.02;
const OPENING_POSITION_TOLERANCE_M: f64 = 0.05;
const OPENING_SIZE_TOLERANCE_M: f64 = 0.05;
const PARALLEL_DOT_TOLERANCE: f64 = 0.99;

/// One physical airflow path derived from an explicit door, window, passage,
/// or unknown opening in the canonical room boundaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningConnection {
    /// Stable canonical ID. When Matterport reports the opening on both room
    /// boundaries, the first source opening ID is retained here.
    pub id: String,
    /// All source records that were merged into this physical opening.
    pub source_opening_ids: Vec<String>,
    pub floor_id: String,
    pub room_a_id: String,
    /// `None` means the opening leads outside the modeled rooms.
    pub room_b_id: Option<String>,
    pub kind: OpeningKind,
    pub center_m: Vec3,
    pub width_m: f64,
    pub height_m: Option<f64>,
    pub lower_elevation_m: Option<f64>,
}

/// Resolves explicit boundary openings into room-to-room or room-to-outside
/// connections. Solid shared walls never produce a connection.
pub fn resolve_opening_connections(
    scene: &Scene,
) -> Result<Vec<OpeningConnection>, OpeningTopologyError> {
    scene.validate()?;
    require_unique_room_ids(scene)?;

    let occurrences = collect_occurrences(scene);
    let groups = group_occurrences(occurrences)?;
    let mut connections = Vec::with_capacity(groups.len());
    let mut connection_ids = HashSet::new();

    for group in groups {
        let connection = resolve_group(scene, &group)?;
        if !connection_ids.insert(connection.id.clone()) {
            return Err(OpeningTopologyError::DuplicateConnectionId {
                opening_id: connection.id,
            });
        }
        connections.push(connection);
    }

    Ok(connections)
}

#[derive(Debug, Error)]
pub enum OpeningTopologyError {
    #[error(transparent)]
    InvalidScene(#[from] SceneValidationErrors),
    #[error("room ID {room_id} is not unique across the scene")]
    DuplicateRoomId { room_id: String },
    #[error("opening ID {opening_id} identifies more than one physical opening")]
    DuplicateConnectionId { opening_id: String },
    #[error("opening {opening_id} has incompatible duplicate geometry")]
    ConflictingDuplicate { opening_id: String },
    #[error("opening {opening_id} is associated with more than two rooms: {room_ids:?}")]
    TooManyOwners {
        opening_id: String,
        room_ids: Vec<String>,
    },
    #[error("opening {opening_id} has multiple possible neighboring rooms: {room_ids:?}")]
    AmbiguousNeighbor {
        opening_id: String,
        room_ids: Vec<String>,
    },
}

#[derive(Clone, Copy)]
struct OpeningOccurrence<'a> {
    floor_id: &'a str,
    room_id: &'a str,
    edge: &'a BoundaryEdge,
    opening: &'a Opening,
    center: Vec3,
    direction_x: f64,
    direction_y: f64,
}

fn require_unique_room_ids(scene: &Scene) -> Result<(), OpeningTopologyError> {
    let mut ids = HashSet::new();
    for room in scene.floors.iter().flat_map(|floor| floor.rooms.iter()) {
        if !ids.insert(room.id.as_str()) {
            return Err(OpeningTopologyError::DuplicateRoomId {
                room_id: room.id.clone(),
            });
        }
    }
    Ok(())
}

fn collect_occurrences(scene: &Scene) -> Vec<OpeningOccurrence<'_>> {
    scene
        .floors
        .iter()
        .flat_map(|floor| {
            floor.rooms.iter().flat_map(move |room| {
                room.boundary.iter().flat_map(move |edge| {
                    let dx = edge.end.x - edge.start.x;
                    let dy = edge.end.y - edge.start.y;
                    let length = dx.hypot(dy);
                    edge.openings.iter().map(move |opening| OpeningOccurrence {
                        floor_id: &floor.id,
                        room_id: &room.id,
                        edge,
                        opening,
                        center: Vec3 {
                            x: edge.start.x + dx * opening.relative_center,
                            y: edge.start.y + dy * opening.relative_center,
                            z: edge.start.z,
                        },
                        direction_x: dx / length,
                        direction_y: dy / length,
                    })
                })
            })
        })
        .collect()
}

fn group_occurrences<'a>(
    occurrences: Vec<OpeningOccurrence<'a>>,
) -> Result<Vec<Vec<OpeningOccurrence<'a>>>, OpeningTopologyError> {
    let mut groups: Vec<Vec<OpeningOccurrence<'a>>> = Vec::new();
    for occurrence in occurrences {
        let matching_group = groups.iter().position(|group| {
            group[0].floor_id == occurrence.floor_id
                && (group[0].opening.id == occurrence.opening.id
                    || same_physical_opening(group[0], occurrence))
        });
        if let Some(index) = matching_group {
            if !same_physical_opening(groups[index][0], occurrence) {
                return Err(OpeningTopologyError::ConflictingDuplicate {
                    opening_id: occurrence.opening.id.clone(),
                });
            }
            groups[index].push(occurrence);
        } else {
            groups.push(vec![occurrence]);
        }
    }
    Ok(groups)
}

fn same_physical_opening(left: OpeningOccurrence<'_>, right: OpeningOccurrence<'_>) -> bool {
    left.center.distance_to(right.center) <= OPENING_POSITION_TOLERANCE_M
        && (left.opening.width_m - right.opening.width_m).abs() <= OPENING_SIZE_TOLERANCE_M
        && optional_close(left.opening.height_m, right.opening.height_m)
        && optional_close(
            left.opening.lower_elevation_m,
            right.opening.lower_elevation_m,
        )
        && left.opening.kind == right.opening.kind
        && (left.direction_x * right.direction_x + left.direction_y * right.direction_y).abs()
            >= PARALLEL_DOT_TOLERANCE
}

fn optional_close(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => (left - right).abs() <= OPENING_SIZE_TOLERANCE_M,
        _ => true,
    }
}

fn resolve_group(
    scene: &Scene,
    group: &[OpeningOccurrence<'_>],
) -> Result<OpeningConnection, OpeningTopologyError> {
    let representative = group[0];
    let mut owner_ids = group
        .iter()
        .map(|occurrence| occurrence.room_id)
        .collect::<Vec<_>>();
    owner_ids.sort_unstable();
    owner_ids.dedup();
    if owner_ids.len() > 2 {
        return Err(OpeningTopologyError::TooManyOwners {
            opening_id: representative.opening.id.clone(),
            room_ids: owner_ids.into_iter().map(str::to_owned).collect(),
        });
    }

    let (room_a_id, room_b_id) = if owner_ids.len() == 2 {
        (owner_ids[0].to_owned(), Some(owner_ids[1].to_owned()))
    } else {
        let owner_id = owner_ids[0];
        let neighbors = probe_neighbors(scene, representative, owner_id);
        match neighbors.as_slice() {
            [] => (owner_id.to_owned(), None),
            [neighbor] => {
                let mut room_ids = [owner_id, neighbor.as_str()];
                room_ids.sort_unstable();
                (room_ids[0].to_owned(), Some(room_ids[1].to_owned()))
            }
            _ => {
                return Err(OpeningTopologyError::AmbiguousNeighbor {
                    opening_id: representative.opening.id.clone(),
                    room_ids: neighbors,
                });
            }
        }
    };

    let mut source_opening_ids = Vec::new();
    for occurrence in group {
        if !source_opening_ids.contains(&occurrence.opening.id) {
            source_opening_ids.push(occurrence.opening.id.clone());
        }
    }

    Ok(OpeningConnection {
        id: representative.opening.id.clone(),
        source_opening_ids,
        floor_id: representative.floor_id.to_owned(),
        room_a_id,
        room_b_id,
        kind: representative.opening.kind,
        center_m: representative.center,
        width_m: representative.opening.width_m,
        height_m: representative.opening.height_m,
        lower_elevation_m: representative.opening.lower_elevation_m,
    })
}

fn probe_neighbors(
    scene: &Scene,
    occurrence: OpeningOccurrence<'_>,
    owner_id: &str,
) -> Vec<String> {
    let normal_x = -occurrence.direction_y;
    let normal_y = occurrence.direction_x;
    let probe_distance = occurrence
        .edge
        .thickness_m
        .map_or(MIN_PROBE_DISTANCE_M, |thickness| {
            (thickness / 2.0 + WALL_CLEARANCE_M).max(MIN_PROBE_DISTANCE_M)
        });
    let probes = [
        Vec3 {
            x: occurrence.center.x + normal_x * probe_distance,
            y: occurrence.center.y + normal_y * probe_distance,
            z: occurrence.center.z,
        },
        Vec3 {
            x: occurrence.center.x - normal_x * probe_distance,
            y: occurrence.center.y - normal_y * probe_distance,
            z: occurrence.center.z,
        },
    ];

    let mut neighbors = scene
        .floors
        .iter()
        .find(|floor| floor.id == occurrence.floor_id)
        .into_iter()
        .flat_map(|floor| &floor.rooms)
        .filter(|room| room.id != owner_id)
        .filter(|room| probes.into_iter().any(|probe| point_in_room(probe, room)))
        .map(|room| room.id.clone())
        .collect::<Vec<_>>();
    neighbors.sort();
    neighbors.dedup();
    neighbors
}

fn point_in_room(point: Vec3, room: &Room) -> bool {
    let mut inside = false;
    for edge in &room.boundary {
        if (edge.start.y > point.y) != (edge.end.y > point.y) {
            let crossing_x = (edge.end.x - edge.start.x) * (point.y - edge.start.y)
                / (edge.end.y - edge.start.y)
                + edge.start.x;
            if point.x < crossing_x {
                inside = !inside;
            }
        }
    }
    inside
}
