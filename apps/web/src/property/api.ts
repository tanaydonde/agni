import { DEFAULT_OCCUPANT_COUNT, DEFAULT_OCCUPANT_LAYOUT, type ScenarioOptions } from '../fire/api'
import type { PropertyBrief } from './types'

/** The loaded scan's CoStar brief, or `null` when the server has none. */
export async function fetchProperty(): Promise<PropertyBrief | null> {
  const response = await fetch('/api/property').catch(() => null)
  if (!response?.ok) return null
  return (await response.json().catch(() => null)) as PropertyBrief | null
}

/** Scenario settings the CoStar record implies, all overridable. */
export function scenarioFromProperty({ profile }: PropertyBrief): Partial<ScenarioOptions> {
  return {
    occupantCount: profile.occupant_count.value,
    // Gemini places people using the building context; crowded mode skips it.
    layout: 'spread',
    alarmConfig: profile.alarm.value,
    injuredFraction: profile.injured_fraction.value,
    floorMaterial: profile.floor_material.value,
  }
}

/** Server defaults: what a run uses with the CoStar assumptions switched off. */
export const SCENARIO_WITHOUT_PROPERTY: Partial<ScenarioOptions> = {
  occupantCount: DEFAULT_OCCUPANT_COUNT,
  layout: DEFAULT_OCCUPANT_LAYOUT,
  alarmConfig: undefined,
  injuredFraction: undefined,
  floorMaterial: undefined,
}
