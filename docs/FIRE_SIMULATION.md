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
consume fuel, and become `burned_out`. Two more paths make fires spread the
way room fires do:

- Furniture and the floor within `furniture_floor_radiation_m` (1 m) of it
  heat each other by radiation: a burning cell sends `radiation_fraction`
  (30%) of its output across those gaps, on top of what it passes to the
  cells it touches. So a burning sofa or desk lights the carpet around it,
  and burning carpet lights furniture. Furniture within
  `furniture_radiation_range_m` (2.5 m) also heats other furniture.
- `room_heat_fraction` (0.5%) of every burning node's output heats the whole
  room it is in, shared over the room's unburnt fuel by area, as hot smoke
  under the ceiling does. It never crosses walls.

With these, a sofa fire takes over a 4 x 4 m carpeted room in under a minute.
A burning cell burns out once less than 1% of its fuel is left; its output
tapers with the fuel, so it would otherwise smoulder at next to no heat
forever.

`start_fire` (what a rehearsal uses) lights a small patch of an object: the
cell `ignite_object` lights plus the cells beside it on the same layer.

`SurfaceFireFrame.active_nodes` contains only heating, burning, or burned-out
nodes. Every entry includes its world position, dimensions, optional
`furniture_cell { column, row, layer, columns, rows, layers }`, material, state,
ignition time, heat-release rate, temperature estimate, and remaining fuel.
`room_heat_release` aggregates the current fire output for the future CFAST
coupling step.

## Rust timeline to CFAST fires

After collecting the complete sequence of `SurfaceFireFrame`s, convert it into
room-level CFAST inputs:

```rust
let cfast_timeline = adapt_surface_fire_for_cfast(
    &surface_frames,
    SurfaceFireAdapterConfig::default(),
)?;
```

`CfastSurfaceFireTimeline.room_fires` contains one `CfastRoomFire` for every
room that burns at any point. Each fire has:

- an HRR curve containing one point per Rust frame;
- a fixed HRR-weighted position representing the spatial cells over the run;
- the largest simultaneous burning-cell area observed in that room; and
- the configured CFAST fuel chemistry.

The adapter verifies that frame times are ordered, scene IDs match, node IDs
are unique, and every reported room HRR equals the sum of its spatial cells.
Assign its output to a scenario and run the all-room solver:

```rust
scenario.duration_seconds = cfast_timeline.duration_seconds;
scenario.fires = cfast_timeline.room_fires;
let cfast_results = runner.run_all_rooms(&scene, &scenario)?;
```

The exporter writes every room fire into one CFAST input, and the parser maps
every resulting fire record back to its canonical room ID. The remaining
coupling call produces the frontend timeline:

```rust
let results = merge_cfast_with_surface_fire(
    &scene,
    &cfast_results,
    &surface_frames,
)?;
```

The merged timeline retains every Rust timestamp and copies its active spatial
cells into `frames[*].surface_fire`. If CFAST output is coarser, room, fire,
gas, and opening values are linearly interpolated. Discrete ignition state
uses the last solver observation until the next CFAST frame. The completed
result is validated against the canonical scene before it reaches the
frontend.

## Combined results

The versioned `SimulationResults.frames[*].surface_fire` field uses the same
node format. This is the frontend contract for placing fire on a furniture
hitbox or floor grid cell. Existing room, fire, and opening fields carry CFAST
conditions in the same timeline.

The current implementation covers floors and furniture. Wall patches and the
conversion of Rust room heat-release timelines into CFAST fire curves remain
future work. Default material values are demo assumptions, not validated fire
engineering data.
