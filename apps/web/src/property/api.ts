import { DEFAULT_OCCUPANT_COUNT, DEFAULT_OCCUPANT_LAYOUT, type ScenarioOptions } from '../fire/api'
import type { PropertyBrief } from './types'

/** The loaded scan's CoStar brief, or `null` when the server has none. */
export async function fetchProperty(): Promise<PropertyBrief | null> {
  const response = await fetch('/api/property').catch(() => null)
  if (!response?.ok) return null
  return (await response.json().catch(() => null)) as PropertyBrief | null
}

/** Scenario settings the CoStar record implies, all overridable. */
export function scenarioFromProperty({
  profile,
  gemini_placement,
}: PropertyBrief): Partial<ScenarioOptions> {
  return {
    occupantCount: profile.occupant_count.value,
    // Gemini places people using the building context (crowded mode skips
    // it), but only when the server has Gemini configured.
    ...(gemini_placement && { layout: 'spread' as const }),
    alarmConfig: profile.alarm.value,
    injuredFraction: profile.injured_fraction.value,
    floorMaterial: profile.floor_material.value,
  }
}

/**
 * Switches the CoStar assumptions off. The count and layout go back to the
 * defaults only if they are still CoStar's; choices made by hand are kept.
 */
export function scenarioWithoutProperty(
  brief: PropertyBrief,
  current: ScenarioOptions,
): Partial<ScenarioOptions> {
  const costar = scenarioFromProperty(brief)
  return {
    alarmConfig: undefined,
    injuredFraction: undefined,
    floorMaterial: undefined,
    ...(current.occupantCount === costar.occupantCount && {
      occupantCount: DEFAULT_OCCUPANT_COUNT,
    }),
    ...(costar.layout &&
      current.layout === costar.layout && { layout: DEFAULT_OCCUPANT_LAYOUT }),
  }
}

/** True until the user changes the count or layout by hand. */
export function isUntouchedScenario(current: ScenarioOptions): boolean {
  return (
    current.occupantCount === DEFAULT_OCCUPANT_COUNT && current.layout === DEFAULT_OCCUPANT_LAYOUT
  )
}
