import * as THREE from 'three'
import { describe, expect, it } from 'vitest'
import {
  PITCH_LIMIT,
  homePose,
  orbit,
  orientation,
  pinchTravel,
  topDownZoom,
  wheelPixels,
  worldPerPixel,
  yawPitchFor,
} from './cameraMath'

const forward = (yaw: number, pitch: number) =>
  new THREE.Vector3(0, 0, -1).applyQuaternion(orientation(yaw, pitch))

describe('orientation and yawPitchFor', () => {
  it('round-trip a look direction', () => {
    const direction = new THREE.Vector3(1, -0.5, 2).normalize()
    const { yaw, pitch } = yawPitchFor(direction)
    expect(forward(yaw, pitch).distanceTo(direction)).toBeLessThan(1e-9)
  })
})

describe('homePose', () => {
  const bounds = { min: { x: 0, y: 0 }, max: { x: 10, y: 6 }, height: 2.7 }

  it('looks at the center of the floor from above', () => {
    const pose = homePose(bounds, 0, 40, 16 / 9)
    const center = new THREE.Vector3(5, 1.35, -3)
    const toCenter = center.clone().sub(pose.position).normalize()
    expect(forward(pose.yaw, pose.pitch).distanceTo(toCenter)).toBeLessThan(1e-9)
    expect(pose.position.y).toBeGreaterThan(center.y)
    expect(pose.position.distanceTo(center)).toBeCloseTo(pose.distance)
  })

  it('backs off further for a narrow viewport', () => {
    expect(homePose(bounds, 0, 40, 0.5).distance).toBeGreaterThan(
      homePose(bounds, 0, 40, 2).distance,
    )
  })
})

describe('orbit', () => {
  const pivot = new THREE.Vector3(2, 0, -3)
  const start = { position: new THREE.Vector3(6, 4, 2), yaw: 0.3, pitch: -0.4 }
  const inCameraSpace = (pose: typeof start) =>
    pivot.clone().sub(pose.position).applyQuaternion(orientation(pose.yaw, pose.pitch).invert())

  it('keeps the pivot at the same place in view', () => {
    const next = orbit(start, pivot, 0.8, -0.3)
    expect(inCameraSpace(next).distanceTo(inCameraSpace(start))).toBeLessThan(1e-9)
  })

  it('keeps its distance to the pivot', () => {
    const next = orbit(start, pivot, -1.2, 0.2)
    expect(next.position.distanceTo(pivot)).toBeCloseTo(start.position.distanceTo(pivot))
  })

  it('stops short of looking straight down', () => {
    expect(orbit(start, pivot, 0, -5).pitch).toBe(-PITCH_LIMIT)
  })
})

describe('pinchTravel', () => {
  it('moves closer when pinching out and away when pinching in', () => {
    expect(pinchTravel(10, -20)).toBeGreaterThan(0)
    expect(pinchTravel(10, 20)).toBeLessThan(0)
  })

  it('never moves closer than the minimum distance', () => {
    expect(pinchTravel(1, -1000)).toBeCloseTo(0.7)
    expect(pinchTravel(0.2, -50)).toBe(0)
  })
})

describe('worldPerPixel', () => {
  it('spans the visible height at that distance', () => {
    // At 90° vertical FOV, the view is 2 * distance tall.
    expect(worldPerPixel(5, 90, 1000) * 1000).toBeCloseTo(10)
  })
})

describe('wheelPixels', () => {
  it('converts Firefox line deltas to pixels', () => {
    expect(wheelPixels({ deltaX: 1, deltaY: -2, deltaMode: 1 })).toEqual({ x: 16, y: -32 })
    expect(wheelPixels({ deltaX: 3, deltaY: 4, deltaMode: 0 })).toEqual({ x: 3, y: 4 })
  })
})

describe('topDownZoom', () => {
  const bounds = { min: { x: 0, y: 0 }, max: { x: 10, y: 5 }, height: 3 }

  it('fits the tighter of the two spans, with the margin', () => {
    expect(topDownZoom(bounds, 1000, 1000, 1)).toBeCloseTo(100)
    expect(topDownZoom(bounds, 1000, 250, 1)).toBeCloseTo(50)
    expect(topDownZoom(bounds, 1100, 1000)).toBeCloseTo(100)
  })
})
