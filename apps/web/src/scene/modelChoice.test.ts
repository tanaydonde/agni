import { describe, expect, it } from 'vitest'
import { chooseModel, distanceToWall, placeModel, placeModels } from './modelChoice'
import type { BoundaryEdge, DetectedObject, FurnitureLabel, FurnitureModel } from './types'

const model = (id: string, label: FurnitureLabel, width_m: number, depth_m: number, height_m: number): FurnitureModel => ({
  id,
  url: `/models/furniture/${id}.glb`,
  label,
  size_m: { width_m, depth_m, height_m },
})

const MODELS = [
  model('sofa-modern', 'sofa', 2.32, 0.98, 0.89),
  model('chair-tub', 'chair', 0.98, 0.91, 0.78),
  model('chair-barcelona', 'chair', 0.78, 0.76, 0.76),
  model('dining-set', 'table', 2.35, 1.12, 1.03),
  model('dining-table', 'table', 2.26, 1.39, 0.88),
]

const detected = (
  label: FurnitureLabel,
  width_m: number,
  depth_m: number,
  height_m: number,
  yaw_degrees = 0,
): DetectedObject => ({
  id: `detected-${label}`,
  floor_id: 'floor-1',
  room_id: null,
  room_type: null,
  label,
  asset_id: null,
  position_m: { x: 0, y: 0, z: 0 },
  yaw_degrees,
  size_m: { width_m, depth_m, height_m },
  footprint_area_m2: width_m * depth_m,
})

const wall = (id: string, x1: number, y1: number, x2: number, y2: number): BoundaryEdge => ({
  id,
  kind: 'wall',
  start: { x: x1, y: y1, z: 0 },
  end: { x: x2, y: y2, z: 0 },
  thickness_m: null,
  center_line_bias: null,
  openings: [],
})

describe('chooseModel', () => {
  it('prefers the label over size', () => {
    // Closer in size to the sofa, but it is a table.
    expect(chooseModel(detected('table', 2.3, 1.0, 0.9), MODELS)?.id).toMatch(/^dining-/)
  })

  it('picks the closest size among models with the label', () => {
    expect(chooseModel(detected('chair', 1.05, 1.02, 0.84), MODELS)?.id).toBe('chair-tub')
    expect(chooseModel(detected('chair', 0.6, 0.5, 0.7), MODELS)?.id).toBe('chair-barcelona')
    expect(chooseModel(detected('table', 2.2, 1.4, 0.8), MODELS)?.id).toBe('dining-table')
  })

  it('leaves labels without a model as boxes', () => {
    expect(chooseModel(detected('bed', 2.0, 1.6, 0.6), MODELS)).toBeNull()
    expect(chooseModel(detected('unknown', 0.5, 0.5, 0.5), MODELS)).toBeNull()
  })
})

describe('placeModel', () => {
  it('stretches the model to fill the box on its own axes', () => {
    const placement = placeModel(detected('sofa', 2.9, 0.9, 0.95), MODELS[0], [])
    expect(placement.scale.x).toBeCloseTo(2.9 / 2.32)
    expect(placement.scale.y).toBeCloseTo(0.95 / 0.89)
    expect(placement.scale.z).toBeCloseTo(0.9 / 0.98)
  })

  it('turns the front away from the nearer wall', () => {
    // Long axis along X; the default front faces −Y.
    const sofa = detected('sofa', 2.0, 0.9, 0.9)
    const wallToSouth = [wall('south', -5, -0.6, 5, -0.6), wall('north', -5, 4, 5, 4)]
    const wallToNorth = [wall('south', -5, -4, 5, -4), wall('north', -5, 0.6, 5, 0.6)]
    expect(placeModel(sofa, MODELS[0], wallToSouth).yawDegrees).toBe(180)
    expect(placeModel(sofa, MODELS[0], wallToNorth).yawDegrees).toBe(0)
  })
})

describe('placeModels', () => {
  it('skips boxes without a sensible model', () => {
    const placements = placeModels([detected('sofa', 2, 0.9, 0.9), detected('bed', 2, 1.6, 0.6)], MODELS, [])
    expect(placements.map((placement) => placement.model.id)).toEqual(['sofa-modern'])
  })
})

describe('distanceToWall', () => {
  it('measures along the ray and ignores walls behind or beside it', () => {
    const walls = [wall('a', 3, -1, 3, 1), wall('behind', -2, -1, -2, 1), wall('aside', 1, 5, 2, 5)]
    expect(distanceToWall({ x: 0, y: 0, z: 0 }, { x: 1, y: 0 }, walls)).toBeCloseTo(3)
    expect(distanceToWall({ x: 0, y: 0, z: 0 }, { x: 0, y: -1 }, walls)).toBe(Infinity)
  })
})
