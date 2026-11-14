import * as THREE from 'three'
import { describe, expect, it } from 'vitest'
import {
  applyEditLocally,
  constrainedMove,
  fromThree,
  nextObjectId,
  rayFloorHit,
  toThree,
} from './placement'
import type { Scene } from './types'

const FLOOR_Z = -0.03

function scene(): Scene {
  return {
    schema_version: 1,
    id: 'scene-1',
    name: null,
    source: { provider: 'fixture', external_id: 'scene-1' },
    assets: [
      {
        id: 'chair-basic',
        category: 'chair',
        model_uri: null,
        dimensions_m: { width_m: 0.5, depth_m: 0.55, height_m: 0.85 },
      },
    ],
    floors: [{ id: 'floor-1', label: null, elevation_m: FLOOR_Z, rooms: [], objects: [] }],
  }
}

describe('coordinate conversion', () => {
  it('maps canonical Z-up to Three.js Y-up and back', () => {
    const canonical = { x: 1, y: 2, z: 3 }
    const three = toThree(canonical)
    expect(three.toArray()).toEqual([1, 3, -2])
    expect(fromThree(three)).toEqual(canonical)
  })
})

describe('rayFloorHit', () => {
  it('finds where a downward ray meets the floor, in canonical coordinates', () => {
    // From 2 m above canonical (1, 2), looking straight down.
    const ray = new THREE.Ray(toThree({ x: 1, y: 2, z: 2 }), new THREE.Vector3(0, -1, 0))
    const hit = rayFloorHit(ray, FLOOR_Z)
    expect(hit?.x).toBeCloseTo(1)
    expect(hit?.y).toBeCloseTo(2)
    expect(hit?.z).toBe(FLOOR_Z)
  })

  it('returns null for rays at or above the horizon', () => {
    const origin = new THREE.Vector3(0, 1.6, 0)
    expect(rayFloorHit(new THREE.Ray(origin, new THREE.Vector3(0, 0, -1)), FLOOR_Z)).toBeNull()
    expect(rayFloorHit(new THREE.Ray(origin, new THREE.Vector3(0, 1, 0)), FLOOR_Z)).toBeNull()
  })
})

describe('constrainedMove', () => {
  const start = { x: 1, y: 1, z: FLOOR_Z }
  const from = { x: 0, y: 0, z: FLOOR_Z }
  const to = { x: 2, y: 3, z: FLOOR_Z }

  it('limits the x and y arrows to their axis', () => {
    expect(constrainedMove(start, from, to, 'x', FLOOR_Z)).toEqual({ x: 3, y: 1, z: FLOOR_Z })
    expect(constrainedMove(start, from, to, 'y', FLOOR_Z)).toEqual({ x: 1, y: 4, z: FLOOR_Z })
  })

  it('moves freely across the floor with the plane handle, never off it', () => {
    const lifted = { x: 2, y: 3, z: 5 }
    expect(constrainedMove(start, from, lifted, 'plane', FLOOR_Z)).toEqual({ x: 3, y: 4, z: FLOOR_Z })
  })
})

describe('nextObjectId', () => {
  it('picks the lowest unused number for the category', () => {
    expect(nextObjectId('chair', [])).toBe('chair-1')
    expect(nextObjectId('chair', ['chair-1', 'chair-3', 'desk-2'])).toBe('chair-2')
  })
})

describe('applyEditLocally', () => {
  it('adds, moves and removes objects, keeping them on the floor', () => {
    let next = applyEditLocally(scene(), {
      type: 'add_object',
      object_id: 'chair-1',
      asset_id: 'chair-basic',
      floor_id: 'floor-1',
      position_m: { x: 1, y: 2, z: 9 },
      rotation_z_degrees: 0,
      mobility: 'movable',
    })
    expect(next.floors[0].objects[0].transform.position_m).toEqual({ x: 1, y: 2, z: FLOOR_Z })

    next = applyEditLocally(next, {
      type: 'move_object',
      object_id: 'chair-1',
      position_m: { x: 4, y: 5, z: -2 },
    })
    expect(next.floors[0].objects[0].transform.position_m).toEqual({ x: 4, y: 5, z: FLOOR_Z })

    next = applyEditLocally(next, { type: 'remove_object', object_id: 'chair-1' })
    expect(next.floors[0].objects).toEqual([])
  })

  it('keeps room data referentially stable so the room is not rebuilt', () => {
    const before = scene()
    const after = applyEditLocally(before, { type: 'remove_object', object_id: 'missing' })
    expect(after.floors[0].rooms).toBe(before.floors[0].rooms)
  })
})
