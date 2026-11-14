import { afterEach, describe, expect, it, vi } from 'vitest'
import { describeFireError, runFireSimulation } from './api'
import type { SimulateFireResponse } from './types'

const response: SimulateFireResponse = {
  revision: 3,
  excluded_object_ids: [],
  results: {
    schema_version: 1,
    scene_id: 'scene-1',
    scenario_id: 'hybrid:sofa-1',
    engine: { name: 'cfast', version: '7.7.7' },
    frames: [],
  },
}

afterEach(() => vi.unstubAllGlobals())

describe('runFireSimulation', () => {
  it('calls the hybrid Rust and CFAST endpoint', async () => {
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => response,
    })
    vi.stubGlobal('fetch', fetch)

    await expect(runFireSimulation('sofa-1')).resolves.toEqual(response)
    expect(fetch).toHaveBeenCalledWith('/api/fire/simulate/hybrid', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ object_id: 'sofa-1' }),
    })
  })

  it('shows a configured backend error instead of calling it unreachable', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: false,
        status: 503,
        json: async () => ({
          code: 'cfast_not_configured',
          message: 'set CFAST_BIN to the CFAST executable path',
        }),
      }),
    )

    const error = await runFireSimulation('sofa-1').catch((error: unknown) => error)
    expect(describeFireError(error)).toBe('CFAST is not configured on the world server')
  })
})
