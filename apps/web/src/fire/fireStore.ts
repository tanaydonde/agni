// Fire playback state: the latest simulated timeline and the clock that
// walks through it. The timeline comes from the Rust server in one piece
// (see api.ts); nothing here feeds back into the simulation.

import { create } from 'zustand'
import { useMemo } from 'react'
import {
  DEFAULT_OCCUPANT_COUNT,
  DEFAULT_OCCUPANT_LAYOUT,
  type ScenarioOptions,
  describeFireError,
  runFireSimulation,
  runScenarioBatch,
} from './api'
import { forgetBatch, loadBatch, saveBatch } from './batchCache'
import { agentsAtTime, agentTimeline, frameAtTime } from './frames'
import type { AgentFrame, SimulateFireResponse, SimulateNResponse, SimulationFrame } from './types'

const NO_AGENTS: AgentFrame[] = []

/** Simulated seconds per real second, to start with. Real time reads as
 * sluggish across a whole building, so playback starts at double speed. */
export const DEFAULT_PLAYBACK_RATE = 2
export const PLAYBACK_RATES = [1, 2, 4, 8, 16] as const
/** Longest clock step per tick, so a backgrounded tab doesn't skip the fire. */
const MAX_TICK_SECONDS = 0.1

export type ClockScope = 'batch' | 'run'
export type FireStatus = 'idle' | 'loading' | 'playing' | 'paused' | 'error'

interface FireState {
  status: FireStatus
  /** The object that was ignited, while loading or once a run exists. */
  objectId: string | null
  run: SimulateFireResponse | null
  /** Complete N-run response while reviewing its selected representatives. */
  batch: SimulateNResponse | null
  /** Position within `batch.fire_ensemble.representatives`. */
  representativePosition: number
  /** Simulated time being shown. */
  clockSeconds: number
  error: string | null
  /** Simulated seconds per real second. */
  playbackRate: number
  /**
   * Where the clock stops while a batch is loaded: at the longest
   * representative (`batch`, for Scenario Review) or at the shown run (`run`,
   * for the demo pages, which only play the first representative).
   */
  clockScope: ClockScope
  /**
   * Where playback begins: restarting, playing from the end, and seeking stop
   * here. 0 normally; a view can move it, e.g. to when the crew starts moving.
   */
  clockStartSeconds: number
  /** Who is in the building for the next run. */
  scenario: ScenarioOptions

  /** Simulates a fire starting at `objectId`, then plays it from the start. */
  start: (objectId: string) => Promise<void>
  startBatch: () => Promise<void>
  /** Brings back the last batch from before a page load, if nothing is loaded yet. */
  restoreBatch: () => Promise<void>
  showRepresentative: (position: number) => void
  previousRepresentative: () => void
  nextRepresentative: () => void
  play: () => void
  pause: () => void
  restart: () => void
  clear: () => void
  /** Advances the clock by `realSeconds`; pauses at the end. */
  tick: (realSeconds: number) => void
  seekBy: (seconds: number) => void
  setClock: (seconds: number) => void
  setPlaybackRate: (rate: number) => void
  setClockScope: (scope: ClockScope) => void
  /** Moves where playback begins, bringing the clock up to it if it's earlier. */
  setClockStart: (seconds: number) => void
  setScenario: (scenario: Partial<ScenarioOptions>) => void
}

/** The time of the last frame, where playback stops. */
export function endSeconds(run: SimulateFireResponse | null): number {
  return run?.results.frames.at(-1)?.time_seconds ?? 0
}

/** Shared end of a review batch; shorter incidents remain on their last frame. */
export function batchEndSeconds(batch: SimulateNResponse | null): number {
  return Math.max(0, ...representativeRuns(batch).map((run) => endSeconds(run)))
}

/** Where playback stops (see `clockScope`). */
export function playbackEnd({
  run,
  batch,
  clockScope,
}: Pick<FireState, 'run' | 'batch' | 'clockScope'>): number {
  return batch && clockScope === 'batch' ? batchEndSeconds(batch) : endSeconds(run)
}

/** Identifies the latest `start`, so an older response can't replace it. */
let latestRequest = 0

export const useFireStore = create<FireState>()((set, get) => ({
  status: 'idle',
  objectId: null,
  run: null,
  batch: null,
  representativePosition: 0,
  clockSeconds: 0,
  error: null,
  playbackRate: DEFAULT_PLAYBACK_RATE,
  clockScope: 'batch',
  clockStartSeconds: 0,
  scenario: { occupantCount: DEFAULT_OCCUPANT_COUNT, layout: DEFAULT_OCCUPANT_LAYOUT },

  start: async (objectId) => {
    const request = ++latestRequest
    set({
      status: 'loading',
      objectId,
      run: null,
      batch: null,
      representativePosition: 0,
      clockSeconds: 0,
      error: null,
    })
    try {
      const run = await runFireSimulation(objectId, get().scenario)
      if (request === latestRequest) set({ status: 'playing', run })
    } catch (error) {
      if (request === latestRequest) set({ status: 'error', error: describeFireError(error) })
    }
  },
  startBatch: async () => {
    const request = ++latestRequest
    set({
      status: 'loading',
      objectId: null,
      run: null,
      batch: null,
      representativePosition: 0,
      clockSeconds: 0,
      error: null,
    })
    try {
      const batch = await runScenarioBatch(get().scenario)
      if (request !== latestRequest) return
      const first = representativeRun(batch, 0)
      if (!first) throw new Error('No scenario completed successfully')
      set({ status: 'playing', batch, run: first, representativePosition: 0 })
      void saveBatch(batch)
    } catch (error) {
      if (request === latestRequest) set({ status: 'error', error: describeFireError(error) })
    }
  },
  restoreBatch: async () => {
    const request = latestRequest
    const batch = await loadBatch()
    // Anything started or loaded meanwhile wins over the saved batch.
    if (!batch || request !== latestRequest || get().status !== 'idle') return
    const first = representativeRun(batch, 0)
    if (!first) return
    set({ status: 'paused', batch, run: first, representativePosition: 0, clockSeconds: 0 })
  },
  showRepresentative: (position) => {
    const { batch, clockSeconds, status } = get()
    if (!batch) return
    const count = representativeCount(batch)
    if (count === 0) return
    const normalized = ((position % count) + count) % count
    const run = representativeRun(batch, normalized)
    if (!run) return
    set({
      run,
      representativePosition: normalized,
      clockSeconds: Math.min(clockSeconds, endSeconds(run)),
      status: status === 'playing' ? 'playing' : 'paused',
    })
  },
  previousRepresentative: () => get().showRepresentative(get().representativePosition - 1),
  nextRepresentative: () => get().showRepresentative(get().representativePosition + 1),
  play: () => {
    const { run, clockSeconds, clockStartSeconds } = get()
    if (!run) return
    // Playing from the end starts over.
    const end = playbackEnd(get())
    set({ status: 'playing', clockSeconds: clockSeconds >= end ? clockStartSeconds : clockSeconds })
  },
  pause: () => set((state) => (state.status === 'playing' ? { status: 'paused' } : {})),
  restart: () =>
    set((state) => (state.run ? { status: 'playing', clockSeconds: state.clockStartSeconds } : {})),
  clear: () => {
    latestRequest++
    if (get().batch) void forgetBatch()
    set({
      status: 'idle',
      objectId: null,
      run: null,
      batch: null,
      representativePosition: 0,
      clockSeconds: 0,
      clockStartSeconds: 0,
      error: null,
    })
  },
  seekBy: (seconds) => {
    const { run, clockSeconds, clockStartSeconds } = get()
    if (!run) return
    const end = playbackEnd(get())
    set({ clockSeconds: Math.max(clockStartSeconds, Math.min(end, clockSeconds + seconds)) })
  },
  setClock: (seconds) => {
    const { run, clockStartSeconds } = get()
    if (!run) return
    const end = playbackEnd(get())
    set({ clockSeconds: Math.max(clockStartSeconds, Math.min(end, seconds)) })
  },
  tick: (realSeconds) => {
    const { status, run, clockSeconds, playbackRate } = get()
    if (status !== 'playing' || !run) return
    const end = playbackEnd(get())
    const next = clockSeconds + Math.min(realSeconds, MAX_TICK_SECONDS) * playbackRate
    set(next >= end ? { clockSeconds: end, status: 'paused' } : { clockSeconds: next })
  },
  setPlaybackRate: (playbackRate) => set({ playbackRate }),
  setClockScope: (clockScope) =>
    set((state) => ({
      clockScope,
      clockSeconds: Math.min(state.clockSeconds, playbackEnd({ ...state, clockScope })),
    })),
  setClockStart: (clockStartSeconds) =>
    set((state) => ({ clockStartSeconds, clockSeconds: Math.max(state.clockSeconds, clockStartSeconds) })),
  setScenario: (scenario) => set((state) => ({ scenario: { ...state.scenario, ...scenario } })),
}))

export function representativeCount(batch: SimulateNResponse | null): number {
  return batch?.fire_ensemble?.representatives.length ?? 0
}

export function representativeRun(
  batch: SimulateNResponse,
  position: number,
): SimulateFireResponse | null {
  const index = batch.fire_ensemble?.representatives[position]?.simulation_index
  if (index === undefined) return null
  const entry = batch.runs.find((run) => run.index === index)
  return entry?.status === 'completed' ? entry.simulation : null
}

export function representativeRuns(batch: SimulateNResponse | null): SimulateFireResponse[] {
  if (!batch?.fire_ensemble) return []
  return batch.fire_ensemble.representatives.flatMap((_, position) => {
    const run = representativeRun(batch, position)
    return run ? [run] : []
  })
}

/**
 * The frame at the playback clock, or undefined with no run. Keeps its
 * identity between ticks until the next frame is reached, so subscribers
 * re-render once per simulation frame, not per animation frame.
 */
export function useFireFrame(): SimulationFrame | undefined {
  return useFireStore((state) =>
    state.run ? frameAtTime(state.run.results.frames, state.clockSeconds) : undefined,
  )
}

/** Smooth positions between the authoritative simulation frames. */
export function useSimulationAgents(): AgentFrame[] {
  const run = useFireStore((state) => state.run)
  const frames = useMemo(() => (run ? agentTimeline(run) : undefined), [run])
  const clockSeconds = useFireStore((state) => state.clockSeconds)
  return useMemo(
    () => (frames ? agentsAtTime(frames, clockSeconds) : NO_AGENTS),
    [frames, clockSeconds],
  )
}
