import { describe, expect, it } from 'vitest'
import { frameAtTime } from './frames'
import type { SimulationFrame } from './types'

const frame = (time_seconds: number): SimulationFrame => ({
  time_seconds,
  surface_fire: [],
  rooms: [],
  fires: [],
  openings: [],
  agents: [],
})

const frames = [frame(0), frame(1), frame(2.5), frame(4)]

describe('frameAtTime', () => {
  it('finds nothing in an empty timeline', () => {
    expect(frameAtTime([], 3)).toBeUndefined()
  })

  it('finds nothing before the first frame', () => {
    expect(frameAtTime([frame(1), frame(2)], 0.5)).toBeUndefined()
  })

  it('returns the frame at an exact time', () => {
    expect(frameAtTime(frames, 2.5)?.time_seconds).toBe(2.5)
  })

  it('holds the previous frame between frames', () => {
    expect(frameAtTime(frames, 3.9)?.time_seconds).toBe(2.5)
    expect(frameAtTime(frames, 0.2)?.time_seconds).toBe(0)
  })

  it('holds the last frame past the end', () => {
    expect(frameAtTime(frames, 100)?.time_seconds).toBe(4)
  })
})
