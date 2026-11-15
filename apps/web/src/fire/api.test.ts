import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  DEFAULT_BATCH_SIMULATION_COUNT,
  DEFAULT_CREW,
  DEFAULT_TOP_K,
  clampOccupantCount,
  describeFireError,
  runFireSimulation,
  runScenarioBatch,
} from './api'
import type {
  GeneratedFireScenariosResponse,
  SimulateFireResponse,
  SimulateNResponse,
  SpawnOccupantsResponse,
} from './types'

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

const batchResponse: SimulateNResponse = {
  revision: 3,
  runs: [{ status: 'completed', index: 0, simulation: response }],
  fire_ensemble: {
    requested_top_k: 5,
    completed_simulation_count: 1,
    config: { cell_size_m: 0.25 },
    heatmap: [],
    representatives: [{ simulation_index: 0, support_count: 1, support_fraction: 1 }],
  },
}

const generatedScenarios: GeneratedFireScenariosResponse = {
  revision: 3,
  scenarios: Array.from({ length: DEFAULT_BATCH_SIMULATION_COUNT }, (_, seed) => ({
    object_id: seed % 2 === 0 ? 'sofa-1' : 'desk-1',
    occupants: occupants.occupants,
    seed,
  })),
}

afterEach(() => vi.unstubAllGlobals())

describe('DEFAULT_CREW', () => {
  it('is one firefighter arriving 10 s after ignition', () => {
    expect(DEFAULT_CREW).toEqual({ id: 'engine-1', member_count: 1, arrival_time_seconds: 10 })
  })
})

describe('runFireSimulation', () => {
  it('asks for the chosen number and layout of occupants', async () => {
    const fetch = vi.fn((url: string, _init?: RequestInit) =>
      Promise.resolve({
        ok: true,
        status: 200,
        json: async () => (url === '/api/agents/spawn_occupants' ? occupants : response),
      }),
    )
    vi.stubGlobal('fetch', fetch)

    await runFireSimulation('sofa-1', { occupantCount: 20, layout: 'spread' })

    expect(fetch).toHaveBeenCalledWith('/api/agents/spawn_occupants', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ count: 20, layout: 'spread' }),
    })
  })

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
      body: JSON.stringify({ count: 10, layout: 'crowded' }),
    })
    expect(fetch).toHaveBeenCalledWith('/api/fire/simulate/hybrid', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        object_id: 'sofa-1',
        occupants: occupants.occupants,
        crews: [DEFAULT_CREW],
      }),
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

describe('runScenarioBatch', () => {
  it('sends ten seeded simulations and requests four representatives', async () => {
    const fetch = vi.fn((url: string, _init?: RequestInit) =>
      Promise.resolve({
        ok: true,
        status: 200,
        json: async () =>
          url === '/api/agents/generate_fire_scenarios' ? generatedScenarios : batchResponse,
      }),
    )
    vi.stubGlobal('fetch', fetch)

    await runScenarioBatch({ occupantCount: 20, layout: 'crowded' })

    expect(fetch).toHaveBeenCalledWith('/api/agents/generate_fire_scenarios', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        count: DEFAULT_BATCH_SIMULATION_COUNT,
        occupant_count: 20,
      }),
    })

    const batchCall = fetch.mock.calls.find(([url]) => url === '/api/fire/simulate_n')
    expect(batchCall).toBeDefined()
    const payload = JSON.parse(batchCall![1]!.body as string)
    expect(payload.top_k).toBe(DEFAULT_TOP_K)
    expect(payload.representatives_only).toBe(true)
    expect(payload.response_interval_seconds).toBe(2)
    expect(payload.simulations).toHaveLength(DEFAULT_BATCH_SIMULATION_COUNT)
    expect(payload.simulations.map((simulation: { seed: number }) => simulation.seed)).toEqual(
      Array.from({ length: DEFAULT_BATCH_SIMULATION_COUNT }, (_, index) => index),
    )
    expect(payload.simulations[0]).toMatchObject({
      object_id: 'sofa-1',
      occupants: occupants.occupants,
      crews: [DEFAULT_CREW],
      duration_seconds: 360,
    })
  })
})

describe('clampOccupantCount', () => {
  it('rounds to a whole number within 1–30', () => {
    expect(clampOccupantCount(0)).toBe(1)
    expect(clampOccupantCount(-5)).toBe(1)
    expect(clampOccupantCount(31)).toBe(30)
    expect(clampOccupantCount(7.6)).toBe(8)
    expect(clampOccupantCount(10)).toBe(10)
  })
})
