import { describe, expect, it } from 'vitest'
import { categoryColor } from '../scene/furnitureStyle'
import type { DetectedObject, FurnitureAsset, FurnitureObject } from '../scene/types'
import { detectedFurnitureItems, furnitureItems } from './furniture'

const desk: FurnitureAsset = {
  id: 'desk-1',
  category: 'desk',
  model_uri: null,
  dimensions_m: { width_m: 1.6, depth_m: 0.8, height_m: 0.75 },
}

const placed = (id: string, assetId: string): FurnitureObject => ({
  id,
  asset_id: assetId,
  transform: {
    position_m: { x: 1, y: -2, z: 0 },
    rotation: { x: 0, y: 0, z: 0.7071, w: 0.7071 },
    scale: { x: 1, y: 1, z: 1 },
  },
  mobility: 'movable',
})

describe('furnitureItems', () => {
  it('turns placed objects into boxes with their asset size and category color', () => {
    const [item] = furnitureItems({ objects: [placed('desk-a', 'desk-1')] }, [desk])
    expect(item).toEqual({
      id: 'desk-a',
      position: { x: 1, y: -2, z: 0 },
      rotation: { x: 0, y: 0, z: 0.7071, w: 0.7071 },
      scale: { x: 1, y: 1, z: 1 },
      size: desk.dimensions_m,
      color: categoryColor('desk'),
    })
  })

  it('skips objects whose asset is missing', () => {
    expect(furnitureItems({ objects: [placed('ghost', 'nope')] }, [desk])).toEqual([])
  })
})

describe('detectedFurnitureItems', () => {
  const scanned: DetectedObject = {
    id: 'detected-001',
    floor_id: 'floor-1',
    room_id: null,
    room_type: null,
    asset_id: 'desk-1',
    position_m: { x: 3, y: 1, z: 0 },
    yaw_degrees: 90,
    size_m: { width_m: 2.2, depth_m: 0.9, height_m: 0.8 },
    footprint_area_m2: 1.9,
  }

  it('draws scanned furniture at its measured size, see-through, in its asset color', () => {
    const [item] = detectedFurnitureItems([scanned], [desk])
    expect(item.size).toEqual(scanned.size_m)
    expect(item.position).toEqual(scanned.position_m)
    expect(item.rotation.z).toBeCloseTo(Math.SQRT1_2)
    expect(item.rotation.w).toBeCloseTo(Math.SQRT1_2)
    expect(item.color).toBe(categoryColor('desk'))
    expect(item.opacity).toBeLessThan(1)
  })
})
