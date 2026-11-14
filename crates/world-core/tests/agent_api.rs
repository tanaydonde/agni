use tflop_world_core::{
    EditCommand, MoveObjectRequest, SCENE_SCHEMA_VERSION, Scene, SceneSource, Vec3, World,
    WorldApiError,
};

#[test]
fn read_only_scene_tool_returns_revision_zero() {
    let world = World::new(empty_scene());

    let response = world.get_scene();

    assert_eq!(response.revision, 0);
    assert_eq!(response.scene.id, "scene-1");
}

#[test]
fn mutation_stubs_return_structured_not_implemented_errors() {
    let mut world = World::new(empty_scene());

    let error = world
        .move_object(MoveObjectRequest {
            expected_revision: 0,
            object_id: "chair-1".to_owned(),
            position_m: Vec3 {
                x: 1.0,
                y: 2.0,
                z: 0.0,
            },
        })
        .expect_err("move_object is a contract-only stub");

    assert_eq!(
        error,
        WorldApiError::NotImplemented {
            operation: "move_object".to_owned()
        }
    );
    assert_eq!(
        serde_json::to_value(error).expect("error should serialize"),
        serde_json::json!({
            "code": "not_implemented",
            "operation": "move_object"
        })
    );
}

#[test]
fn edit_commands_use_agent_friendly_tagged_json() {
    let command = EditCommand::RotateObject {
        object_id: "chair-1".to_owned(),
        rotation_z_degrees: 45.0,
    };

    assert_eq!(
        serde_json::to_value(command).expect("command should serialize"),
        serde_json::json!({
            "type": "rotate_object",
            "object_id": "chair-1",
            "rotation_z_degrees": 45.0
        })
    );
}

fn empty_scene() -> Scene {
    Scene {
        schema_version: SCENE_SCHEMA_VERSION,
        id: "scene-1".to_owned(),
        name: Some("Empty scene".to_owned()),
        source: SceneSource {
            provider: "fixture".to_owned(),
            external_id: "scene-1".to_owned(),
        },
        assets: vec![],
        floors: vec![],
    }
}
