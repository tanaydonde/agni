# Fire simulation contract

For the renderer-facing types and Three.js usage, see
[`FIRE_FRONTEND.md`](FIRE_FRONTEND.md).

The hybrid simulation has two responsibilities:

- `SurfaceFireWorld` tracks spatial fire spread across floor cells and furniture hitboxes.
- `tflop-cfast` calculates room smoke, gas conditions, and flow through openings from the resulting heat-release timeline.

## Surface fire

```rust
let mut fire = SurfaceFireWorld::from_scene(
    &scene,
    &FireMaterialCatalog::default(),
    SurfaceFireConfig::default(),
)?;

fire.ignite_object("sofa-1")?;
let frame = fire.step(0.5)?;
```

Each furniture hitbox is divided across its local width, depth, and height into
3D cells no larger than 0.25 metres on an axis. Face-sharing cells exchange
heat, with stronger upward transfer, so fire can move across one side of a
wide object and climb it without igniting the entire hitbox at once.

`ignite_object` ignites a bottom cell near the object's center and retains the
stable `object:<object_id>` node ID. `ignite_object_cell` can instead ignite an
exact `(column, row, layer)`. Other cells use
`object:<object_id>:cell:<column>:<row>:<layer>`. The object's original total
fuel and peak heat release are divided between its cells rather than
multiplied. Every cell currently inherits the object's catalog material; the
data model already stores material state per cell for future mixed materials.

Each combustible floor patch is identified by `floor_id` and grid coordinate.
Nearby nodes exchange heat, ignite after reaching their material threshold,
consume fuel, and become `burned_out`.

`SurfaceFireFrame.active_nodes` contains only heating, burning, or burned-out
nodes. Every entry includes its world position, dimensions, optional
`furniture_cell { column, row, layer, columns, rows, layers }`, material, state,
ignition time, heat-release rate, temperature estimate, and remaining fuel.
`room_heat_release` aggregates the current fire output for the future CFAST
coupling step.

## Combined results

The versioned `SimulationResults.frames[*].surface_fire` field uses the same
node format. This is the frontend contract for placing fire on a furniture
hitbox or floor grid cell. Existing room, fire, and opening fields carry CFAST
conditions in the same timeline.

The current implementation covers floors and furniture. Wall patches and the
conversion of Rust room heat-release timelines into CFAST fire curves remain
future work. Default material values are demo assumptions, not validated fire
engineering data.
