import { describe, expect, it } from 'vitest'
import type { AgentFrame, AgentTimelineFrame } from '../fire/types'
import { leadRoute, routeRibbon, type RoutePoint } from './route'

const firefighter = (id: string, x: number, y: number) =>
  ({
    id,
    position_m: { x, y, z: 0 },
    role: { type: 'firefighter', crew_id: 'engine-1', phase: 'advancing' },
  }) as AgentFrame

const frame = (seconds: number, ...agents: AgentFrame[]): AgentTimelineFrame => ({
  time_seconds: seconds,
  agents,
})

const point = (x: number, y: number, seconds = 0): RoutePoint => ({ x, y, z: 0, seconds })

describe('leadRoute', () => {
  it('is empty with no crew, or a crew that never moves', () => {
    expect(leadRoute([frame(0), frame(1)])).toEqual([])
    expect(leadRoute([frame(0, firefighter('engine-1/1', 1, 1)), frame(1, firefighter('engine-1/1', 1, 1))])).toEqual([])
  })

  it("follows the crew's leader from where they start to where they finish", () => {
    const route = leadRoute([
      frame(0, firefighter('engine-1/2', 9, 9), firefighter('engine-1/1', 0, 0)),
      frame(1, firefighter('engine-1/2', 9, 8), firefighter('engine-1/1', 1, 0)),
      frame(2, firefighter('engine-1/2', 9, 7), firefighter('engine-1/1', 2, 0)),
    ])
    expect(route[0]).toEqual({ x: 0, y: 0, z: 0, seconds: 0 })
    expect(route.at(-1)).toEqual({ x: 2, y: 0, z: 0, seconds: 2 })
    expect(route.every((p) => p.y === 0)).toBe(true)
  })

  it('leaves a spot when the firefighter next moves, not when they arrived', () => {
    const route = leadRoute([
      frame(0, firefighter('engine-1/1', 0, 0)),
      frame(5, firefighter('engine-1/1', 0, 0)),
      frame(6, firefighter('engine-1/1', 1, 0)),
    ])
    expect(route[0].seconds).toBe(5)
  })

  it('rounds corners off without moving the ends', () => {
    const route = leadRoute([
      frame(0, firefighter('engine-1/1', 0, 0)),
      frame(1, firefighter('engine-1/1', 1, 0)),
      frame(2, firefighter('engine-1/1', 1, 1)),
    ])
    expect(route.some((p) => p.x === 1 && p.y === 0)).toBe(false)
    expect(route[0]).toMatchObject({ x: 0, y: 0 })
    expect(route.at(-1)).toMatchObject({ x: 1, y: 1 })
    // Times still run forward along it.
    expect(route.every((p, i) => i === 0 || p.seconds >= route[i - 1].seconds)).toBe(true)
  })
})

describe('routeRibbon', () => {
  it('is null without a route', () => {
    expect(routeRibbon([])).toBeNull()
    expect(routeRibbon([point(1, 1), point(1, 1)])).toBeNull()
  })

  it('runs a ribbon along the route and ends in an arrowhead at the last point', () => {
    const ribbon = routeRibbon([point(0, 0, 0), point(2, 0, 4), point(4, 0, 8)])!
    expect(ribbon.lengthM).toBe(4)
    const corners = ribbon.positions.length / 3
    const corner = (i: number) => [...ribbon.positions.slice(i * 3, i * 3 + 2)].map((v) => +v.toFixed(3))
    // The ribbon starts either side of the first point…
    expect(corner(0)).toEqual([0, 0.14])
    expect(corner(1)).toEqual([0, -0.14])
    // …and the last corner is the arrowhead's tip, wider at its base than the ribbon.
    expect(corner(corners - 1)).toEqual([4, 0])
    expect(corner(corners - 3)).toEqual([3.3, 0.4])
    expect(ribbon.along[corners - 1]).toBe(4)
    expect(ribbon.seconds[corners - 1]).toBe(8)
    expect(Math.max(...ribbon.index)).toBe(corners - 1)
    expect(ribbon.index.length % 3).toBe(0)
  })

  it('sits just above the floor, the start a little higher than the end', () => {
    const ribbon = routeRibbon([point(0, 0), point(4, 0)])!
    const heights = [...ribbon.positions].filter((_, i) => i % 3 === 2)
    expect(heights[0]).toBeCloseTo(0.07)
    expect(heights.at(-1)).toBeCloseTo(0.05)
  })

  it('shrinks the arrowhead on a very short route', () => {
    const ribbon = routeRibbon([point(0, 0), point(0.5, 0)])!
    const corners = ribbon.positions.length / 3
    expect(ribbon.positions[(corners - 3) * 3]).toBeCloseTo(0.2)
  })
})
