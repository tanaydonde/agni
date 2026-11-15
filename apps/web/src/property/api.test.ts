import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchProperty, scenarioFromProperty } from './api'
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

  it('is null when the server has no CoStar record or is down', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve({ ok: false, status: 404 })))
    expect(await fetchProperty()).toBeNull()
    vi.stubGlobal('fetch', vi.fn(() => Promise.reject(new Error('down'))))
    expect(await fetchProperty()).toBeNull()
  })
})
