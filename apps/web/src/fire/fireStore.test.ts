import { createElement } from 'react'
import { renderToString } from 'react-dom/server'
import { afterEach, describe, expect, it } from 'vitest'
import { DEFAULT_PLAYBACK_RATE, PLAYBACK_RATES, useFireStore, useSimulationAgents } from './fireStore'
import type { SimulateFireResponse, SimulateNResponse } from './types'

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

const shorterRun: SimulateFireResponse = {
  ...run,
  results: {
    ...run.results,
    scenario_id: 'surface-fire:desk',
    frames: run.results.frames.slice(0, 2),
  },
}

const batch: SimulateNResponse = {
  revision: 0,
  runs: [
    { status: 'completed', index: 1, simulation: run },
    { status: 'failed', index: 2, http_status: 422, error: { code: 'simulation_failed' } },
    { status: 'completed', index: 4, simulation: shorterRun },
  ],
  fire_ensemble: {
    requested_top_k: 2,
    completed_simulation_count: 2,
    config: { cell_size_m: 0.25 },
    heatmap: [],
    representatives: [
      { simulation_index: 1, support_count: 1, support_fraction: 0.5 },
      { simulation_index: 4, support_count: 1, support_fraction: 0.5 },
    ],
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
    expect(useFireStore.getState().clockSeconds).toBeCloseTo(0.05 * DEFAULT_PLAYBACK_RATE)

    for (let i = 0; i < 100; i++) useFireStore.getState().tick(0.1)
    expect(useFireStore.getState()).toMatchObject({ clockSeconds: 2, status: 'paused' })
  })

  it('keeps playback from a moved start: restart, seeks and replays stop there', () => {
    useFireStore.setState({ status: 'paused', run, clockSeconds: 0 })
    const store = useFireStore.getState()
    store.setClockStart(1)
    expect(useFireStore.getState().clockSeconds).toBe(1)
    store.seekBy(-10)
    expect(useFireStore.getState().clockSeconds).toBe(1)
    store.setClock(0)
    expect(useFireStore.getState().clockSeconds).toBe(1)
    store.setClock(2)
    store.play()
    expect(useFireStore.getState()).toMatchObject({ status: 'playing', clockSeconds: 1 })
    store.setClock(1.5)
    store.restart()
    expect(useFireStore.getState().clockSeconds).toBe(1)
    store.clear()
    expect(useFireStore.getState().clockStartSeconds).toBe(0)
  })

  it('advances at the chosen playback rate', () => {
    useFireStore.setState({ status: 'playing', run, clockSeconds: 0 })
    expect(PLAYBACK_RATES).toEqual([1, 2, 4, 8, 16])
    useFireStore.getState().setPlaybackRate(16)
    useFireStore.getState().tick(0.05)
    expect(useFireStore.getState().clockSeconds).toBeCloseTo(0.8)
    useFireStore.getState().setPlaybackRate(DEFAULT_PLAYBACK_RATE)
  })

  it('keeps scenario choices it was not given', () => {
    const before = useFireStore.getState().scenario
    useFireStore.getState().setScenario({ occupantCount: 20 })
    expect(useFireStore.getState().scenario).toEqual({ ...before, occupantCount: 20 })
    useFireStore.getState().setScenario(before)
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

  it('switches representatives by original batch index and keeps comparable time', () => {
    useFireStore.setState({
      status: 'paused',
      batch,
      run,
      representativePosition: 0,
      clockSeconds: 1.5,
    })

    useFireStore.getState().nextRepresentative()
    expect(useFireStore.getState()).toMatchObject({
      run: shorterRun,
      representativePosition: 1,
      clockSeconds: 1,
      status: 'paused',
    })

    useFireStore.getState().nextRepresentative()
    expect(useFireStore.getState()).toMatchObject({
      run,
      representativePosition: 0,
      clockSeconds: 1,
    })
  })

  it('seeks within the active simulation bounds', () => {
    useFireStore.setState({ status: 'paused', run, clockSeconds: 1 })
    useFireStore.getState().seekBy(10)
    expect(useFireStore.getState().clockSeconds).toBe(2)
    useFireStore.getState().seekBy(-10)
    expect(useFireStore.getState().clockSeconds).toBe(0)
  })

  it('uses the longest representative timeline for synchronized batch playback', () => {
    useFireStore.setState({ status: 'paused', batch, run: shorterRun, clockSeconds: 1 })
    useFireStore.getState().seekBy(10)
    expect(useFireStore.getState().clockSeconds).toBe(2)
    useFireStore.getState().play()
    expect(useFireStore.getState().status).toBe('playing')
  })

  it('stops at the shown run instead when the clock is scoped to it', () => {
    useFireStore.setState({ status: 'paused', batch, run: shorterRun, clockSeconds: 2 })
    useFireStore.getState().setClockScope('run')
    const shorterEnd = shorterRun.results.frames.at(-1)!.time_seconds
    expect(useFireStore.getState().clockSeconds).toBe(shorterEnd)
    useFireStore.getState().seekBy(10)
    expect(useFireStore.getState().clockSeconds).toBe(shorterEnd)
    useFireStore.getState().setClockScope('batch')
  })
})
