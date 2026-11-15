# Frontend fire simulation guide

This is the frontend contract for rendering the Rust surface fire and the
CFAST room conditions on one timeline. The same payload can come from an HTTP request,
WebSocket, or a local fixture.

Today the world server exposes it over HTTP (`crates/server/src/fire.rs`):

```
POST /api/fire/simulate/hybrid
{
  "object_id": "sofa-1",
  "duration_seconds": 180,
  "step_seconds": 0.5,
  "occupants": [/* OccupantSpawn records from /api/agents/spawn_occupants */],
  "crews": [{ "id": "engine-1", "arrival_time_seconds": 10, "route": ["kitchen", "hall"] }],
  "alarm": "detected",        // or "immediate": everyone knows at ignition
  "injured_fraction": 0.1,    // share of occupants who start injured
  "seed": 0                   // repeatable randomness for the run
}
→ { "revision": 4, "excluded_object_ids": [], "results": SimulationResults }
```

Several complete single-run requests can be executed as one bounded parallel
batch. The surface-only route is `POST /api/fire/simulate_n`; use
`POST /api/fire/simulate_n/hybrid` for the same CFAST-backed results as above.

```json
{
  "top_k": 2,
  "representatives_only": true,
  "response_interval_seconds": 2,
  "simulations": [
    { "object_id": "sofa-1", "occupants": [], "crews": [], "seed": 1 },
    { "object_id": "desk-1", "occupants": [], "crews": [], "seed": 2 }
  ]
}
```

Scenario Review uses the surface route with `representatives_only: true`.
All N full-resolution runs still participate in ranking, but the response
contains only the selected K timelines. `response_interval_seconds` samples
only those returned playback copies after ranking; it does not change the
simulation or selection calculations. Review responses also omit the large
`frequencies` and `assignments` arrays that the UI does not display.

The current review requests ten 360-second candidates and K=4. It shows the
four returned timelines one at a time, full size, with one global clock and
1x/2x/4x/8x/16x controls. The arrows, the dots, or the ←/→ keys move between
runs; paging never moves the clock, so every run is seen at the same moment.
Runs that settle before 360 seconds hold their final frame.

The camera is a bird's-eye orbit (`scene/orbitCamera.ts`): drag to orbit
around the building, right-drag (or Shift-drag) to pan, scroll or pinch to
zoom, and **Reset view** to return to the whole-floor framing. A 3D / Textured
switch shows the same run on the 3D mesh or in Matterport's textured
dollhouse, with the fire and people drawn in. Both views share one orbit, so
switching keeps the view. Textured sends each orbit to Showcase with
`Mode.moveTo('mode.dollhouse', { position, rotation })`. Showcase's dollhouse
lens is wider (about 75° against our 50°, read from its projection matrix), so
the distance is scaled to frame the scene the same way. Our layer takes the
pointer, so a click can never fly inside. Showcase's mode buttons, gear and
"AI Enhanced | Help | Terms" footer are hidden.

### Demo and Methods pages

`/demo` and `/methods` present one batch (the editor stays at `/`). **Run
simulation** on `/demo` runs the same ten-scenario batch as Scenario Review
and keeps it in memory, so move between the pages with the links in the
header; a reload drops it. Both pages play the first representative (the top
scenario) and stop the clock at that run's end (`clockScope: 'run'`).

- **Demo → Heatmap** draws `fire_ensemble.heatmap` over the grey defurnished
  mesh and furniture boxes from straight above (`demo/HeatmapView.tsx`). The
  view is fixed.
- **Demo → Optimal route** plays the top scenario in the furnished scan from
  the lead firefighter's eyes (`firstPersonPose` in `demo/cameras.ts`), with
  the fire and people drawn in and the usual playback bar.
- **Methods → Basic** is the editor's 3D view with its fly controls and no
  selection. **Methods → Furnished** watches from one fixed spot: the scan
  position farthest from the fire's origin inside the origin's room
  (`vantagePose`), or a hand-picked spot in `VANTAGE_OVERRIDES`.

Both Matterport views steer Showcase's camera with `showcase/poseDriver.ts`,
which takes the camera over the way free flying does, and take no camera
input. Between scan positions the photo textures smear, as they do when
flying. If Showcase won't hand its camera over, the view stands at the scan
nearest the pose instead.

Known limitations of the review and textured views (accepted for now):

- **Textured load time.** Opening Textured takes about 8–13 s. Showcase
  first steps inside to a scan and then moves to the dollhouse. Going straight
  to the dollhouse from Showcase's intro is faster but leaves the model small
  and far away, so the extra step stays.
- **One Matterport player at a time.** The Building view's player unmounts
  while Scenario Review is open, so returning to Building → Textured or
  Furnished reloads it.
- **Fire inside scanned-furniture models.** The flames are depth-tested like
  any other object, so walls hide them. Burning furniture boxes turn
  see-through so the fire inside shows. The GLB stand-in models for scanned
  furniture (Building → Textured) don't, so fire inside one can be hidden.
- **Controls legend.** It lists only the essential editor controls. Trackpad
  pinch (move to cursor) and two-finger swipe (look around) still work but
  aren't listed.
- **Showcase's footer links are hidden.** Matterport's embed terms may require
  the Terms link on a public deployment; restore it there
  (`HIDDEN_SHOWCASE_CONTROLS` in `showcase/sdk.ts`).

The batch accepts 1–32 distinct inputs. By default its `runs` array stays in
request order even though the server executes up to four runs concurrently. A
successful item has `{ "status": "completed", "index", "simulation" }`,
where `simulation` is the ordinary response above. A failed item instead has
`{ "status": "failed", "index", "http_status", "error" }`; one invalid run
does not discard the successful runs. Every run uses the same scene snapshot
and the response includes that shared `revision`.

When `top_k` is present, the response also includes `fire_ensemble`:

- `frequencies` is the empirical `simulation_count / completed_simulation_count`
  for every burning 0.25 m cell in every one-second time bin;
- `representatives` contains up to `top_k` diverse run indexes plus the count
  and fraction of completed runs assigned to each;
- `heatmap` is the same fire with time and height summed away: one entry per
  0.25 m floor-plan cell (`column`, `row`; the cell spans `column * 0.25` to
  `(column + 1) * 0.25` m in x) with `expected_burning_seconds` (seconds
  burning, averaged over the completed runs) and `burned_fraction` (share of
  runs in which it burned at all). A node covers every cell under its
  footprint. It is small, so `representatives_only` keeps it;
- `assignments` maps every successful run index to its nearest selected run;
- `config.time_tolerance_bins` is used only when comparing runs. Frequency
  values stay on the original timeline.

Representative selection uses greedy time-aware Jaccard medoids: the first
run minimizes total distance to the ensemble, and each later run is the one
that most reduces distance to the nearest already-selected representative.
Only `burning` cells participate. The indexes always refer to the original
`runs` array, including when some batch entries failed.

It ignites the object, advances the surface fire, occupants, and firefighter
crews together frame by frame on a snapshot of the current scene (crews' water
changes how the fire spreads), feeds the resulting room heat-release curves
through CFAST, and returns the merged timeline. A crew may also give
`entry_opening_id` (default: the first exterior exit), `route` (room IDs to
walk into first; the rest are entered nearest first; `search_order` is
accepted as the old name), and `member_count` (default 1). The server uses the CFAST 7.7.7 solver that `cargo build`
compiles from `crates/cfast/vendor/` (this needs `gfortran`), or the
executable in `CFAST_BIN` if that is set. Objects that don't stand inside a room are left out of the run and
listed in `excluded_object_ids`. Errors: 404 `object_not_found`, 422
`object_outside_rooms`, `object_not_combustible`, `simulation_failed`,
`evacuation_failed`, or `firefighting_failed` (an invalid crew); 503
`cfast_not_configured`; and 502 `cfast_failed`. The web app draws the fire
with `apps/web/src/scene/SurfaceFire.tsx`.

### Volumetric flames

The fire is drawn as 3D flames, not as its tiles, in both the 3D canvas and
the Matterport views:

1. `fire/volume.ts` turns the frame's heating and burning cells into a
   smooth 3D heat field on a 0.125 m voxel grid (coarser for very large
   fires). Heat rises from burning cells and thins with height, so a wide
   fire stands taller than a single cell.
2. `fire/volumeRenderer.ts` uploads the field as a 3D texture and draws one
   box around the fire. Its GLSL fragment shader marches each view ray
   through the box and shapes the heat into flames with upward-streaming
   noise. Heating cells smoulder with a dim glow.

Each new simulation frame is eased in over 0.4 s, so the fire grows smoothly
between frames. The flames keep moving while playback is paused, so a view
with fire in it renders every frame. Each pixel is depth-tested at the
nearest flame along its ray: a wall in front of the fire hides it. A camera
inside the flames sees solid flame.

The shader needs WebGL2 (3D textures). It is a fragment-shader ray march
rather than a compute shader: compute shaders need WebGPU, and Matterport's
Showcase renders with its own WebGL three.js.

## Which data to render

The final payload is `SimulationResults`. Choose a frame using
`frame.time_seconds`, then render these parts:

- `frame.surface_fire`: spatial floor and furniture cells from Rust.
- `frame.rooms`: CFAST smoke and gas conditions for each room.
- `frame.openings`: CFAST flow through doors, windows, and passages.
- `frame.fires`: CFAST's room-level representation of the Rust heat output.
- `frame.agents`: occupants advanced by the Rust evacuation engine. Nobody
  reacts at ignition: once a room's fire reaches about 50 kW, whoever is in
  that room raises the alarm, and everyone else hears it later the farther
  they are (about 0.2 s per metre of walk); a fire in an empty room alerts
  nobody until it spreads to someone. Each person's pre-movement delay runs
  from when they hear it. It honors walking speed, routes to exterior doors/passages,
  replans around fire and crowds, and uses `rerouting` when fire makes it
  change exit or no safe path is available. Nobody walks into burning
  cells. Occupants and firefighters move through one
  shared crowd (see "Crowds" below), and each agent's `movement` says how:
  `stopped`, `walking`, `queued` (held up by people in the way), or
  `squeezing` (pressing past someone). The web app tints queued blobs red and
  squeezing blobs amber.
  Occupants who start injured (`role.injured`) lie `injured` and can't move;
  a firefighter carries them out (`carried`, then `exited`). The web app
  lays them on the floor, and carries them over the firefighter's shoulder.
  Firefighters follow the occupants in the same array (`role.type ===
  'firefighter'`, IDs `{crew_id}/{n}`, `/1` leads). A crew doesn't know where
  the fire is: it stages outside until it is called (when the occupants'
  alarm is raised; at ignition if nobody is in the building), arrives
  `arrival_time_seconds` after the call, then walks
  into the rooms on its `route` in order without stopping (`advancing`, at
  1.5 m/s until it knows of fire), sees whatever is in a room once inside it,
  and enters the remaining rooms nearest first. Fire it sees or feels nearby
  comes first (`suppressing`): the hose is aimed at one burning spot at a time
  (`role.nozzle_target_m`, drawn as a blue cone) and wets a 30° cone, so a
  burning sofa takes about 25 s to knock down. Then it picks up injured people
  it has seen (`rescuing`) and carries each to the nearest exit at 0.6 m/s
  (`carrying`), and finally leaves (`withdrawing`, `exited`). Put-out cells
  appear in `surface_fire` as `extinguished`.

### Crowds

Every body (occupant or firefighter) is a circle that moves in 0.1 s
sub-steps; frames default to 0.5 s.

- Doors (and a door-wide chute half a metre either side) and hallways
  narrower than 1.8 m are **single file**: nobody walks abreast or overtakes
  there. People meeting head-on, or someone standing in the way, squeeze
  past each other slowly.
- In open rooms people step around each other, press together a little
  (`CROWD_GIVE_M`), and whoever is closer to their goal goes first and may
  shove others aside, so crowds drain through a door instead of wedging.
  Through a 0.9 m door that comes to roughly 1.2 to 1.4 people per second.
- Firefighters outrank occupants: they usually win a contest for space (they
  give way 20% of the time, by seeded roll), but are always slowed by
  squeezing past people and by walking against an evacuating crowd.
- Each occupant is given a random exit (evenly among the exits they can
  reach, repeatable per seed), so people don't all head for the nearest
  door. If fire cuts their exit off they switch, for good, to the nearest
  exit they can still reach (`rerouting`). Routes avoid crowded cells.

`POST /api/agents/spawn_occupants` takes `{ "count": 1..200, "layout":
"spread" | "crowded" }`. `spread` (the default) has Gemini place people;
`crowded` packs them into the largest rooms without Gemini, with hurried
walking speeds, so queues reliably form.

The Rust-only simulator currently returns `SurfaceFireFrame`, where the same
cell array is named `active_nodes`. When the CFAST adapter is complete,
`active_nodes` is copied into `SimulationResults.frames[*].surface_fire`.
Frontend rendering should use the cell type below in either location.

## TypeScript contract

These interfaces mirror the serialized Rust types. New optional fields may be
added later, but the existing fields and meanings should remain stable.

```ts
export interface SimulationResults {
  schema_version: 1
  scene_id: string
  scenario_id: string
  engine: { name: string; version: string }
  frames: SimulationFrame[]
}

export interface SimulationFrame {
  time_seconds: number
  surface_fire: SurfaceFireNode[]
  rooms: RoomConditions[]
  fires: FireConditions[]
  openings: OpeningConditions[]
}

export type CombustionState =
  | 'unignited'
  | 'heating'
  | 'burning'
  | 'burned_out'
  | 'extinguished' // put out by suppression with fuel left; can reignite

export type SurfaceFireSource =
  | {
      type: 'floor_cell'
      floor_id: string
      coordinate: { column: number; row: number }
    }
  | {
      type: 'furniture'
      object_id: string
    }

export interface FurnitureFireCell {
  column: number
  row: number
  layer: number
  columns: number
  rows: number
  layers: number
}

export interface SurfaceFireNode {
  /** Stable across every frame; use as the React/Three.js object key. */
  id: string
  source: SurfaceFireSource
  room_id: string
  /** Cell center in canonical Z-up metres. */
  position_m: { x: number; y: number; z: number }
  dimensions_m: { width_m: number; depth_m: number; height_m: number }
  /** Present on furniture nodes and absent on floor nodes. */
  furniture_cell?: FurnitureFireCell
  material_id: string
  state: CombustionState
  ignition_time_seconds: number | null
  surface_temperature_c: number
  heat_release_rate_kw: number
  /** Always between 0 and 1. */
  fuel_remaining_fraction: number
}

export interface RoomConditions {
  room_id: string
  smoke_layer_height_m: number
  gauge_pressure_pa: number
  absolute_pressure_pa: number
  upper_layer: GasLayerConditions
  lower_layer: GasLayerConditions
}

export interface GasLayerConditions {
  temperature_c: number
  oxygen_percent: number
  carbon_monoxide_percent: number
  hydrogen_cyanide_percent: number
  optical_density_per_m: number
}

export interface FireConditions {
  fire_id: string
  room_id: string
  ignited: boolean
  expected_hrr_kw: number
  actual_hrr_kw: number
  convective_hrr_kw: number
  flame_height_m: number
  pyrolysis_rate_kg_per_s: number
}

export interface OpeningConditions {
  opening_id: string
  net_flow_kg_per_s: number
  opening_fraction: number
}
```

## Rendering a surface-fire cell

`position_m` is already the center of the exact cell that is heating or
burning. Do not reconstruct its position from `furniture_cell`; those indices
are metadata for selection, debugging, and cell-specific interactions.

The existing scene puts canonical geometry inside a group rotated from Z-up to
Three.js Y-up. Put fire cells in that same group and use canonical values
directly:

```tsx
const Z_UP_TO_Y_UP: [number, number, number] = [-Math.PI / 2, 0, 0]

function SurfaceFire({ nodes }: { nodes: SurfaceFireNode[] }) {
  return (
    <group rotation={Z_UP_TO_Y_UP}>
      {nodes.map((node) => (
        <FireCell key={node.id} node={node} />
      ))}
    </group>
  )
}

function FireCell({ node }: { node: SurfaceFireNode }) {
  const { x, y, z } = node.position_m
  const { width_m, depth_m, height_m } = node.dimensions_m
  const intensity = 1 - Math.exp(-node.heat_release_rate_kw / 20)

  if (node.state === 'burned_out') return null

  return (
    <group position={[x, y, z]}>
      {node.state === 'heating' && (
        <mesh scale={[width_m, depth_m, height_m]}>
          <boxGeometry />
          <meshBasicMaterial
            color="#ff5a24"
            transparent
            opacity={0.1 + 0.25 * intensity}
          />
        </mesh>
      )}

      {node.state === 'burning' && (
        <FireEffect
          width={width_m}
          depth={depth_m}
          height={height_m}
          intensity={intensity}
        />
      )}
    </group>
  )
}
```

`FireEffect` is renderer-owned. It can initially be a transparent orange cone,
sprite, or particle emitter. Drive its size, opacity, particle count, and light
strength with `heat_release_rate_kw`; use `surface_temperature_c` for color and
`fuel_remaining_fraction` for the late-fire fade. The exponential `intensity`
mapping above is only a visual normalization—it must not feed back into the
simulation.

Furniture cell dimensions are in the furniture object's local axes. For a
box-shaped debug overlay, look up `source.object_id` in the canonical `Scene`
and apply that object's yaw to the cell mesh. Flame sprites and point emitters
usually do not need this rotation because `position_m` is already in world
space.

## Choosing frames

Simulation frames are ordered and have strictly increasing timestamps. For a
first implementation, display the newest frame whose timestamp is not later
than the playback clock:

```ts
export function frameAtTime(frames: SimulationFrame[], seconds: number) {
  if (frames.length === 0) return undefined

  let low = 0
  let high = frames.length - 1
  let answer: SimulationFrame | undefined

  while (low <= high) {
    const middle = Math.floor((low + high) / 2)
    if (frames[middle].time_seconds <= seconds) {
      answer = frames[middle]
      low = middle + 1
    } else {
      high = middle - 1
    }
  }
  return answer
}
```

Do not interpolate `state`, `ignition_time_seconds`, or cell identity. If
smoother visuals are needed, interpolate temperature and HRR only between
matching node IDs in neighboring frames. A node is absent before it begins
heating, because unignited cells are intentionally omitted to keep payloads
small.

## Rendering CFAST smoke later

`RoomConditions` is room-level, not cell-level. For each room:

1. Find its canonical room boundary.
2. Use `smoke_layer_height_m` as the lower edge of the upper smoke volume.
3. Use `upper_layer.optical_density_per_m` to control smoke opacity.
4. Use upper/lower temperatures for color or hazard overlays.
5. Use the gas percentages for UI readouts, not flame placement.

The Rust cells answer **where the fire is**. CFAST room conditions answer
**what that fire does to the room air**. Do not create extra flames from the
CFAST `fires` array; it is an aggregate of the same surface fire.

## Example cell payload

```json
{
  "id": "object:sofa-1:cell:0:1:0",
  "source": { "type": "furniture", "object_id": "sofa-1" },
  "room_id": "room-4",
  "position_m": { "x": 3.125, "y": 1.875, "z": 0.125 },
  "dimensions_m": { "width_m": 0.25, "depth_m": 0.25, "height_m": 0.25 },
  "furniture_cell": {
    "column": 0,
    "row": 1,
    "layer": 0,
    "columns": 8,
    "rows": 4,
    "layers": 4
  },
  "material_id": "upholstered_sofa",
  "state": "burning",
  "ignition_time_seconds": 12.5,
  "surface_temperature_c": 610.0,
  "heat_release_rate_kw": 18.2,
  "fuel_remaining_fraction": 0.73
}
```

## Minimal frontend checklist

1. Add the TypeScript interfaces above to the frontend simulation module.
2. Load a simulation payload or fixture.
3. Advance a playback clock and select its current frame.
4. Render `surface_fire` using stable node IDs.
5. Add play, pause, restart, and timeline controls.
6. Add room smoke after the Rust-to-CFAST timeline adapter is complete.

