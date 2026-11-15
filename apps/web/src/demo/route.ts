// The lead firefighter's route through a run, as an arrow to lay on the
// floor: a ribbon along the path they walk, ending in an arrowhead.

import { leadFirefighter } from '../fire/frames'
import type { AgentTimelineFrame } from '../fire/types'

/** A point on the route, in canonical Z-up metres, and when the firefighter is there. */
export interface RoutePoint {
  x: number
  y: number
  z: number
  seconds: number
}

/** Steps shorter than this are standing still, not route. */
const MIN_STEP_M = 0.1
const RIBBON_WIDTH_M = 0.28
const HEAD_LENGTH_M = 0.7
const HEAD_WIDTH_M = 0.8
/** Height above the floor: clear of the scanned floor's bumps. */
const LIFT_M = 0.05
/**
 * The start of the route sits this much higher than its end. Where the route
 * crosses or retraces itself, the part walked sooner is then on top.
 */
const STACK_M = 0.02

/**
 * Where the lead firefighter walks over the whole run, in order, with the
 * corners of their steps rounded off. Empty if there is no crew or they
 * never move.
 */
export function leadRoute(frames: AgentTimelineFrame[]): RoutePoint[] {
  const leadId = frames.map((frame) => leadFirefighter(frame.agents)).find(Boolean)?.id
  const points: RoutePoint[] = []
  for (const frame of frames) {
    const lead = frame.agents.find((agent) => agent.id === leadId)
    if (!lead) continue
    const point = { ...lead.position_m, seconds: frame.time_seconds }
    const last = points.at(-1)
    if (!last) points.push(point)
    else if (Math.hypot(point.x - last.x, point.y - last.y, point.z - last.z) >= MIN_STEP_M) points.push(point)
    // Standing still: the route leaves this spot when they next move.
    else last.seconds = frame.time_seconds
  }
  if (points.length < 2) return []
  return roundCorners(roundCorners(points))
}

/** One pass of corner cutting (Chaikin); the two ends stay put. */
function roundCorners(points: RoutePoint[]): RoutePoint[] {
  const rounded = [points[0]]
  for (let index = 0; index < points.length - 1; index++) {
    rounded.push(between(points[index], points[index + 1], 0.25), between(points[index], points[index + 1], 0.75))
  }
  rounded.push(points[points.length - 1])
  return rounded
}

function between(from: RoutePoint, to: RoutePoint, fraction: number): RoutePoint {
  const mix = (a: number, b: number) => a + (b - a) * fraction
  return { x: mix(from.x, to.x), y: mix(from.y, to.y), z: mix(from.z, to.z), seconds: mix(from.seconds, to.seconds) }
}

/** The arrow as triangles, with what the shader needs at each corner. */
export interface RouteRibbon {
  /** x, y, z per corner, canonical Z-up metres. */
  positions: Float32Array
  /** Per corner: metres to the side of the route's centre line (left positive). */
  across: Float32Array
  /** Per corner: metres along the route from its start. */
  along: Float32Array
  /** Per corner: when the firefighter is there. */
  seconds: Float32Array
  index: Uint32Array
  lengthM: number
}

/**
 * The arrow for `points`: a ribbon along them, then an arrowhead whose tip is
 * the last point. Null with fewer than two points.
 */
export function routeRibbon(points: RoutePoint[]): RouteRibbon | null {
  if (points.length < 2) return null
  const distances = [0]
  for (let index = 1; index < points.length; index++) {
    const from = points[index - 1]
    const to = points[index]
    distances.push(distances[index - 1] + Math.hypot(to.x - from.x, to.y - from.y))
  }
  const lengthM = distances[distances.length - 1]
  if (lengthM < 1e-6) return null
  // A short route is mostly arrowhead.
  const headLength = Math.min(HEAD_LENGTH_M, lengthM * 0.6)
  const headWidth = HEAD_WIDTH_M * (headLength / HEAD_LENGTH_M)
  const neckM = lengthM - headLength

  // The ribbon's centre line: the points up to the arrowhead's base.
  const neckIndex = distances.findIndex((distance) => distance >= neckM)
  const before = points[neckIndex - 1]
  const span = distances[neckIndex] - distances[neckIndex - 1]
  const neck = between(before, points[neckIndex], span > 0 ? (neckM - distances[neckIndex - 1]) / span : 0)
  const line = [...points.slice(0, neckIndex), neck]
  const lineDistances = [...distances.slice(0, neckIndex), neckM]

  const positions: number[] = []
  const across: number[] = []
  const along: number[] = []
  const seconds: number[] = []
  const index: number[] = []
  const corner = (point: RoutePoint, side: { x: number; y: number }, offset: number, distance: number) => {
    positions.push(
      point.x + side.x * offset,
      point.y + side.y * offset,
      point.z + LIFT_M + STACK_M * (1 - distance / lengthM),
    )
    across.push(offset)
    along.push(distance)
    seconds.push(point.seconds)
  }

  const tip = points[points.length - 1]
  for (let i = 0; i < line.length; i++) {
    // Sideways is square to the way the line runs through this point.
    const from = line[Math.max(0, i - 1)]
    const to = i === line.length - 1 ? tip : line[i + 1]
    const side = leftOf(from, to)
    corner(line[i], side, RIBBON_WIDTH_M / 2, lineDistances[i])
    corner(line[i], side, -RIBBON_WIDTH_M / 2, lineDistances[i])
    if (i > 0) index.push(2 * i - 2, 2 * i - 1, 2 * i, 2 * i, 2 * i - 1, 2 * i + 1)
  }

  const headSide = leftOf(neck, tip)
  const first = positions.length / 3
  corner(neck, headSide, headWidth / 2, neckM)
  corner(neck, headSide, -headWidth / 2, neckM)
  corner(tip, headSide, 0, lengthM)
  index.push(first, first + 1, first + 2)

  return {
    positions: Float32Array.from(positions),
    across: Float32Array.from(across),
    along: Float32Array.from(along),
    seconds: Float32Array.from(seconds),
    index: Uint32Array.from(index),
    lengthM,
  }
}

/** The unit horizontal direction to the left of travel from `from` to `to`. */
function leftOf(from: RoutePoint, to: RoutePoint): { x: number; y: number } {
  const x = to.x - from.x
  const y = to.y - from.y
  const length = Math.hypot(x, y) || 1
  return { x: -y / length, y: x / length }
}
