import { createElement } from 'react'
import { renderToString } from 'react-dom/server'
import { afterEach, describe, expect, it } from 'vitest'
import { PLAYBACK_RATE, useFireStore, useSimulationAgents } from './fireStore'
import type { SimulateFireResponse } from './types'

const run: SimulateFireResponse = {
  revision: 0,
  excluded_object_ids: [],
  results: {
    schema_version: 1,
    scene_id: 'scene',
    scenario_id: 'surface-fire:sofa',
    engine: { name: 'test', version: '0' },
    frames: [0, 1, 2].map((time_seconds) => ({
      time_seconds,
      surface_fire: [],
      rooms: [],
      fires: [],
      openings: [],
      agents: [],
    })),
  },
}

afterEach(() => useFireStore.getState().clear())

describe('fire playback', () => {
  it('mounts with no simulation without entering an empty-agent render loop', () => {
    function AgentCount() {
      return createElement('span', null, useSimulationAgents().length)
    }

    expect(renderToString(createElement(AgentCount))).toBe('<span>0</span>')
  })

  it('advances while playing and pauses at the last frame', () => {
    useFireStore.setState({ status: 'playing', run, clockSeconds: 0 })
    useFireStore.getState().tick(0.05)
    expect(useFireStore.getState().clockSeconds).toBeCloseTo(0.05 * PLAYBACK_RATE)

    for (let i = 0; i < 100; i++) useFireStore.getState().tick(0.1)
    expect(useFireStore.getState()).toMatchObject({ clockSeconds: 2, status: 'paused' })
  })

  it('does not advance while paused', () => {
    useFireStore.setState({ status: 'paused', run, clockSeconds: 1 })
    useFireStore.getState().tick(0.1)
    expect(useFireStore.getState().clockSeconds).toBe(1)
  })

  it('plays from the start after reaching the end', () => {
    useFireStore.setState({ status: 'paused', run, clockSeconds: 2 })
    useFireStore.getState().play()
    expect(useFireStore.getState()).toMatchObject({ clockSeconds: 0, status: 'playing' })
  })
})
