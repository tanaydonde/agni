// An orbit camera around a target point, for the review's bird's-eye views.
// Positions are in Three.js Y-up world space (canonical x, z, -y), the space
// both the R3F canvas and Showcase render in.

import * as THREE from 'three'
import { homePose, worldPerPixel, yawPitchFor } from './cameraMath'
import type { FloorGeometry } from './geometry'

/** The vertical field of view orbits are framed for (the review's 3D camera). */
export const ORBIT_FOV_DEGREES = 50

export interface Orbit {
  target: { x: number; y: number; z: number }
  /** Around the vertical axis, radians; 0 looks toward -z. */
  azimuth: number
  /** Above the horizon, radians. */
  elevation: number
  distance: number
}

/** Drag sensitivity: radians per pixel. */
const ROTATE_PER_PIXEL = 0.006
/** Never flat on the floor or exactly overhead (where yaw is undefined). */
const MIN_ELEVATION = THREE.MathUtils.degToRad(8)
const MAX_ELEVATION = THREE.MathUtils.degToRad(88)
/** Wheel sensitivity: distance scales by e^(deltaY * this). */
const ZOOM_PER_PIXEL = 0.0015

/** The same whole-floor framing as the 3D overview's `homePose`. */
export function orbitHome(
  bounds: FloorGeometry['bounds'],
  floorZ: number,
  fovDegrees: number,
  aspect: number,
): Orbit {
  const pose = homePose(bounds, floorZ, fovDegrees, aspect)
  const target = pose.position
    .clone()
    .add(new THREE.Vector3(0, 0, -1).applyEuler(new THREE.Euler(pose.pitch, pose.yaw, 0, 'YXZ')).multiplyScalar(pose.distance))
  const offset = pose.position.clone().sub(target)
  return {
    target: { x: target.x, y: target.y, z: target.z },
    azimuth: Math.atan2(offset.x, offset.z),
    elevation: Math.asin(THREE.MathUtils.clamp(offset.y / pose.distance, -1, 1)),
    distance: pose.distance,
  }
}

/** Where the camera sits. */
export function orbitPosition(orbit: Orbit): THREE.Vector3 {
  const { target, azimuth, elevation, distance } = orbit
  return new THREE.Vector3(
    target.x + distance * Math.cos(elevation) * Math.sin(azimuth),
    target.y + distance * Math.sin(elevation),
    target.z + distance * Math.cos(elevation) * Math.cos(azimuth),
  )
}

/** Yaw and pitch (radians, YXZ) that look from the camera at the target. */
export function orbitYawPitch(orbit: Orbit): { yaw: number; pitch: number } {
  const position = orbitPosition(orbit)
  const { x, y, z } = orbit.target
  return yawPitchFor(new THREE.Vector3(x, y, z).sub(position))
}

/** Dragging turns the view around the target (right drags turn it right). */
export function rotateOrbit(orbit: Orbit, dxPixels: number, dyPixels: number): Orbit {
  return {
    ...orbit,
    azimuth: orbit.azimuth - dxPixels * ROTATE_PER_PIXEL,
    elevation: THREE.MathUtils.clamp(orbit.elevation + dyPixels * ROTATE_PER_PIXEL, MIN_ELEVATION, MAX_ELEVATION),
  }
}

/** Wheel down (positive deltaY) moves away, up moves closer. */
export function zoomOrbit(orbit: Orbit, deltaY: number, minDistance: number, maxDistance: number): Orbit {
  return {
    ...orbit,
    distance: THREE.MathUtils.clamp(orbit.distance * Math.exp(deltaY * ZOOM_PER_PIXEL), minDistance, maxDistance),
  }
}

/**
 * Slides the target across the ground so the scene follows the pointer:
 * drag right and the building moves right, drag down and it moves down.
 */
export function panOrbit(
  orbit: Orbit,
  dxPixels: number,
  dyPixels: number,
  fovDegrees: number,
  viewportHeight: number,
): Orbit {
  const scale = worldPerPixel(orbit.distance, fovDegrees, viewportHeight)
  const { azimuth } = orbit
  // Screen right, and screen up projected onto the ground (away from the camera).
  const right = { x: Math.cos(azimuth), z: -Math.sin(azimuth) }
  const away = { x: -Math.sin(azimuth), z: -Math.cos(azimuth) }
  return {
    ...orbit,
    target: {
      x: orbit.target.x - right.x * dxPixels * scale + away.x * dyPixels * scale,
      y: orbit.target.y,
      z: orbit.target.z - right.z * dxPixels * scale + away.z * dyPixels * scale,
    },
  }
}

/**
 * The same orbit for a camera with another vertical field of view: the
 * distance scales so the scene fills the view the same way.
 */
export function orbitForFov(orbit: Orbit, fovDegrees: number): Orbit {
  const scale =
    Math.tan(THREE.MathUtils.degToRad(ORBIT_FOV_DEGREES) / 2) / Math.tan(THREE.MathUtils.degToRad(fovDegrees) / 2)
  return { ...orbit, distance: orbit.distance * scale }
}

/** Vertical field of view, in degrees, from a column-major perspective projection matrix. */
export function fovFromProjection(projection: ArrayLike<number>): number | null {
  const f = projection[5]
  return f > 0 ? THREE.MathUtils.radToDeg(2 * Math.atan(1 / f)) : null
}
