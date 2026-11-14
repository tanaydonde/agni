import { describe, expect, it } from 'vitest'
import { categoryColor } from '../scene/furnitureStyle'
import type { FurnitureAsset, FurnitureObject } from '../scene/types'
import { furnitureItems } from './furniture'

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
