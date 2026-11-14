// Fire playback state: the latest simulated timeline and the clock that
// walks through it. The timeline comes from the Rust server in one piece
// (see api.ts); nothing here feeds back into the simulation.

import { create } from 'zustand'
import { useMemo } from 'react'
import { describeFireError, runFireSimulation } from './api'
import { agentsAtTime, frameAtTime } from './frames'
import type { AgentFrame, SimulateFireResponse, SimulationFrame } from './types'

const NO_AGENTS: AgentFrame[] = []

/** Simulated seconds per real second. */
export const PLAYBACK_RATE = 1
/** Longest clock step per tick, so a backgrounded tab doesn't skip the fire. */
const MAX_TICK_SECONDS = 0.1

export type FireStatus = 'idle' | 'loading' | 'playing' | 'paused' | 'error'

interface FireState {
  status: FireStatus
  /** The object that was ignited, while loading or once a run exists. */
  objectId: string | null
  run: SimulateFireResponse | null
  /** Simulated time being shown. */
  clockSeconds: number
  error: string | null

  /** Simulates a fire starting at `objectId`, then plays it from the start. */
  start: (objectId: string) => Promise<void>
  play: () => void
  pause: () => void
  restart: () => void
  clear: () => void
  /** Advances the clock by `realSeconds`; pauses at the end. */
  tick: (realSeconds: number) => void
}

/** The time of the last frame, where playback stops. */
export function endSeconds(run: SimulateFireResponse | null): number {
  return run?.results.frames.at(-1)?.time_seconds ?? 0
}

/** Identifies the latest `start`, so an older response can't replace it. */
let latestRequest = 0

export const useFireStore = create<FireState>()((set, get) => ({
  status: 'idle',
  objectId: null,
  run: null,
  clockSeconds: 0,
  error: null,

  start: async (objectId) => {
    const request = ++latestRequest
    set({ status: 'loading', objectId, run: null, clockSeconds: 0, error: null })
    try {
      const run = await runFireSimulation(objectId)
      if (request === latestRequest) set({ status: 'playing', run })
    } catch (error) {
      if (request === latestRequest) set({ status: 'error', error: describeFireError(error) })
    }
  },
  play: () => {
    const { run, clockSeconds } = get()
    if (!run) return
    // Playing from the end starts over.
    set({ status: 'playing', clockSeconds: clockSeconds >= endSeconds(run) ? 0 : clockSeconds })
  },
  pause: () => set((state) => (state.status === 'playing' ? { status: 'paused' } : {})),
  restart: () => set((state) => (state.run ? { status: 'playing', clockSeconds: 0 } : {})),
  clear: () => {
    latestRequest++
    set({ status: 'idle', objectId: null, run: null, clockSeconds: 0, error: null })
  },
  tick: (realSeconds) => {
    const { status, run, clockSeconds } = get()
    if (status !== 'playing' || !run) return
    const end = endSeconds(run)
    const next = clockSeconds + Math.min(realSeconds, MAX_TICK_SECONDS) * PLAYBACK_RATE
    set(next >= end ? { clockSeconds: end, status: 'paused' } : { clockSeconds: next })
  },
}))

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

/** Smooth positions between the one-second authoritative simulation frames. */
export function useSimulationAgents(): AgentFrame[] {
  const frames = useFireStore((state) => state.run?.results.frames)
  const clockSeconds = useFireStore((state) => state.clockSeconds)
  return useMemo(
    () => (frames ? agentsAtTime(frames, clockSeconds) : NO_AGENTS),
    [frames, clockSeconds],
  )
}
