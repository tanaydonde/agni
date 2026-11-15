import { describe, expect, it } from 'vitest'
import { fromThree, ndcFromClient, normalizeDegrees, toThree, yawDegrees, zRotation } from './placement'

describe('coordinate conversion', () => {
  it('maps canonical Z-up to Three.js Y-up and back', () => {
    const canonical = { x: 1, y: 2, z: 3 }
    const three = toThree(canonical)
    expect(three.toArray()).toEqual([1, 3, -2])
    expect(fromThree(three)).toEqual(canonical)
  })
})

describe('rotation helpers', () => {
  it('wraps angles into (-180, 180]', () => {
    expect(normalizeDegrees(190)).toBe(-170)
    expect(normalizeDegrees(-180)).toBe(180)
    expect(normalizeDegrees(720 + 45)).toBe(45)
  })

  it('reads the heading back from a Z rotation', () => {
    for (const degrees of [0, 45, 90, -135, 180]) {
      expect(yawDegrees(zRotation(degrees))).toBeCloseTo(degrees)
    }
  })

})

describe('ndcFromClient', () => {
  it('maps the rect corners and center to NDC', () => {
    const rect = { left: 10, top: 20, width: 200, height: 100 }
    expect(ndcFromClient(110, 70, rect).toArray()).toEqual([0, 0])
    expect(ndcFromClient(10, 20, rect).toArray()).toEqual([-1, 1])
    expect(ndcFromClient(210, 120, rect).toArray()).toEqual([1, -1])
  })
})
