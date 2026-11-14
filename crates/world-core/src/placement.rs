use crate::{
    BoundaryEdge, BoundaryKind, FurnitureAsset, FurnitureObject, OpeningKind, Room, Transform3,
    WorldApiError,
};

const GEOMETRY_EPSILON: f64 = 1.0e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementRules {
    /// Empty distance measured perpendicular to a door or passage, on each side
    /// of its boundary edge.
    pub opening_clearance_depth_m: f64,
    /// Extra empty distance beyond both sides of a door or passage.
    pub opening_clearance_side_m: f64,
}

impl Default for PlacementRules {
    fn default() -> Self {
        Self {
            opening_clearance_depth_m: 0.75,
            opening_clearance_side_m: 0.15,
        }
    }
}

#[derive(Clone, Copy)]
struct Point2 {
    x: f64,
    y: f64,
}

pub(crate) fn validate_candidate_placement(
    object: &FurnitureObject,
    asset: &FurnitureAsset,
    floor_id: &str,
    rooms: &[Room],
    other_objects: &[FurnitureObject],
    assets: &[FurnitureAsset],
    rules: PlacementRules,
) -> Result<(), WorldApiError> {
    let candidate_footprint = footprint(&object.id, asset, &object.transform)?;
    validate_room_geometry(object, floor_id, rooms, &candidate_footprint, rules)?;

    for other in other_objects {
        if other.id == object.id {
            continue;
        }
        let Some(other_asset) = assets.iter().find(|asset| asset.id == other.asset_id) else {
            continue;
        };
        let other_footprint = footprint(&other.id, other_asset, &other.transform)?;
        if convex_polygons_intersect(&candidate_footprint, &other_footprint, false) {
            return Err(WorldApiError::FurnitureOverlap {
                object_id: object.id.clone(),
                conflicting_object_id: other.id.clone(),
            });
        }
    }

    Ok(())
}

fn validate_room_geometry(
    object: &FurnitureObject,
    floor_id: &str,
    rooms: &[Room],
    footprint: &[Point2; 4],
    rules: PlacementRules,
) -> Result<(), WorldApiError> {
    let center = Point2 {
        x: object.transform.position_m.x,
        y: object.transform.position_m.y,
    };
    let containing_rooms = rooms
        .iter()
        .filter(|room| point_in_room(center, room))
        .collect::<Vec<_>>();

    if containing_rooms.is_empty() {
        return Err(WorldApiError::ObjectOutsideRooms {
            object_id: object.id.clone(),
            floor_id: floor_id.to_owned(),
        });
    }

    let mut first_error = None;
    for room in containing_rooms {
        match validate_in_room(object, room, footprint, rules) {
            Ok(()) => return Ok(()),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }

    Err(first_error.expect("a containing room always produces a result"))
}

fn validate_in_room(
    object: &FurnitureObject,
    room: &Room,
    footprint: &[Point2; 4],
    rules: PlacementRules,
) -> Result<(), WorldApiError> {
    for edge in &room.boundary {
        for opening in &edge.openings {
            if !matches!(opening.kind, OpeningKind::Door | OpeningKind::Passage) {
                continue;
            }
            let zone =
                opening_clearance_polygon(edge, opening.relative_center, opening.width_m, rules);
            if convex_polygons_intersect(footprint, &zone, true) {
                return Err(WorldApiError::OpeningClearanceBlocked {
                    object_id: object.id.clone(),
                    room_id: room.id.clone(),
                    edge_id: edge.id.clone(),
                    opening_id: opening.id.clone(),
                });
            }
        }
    }

    for edge in &room.boundary {
        let start = point(edge.start.x, edge.start.y);
        let end = point(edge.end.x, edge.end.y);
        if polygon_intersects_segment(footprint, start, end, true) {
            return Err(wall_collision(object, room, edge));
        }
    }

    if !footprint.iter().all(|corner| point_in_room(*corner, room)) {
        return Err(WorldApiError::ObjectOutsideRoom {
            object_id: object.id.clone(),
            room_id: room.id.clone(),
        });
    }

    for edge in &room.boundary {
        if edge.kind != BoundaryKind::Wall {
            continue;
        }
        let thickness = edge.thickness_m.unwrap_or(0.0);
        if thickness <= GEOMETRY_EPSILON {
            continue;
        }
        let wall = rectangle_around_segment(
            point(edge.start.x, edge.start.y),
            point(edge.end.x, edge.end.y),
            thickness / 2.0,
            0.0,
        );
        if convex_polygons_intersect(footprint, &wall, true) {
            return Err(wall_collision(object, room, edge));
        }
    }

    Ok(())
}

fn wall_collision(object: &FurnitureObject, room: &Room, edge: &BoundaryEdge) -> WorldApiError {
    WorldApiError::WallCollision {
        object_id: object.id.clone(),
        room_id: room.id.clone(),
        edge_id: edge.id.clone(),
    }
}

fn footprint(
    object_id: &str,
    asset: &FurnitureAsset,
    transform: &Transform3,
) -> Result<[Point2; 4], WorldApiError> {
    let rotation = transform.rotation;
    let vertical_x = 2.0 * (rotation.x * rotation.z + rotation.w * rotation.y);
    let vertical_y = 2.0 * (rotation.y * rotation.z - rotation.w * rotation.x);
    let vertical_z = 1.0 - 2.0 * (rotation.x * rotation.x + rotation.y * rotation.y);
    if vertical_x.abs() > 1.0e-6 || vertical_y.abs() > 1.0e-6 || vertical_z < 1.0 - 1.0e-6 {
        return Err(WorldApiError::UnsupportedFurnitureRotation {
            object_id: object_id.to_owned(),
        });
    }

    let yaw = (2.0 * (rotation.w * rotation.z + rotation.x * rotation.y))
        .atan2(1.0 - 2.0 * (rotation.y * rotation.y + rotation.z * rotation.z));
    let half_width = asset.dimensions_m.width_m * transform.scale.x / 2.0;
    let half_depth = asset.dimensions_m.depth_m * transform.scale.y / 2.0;
    let cosine = yaw.cos();
    let sine = yaw.sin();
    let center = point(transform.position_m.x, transform.position_m.y);

    Ok([
        rotated_corner(center, -half_width, -half_depth, cosine, sine),
        rotated_corner(center, half_width, -half_depth, cosine, sine),
        rotated_corner(center, half_width, half_depth, cosine, sine),
        rotated_corner(center, -half_width, half_depth, cosine, sine),
    ])
}

fn rotated_corner(center: Point2, local_x: f64, local_y: f64, cosine: f64, sine: f64) -> Point2 {
    point(
        center.x + local_x * cosine - local_y * sine,
        center.y + local_x * sine + local_y * cosine,
    )
}

fn point_in_room(point: Point2, room: &Room) -> bool {
    let mut inside = false;
    for edge in &room.boundary {
        let start = Point2 {
            x: edge.start.x,
            y: edge.start.y,
        };
        let end = Point2 {
            x: edge.end.x,
            y: edge.end.y,
        };
        if point_on_segment(point, start, end) {
            return false;
        }
        if (start.y > point.y) != (end.y > point.y) {
            let crossing_x = (end.x - start.x) * (point.y - start.y) / (end.y - start.y) + start.x;
            if point.x < crossing_x {
                inside = !inside;
            }
        }
    }
    inside
}

fn opening_clearance_polygon(
    edge: &BoundaryEdge,
    relative_center: f64,
    width_m: f64,
    rules: PlacementRules,
) -> [Point2; 4] {
    let start = point(edge.start.x, edge.start.y);
    let end = point(edge.end.x, edge.end.y);
    let center = point(
        start.x + (end.x - start.x) * relative_center,
        start.y + (end.y - start.y) * relative_center,
    );
    let direction = normalized_direction(start, end);
    let half_length = width_m / 2.0 + rules.opening_clearance_side_m;
    let along_start = point(
        center.x - direction.x * half_length,
        center.y - direction.y * half_length,
    );
    let along_end = point(
        center.x + direction.x * half_length,
        center.y + direction.y * half_length,
    );
    rectangle_around_segment(along_start, along_end, rules.opening_clearance_depth_m, 0.0)
}

fn rectangle_around_segment(
    start: Point2,
    end: Point2,
    half_thickness: f64,
    end_extension: f64,
) -> [Point2; 4] {
    let direction = normalized_direction(start, end);
    let normal = point(-direction.y, direction.x);
    let extended_start = point(
        start.x - direction.x * end_extension,
        start.y - direction.y * end_extension,
    );
    let extended_end = point(
        end.x + direction.x * end_extension,
        end.y + direction.y * end_extension,
    );
    [
        point(
            extended_start.x + normal.x * half_thickness,
            extended_start.y + normal.y * half_thickness,
        ),
        point(
            extended_end.x + normal.x * half_thickness,
            extended_end.y + normal.y * half_thickness,
        ),
        point(
            extended_end.x - normal.x * half_thickness,
            extended_end.y - normal.y * half_thickness,
        ),
        point(
            extended_start.x - normal.x * half_thickness,
            extended_start.y - normal.y * half_thickness,
        ),
    ]
}

fn normalized_direction(start: Point2, end: Point2) -> Point2 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = dx.hypot(dy);
    point(dx / length, dy / length)
}

fn convex_polygons_intersect(a: &[Point2], b: &[Point2], touching_counts: bool) -> bool {
    for polygon in [a, b] {
        for index in 0..polygon.len() {
            let current = polygon[index];
            let next = polygon[(index + 1) % polygon.len()];
            let axis = point(-(next.y - current.y), next.x - current.x);
            let (a_min, a_max) = project(a, axis);
            let (b_min, b_max) = project(b, axis);
            let separated = if touching_counts {
                a_max < b_min - GEOMETRY_EPSILON || b_max < a_min - GEOMETRY_EPSILON
            } else {
                a_max <= b_min + GEOMETRY_EPSILON || b_max <= a_min + GEOMETRY_EPSILON
            };
            if separated {
                return false;
            }
        }
    }
    true
}

fn project(polygon: &[Point2], axis: Point2) -> (f64, f64) {
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for point in polygon {
        let projection = point.x * axis.x + point.y * axis.y;
        minimum = minimum.min(projection);
        maximum = maximum.max(projection);
    }
    (minimum, maximum)
}

fn polygon_intersects_segment(
    polygon: &[Point2],
    start: Point2,
    end: Point2,
    touching_counts: bool,
) -> bool {
    (0..polygon.len()).any(|index| {
        segments_intersect(
            polygon[index],
            polygon[(index + 1) % polygon.len()],
            start,
            end,
            touching_counts,
        )
    })
}

fn segments_intersect(a: Point2, b: Point2, c: Point2, d: Point2, touching_counts: bool) -> bool {
    let ab_c = orientation(a, b, c);
    let ab_d = orientation(a, b, d);
    let cd_a = orientation(c, d, a);
    let cd_b = orientation(c, d, b);
    if ((ab_c > GEOMETRY_EPSILON && ab_d < -GEOMETRY_EPSILON)
        || (ab_c < -GEOMETRY_EPSILON && ab_d > GEOMETRY_EPSILON))
        && ((cd_a > GEOMETRY_EPSILON && cd_b < -GEOMETRY_EPSILON)
            || (cd_a < -GEOMETRY_EPSILON && cd_b > GEOMETRY_EPSILON))
    {
        return true;
    }
    touching_counts
        && ((ab_c.abs() <= GEOMETRY_EPSILON && point_on_segment(c, a, b))
            || (ab_d.abs() <= GEOMETRY_EPSILON && point_on_segment(d, a, b))
            || (cd_a.abs() <= GEOMETRY_EPSILON && point_on_segment(a, c, d))
            || (cd_b.abs() <= GEOMETRY_EPSILON && point_on_segment(b, c, d)))
}

fn point_on_segment(point: Point2, start: Point2, end: Point2) -> bool {
    orientation(start, end, point).abs() <= GEOMETRY_EPSILON
        && point.x >= start.x.min(end.x) - GEOMETRY_EPSILON
        && point.x <= start.x.max(end.x) + GEOMETRY_EPSILON
        && point.y >= start.y.min(end.y) - GEOMETRY_EPSILON
        && point.y <= start.y.max(end.y) + GEOMETRY_EPSILON
}

fn orientation(a: Point2, b: Point2, c: Point2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

const fn point(x: f64, y: f64) -> Point2 {
    Point2 { x, y }
}
