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

export type CombustionState = 'unignited' | 'heating' | 'burning' | 'burned_out'

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

export type FirefighterPhase =
  | 'staging'
  | 'advancing'
  | 'searching'
  | 'suppressing'
  | 'withdrawing'
  | 'exited'
  | 'incapacitated'

export type AgentRole =
  | { type: 'occupant'; phase: OccupantPhase }
  | { type: 'firefighter'; crew_id: string; phase: FirefighterPhase }

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
}
