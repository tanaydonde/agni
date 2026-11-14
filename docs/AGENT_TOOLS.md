# Agent tool contract

These are the planned JSON tools for the Gemini agent. The matching request and
response types are exported by the `tflop-world-core` crate and use `snake_case`
JSON fields.

| Tool | Status | Rust method |
| --- | --- | --- |
| `get_scene` | Working | `World::get_scene()` |
| `list_assets` | Working | `World::list_assets()` |
| `add_object` | Contract only | `World::add_object(...)` |
| `move_object` | Contract only | `World::move_object(...)` |
| `rotate_object` | Contract only | `World::rotate_object(...)` |
| `set_object_transform` | Contract only | `World::set_object_transform(...)` |
| `remove_object` | Contract only | `World::remove_object(...)` |
| `apply_edits` | Contract only | `World::apply_edits(...)` |
| `validate_layout` | Contract only | `World::validate_layout()` |

Contract-only methods return:

```json
{
  "code": "not_implemented",
  "operation": "move_object"
}
```

## Example requests

### `add_object`

```json
{
  "expected_revision": 0,
  "object_id": "chair-1",
  "asset_id": "chair-basic",
  "floor_id": "floor-1",
  "position_m": { "x": 2.0, "y": 4.0, "z": 0.0 },
  "rotation_z_degrees": 90.0,
  "mobility": "movable"
}
```

### `move_object`

```json
{
  "expected_revision": 1,
  "object_id": "chair-1",
  "position_m": { "x": 3.0, "y": 4.0, "z": 0.0 }
}
```

### `apply_edits`

```json
{
  "expected_revision": 2,
  "edits": [
    {
      "type": "rotate_object",
      "object_id": "chair-1",
      "rotation_z_degrees": 45.0
    },
    {
      "type": "remove_object",
      "object_id": "chair-2"
    }
  ]
}
```

Successful mutations will eventually return:

```json
{
  "revision": 3,
  "changed_object_ids": ["chair-1", "chair-2"]
}
```

`expected_revision` prevents an agent or editor from overwriting a newer scene
snapshot. Angles in the agent-facing API use degrees around the canonical Z
axis; Rust will convert them to normalized quaternions internally.
