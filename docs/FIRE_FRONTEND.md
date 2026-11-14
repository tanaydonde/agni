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
  "crews": [{ "id": "engine-1", "member_count": 2, "arrival_time_seconds": 30 }]
}
→ { "revision": 4, "excluded_object_ids": [], "results": SimulationResults }
```

It ignites the object, advances the surface fire, occupants, and firefighter
crews together frame by frame on a snapshot of the current scene (crews' water
changes how the fire spreads), feeds the resulting room heat-release curves
through CFAST, and returns the merged timeline. A crew may also give
`entry_opening_id` (default: the first exterior exit) and `search_order` (room
IDs to search first; the rest are searched nearest first). The server uses the CFAST 7.7.7 solver that `cargo build`
compiles from `crates/cfast/vendor/` (this needs `gfortran`), or the
executable in `CFAST_BIN` if that is set. Objects that don't stand inside a room are left out of the run and
listed in `excluded_object_ids`. Errors: 404 `object_not_found`, 422
`object_outside_rooms`, `object_not_combustible`, `simulation_failed`,
`evacuation_failed`, or `firefighting_failed` (an invalid crew); 503
`cfast_not_configured`; and 502 `cfast_failed`. The web app's placeholder
renderer is `apps/web/src/scene/SurfaceFire.tsx`.

## Which data to render

The final payload is `SimulationResults`. Choose a frame using
`frame.time_seconds`, then render these parts:

- `frame.surface_fire`: spatial floor and furniture cells from Rust.
- `frame.rooms`: CFAST smoke and gas conditions for each room.
- `frame.openings`: CFAST flow through doors, windows, and passages.
- `frame.fires`: CFAST's room-level representation of the Rust heat output.
- `frame.agents`: occupants advanced by the Rust evacuation engine. It honors
  pre-movement delay and walking speed, routes to exterior doors/passages,
  replans around fire and crowds, and uses `rerouting` when it changes exit
  or no safe path is available. Occupants and firefighters move through one
  shared crowd (see "Crowds" below), and each agent's `movement` says how:
  `stopped`, `walking`, `queued` (held up by people in the way), or
  `squeezing` (pressing past someone). The web app tints queued blobs red and
  squeezing blobs amber.
  Firefighters follow the occupants in the same array (`role.type ===
  'firefighter'`, IDs `{crew_id}/{n}`, `/1` leads). A crew doesn't know where
  the fire is: it stages at its entry until `arrival_time_seconds`, searches
  room by room (`advancing`, `searching`), fights fire it sees or feels nearby
  (`suppressing`), then leaves (`withdrawing`, `exited`). Put-out cells appear
  in `surface_fire` as `extinguished`.

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
- Occupants route on a shared exit field with a crowding cost, so people at
  the back of a long queue may head for another exit.

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

