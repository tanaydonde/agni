import { afterEach, describe, expect, it, vi } from 'vitest'
import { describeFireError, runFireSimulation } from './api'
import type { SimulateFireResponse, SpawnOccupantsResponse } from './types'

const agent = {
  id: 'occupant-1',
  floor_id: 'floor-1',
  position_m: { x: 1, y: 2, z: 0 },
  velocity_m_per_s: { x: 0, y: 0, z: 0 },
  radius_m: 0.25,
  exposure: { heat: 0, toxic_gas: 0 },
  role: { type: 'occupant' as const, phase: 'unaware' as const },
}

const occupants: SpawnOccupantsResponse = {
  revision: 3,
  attempts: 1,
  occupants: [
    {
      id: agent.id,
      floor_id: agent.floor_id,
      position_m: agent.position_m,
      walking_speed_m_per_s: 1.2,
      pre_movement_delay_seconds: 2,
    },
  ],
  agents: [agent],
}

const response: SimulateFireResponse = {
  revision: 3,
  excluded_object_ids: [],
  results: {
    schema_version: 1,
    scene_id: 'scene-1',
    scenario_id: 'hybrid:sofa-1',
    engine: { name: 'cfast', version: '7.7.7' },
    frames: [
      {
        time_seconds: 0,
        surface_fire: [],
        rooms: [],
        fires: [],
        openings: [],
        agents: [agent],
      },
    ],
  },
}

afterEach(() => vi.unstubAllGlobals())

describe('runFireSimulation', () => {
  it('generates occupants then sends them into the hybrid simulation', async () => {
    const fetch = vi.fn((url: string) =>
      Promise.resolve({
        ok: true,
        status: 200,
        json: async () => (url === '/api/agents/spawn_occupants' ? occupants : response),
      }),
    )
    vi.stubGlobal('fetch', fetch)

    const run = await runFireSimulation('sofa-1')

    expect(run.results.frames[0].agents).toEqual([agent])
    expect(fetch).toHaveBeenCalledWith('/api/agents/spawn_occupants', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ count: 10 }),
    })
    expect(fetch).toHaveBeenCalledWith('/api/fire/simulate/hybrid', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ object_id: 'sofa-1', occupants: occupants.occupants }),
    })
  })

  it('shows a configured backend error instead of calling it unreachable', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn((url: string) =>
        Promise.resolve({
          ok: url === '/api/agents/spawn_occupants',
          status: url === '/api/agents/spawn_occupants' ? 200 : 503,
          json: async () =>
            url === '/api/agents/spawn_occupants'
              ? occupants
              : {
                  code: 'cfast_not_configured',
                  message: 'set CFAST_BIN to the CFAST executable path',
                },
        }),
      ),
    )

    const error = await runFireSimulation('sofa-1').catch((error: unknown) => error)
    expect(describeFireError(error)).toBe('CFAST is not configured on the world server')
  })
})
