import { describe, expect, it } from 'vitest'
import type { ModelPlacement } from '../scene/modelChoice'
import { showcaseTransform } from './models'

const placement: ModelPlacement = {
  objectId: 'detected-001',
  model: {
    id: 'sofa-modern',
    url: '/models/furniture/sofa-modern.glb',
    label: 'sofa',
    size_m: { width_m: 2.32, depth_m: 0.98, height_m: 0.89 },
  },
  position: { x: 1, y: 2, z: -0.03 },
  yawDegrees: 90,
  scale: { x: 1.2, y: 1, z: 0.9 },
}

describe('showcaseTransform', () => {
  it('moves a Z-up placement into Showcase’s Y-up world', () => {
    const { position, quaternion, scale } = showcaseTransform(placement)
    expect(position).toEqual({ x: 1, y: -0.03, z: -2 })
    // 90° about Z-up's vertical is 90° about Y-up's vertical.
    expect(quaternion.y).toBeCloseTo(Math.SQRT1_2)
    expect(quaternion.w).toBeCloseTo(Math.SQRT1_2)
    expect(scale).toEqual(placement.scale)
  })
})
