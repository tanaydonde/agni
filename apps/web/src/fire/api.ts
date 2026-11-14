import { WorldApiError, WorldServerUnavailableError } from '../scene/api'
import { SIMULATION_RESULTS_SCHEMA_VERSION, type SimulateFireResponse } from './types'

/**
 * Ignites `objectId` and runs the surface-fire simulation on the world's
 * current scene. Resolves with the whole timeline.
 */
export async function runFireSimulation(objectId: string): Promise<SimulateFireResponse> {
  const response = await fetch('/api/fire/simulate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ object_id: objectId }),
  }).catch(() => null)
  // The Vite proxy answers 5xx when the Rust server is down.
  if (!response || response.status >= 500) throw new WorldServerUnavailableError()
  const body = await response.json().catch(() => null)
  if (!response.ok) throw new WorldApiError(response.status, body ?? { code: 'unknown' })

  const run = body as SimulateFireResponse
  if (run.results.schema_version !== SIMULATION_RESULTS_SCHEMA_VERSION) {
    throw new Error(
      `Fire results use schema v${run.results.schema_version}; this app reads v${SIMULATION_RESULTS_SCHEMA_VERSION}`,
    )
  }
  return run
}

/** A short sentence explaining why a fire run failed. */
export function describeFireError(error: unknown): string {
  if (error instanceof WorldServerUnavailableError) return "Couldn't reach the world server"
  if (error instanceof WorldApiError) {
    switch (error.body.code) {
      case 'object_not_found':
        return 'That item no longer exists'
      case 'object_outside_rooms':
        return "That item isn't inside a room, so it can't burn"
      case 'object_not_combustible':
        return "That item doesn't burn"
      case 'simulation_failed':
        return `The simulation failed: ${String(error.body.message ?? 'unknown error')}`
      default:
        return `The simulation failed (${error.body.code})`
    }
  }
  return error instanceof Error ? error.message : 'The simulation failed'
}
