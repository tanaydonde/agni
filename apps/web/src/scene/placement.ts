// Pure helpers for placing furniture on the floor. Canonical scene space is
// Z-up (x/y horizontal, z up, metres); Three.js world space is Y-up. The
// renderer maps (x, y, z) → (x, z, -y) with the `Z_UP_TO_Y_UP` group rotation.

import * as THREE from 'three'
import type { EditCommand, Floor, FurnitureAsset, FurnitureObject, Scene, Vec3 } from './types'

/** Rotates the canonical Z-up world into Three.js Y-up: (x, y, z) → (x, z, -y). */
export const Z_UP_TO_Y_UP: [number, number, number] = [-Math.PI / 2, 0, 0]

/** Object names used to raycast only furniture or only the gizmo. */
export const FURNITURE_GROUP = 'furniture'
export const GIZMO_GROUP = 'transform-gizmo'

/** Gizmo handles that slide an object: along one floor axis, or both. */
export type MoveHandle = 'x' | 'y' | 'plane'
/** Every gizmo handle: the move handles plus the ring that turns about Z. */
export type GizmoHandle = MoveHandle | 'rotate'

export function floorZ(floor: Pick<Floor, 'elevation_m'>): number {
  return floor.elevation_m ?? 0
}

/** Canonical Z-up → Three.js Y-up. */
export function toThree(v: Vec3, target = new THREE.Vector3()): THREE.Vector3 {
  return target.set(v.x, v.z, -v.y)
}

/** Three.js Y-up → canonical Z-up. */
export function fromThree(v: THREE.Vector3): Vec3 {
  return { x: v.x, y: -v.z, z: v.y }
}

/**
 * Where a Three.js ray meets the floor, in canonical coordinates. Null when
 * the ray runs parallel to the floor or points away from it.
 */
export function rayFloorHit(ray: THREE.Ray, z: number): Vec3 | null {
  const dy = ray.direction.y
  if (Math.abs(dy) < 1e-9) return null
  const t = (z - ray.origin.y) / dy
  if (t <= 0) return null
  const point = ray.at(t, new THREE.Vector3())
  return { x: point.x, y: -point.z, z }
}

/**
 * Moves `start` by the floor-plane drag `from` → `to`, limited to the handle's
 * axis. The result always stays at the floor height `z`.
 */
export function constrainedMove(start: Vec3, from: Vec3, to: Vec3, handle: MoveHandle, z: number): Vec3 {
  const dx = handle === 'y' ? 0 : to.x - from.x
  const dy = handle === 'x' ? 0 : to.y - from.y
  return { x: start.x + dx, y: start.y + dy, z }
}

/** Wraps an angle into (-180, 180] degrees. */
export function normalizeDegrees(degrees: number): number {
  const wrapped = ((((degrees + 180) % 360) + 360) % 360) - 180
  return wrapped === -180 ? 180 : wrapped
}

/** Heading of an upright object: its rotation about canonical +Z, in degrees. */
export function yawDegrees(q: { z: number; w: number }): number {
  return normalizeDegrees((2 * Math.atan2(q.z, q.w) * 180) / Math.PI)
}

/**
 * New heading after dragging the rotation ring from `from` to `to` (floor
 * points) around `center`: the start heading plus the angle swept, optionally
 * snapped to multiples of `snapDegrees`.
 */
export function rotationDrag(
  center: Vec3,
  from: Vec3,
  to: Vec3,
  startYaw: number,
  snapDegrees?: number,
): number {
  const angle = (p: Vec3) => (Math.atan2(p.y - center.y, p.x - center.x) * 180) / Math.PI
  const yaw = startYaw + angle(to) - angle(from)
  return normalizeDegrees(snapDegrees ? Math.round(yaw / snapDegrees) * snapDegrees : yaw)
}

/** Normalized device coordinates (-1..1, +y up) of a client point within `rect`. */
export function ndcFromClient(
  clientX: number,
  clientY: number,
  rect: Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>,
  target = new THREE.Vector2(),
): THREE.Vector2 {
  return target.set(
    ((clientX - rect.left) / rect.width) * 2 - 1,
    -((clientY - rect.top) / rect.height) * 2 + 1,
  )
}

/** A readable, unused object ID such as `chair-3`. Agents see these IDs. */
export function nextObjectId(category: string, existingIds: Iterable<string>): string {
  const taken = new Set(existingIds)
  for (let n = 1; ; n++) {
    const id = `${category}-${n}`
    if (!taken.has(id)) return id
  }
}

export function allObjectIds(scene: Scene): string[] {
  return scene.floors.flatMap((floor) => floor.objects.map((object) => object.id))
}

/** Quaternion for a rotation about canonical +Z, as world-core builds it. */
export function zRotation(degrees: number) {
  const half = (degrees * Math.PI) / 360
  return { x: 0, y: 0, z: Math.sin(half), w: Math.cos(half) }
}

/**
 * Applies an edit to a scene the way the Rust world will, for optimistic UI.
 * Returns the scene unchanged when the edit doesn't apply; the server's answer
 * replaces this either way.
 */
export function applyEditLocally(scene: Scene, edit: EditCommand): Scene {
  const mapObjects = (fn: (objects: FurnitureObject[], floor: Floor) => FurnitureObject[]) => ({
    ...scene,
    floors: scene.floors.map((floor) => ({ ...floor, objects: fn(floor.objects, floor) })),
  })

  switch (edit.type) {
    case 'add_object': {
      if (!scene.assets.some((asset) => asset.id === edit.asset_id)) return scene
      return {
        ...scene,
        floors: scene.floors.map((floor) =>
          floor.id !== edit.floor_id
            ? floor
            : {
                ...floor,
                objects: [
                  ...floor.objects,
                  {
                    id: edit.object_id,
                    asset_id: edit.asset_id,
                    transform: {
                      position_m: { ...edit.position_m, z: floorZ(floor) },
                      rotation: zRotation(edit.rotation_z_degrees),
                      scale: { x: 1, y: 1, z: 1 },
                    },
                    mobility: edit.mobility,
                  },
                ],
              },
        ),
      }
    }
    case 'move_object':
      return mapObjects((objects, floor) =>
        objects.map((object) =>
          object.id !== edit.object_id
            ? object
            : {
                ...object,
                transform: { ...object.transform, position_m: { ...edit.position_m, z: floorZ(floor) } },
              },
        ),
      )
    case 'rotate_object':
      return mapObjects((objects) =>
        objects.map((object) =>
          object.id !== edit.object_id
            ? object
            : { ...object, transform: { ...object.transform, rotation: zRotation(edit.rotation_z_degrees) } },
        ),
      )
    case 'remove_object':
      return mapObjects((objects) => objects.filter((object) => object.id !== edit.object_id))
  }
}

export function assetById(scene: Scene, id: string): FurnitureAsset | undefined {
  return scene.assets.find((asset) => asset.id === id)
}
