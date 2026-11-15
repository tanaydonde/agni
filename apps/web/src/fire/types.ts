// TypeScript mirror of the fire simulation results (crates/simulation-core
// `SimulationResults`, `SurfaceFireNodeFrame`, and `AgentFrame`); see
// docs/FIRE_FRONTEND.md.

import type { Dimensions3, Vec3 } from '../scene/types'

export const SIMULATION_RESULTS_SCHEMA_VERSION = 1

export interface SimulationResults {
  schema_version: number
  scene_id: string
  scenario_id: string
  engine: { name: string; version: string }
  frames: SimulationFrame[]
}

export interface SimulationFrame {
  time_seconds: number
  surface_fire: SurfaceFireNode[]
  rooms: RoomConditions[]
  fires: FireConditions[]
  openings: OpeningConditions[]
  /** Occupants and firefighters. Empty for fire-only runs. */
  agents: AgentFrame[]
}

/** `extinguished`: put out by suppression with fuel left; it can reignite. */
export type CombustionState = 'unignited' | 'heating' | 'burning' | 'burned_out' | 'extinguished'

export type SurfaceFireSource =
  | { type: 'floor_cell'; floor_id: string; coordinate: { column: number; row: number } }
  | { type: 'furniture'; object_id: string }

export interface FurnitureFireCell {
  column: number
  row: number
  layer: number
  columns: number
  rows: number
  layers: number
}

export interface SurfaceFireNode {
  /** Stable across every frame; use as the React key. */
  id: string
  source: SurfaceFireSource
  room_id: string
  /** Cell center in canonical Z-up metres. */
  position_m: Vec3
  /** For furniture cells, in the object's local axes. */
  dimensions_m: Dimensions3
  /** Present on furniture nodes and absent on floor nodes. */
  furniture_cell?: FurnitureFireCell
  material_id: string
  state: CombustionState
  ignition_time_seconds: number | null
  surface_temperature_c: number
  heat_release_rate_kw: number
  /** Between 0 and 1. */
  fuel_remaining_fraction: number
}

export type OccupantPhase =
  | 'unaware'
  | 'preparing'
  | 'moving_to_exit'
  | 'rerouting'
  | 'exited'
  | 'incapacitated'
  /** Hurt and unable to move; waiting for a firefighter. */
  | 'injured'
  /** Being carried out by a firefighter. */
  | 'carried'

export type FirefighterPhase =
  | 'staging'
  | 'advancing'
  | 'suppressing'
  /** Going to pick up an injured occupant. */
  | 'rescuing'
  /** Carrying an injured occupant out. */
  | 'carrying'
  | 'withdrawing'
  | 'exited'
  | 'incapacitated'

/**
 * How an agent moved through the crowd in the last step. `queued`: held up
 * by people in the way. `squeezing`: pressing past someone in a doorway or
 * hallway too narrow to pass side by side.
 */
export type Movement = 'stopped' | 'walking' | 'queued' | 'squeezing'

/** `spread`: Gemini places people plausibly. `crowded`: packed into the largest rooms. */
export type OccupantLayout = 'spread' | 'crowded'

export type AgentRole =
  | {
      type: 'occupant'
      phase: OccupantPhase
      /** Started the run injured; once exited, they were carried out. */
      injured?: boolean
    }
  | {
      type: 'firefighter'
      crew_id: string
      phase: FirefighterPhase
      /** Where the hose is aimed, while spraying. Canonical Z-up metres. */
      nozzle_target_m?: Vec3
    }

/** Fractional effective doses; reaching 1 in either means incapacitated. */
export interface Exposure {
  heat: number
  toxic_gas: number
}

export interface AgentFrame {
  /** Stable across every frame; use as the React key. Firefighters are `{crew_id}/{n}`. */
  id: string
  floor_id: string
  /** Canonical Z-up metres; `z` is the floor surface. */
  position_m: Vec3
  velocity_m_per_s: Vec3
  radius_m: number
  exposure: Exposure
  /** Missing from timelines simulated before crowd movement. */
  movement?: Movement
  role: AgentRole
}

export interface RoomConditions {
  room_id: string
  smoke_layer_height_m: number
  gauge_pressure_pa: number
  absolute_pressure_pa: number
  upper_layer: GasLayerConditions
  lower_layer: GasLayerConditions
}

export interface GasLayerConditions {
  temperature_c: number
  oxygen_percent: number
  carbon_monoxide_percent: number
  hydrogen_cyanide_percent: number
  optical_density_per_m: number
}

export interface FireConditions {
  fire_id: string
  room_id: string
  ignited: boolean
  expected_hrr_kw: number
  actual_hrr_kw: number
  convective_hrr_kw: number
  flame_height_m: number
  pyrolysis_rate_kg_per_s: number
}

export interface OpeningConditions {
  opening_id: string
  net_flow_kg_per_s: number
  opening_fraction: number
}

/** `POST /api/fire/simulate/hybrid` (crates/server/src/fire.rs). */
export interface SimulateFireResponse {
  /** World revision the simulated scene was taken at. */
  revision: number
  /** Objects left out because they don't stand inside a room. */
  excluded_object_ids: string[]
  results: SimulationResults
  /**
   * Everyone at every simulation step, sent when `results.frames` are thinned
   * for playback. Interpolating between thinned frames cuts through walls.
   */
  agent_frames?: AgentTimelineFrame[]
}

/** Just the agents at one simulation step. */
export interface AgentTimelineFrame {
  time_seconds: number
  agents: AgentFrame[]
}

export interface FireRepresentative {
  /** Index in the original `runs` array. */
  simulation_index: number
  support_count: number
  support_fraction: number
}

/** One floor-plan cell of the batch's fire, summed over time and height. */
export interface FireHeatmapCell {
  /** The cell spans `column * cell_size_m` to `(column + 1) * cell_size_m` in x; rows likewise in y. */
  column: number
  row: number
  /** Seconds the cell spent burning, averaged over the completed simulations. */
  expected_burning_seconds: number
  /** Share of the completed simulations in which the cell burned at all. */
  burned_fraction: number
}

export interface FireEnsembleAnalysis {
  requested_top_k: number
  completed_simulation_count: number
  config: { cell_size_m: number }
  heatmap: FireHeatmapCell[]
  representatives: FireRepresentative[]
}

export type SimulateNRun =
  | { status: 'completed'; index: number; simulation: SimulateFireResponse }
  | { status: 'failed'; index: number; http_status: number; error: { code?: string; message?: string } }

/** Compact representative response from `POST /api/fire/simulate_n`. */
export interface SimulateNResponse {
  revision: number
  runs: SimulateNRun[]
  fire_ensemble?: FireEnsembleAnalysis
}

/**
 * A firefighter crew sent with a fire run (crates/simulation-core `CrewSpawn`).
 * The crew moves as one unit: `{id}/1` leads, the rest follow.
 */
export interface CrewSpawn {
  id: string
  /** Exterior door or passage to enter by. Omitted: the first exterior exit. */
  entry_opening_id?: string
  /** Rooms to walk into first, in order; the rest are entered nearest first. */
  route?: string[]
  /** Seconds after ignition when the crew reaches the entry. */
  arrival_time_seconds?: number
  member_count: number
  walking_speed_m_per_s?: number
}

/** `POST /api/agents/spawn_occupants` (crates/server/src/occupant_agent.rs). */
export interface SpawnOccupantsResponse {
  revision: number
  attempts: number
  occupants: Array<{
    id: string
    floor_id: string
    position_m: Vec3
    walking_speed_m_per_s: number
    pre_movement_delay_seconds: number
  }>
  agents: AgentFrame[]
}

export interface GeneratedFireScenariosResponse {
  revision: number
  scenarios: Array<{
    object_id: string
    occupants: SpawnOccupantsResponse['occupants']
    seed: number
  }>
}
