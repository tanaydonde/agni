import { describe, expect, it } from 'vitest'
import { zRotation } from './placement'
import { detectedInScene, isScannedObject } from './scanned'
import type { DetectedObject, FurnitureObject } from './types'

const detected = (id: string): DetectedObject => ({
  id,
  floor_id: 'floor-1',
  room_id: 'room-1',
  room_type: null,
  label: 'sofa',
  asset_id: 'sofa-basic',
  position_m: { x: 1, y: 1, z: 0 },
  yaw_degrees: 0,
  size_m: { width_m: 2, depth_m: 0.9, height_m: 0.8 },
  footprint_area_m2: 1.8,
})

const placed = (id: string, x: number, yaw: number): FurnitureObject => ({
  id,
  asset_id: 'sofa-basic',
  transform: { position_m: { x, y: 2, z: 0 }, rotation: zRotation(yaw), scale: { x: 1, y: 1, z: 1 } },
  mobility: 'movable',
})

describe('detectedInScene', () => {
  it('follows scanned objects the scene moved and drops deleted ones', () => {
    const result = detectedInScene([detected('detected-001'), detected('detected-002')], {
      objects: [placed('scan-detected-001', 5, 90), placed('desk-1', 0, 0)],
    })
    expect(result).toHaveLength(1)
    expect(result[0]).toMatchObject({ id: 'detected-001', position_m: { x: 5, y: 2, z: 0 } })
    expect(result[0].yaw_degrees).toBeCloseTo(90)
  })

  it('passes detections through when the scene has no scanned objects', () => {
    const all = [detected('detected-001')]
    expect(detectedInScene(all, { objects: [placed('desk-1', 0, 0)] })).toBe(all)
  })

  it('recognizes scanned object IDs', () => {
    expect(isScannedObject({ id: 'scan-detected-001' })).toBe(true)
    expect(isScannedObject({ id: 'sofa-1' })).toBe(false)
  })
})
