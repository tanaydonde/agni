// The fire's tiles as a smooth 3D heat field for the volumetric renderer
// (volumeRenderer.ts). Burning cells are the fuel; heat rises from them,
// thinning with height, so wide fires stand taller than a single cell.

import type { FireCellBox } from './cells'

/** Finest voxel edge. Fire cells are 0.25 m, so each spans two voxels. */
const BASE_VOXEL_M = 0.125
/** Above this many voxels the grid is coarsened (each doubling is 8× fewer). */
const MAX_VOXELS = 500_000
/** Room left around the cells for flames to lean and flicker into. */
const SIDE_MARGIN_M = 0.5
const BOTTOM_MARGIN_M = 0.25
/** Room left above the highest cell for the flames. */
const FLAME_HEADROOM_M = 2
/** Height over which rising heat falls to 1/e. Sets how tall flames stand. */
const HEAT_FALLOFF_M = 1.4

/** A box of voxels on the lattice of `voxelM`-sized cubes anchored at the canonical origin. */
export interface VoxelGrid {
  voxelM: number
  /** Lowest corner, in whole voxels: metres = index × voxelM. */
  origin: [number, number, number]
  /** Voxels along x, y and z. */
  size: [number, number, number]
}

/** Heat fields on a grid, 0–255, x fastest then y then z (canonical Z-up). */
export interface FireVolume extends VoxelGrid {
  /** Flame heat: full at burning cells, fading upward. */
  flame: Uint8Array
  /** Glow of cells that are heating but not yet burning. */
  ember: Uint8Array
}

/** The heat fields for `cells` (from `fireCellBoxes`), or null with no cells. */
export function fireVolume(cells: FireCellBox[]): FireVolume | null {
  if (cells.length === 0) return null
  const low = [Infinity, Infinity, Infinity]
  const high = [-Infinity, -Infinity, -Infinity]
  for (const cell of cells) {
    for (const point of samplePoints(cell, 1)) {
      for (let axis = 0; axis < 3; axis++) {
        low[axis] = Math.min(low[axis], point[axis])
        high[axis] = Math.max(high[axis], point[axis])
      }
    }
  }
  low[0] -= SIDE_MARGIN_M
  low[1] -= SIDE_MARGIN_M
  low[2] -= BOTTOM_MARGIN_M
  high[0] += SIDE_MARGIN_M
  high[1] += SIDE_MARGIN_M
  high[2] += FLAME_HEADROOM_M

  let voxelM = BASE_VOXEL_M
  let grid = gridAround(low, high, voxelM)
  while (voxelCount(grid) > MAX_VOXELS) {
    voxelM *= 2
    grid = gridAround(low, high, voxelM)
  }
  const [nx, ny, nz] = grid.size

  const fuel = new Float32Array(nx * ny * nz)
  const ember = new Float32Array(nx * ny * nz)
  for (const cell of cells) {
    const field = cell.burning ? fuel : ember
    for (const [x, y, z] of samplePoints(cell, voxelM)) {
      const i = Math.floor(x / voxelM) - grid.origin[0]
      const j = Math.floor(y / voxelM) - grid.origin[1]
      const k = Math.floor(z / voxelM) - grid.origin[2]
      field[i + nx * (j + ny * k)] = 1
    }
  }

  // Carry heat up layer by layer. Each layer is the one below, spread
  // sideways and cooled, so a lone cell's flame dies quickly while the
  // middle of a wide fire keeps its heat and climbs.
  const cooling = Math.exp(-voxelM / HEAT_FALLOFF_M)
  const layer = nx * ny
  const rising = new Float32Array(layer)
  const scratch = new Float32Array(layer)
  for (let k = 0; k < nz; k++) {
    const slice = fuel.subarray(k * layer, (k + 1) * layer)
    blurSlice(rising, scratch, nx, ny)
    for (let i = 0; i < layer; i++) {
      rising[i] = Math.max(slice[i], rising[i] * cooling)
      slice[i] = rising[i]
    }
  }

  return { ...grid, flame: quantize(smooth(fuel, grid.size)), ember: quantize(smooth(ember, grid.size)) }
}

export function voxelCount(grid: VoxelGrid): number {
  return grid.size[0] * grid.size[1] * grid.size[2]
}

/** The smallest grid holding both. They must share a voxel size. */
export function unionGrid(left: VoxelGrid, right: VoxelGrid): VoxelGrid {
  const origin = [0, 1, 2].map((a) => Math.min(left.origin[a], right.origin[a]))
  const end = [0, 1, 2].map((a) =>
    Math.max(left.origin[a] + left.size[a], right.origin[a] + right.size[a]),
  )
  return {
    voxelM: left.voxelM,
    origin: origin as [number, number, number],
    size: end.map((value, a) => value - origin[a]) as [number, number, number],
  }
}

/**
 * Copies a field from grid `from` into channel `channel` of `target`, which
 * holds `stride` interleaved channels on grid `to` (same voxel size). Voxels
 * outside `from` are left as they are.
 */
export function blitField(
  field: Uint8Array,
  from: VoxelGrid,
  target: Uint8Array,
  to: VoxelGrid,
  channel: number,
  stride: number,
): void {
  const [dx, dy, dz] = [0, 1, 2].map((a) => from.origin[a] - to.origin[a])
  const [fx, fy, fz] = from.size
  const [tx, ty, tz] = to.size
  for (let k = Math.max(0, -dz); k < Math.min(fz, tz - dz); k++) {
    for (let j = Math.max(0, -dy); j < Math.min(fy, ty - dy); j++) {
      for (let i = Math.max(0, -dx); i < Math.min(fx, tx - dx); i++) {
        target[(i + dx + tx * (j + dy + ty * (k + dz))) * stride + channel] =
          field[i + fx * (j + fy * k)]
      }
    }
  }
}

function gridAround(low: number[], high: number[], voxelM: number): VoxelGrid {
  const origin = low.map((value) => Math.floor(value / voxelM)) as [number, number, number]
  const size = high.map((value, a) => Math.ceil(value / voxelM) - origin[a]) as [number, number, number]
  return { voxelM, origin, size }
}

/**
 * Points filling a cell at most `spacingM` apart, in canonical metres. The
 * cell's corners are always included, so the points span its full extent.
 */
function samplePoints(cell: FireCellBox, spacingM: number): [number, number, number][] {
  const { position: p, rotation: q, scale: s } = cell
  const counts = [s.x, s.y, s.z].map((length) => Math.max(1, Math.ceil(length / spacingM)))
  const points: [number, number, number][] = []
  for (let i = 0; i <= counts[0]; i++) {
    for (let j = 0; j <= counts[1]; j++) {
      for (let k = 0; k <= counts[2]; k++) {
        const x = (i / counts[0] - 0.5) * s.x
        const y = (j / counts[1] - 0.5) * s.y
        const z = (k / counts[2] - 0.5) * s.z
        // v + 2w(q × v) + 2 q × (q × v)
        const cx = q.y * z - q.z * y
        const cy = q.z * x - q.x * z
        const cz = q.x * y - q.y * x
        points.push([
          p.x + x + 2 * (q.w * cx + q.y * cz - q.z * cy),
          p.y + y + 2 * (q.w * cy + q.z * cx - q.x * cz),
          p.z + z + 2 * (q.w * cz + q.x * cy - q.y * cx),
        ])
      }
    }
  }
  return points
}

/** A 1-2-1 blur of one horizontal slice, in place. */
function blurSlice(slice: Float32Array, scratch: Float32Array, nx: number, ny: number): void {
  blurAxis(slice, scratch, nx, ny, 1, 1)
  blurAxis(scratch, slice, nx, ny, 1, nx)
}

/** A 1-2-1 blur along all three axes, which rounds the cells' corners off. */
function smooth(field: Float32Array, [nx, ny, nz]: [number, number, number]): Float32Array {
  const scratch = new Float32Array(field.length)
  blurAxis(field, scratch, nx, ny, nz, 1)
  blurAxis(scratch, field, nx, ny, nz, nx)
  blurAxis(field, scratch, nx, ny, nz, nx * ny)
  return scratch
}

/** 1-2-1 blur along the axis whose neighbours are `step` apart; edges clamp. */
function blurAxis(
  source: Float32Array,
  target: Float32Array,
  nx: number,
  ny: number,
  nz: number,
  step: number,
): void {
  const length = step === 1 ? nx : step === nx ? ny : nz
  const total = nx * ny * nz
  for (let index = 0; index < total; index++) {
    const along = Math.floor(index / step) % length
    const before = along > 0 ? source[index - step] : source[index]
    const after = along < length - 1 ? source[index + step] : source[index]
    target[index] = (before + 2 * source[index] + after) / 4
  }
}

function quantize(field: Float32Array): Uint8Array {
  const bytes = new Uint8Array(field.length)
  for (let i = 0; i < field.length; i++) bytes[i] = Math.round(Math.min(1, field[i]) * 255)
  return bytes
}
