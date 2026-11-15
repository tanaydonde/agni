import { WorldApiError, WorldServerUnavailableError } from '../scene/api'
import {
  type CrewSpawn,
  type GeneratedFireScenariosResponse,
  type OccupantLayout,
  SIMULATION_RESULTS_SCHEMA_VERSION,
  type SimulateFireResponse,
  type SimulateNResponse,
  type SpawnOccupantsResponse,
} from './types'

export const DEFAULT_OCCUPANT_COUNT = 10
export const MIN_OCCUPANT_COUNT = 1
export const MAX_OCCUPANT_COUNT = 30
/** Crowded by default, so a run shows people queuing at doors. */
export const DEFAULT_OCCUPANT_LAYOUT: OccupantLayout = 'crowded'
export const DEFAULT_BATCH_SIMULATION_COUNT = 10
export const DEFAULT_TOP_K = 4

export interface ScenarioOptions {
  occupantCount: number
  layout: OccupantLayout
}

/** Rounds `value` to a whole number of occupants within the allowed range. */
export function clampOccupantCount(value: number): number {
  return Math.min(MAX_OCCUPANT_COUNT, Math.max(MIN_OCCUPANT_COUNT, Math.round(value)))
}

/** One firefighter who arrives 10 s after ignition at the first exterior exit. */
export const DEFAULT_CREW: CrewSpawn = { id: 'engine-1', member_count: 1, arrival_time_seconds: 10 }

/**
 * Generates a validated occupant layout, ignites `objectId`, and runs the
 * hybrid fire simulation with occupants evacuating and the default crew
 * looking for the fire, fighting it, and carrying out anyone injured.
 */
export async function runFireSimulation(
  objectId: string,
  { occupantCount, layout }: ScenarioOptions = {
    occupantCount: DEFAULT_OCCUPANT_COUNT,
    layout: DEFAULT_OCCUPANT_LAYOUT,
  },
): Promise<SimulateFireResponse> {
  const occupants = await spawnOccupants(occupantCount, layout)
  const run = await requestFireSimulation(objectId, occupants.occupants)
  if (occupants.revision !== run.revision) {
    throw new Error('The building changed while the scenario was being generated. Run it again.')
  }
  return run
}

/**
 * Runs a small ensemble with the ordinary surface simulator and asks the
 * server to return only four representative timelines for browser playback.
 */
export async function runScenarioBatch(
  { occupantCount }: ScenarioOptions,
  simulationCount = DEFAULT_BATCH_SIMULATION_COUNT,
  topK = DEFAULT_TOP_K,
): Promise<SimulateNResponse> {
  const generated = await requestJson<GeneratedFireScenariosResponse>(
    '/api/agents/generate_fire_scenarios',
    { count: simulationCount, occupant_count: occupantCount },
  )
  const simulations = generated.scenarios.map((scenario) => ({
    object_id: scenario.object_id,
    occupants: scenario.occupants,
    crews: [DEFAULT_CREW],
    seed: scenario.seed,
    duration_seconds: 360,
  }))
  const batch = await requestJson<SimulateNResponse>('/api/fire/simulate_n', {
    top_k: topK,
    representatives_only: true,
    response_interval_seconds: 2,
    simulations,
  })
  if (generated.revision !== batch.revision) {
    throw new Error(
      'The building changed while the scenarios were being generated. Run them again.',
    )
  }
  for (const run of batch.runs) {
    if (
      run.status === 'completed' &&
      run.simulation.results.schema_version !== SIMULATION_RESULTS_SCHEMA_VERSION
    ) {
      throw new Error(
        `Fire results use schema v${run.simulation.results.schema_version}; this app reads v${SIMULATION_RESULTS_SCHEMA_VERSION}`,
      )
    }
  }
  return batch
}

async function spawnOccupants(
  count: number,
  layout: OccupantLayout,
): Promise<SpawnOccupantsResponse> {
  return requestJson<SpawnOccupantsResponse>('/api/agents/spawn_occupants', { count, layout })
}

async function requestFireSimulation(
  objectId: string,
  occupants: SpawnOccupantsResponse['occupants'],
): Promise<SimulateFireResponse> {
  const run = await requestJson<SimulateFireResponse>('/api/fire/simulate/hybrid', {
    object_id: objectId,
    occupants,
    crews: [DEFAULT_CREW],
  })
  if (run.results.schema_version !== SIMULATION_RESULTS_SCHEMA_VERSION) {
    throw new Error(
      `Fire results use schema v${run.results.schema_version}; this app reads v${SIMULATION_RESULTS_SCHEMA_VERSION}`,
    )
  }
  return run
}

async function requestJson<T>(url: string, payload: unknown): Promise<T> {
  const response = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  }).catch(() => null)
  if (!response) throw new WorldServerUnavailableError()
  const body = await response.json().catch(() => null)
  if (!response.ok) {
    // Preserve structured backend errors such as cfast_not_configured. Vite's
    // proxy failure response is not JSON, so only that case means unreachable.
    if (body && typeof body === 'object' && typeof body.code === 'string') {
      throw new WorldApiError(response.status, body)
    }
    if (response.status >= 500) throw new WorldServerUnavailableError()
    throw new WorldApiError(response.status, { code: 'unknown' })
  }

  return body as T
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
      case 'firefighting_failed':
        return `The crew couldn't be placed: ${String(error.body.message ?? 'unknown error')}`
      case 'simulation_failed':
        return `The simulation failed: ${String(error.body.message ?? 'unknown error')}`
      case 'cfast_not_configured':
        return 'CFAST is not configured on the world server'
      case 'cfast_failed':
      case 'cfast_preparation_failed':
      case 'cfast_adapter_failed':
      case 'cfast_merge_failed':
        return `CFAST failed: ${String(error.body.message ?? 'unknown error')}`
      case 'gemini_not_configured':
        return 'Gemini is not configured on the world server'
      case 'gemini_rate_limited':
        return 'Gemini is rate limited; try again shortly'
      case 'gemini_failed':
      case 'occupant_generation_failed':
        return `Generating occupants failed: ${String(error.body.message ?? 'unknown error')}`
      case 'scenario_generation_failed':
        return `Generating scenarios failed: ${String(error.body.message ?? 'unknown error')}`
      case 'evacuation_failed':
        return `Evacuation failed: ${String(error.body.message ?? 'unknown error')}`
      default:
        return `The simulation failed (${error.body.code})`
    }
  }
  return error instanceof Error ? error.message : 'The simulation failed'
}
