import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchProperty, isUntouchedScenario, scenarioFromProperty, scenarioWithoutProperty } from './api'
import type { PropertyBrief } from './types'

const derived = <T,>(value: T) => ({ value, source_field: 'property.bedsMax', rationale: 'test' })

const brief: PropertyBrief = {
  record: {
    source: { provider: 'costar', dataset: 'residential-listings', record_id: 1, as_of: null },
    kind: 'residential',
    scan: { model_id: 'm', description: null, partial: false, captured: null },
    name: null,
    address: '1 Test St',
    city: null,
    state: null,
    latitude: null,
    longitude: null,
    tenants: [],
    conflicts: [],
    facts: [],
  },
  profile: {
    occupant_context: derived('A household asleep'),
    occupant_count: derived(6),
    alarm: derived({ notice_hrr_kw: 100, same_room_seconds: 30, seconds_per_metre: 1 }),
    injured_fraction: derived(0.15),
    floor_material: derived('hardwood' as const),
    verification: [],
  },
  gemini_placement: true,
}

afterEach(() => vi.unstubAllGlobals())

describe('property', () => {
  it('turns the CoStar profile into scenario settings that use Gemini placement', () => {
    expect(scenarioFromProperty(brief)).toEqual({
      occupantCount: 6,
      layout: 'spread',
      alarmConfig: { notice_hrr_kw: 100, same_room_seconds: 30, seconds_per_metre: 1 },
      injuredFraction: 0.15,
      floorMaterial: 'hardwood',
    })
  })

  it('keeps the current layout when the server cannot run Gemini placement', () => {
    expect(scenarioFromProperty({ ...brief, gemini_placement: false })).not.toHaveProperty('layout')
  })

  it('switching CoStar off restores defaults only for values CoStar still owns', () => {
    const costar = { occupantCount: 6, layout: 'spread' as const }
    expect(scenarioWithoutProperty(brief, costar)).toEqual({
      alarmConfig: undefined,
      injuredFraction: undefined,
      floorMaterial: undefined,
      occupantCount: 50,
      layout: 'crowded',
    })
    const byHand = { occupantCount: 20, layout: 'crowded' as const }
    expect(scenarioWithoutProperty(brief, byHand)).toEqual({
      alarmConfig: undefined,
      injuredFraction: undefined,
      floorMaterial: undefined,
    })
  })

  it('prefills only a scenario the user has not changed', () => {
    expect(isUntouchedScenario({ occupantCount: 50, layout: 'crowded' })).toBe(true)
    expect(isUntouchedScenario({ occupantCount: 20, layout: 'crowded' })).toBe(false)
  })

  it('is null when the server has no CoStar record or is down', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve({ ok: false, status: 404 })))
    expect(await fetchProperty()).toBeNull()
    vi.stubGlobal('fetch', vi.fn(() => Promise.reject(new Error('down'))))
    expect(await fetchProperty()).toBeNull()
  })
})
