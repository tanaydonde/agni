# Agent tool contract

These are the planned JSON tools for the Gemini agent. The matching request and
response types are exported by the `tflop-world-core` crate and use `snake_case`
JSON fields.

| Tool | Status | Rust method |
| --- | --- | --- |
| `get_scene` | Working | `World::get_scene()` |
| `list_assets` | Working | `World::list_assets()` |
| `add_object` | Working | `World::add_object(...)` |
| `move_object` | Working | `World::move_object(...)` |
| `rotate_object` | Working | `World::rotate_object(...)` |
| `set_object_transform` | Working | `World::set_object_transform(...)` |
| `remove_object` | Working | `World::remove_object(...)` |
| `apply_edits` | Working | `World::apply_edits(...)` |
| `validate_layout` | Working | `World::validate_layout()` |

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

Successful mutations return:

```json
{
  "revision": 3,
  "changed_object_ids": ["chair-1", "chair-2"]
}
```

`expected_revision` prevents an agent or editor from overwriting a newer scene
snapshot. Angles in the agent-facing API use degrees around the canonical Z
axis; Rust will convert them to normalized quaternions internally.

Edits validate IDs, asset and floor references, transforms, mobility, revisions,
room containment, furniture overlap, wall collision, and door/passage clearance.
Each mutation validates only the proposed object against its floor geometry and
the other objects on that floor. Invalid edits leave the object and world
revision unchanged. `validate_layout` runs both structural and placement checks
across the complete scene.

Furniture may touch but not overlap other furniture. It must remain upright and
may only rotate around the canonical Z axis. The default door/passage clearance
extends 0.75 m from each side of its wall and 0.15 m past each side of the
opening; Rust callers may override these values with `PlacementRules` and
`World::with_placement_rules`.

## Error codes

Errors are serialized with a `code` field. Current codes are:

- `stale_revision`
- `floor_not_found`
- `asset_not_found`
- `object_not_found`
- `duplicate_object_id`
- `object_fixed`
- `object_outside_rooms`
- `object_outside_room`
- `furniture_overlap`
- `wall_collision`
- `opening_clearance_blocked`
- `unsupported_furniture_rotation`
- `invalid_request`
- `batch_edit_failed`

A failed batch includes its zero-based `edit_index` and the underlying error in
`reason`. Failed commands do not increment the revision or partially change the
world.
