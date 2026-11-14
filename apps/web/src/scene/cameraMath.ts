// Camera math for the viewport controls, kept free of DOM and React so it can
// be unit-tested. Everything here is in Three.js Y-up world space.

import * as THREE from 'three'
import type { FloorGeometry } from './geometry'

/** A camera pose: position plus yaw (about +Y) and pitch (about the camera's right axis). */
export interface CameraPose {
  position: THREE.Vector3
  yaw: number
  pitch: number
}

/** Just short of straight up/down, so yaw stays well defined. */
export const PITCH_LIMIT = Math.PI / 2 - 0.01

/** Fraction of the distance to the target covered per pixel of pinch. */
const PINCH_RATE = 0.01

const EULER = new THREE.Euler(0, 0, 0, 'YXZ')

/** Camera orientation for a yaw/pitch pair; yaw 0 looks along -Z. */
export function orientation(yaw: number, pitch: number, target = new THREE.Quaternion()) {
  return target.setFromEuler(EULER.set(pitch, yaw, 0, 'YXZ'))
}

/** Yaw and pitch that look along `direction`. */
export function yawPitchFor(direction: THREE.Vector3): { yaw: number; pitch: number } {
  const d = direction.clone().normalize()
  return { yaw: Math.atan2(-d.x, -d.z), pitch: Math.asin(THREE.MathUtils.clamp(d.y, -1, 1)) }
}

export function clampPitch(pitch: number): number {
  return THREE.MathUtils.clamp(pitch, -PITCH_LIMIT, PITCH_LIMIT)
}

/**
 * A three-quarter overview that fits the whole floor in view.
 * `bounds` and `floorZ` are in the canonical Z-up frame.
 */
export function homePose(
  bounds: FloorGeometry['bounds'],
  floorZ: number,
  fovDegrees: number,
  aspect: number,
): CameraPose & { distance: number } {
  const { min, max, height } = bounds
  const center = new THREE.Vector3(
    (min.x + max.x) / 2,
    floorZ + height / 2,
    -(min.y + max.y) / 2, // canonical y → Three.js -z
  )
  const radius = Math.hypot(max.x - min.x, max.y - min.y, height) / 2

  const vFov = THREE.MathUtils.degToRad(fovDegrees)
  const hFov = 2 * Math.atan(Math.tan(vFov / 2) * aspect)
  const distance = (radius / Math.sin(Math.min(vFov, hFov) / 2)) * 1.05

  const elevation = THREE.MathUtils.degToRad(50)
  const azimuth = THREE.MathUtils.degToRad(35)
  const position = new THREE.Vector3(
    center.x + distance * Math.cos(elevation) * Math.sin(azimuth),
    center.y + distance * Math.sin(elevation),
    center.z + distance * Math.cos(elevation) * Math.cos(azimuth),
  )

  return { position, ...yawPitchFor(center.clone().sub(position)), distance }
}

/** Fly speed in m/s, scaled to the size of the space. */
export function flySpeed(bounds: FloorGeometry['bounds']): number {
  const diagonal = Math.hypot(bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y)
  return THREE.MathUtils.clamp(diagonal / 6, 1.5, 6)
}

/**
 * Rotates the camera around `pivot` by the given yaw/pitch deltas, turning it
 * by the same amount so the pivot stays at the same spot on screen.
 */
export function orbit(pose: CameraPose, pivot: THREE.Vector3, dYaw: number, dPitch: number): CameraPose {
  const yaw = pose.yaw + dYaw
  const pitch = clampPitch(pose.pitch + dPitch)
  const offset = pose.position
    .clone()
    .sub(pivot)
    .applyQuaternion(orientation(pose.yaw, pose.pitch).invert()) // into camera space
    .applyQuaternion(orientation(yaw, pitch)) // back out with the new orientation
  return { position: pivot.clone().add(offset), yaw, pitch }
}

/**
 * How far to move toward a target `distance` away for a pinch of `deltaY`
 * pixels (negative = pinch out = move closer). Proportional to the distance so
 * it feels the same near and far, and never closer than `minDistance`.
 */
export function pinchTravel(distance: number, deltaY: number, minDistance = 0.3): number {
  const travel = distance * (1 - Math.exp(deltaY * PINCH_RATE))
  return Math.min(travel, Math.max(0, distance - minDistance))
}

/** World units per screen pixel at `distance` from a perspective camera. */
export function worldPerPixel(distance: number, fovDegrees: number, viewportHeight: number): number {
  return (2 * distance * Math.tan(THREE.MathUtils.degToRad(fovDegrees) / 2)) / viewportHeight
}

/** Wheel deltas in pixels; Firefox can report lines or pages. */
export function wheelPixels(event: Pick<WheelEvent, 'deltaX' | 'deltaY' | 'deltaMode'>) {
  const scale = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? 800 : 1
  return { x: event.deltaX * scale, y: event.deltaY * scale }
}
