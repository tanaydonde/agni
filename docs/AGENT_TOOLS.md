# Agent tool contract

These are the world-editing JSON tools implemented today. The server exposes
each one at `/api/world/<tool>`. The matching request and response types are
exported by the `tflop-world-core` crate and use `snake_case` JSON fields.

In the fire preplan product, these tools edit **interior obstacles**
(furniture, storage, fixed equipment) so the scene can be corrected to match a
walkthrough. The preplan agent's full tool surface (`get_property`,
`propose_findings`, `create_scenario`, `run_scenario`, `compare_scenarios`) is
planned and specified in `PREPLAN_AGENT.md`.

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
| `check_edits` | Working | `World::check_edits(...)` |
| `validate_layout` | Working | `World::validate_layout()` |
| `spawn_occupants` | Working | Gemini proposes exactly N occupants; `Simulation::spawn_occupants` validates them and retries up to three attempts |
| `generate_fire_scenarios` | Working | Gemini chooses N fire origins and occupied rooms; Rust resolves and validates exact occupant positions |

`POST /api/agents/spawn_occupants` takes `{ "count": N }`. `count` is a target:
when the modeled floor has fewer collision-free positions, Rust uses the
verified safe capacity instead of rejecting the run. Rust first derives a
catalog of collision-free, mutually separated spawn sites from the simulation
grid. Gemini must call the `spawn_occupants` function with the complete batch
and select only those site IDs; Rust resolves the IDs back to exact world
coordinates and validates the result. A rejected batch is returned to Gemini
as field-level feedback; after three failed proposals the endpoint returns
`occupant_generation_failed`. Set `GEMINI_API_KEY` and optionally
`GEMINI_MODEL` in `.env`.

`POST /api/agents/generate_fire_scenarios` takes
`{ "count": N, "occupant_count": M }`. Gemini chooses only from verified
furniture-object and room IDs. Rust converts each room-level choice into M
collision-free occupant spawns, rejects invented IDs and duplicate scenarios,
and retries rejected tool calls up to three times. Scenario Review feeds the
response into `/api/fire/simulate_n` and requests only compact top-K playback
timelines; no manually selected fire origin is required.

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

Furniture always stands on the floor. The asset origin is the bottom center,
so the world sets `position_m.z` to the floor's `elevation_m` on every add and
move; any `z` you send is ignored. Furniture may touch but not overlap other
furniture. It must remain upright and may only rotate around the canonical Z
axis (`unsupported_furniture_rotation` otherwise). `validate_layout` also
reports objects that are off the floor or tilted. The default door/passage
clearance extends 0.75 m from each side of its wall and 0.15 m past each side
of the opening; Rust callers may override these values with `PlacementRules`
and `World::with_placement_rules`.

`check_edits` takes `{ "edits": [...] }` (no revision) and dry-runs the batch
exactly like `apply_edits` without changing anything. It returns
`{ "valid": true, "error": null }` or `{ "valid": false, "error": { ... } }`,
where `error` is what `apply_edits` would have returned. Use it to test a
placement before committing it.

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
