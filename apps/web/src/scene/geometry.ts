// Pure geometry derived from the canonical scene. Everything here stays in the
// canonical Z-up world (x/y horizontal, z up, metres); the renderer converts to
// Three.js Y-up once, at its boundary.

import type { BoundaryEdge, Floor, Opening } from './types'

export interface Point2 {
  x: number
  y: number
}

/** An axis-aligned box in the wall's local frame, placed in the world. */
export interface WallPiece {
  center: [number, number, number]
  length: number
  height: number
  thickness: number
  /** Rotation about world +Z, in radians. */
  angle: number
}

export interface FloorGeometry {
  floorZ: number
  /** Closed outlines per room; the first loop is the outer boundary. */
  roomOutlines: Point2[][][]
  walls: WallPiece[]
  windows: WallPiece[]
  /** Non-wall room separators, as line segments. */
  separators: [Point2, Point2][]
  bounds: { min: Point2; max: Point2; height: number }
}

export const DEFAULT_WALL_HEIGHT_M = 2.7
export const DEFAULT_WALL_THICKNESS_M = 0.1
const DEFAULT_DOOR_HEIGHT_M = 2.1
const DEFAULT_WINDOW_SILL_M = 0.9
const DEFAULT_WINDOW_HEIGHT_M = 1.2
const WINDOW_PANE_THICKNESS_M = 0.02
const MIN_SIZE_M = 1e-3

/** Joins a room's boundary edges into closed loops, largest first. */
export function chainLoops(edges: BoundaryEdge[], tolerance = 0.02): Point2[][] {
  const near = (a: Point2, b: Point2) => Math.hypot(a.x - b.x, a.y - b.y) <= tolerance
  const remaining = edges.map((edge) => [toPoint(edge.start), toPoint(edge.end)] as const)
  const loops: Point2[][] = []

  while (remaining.length > 0) {
    const [first, second] = remaining.shift()!
    const loop = [first, second]

    for (;;) {
      const tail = loop[loop.length - 1]
      if (loop.length > 2 && near(tail, loop[0])) {
        loop.pop()
        if (loop.length >= 3) loops.push(loop)
        break
      }
      const index = remaining.findIndex(([a, b]) => near(a, tail) || near(b, tail))
      if (index === -1) break // open chain: boundary is incomplete, skip it
      const [a, b] = remaining.splice(index, 1)[0]
      loop.push(near(a, tail) ? b : a)
    }
  }

  return loops.sort((a, b) => Math.abs(signedArea(b)) - Math.abs(signedArea(a)))
}

/**
 * Splits one wall edge into solid boxes around its openings.
 *
 * `center_line_bias` is read as where the edge sits across the wall's
 * thickness (0.5 = centered), shifting the wall along the edge's left normal.
 */
export function wallPieces(
  edge: BoundaryEdge,
  floorZ: number,
  wallHeight: number,
): { walls: WallPiece[]; windows: WallPiece[] } {
  const start = toPoint(edge.start)
  const end = toPoint(edge.end)
  const length = Math.hypot(end.x - start.x, end.y - start.y)
  if (length < MIN_SIZE_M || wallHeight < MIN_SIZE_M) return { walls: [], windows: [] }

  const dir = { x: (end.x - start.x) / length, y: (end.y - start.y) / length }
  const normal = { x: -dir.y, y: dir.x }
  const angle = Math.atan2(dir.y, dir.x)
  const thickness =
    edge.thickness_m && edge.thickness_m > 0 ? edge.thickness_m : DEFAULT_WALL_THICKNESS_M
  const offset = ((edge.center_line_bias ?? 0.5) - 0.5) * thickness

  const place = (along: number, z0: number, z1: number, depth: number): WallPiece => ({
    center: [
      start.x + dir.x * along + normal.x * offset,
      start.y + dir.y * along + normal.y * offset,
      floorZ + (z0 + z1) / 2,
    ],
    length: 0,
    height: z1 - z0,
    thickness: depth,
    angle,
  })

  const holes = edge.openings
    .map((opening) => openingSpan(opening, length, floorZ, wallHeight))
    .filter((span): span is OpeningSpan => span !== null)

  const breaks = [0, length, ...holes.flatMap((hole) => [hole.from, hole.to])]
  const stops = [...new Set(breaks)].sort((a, b) => a - b)

  const walls: WallPiece[] = []
  for (let i = 0; i < stops.length - 1; i++) {
    const a = stops[i]
    const b = stops[i + 1]
    if (b - a < MIN_SIZE_M) continue
    const mid = (a + b) / 2
    const covering = holes.filter((hole) => hole.from <= mid && mid <= hole.to)
    for (const [z0, z1] of subtractRanges([0, wallHeight], covering)) {
      walls.push({ ...place(mid, z0, z1, thickness), length: b - a })
    }
  }

  const windows = holes
    .filter((hole) => hole.kind === 'window')
    .map((hole) => ({
      ...place((hole.from + hole.to) / 2, hole.bottom, hole.top, WINDOW_PANE_THICKNESS_M),
      length: hole.to - hole.from,
    }))

  return { walls, windows }
}

/** Builds renderable geometry for one floor. */
export function buildFloorGeometry(floor: Pick<Floor, 'elevation_m' | 'rooms'>): FloorGeometry {
  const floorZ = floor.elevation_m ?? 0
  const roomOutlines = floor.rooms
    .map((room) => chainLoops(room.boundary))
    .filter((loops) => loops.length > 0)

  // Rooms share edges (same id) along common walls; build each edge once,
  // using the tallest adjoining room.
  const edges = new Map<string, { edge: BoundaryEdge; height: number }>()
  for (const room of floor.rooms) {
    const height = room.dimensions?.height_m ?? DEFAULT_WALL_HEIGHT_M
    for (const edge of room.boundary) {
      const seen = edges.get(edge.id)
      if (!seen || seen.height < height) edges.set(edge.id, { edge, height })
    }
  }

  const walls: WallPiece[] = []
  const windows: WallPiece[] = []
  const separators: [Point2, Point2][] = []
  for (const { edge, height } of edges.values()) {
    if (edge.kind === 'wall') {
      const pieces = wallPieces(edge, floorZ, height)
      walls.push(...pieces.walls)
      windows.push(...pieces.windows)
    } else {
      separators.push([toPoint(edge.start), toPoint(edge.end)])
    }
  }

  const points = [...edges.values()].flatMap(({ edge }) => [toPoint(edge.start), toPoint(edge.end)])
  const height = Math.max(0, ...[...edges.values()].map(({ height }) => height))

  return {
    floorZ,
    roomOutlines,
    walls,
    windows,
    separators,
    bounds: {
      min: {
        x: Math.min(...points.map((p) => p.x)),
        y: Math.min(...points.map((p) => p.y)),
      },
      max: {
        x: Math.max(...points.map((p) => p.x)),
        y: Math.max(...points.map((p) => p.y)),
      },
      height,
    },
  }
}

interface OpeningSpan {
  kind: Opening['kind']
  from: number
  to: number
  bottom: number
  top: number
}

function openingSpan(
  opening: Opening,
  edgeLength: number,
  floorZ: number,
  wallHeight: number,
): OpeningSpan | null {
  const center = opening.relative_center * edgeLength
  const from = Math.max(0, center - opening.width_m / 2)
  const to = Math.min(edgeLength, center + opening.width_m / 2)
  if (to - from < MIN_SIZE_M) return null

  const isWindow = opening.kind === 'window'
  // Matterport reports opening elevations in the same absolute frame as the
  // floor elevation.
  const bottom = clamp(
    opening.lower_elevation_m !== null
      ? opening.lower_elevation_m - floorZ
      : isWindow
        ? DEFAULT_WINDOW_SILL_M
        : 0,
    0,
    wallHeight,
  )
  const defaultHeight =
    opening.kind === 'window'
      ? DEFAULT_WINDOW_HEIGHT_M
      : opening.kind === 'door'
        ? DEFAULT_DOOR_HEIGHT_M
        : wallHeight
  const top = clamp(bottom + (opening.height_m ?? defaultHeight), bottom, wallHeight)
  if (top - bottom < MIN_SIZE_M) return null

  return { kind: opening.kind, from, to, bottom, top }
}

/** Removes each hole's [bottom, top] from `range`, returning what remains. */
function subtractRanges(range: [number, number], holes: OpeningSpan[]): [number, number][] {
  let solid: [number, number][] = [range]
  for (const { bottom, top } of holes) {
    solid = solid.flatMap(([a, b]): [number, number][] => {
      if (top <= a || bottom >= b) return [[a, b]]
      const parts: [number, number][] = []
      if (bottom > a) parts.push([a, bottom])
      if (top < b) parts.push([top, b])
      return parts
    })
  }
  return solid.filter(([a, b]) => b - a >= MIN_SIZE_M)
}

function signedArea(loop: Point2[]): number {
  let area = 0
  for (let i = 0; i < loop.length; i++) {
    const a = loop[i]
    const b = loop[(i + 1) % loop.length]
    area += a.x * b.y - b.x * a.y
  }
  return area / 2
}

function toPoint(v: { x: number; y: number }): Point2 {
  return { x: v.x, y: v.y }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}
