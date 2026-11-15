import { describe, expect, it } from 'vitest'
import type { FireCellBox } from './cells'
import { blitField, fireVolume, unionGrid, type FireVolume, type VoxelGrid } from './volume'

const cell = (overrides: Partial<FireCellBox>): FireCellBox => ({
  id: 'c',
  burning: true,
  position: { x: 2, y: 3, z: 0.1 },
  rotation: { x: 0, y: 0, z: 0, w: 1 },
  scale: { x: 0.25, y: 0.25, z: 0.2 },
  ...overrides,
})

/** The field's value at a point in canonical metres. */
function at(volume: FireVolume, field: Uint8Array, x: number, y: number, z: number): number {
  const [i, j, k] = [x, y, z].map((value, a) => Math.floor(value / volume.voxelM) - volume.origin[a])
  return field[i + volume.size[0] * (j + volume.size[1] * k)]
}

/** A square of burning floor cells `count` on a side, centred near (5, 5). */
function burningSquare(count: number): FireCellBox[] {
  const cells: FireCellBox[] = []
  for (let i = 0; i < count; i++) {
    for (let j = 0; j < count; j++) {
      cells.push(
        cell({
          position: { x: 5 + (i - count / 2) * 0.25, y: 5 + (j - count / 2) * 0.25, z: 0.01 },
          scale: { x: 0.25, y: 0.25, z: 0.02 },
        }),
      )
    }
  }
  return cells
}

describe('fireVolume', () => {
  it('is null with no cells', () => {
    expect(fireVolume([])).toBeNull()
  })

  it('is hot at a burning cell, cooler above it, and nearly cold at the top of its box', () => {
    const volume = fireVolume([cell({})])!
    const atCell = at(volume, volume.flame, 2, 3, 0.1)
    const above = at(volume, volume.flame, 2, 3, 0.7)
    expect(atCell).toBeGreaterThan(150)
    expect(above).toBeLessThan(atCell)
    expect(volume.flame[0]).toBe(0)
    expect(volume.flame.at(-1)).toBeLessThan(20)
  })

  it('carries heat higher over a wide fire than over a single cell', () => {
    const single = fireVolume([cell({ position: { x: 5, y: 5, z: 0.01 }, scale: { x: 0.25, y: 0.25, z: 0.02 } })])!
    const wide = fireVolume(burningSquare(8))!
    expect(at(wide, wide.flame, 5, 5, 1)).toBeGreaterThan(at(single, single.flame, 5, 5, 1) + 40)
  })

  it('keeps heating cells out of the flames', () => {
    const volume = fireVolume([cell({ burning: false })])!
    expect(Math.max(...volume.flame)).toBe(0)
    expect(at(volume, volume.ember, 2, 3, 0.1)).toBeGreaterThan(100)
  })

  it('covers a rotated cell along its turned axes', () => {
    // A long cell turned a quarter turn about z runs along y, not x.
    const volume = fireVolume([
      cell({ scale: { x: 2, y: 0.25, z: 0.2 }, rotation: { x: 0, y: 0, z: Math.SQRT1_2, w: Math.SQRT1_2 } }),
    ])!
    expect(at(volume, volume.flame, 2, 3.8, 0.1)).toBeGreaterThan(150)
    expect(at(volume, volume.flame, 2.8, 3, 0.1)).toBe(0)
  })

  it('coarsens the voxels of a very large fire', () => {
    const far = fireVolume([cell({}), cell({ position: { x: 40, y: 30, z: 0.1 } })])!
    expect(far.voxelM).toBeGreaterThan(fireVolume([cell({})])!.voxelM)
  })
})

describe('unionGrid and blitField', () => {
  const left: VoxelGrid = { voxelM: 0.125, origin: [0, 0, 0], size: [2, 2, 1] }
  const right: VoxelGrid = { voxelM: 0.125, origin: [1, 1, 0], size: [2, 2, 2] }

  it('spans both grids', () => {
    expect(unionGrid(left, right)).toEqual({ voxelM: 0.125, origin: [0, 0, 0], size: [3, 3, 2] })
  })

  it('copies a field to the same place in a larger grid, into one channel', () => {
    const union = unionGrid(left, right)
    const target = new Uint8Array(3 * 3 * 2 * 2)
    blitField(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]), right, target, union, 1, 2)
    const channel = (i: number, j: number, k: number) => target[(i + 3 * (j + 3 * k)) * 2 + 1]
    expect(channel(1, 1, 0)).toBe(1)
    expect(channel(2, 2, 1)).toBe(8)
    expect(channel(0, 0, 0)).toBe(0)
    expect(target.filter((_, index) => index % 2 === 0).every((value) => value === 0)).toBe(true)
  })

  it('drops what falls outside a smaller grid', () => {
    const target = new Uint8Array(4)
    blitField(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]), right, target, left, 0, 1)
    expect([...target]).toEqual([0, 0, 0, 1])
  })
})
