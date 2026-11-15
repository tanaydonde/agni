import { describe, expect, it } from 'vitest'
import { fireCellBoxes } from './cells'
import type { SimulationFrame, SurfaceFireNode } from './types'

const node = (overrides: Partial<SurfaceFireNode>): SurfaceFireNode => ({
  id: 'n',
  source: { type: 'furniture', object_id: 'sofa-1' },
  room_id: 'room-1',
  position_m: { x: 1, y: 2, z: 0.1 },
  dimensions_m: { width_m: 0.25, depth_m: 0.25, height_m: 0.2 },
  material_id: 'm',
  state: 'burning',
  ignition_time_seconds: 0,
  surface_temperature_c: 600,
  heat_release_rate_kw: 10,
  fuel_remaining_fraction: 1,
  ...overrides,
})

const frame = (nodes: SurfaceFireNode[]): SimulationFrame => ({
  time_seconds: 0,
  surface_fire: nodes,
  rooms: [],
  fires: [],
  openings: [],
  agents: [],
})

const sofa = {
  id: 'sofa-1',
  transform: {
    position_m: { x: 0, y: 0, z: 0 },
    rotation: { x: 0, y: 0, z: 0.7071, w: 0.7071 },
    scale: { x: 1, y: 1, z: 1 },
  },
}

describe('fireCellBoxes', () => {
  it('draws heating and burning cells, not burned-out ones', () => {
    const boxes = fireCellBoxes(
      frame([
        node({ id: 'a', state: 'burning' }),
        node({ id: 'b', state: 'heating' }),
        node({ id: 'c', state: 'burned_out' }),
      ]),
      [sofa],
    )
    expect(boxes.map((box) => [box.id, box.burning])).toEqual([
      ['a', true],
      ['b', false],
    ])
  })

  it('turns furniture cells with their object and lifts floor cells', () => {
    const [furniture, floor] = fireCellBoxes(
      frame([
        node({ id: 'a' }),
        node({ id: 'b', source: { type: 'floor_cell', floor_id: 'f', coordinate: { column: 0, row: 0 } } }),
      ]),
      [sofa],
    )
    expect(furniture.rotation).toEqual(sofa.transform.rotation)
    expect(floor.rotation).toEqual({ x: 0, y: 0, z: 0, w: 1 })
    expect(floor.position.z).toBeGreaterThan(0.1)
  })

  it('draws nothing once the fire is out, however warm cells still are', () => {
    const out = fireCellBoxes(
      frame([
        node({ id: 'a', state: 'heating' }),
        node({ id: 'b', state: 'extinguished' }),
        node({ id: 'c', state: 'burned_out' }),
      ]),
      [sofa],
    )
    expect(out).toEqual([])
  })

  it('draws nothing without a frame', () => {
    expect(fireCellBoxes(undefined, [sofa])).toEqual([])
  })
})
