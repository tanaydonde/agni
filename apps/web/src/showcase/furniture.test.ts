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
      burning: false,
    })
  })

  it('marks objects that have fire cells as burning', () => {
    const items = furnitureItems(
      { objects: [placed('desk-a', 'desk-1'), placed('desk-b', 'desk-1')] },
      [desk],
      new Set(['desk-b']),
    )
    expect(items.map((item) => [item.id, item.burning])).toEqual([
      ['desk-a', false],
      ['desk-b', true],
    ])
  })

  it('skips objects whose asset is missing', () => {
    expect(furnitureItems({ objects: [placed('ghost', 'nope')] }, [desk])).toEqual([])
  })
})

describe('furnitureItems with scanned furniture', () => {
  it('leaves scanned objects to their models', () => {
    expect(furnitureItems({ objects: [placed('scan-detected-001', 'desk-1')] }, [desk])).toEqual([])
  })
})
