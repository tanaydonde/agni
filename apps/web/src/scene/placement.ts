// Pure scene-space helpers. Canonical scene space is
// Z-up (x/y horizontal, z up, metres); Three.js world space is Y-up. The
// renderer maps (x, y, z) → (x, z, -y) with the `Z_UP_TO_Y_UP` group rotation.

import * as THREE from 'three'
import type { Vec3 } from './types'

/** Rotates the canonical Z-up world into Three.js Y-up: (x, y, z) → (x, z, -y). */
export const Z_UP_TO_Y_UP: [number, number, number] = [-Math.PI / 2, 0, 0]

/** Object name used to raycast only furniture. */
export const FURNITURE_GROUP = 'furniture'

/** Canonical Z-up → Three.js Y-up. */
export function toThree(v: Vec3, target = new THREE.Vector3()): THREE.Vector3 {
  return target.set(v.x, v.z, -v.y)
}

/** Three.js Y-up → canonical Z-up. */
export function fromThree(v: THREE.Vector3): Vec3 {
  return { x: v.x, y: -v.z, z: v.y }
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

/** Quaternion for a rotation about canonical +Z, as world-core builds it. */
export function zRotation(degrees: number) {
  const half = (degrees * Math.PI) / 360
  return { x: 0, y: 0, z: Math.sin(half), w: Math.cos(half) }
}
