import { describe, expect, it } from 'vitest'
import { agentsAtTime, crewStatus, frameAtTime, isShownAgent, occupantCounts } from './frames'
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

describe('agentsAtTime', () => {
  it('smoothly interpolates positions between simulation frames', () => {
    const first = frame(0)
    const second = frame(1)
    first.agents = [agent(0)]
    second.agents = [agent(2)]

    expect(agentsAtTime([first, second], 0.25)[0].position_m.x).toBe(0.5)
    expect(agentsAtTime([first, second], 0.75)[0].position_m.x).toBe(1.5)
  })
})

describe('occupantCounts', () => {
  it('counts only exited occupants as having left', () => {
    const inside = agent(0)
    const exited = { ...agent(1), id: 'occupant-2', role: { type: 'occupant' as const, phase: 'exited' as const } }
    const firefighter = {
      ...agent(2),
      id: 'engine-1/1',
      role: { type: 'firefighter' as const, crew_id: 'engine-1', phase: 'searching' as const },
    }

    expect(occupantCounts([inside, exited, firefighter])).toEqual({ inside: 1, exited: 1 })
  })
})

describe('crewStatus', () => {
  const firefighter = (id: string, phase: 'staging' | 'suppressing') => ({
    ...agent(0),
    id,
    role: { type: 'firefighter' as const, crew_id: 'engine-1', phase },
  })

  it('reports nothing without a crew', () => {
    expect(crewStatus([agent(0)])).toBeNull()
  })

  it("describes the leader's phase", () => {
    expect(
      crewStatus([firefighter('engine-1/2', 'staging'), firefighter('engine-1/1', 'suppressing')]),
    ).toBe('Crew spraying')
  })
})

describe('isShownAgent', () => {
  it('shows occupants and firefighters on the floor until they leave', () => {
    const firefighter = {
      ...agent(0),
      id: 'engine-1/1',
      role: { type: 'firefighter' as const, crew_id: 'engine-1', phase: 'advancing' as const },
    }
    const gone = { ...firefighter, role: { ...firefighter.role, phase: 'exited' as const } }

    expect(isShownAgent(agent(0), 'floor-1')).toBe(true)
    expect(isShownAgent(firefighter, 'floor-1')).toBe(true)
    expect(isShownAgent(gone, 'floor-1')).toBe(false)
    expect(isShownAgent(firefighter, 'floor-2')).toBe(false)
  })
})

function agent(x: number) {
  return {
    id: 'occupant-1',
    floor_id: 'floor-1',
    position_m: { x, y: 0, z: 0 },
    velocity_m_per_s: { x: 1, y: 0, z: 0 },
    radius_m: 0.25,
    exposure: { heat: 0, toxic_gas: 0 },
    role: { type: 'occupant' as const, phase: 'moving_to_exit' as const },
  }
}
