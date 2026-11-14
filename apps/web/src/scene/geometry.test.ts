import { describe, expect, it } from 'vitest'
import { buildFloorGeometry, chainLoops, wallPieces } from './geometry'
import type { BoundaryEdge, Floor, Opening } from './types'

function edge(
  id: string,
  [x0, y0]: [number, number],
  [x1, y1]: [number, number],
  overrides: Partial<BoundaryEdge> = {},
): BoundaryEdge {
  return {
    id,
    kind: 'wall',
    start: { x: x0, y: y0, z: 0 },
    end: { x: x1, y: y1, z: 0 },
    thickness_m: 0.2,
    center_line_bias: 0.5,
    openings: [],
    ...overrides,
  }
}

function opening(overrides: Partial<Opening>): Opening {
  return {
    id: 'o',
    label: null,
    kind: 'door',
    width_m: 1,
    height_m: 2,
    lower_elevation_m: 0,
    relative_center: 0.5,
    ...overrides,
  }
}

describe('chainLoops', () => {
  it('closes a square whose edges are shuffled and inconsistently oriented', () => {
    const loops = chainLoops([
      edge('a', [0, 0], [4, 0]),
      edge('c', [0, 3], [4, 3]), // reversed relative to the loop
      edge('b', [4, 0], [4, 3]),
      edge('d', [0, 3], [0, 0]),
    ])
    expect(loops).toHaveLength(1)
    expect(loops[0]).toHaveLength(4)
  })

  it('drops an open chain', () => {
    expect(chainLoops([edge('a', [0, 0], [4, 0]), edge('b', [4, 0], [4, 3])])).toEqual([])
  })
})

describe('wallPieces', () => {
  it('builds one full-height box for a plain wall', () => {
    const { walls, windows } = wallPieces(edge('a', [0, 0], [4, 0]), 0, 2.5)
    expect(windows).toEqual([])
    expect(walls).toHaveLength(1)
    expect(walls[0]).toMatchObject({ length: 4, height: 2.5, thickness: 0.2 })
    expect(walls[0].center).toEqual([2, 0, 1.25])
  })

  it('leaves a gap for a door and a lintel above it', () => {
    const wall = edge('a', [0, 0], [4, 0], { openings: [opening({ width_m: 1, height_m: 2 })] })
    const { walls } = wallPieces(wall, 0, 2.5)
    const summary = walls.map((w) => [w.length, w.height].map((n) => +n.toFixed(3)))
    expect(summary).toEqual([
      [1.5, 2.5],
      [1, 0.5],
      [1.5, 2.5],
    ])
  })

  it('keeps a sill and lintel around a window and adds a pane', () => {
    const wall = edge('a', [0, 0], [4, 0], {
      openings: [opening({ kind: 'window', lower_elevation_m: 1, height_m: 1 })],
    })
    const { walls, windows } = wallPieces(wall, 0, 2.5)
    expect(walls.filter((w) => w.length === 1).map((w) => w.height)).toEqual([1, 0.5])
    expect(windows).toHaveLength(1)
    expect(windows[0].center[2]).toBe(1.5)
  })

  it('measures opening elevation from the floor', () => {
    const wall = edge('a', [0, 0], [4, 0], {
      openings: [opening({ kind: 'window', lower_elevation_m: 11, height_m: 1 })],
    })
    const { windows } = wallPieces(wall, 10, 2.5)
    expect(windows[0].center[2]).toBe(11.5)
  })

  it('clamps an opening that runs past the end of the wall', () => {
    const wall = edge('a', [0, 0], [4, 0], {
      openings: [opening({ relative_center: 1, width_m: 1, height_m: 2.5 })],
    })
    const { walls } = wallPieces(wall, 0, 2.5)
    expect(walls).toHaveLength(1)
    expect(walls[0].length).toBeCloseTo(3.5)
  })
})

describe('buildFloorGeometry', () => {
  it('builds shared edges once and keeps invisible edges as separators', () => {
    const shared = edge('shared', [4, 0], [4, 3])
    const floor: Floor = {
      id: 'f',
      label: null,
      elevation_m: 0,
      objects: [],
      rooms: [
        {
          id: 'left',
          label: null,
          classifications: [],
          dimensions: null,
          boundary: [
            edge('l1', [0, 0], [4, 0]),
            shared,
            edge('l2', [4, 3], [0, 3]),
            edge('l3', [0, 3], [0, 0]),
          ],
        },
        {
          id: 'right',
          label: null,
          classifications: [],
          dimensions: null,
          boundary: [
            edge('r1', [4, 0], [8, 0]),
            edge('r2', [8, 0], [8, 3], { kind: 'invisible' }),
            edge('r3', [8, 3], [4, 3]),
            shared,
          ],
        },
      ],
    }

    const geometry = buildFloorGeometry(floor)
    expect(geometry.roomOutlines).toHaveLength(2)
    expect(geometry.walls).toHaveLength(6)
    expect(geometry.separators).toHaveLength(1)
    expect(geometry.bounds).toEqual({ min: { x: 0, y: 0 }, max: { x: 8, y: 3 }, height: 2.7 })
  })
})
