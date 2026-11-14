use tflop_matterport::scene_from_graphql_json;
use tflop_world_core::{BoundaryKind, OpeningKind};

#[test]
fn converts_graphql_geometry_into_a_valid_scene() {
    let scene = scene_from_graphql_json(RESPONSE).expect("fixture should convert");

    assert_eq!(scene.id, "model-1");
    assert_eq!(scene.name.as_deref(), Some("Example model"));
    assert!(scene.assets.is_empty());
    assert_eq!(scene.floors.len(), 1);

    let floor = &scene.floors[0];
    assert_eq!(floor.elevation_m, Some(0.02));
    assert!(floor.objects.is_empty());
    assert_eq!(floor.rooms.len(), 1);

    let room = &floor.rooms[0];
    assert_eq!(room.dimensions.as_ref().and_then(|d| d.area_m2), Some(20.0));
    assert_eq!(room.boundary.len(), 1);

    let edge = &room.boundary[0];
    assert_eq!(edge.kind, BoundaryKind::Wall);
    assert_eq!(edge.start.z, 0.02);
    assert_eq!(edge.end.z, 0.02);
    assert_eq!(edge.openings.len(), 1);
    assert_eq!(edge.openings[0].kind, OpeningKind::Door);
    assert!(scene.validate().is_ok());
}

const RESPONSE: &str = r#"
{
  "data": {
    "model": {
      "id": "model-1",
      "name": "Example model",
      "floors": [{ "id": "floor-1", "label": "Floor 1" }],
      "rooms": [{
        "id": "room-1",
        "label": "Office",
        "floor": { "id": "floor-1" },
        "classifications": [{ "label": "Office", "confidence": 0.95 }],
        "dimensions": {
          "areaFloor": 19.5,
          "width": 5.0,
          "depth": 4.0,
          "height": 2.7
        },
        "dimensionEstimates": {
          "area": 20.0,
          "width": 5.1,
          "depth": 4.1,
          "height": 2.8,
          "lowerElevation": 0.02
        },
        "boundary": {
          "edges": [{
            "id": "edge-1",
            "type": "wall",
            "thickness": 0.15,
            "centerLineBias": 0.5,
            "vertices": [
              { "id": "vertex-1", "position": { "x": 0.0, "y": 0.0 } },
              { "id": "vertex-2", "position": { "x": 5.0, "y": 0.0 } }
            ],
            "openings": [{
              "id": "opening-1",
              "label": "door-pivot",
              "type": "doorway",
              "width": 1.0,
              "height": 2.1,
              "lowerElevation": -0.01,
              "relativeCenter": 0.5
            }]
          }]
        }
      }]
    }
  }
}
"#;
