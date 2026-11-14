// TypeScript mirror of the canonical scene in crates/world-core/src/model.rs.
// Hand-written for now; replace with ts-rs output once the Rust crate exports
// bindings. `Option<T>` serializes as `T | null`.

export const SCENE_SCHEMA_VERSION = 1

export interface Scene {
  schema_version: number
  id: string
  name: string | null
  source: SceneSource
  assets: FurnitureAsset[]
  floors: Floor[]
}

export interface SceneSource {
  provider: string
  external_id: string
}

export interface Floor {
  id: string
  label: string | null
  elevation_m: number | null
  rooms: Room[]
  objects: FurnitureObject[]
}

export interface Room {
  id: string
  label: string | null
  classifications: RoomClassification[]
  dimensions: RoomDimensions | null
  boundary: BoundaryEdge[]
}

export interface RoomClassification {
  label: string
  confidence: number | null
}

export interface RoomDimensions {
  area_m2: number | null
  width_m: number | null
  depth_m: number | null
  height_m: number | null
  lower_elevation_m: number | null
}

export interface FurnitureAsset {
  id: string
  category: string
  model_uri: string | null
  dimensions_m: Dimensions3
}

export interface FurnitureObject {
  id: string
  asset_id: string
  transform: Transform3
  mobility: 'fixed' | 'movable'
}

export interface Dimensions3 {
  width_m: number
  depth_m: number
  height_m: number
}

export interface Transform3 {
  position_m: Vec3
  rotation: Quaternion
  scale: Vec3
}

export interface Quaternion {
  x: number
  y: number
  z: number
  w: number
}

export type BoundaryKind = 'wall' | 'invisible' | 'unknown'

export interface BoundaryEdge {
  id: string
  kind: BoundaryKind
  start: Vec3
  end: Vec3
  thickness_m: number | null
  center_line_bias: number | null
  openings: Opening[]
}

export type OpeningKind = 'door' | 'window' | 'passage' | 'unknown'

export interface Opening {
  id: string
  label: string | null
  kind: OpeningKind
  width_m: number
  height_m: number | null
  lower_elevation_m: number | null
  /** Position of the opening center along its edge, from 0.0 to 1.0. */
  relative_center: number
}

export interface Vec3 {
  x: number
  y: number
  z: number
}

// World API (crates/world-core/src/agent_api.rs), served by crates/server.

export interface GetSceneResponse {
  revision: number
  scene: Scene
}

export type Mobility = FurnitureObject['mobility']

export type EditCommand =
  | {
      type: 'add_object'
      object_id: string
      asset_id: string
      floor_id: string
      /** `z` is ignored: the world snaps furniture to the floor. */
      position_m: Vec3
      rotation_z_degrees: number
      mobility: Mobility
    }
  | { type: 'move_object'; object_id: string; position_m: Vec3 }
  | { type: 'rotate_object'; object_id: string; rotation_z_degrees: number }
  | { type: 'remove_object'; object_id: string }

export interface ApplyEditsRequest {
  expected_revision: number
  edits: EditCommand[]
}

export interface EditResponse {
  revision: number
  changed_object_ids: string[]
}

/** World errors serialize as `{ code, ...details }`. */
export interface WorldApiErrorBody {
  code: string
  [detail: string]: unknown
}
