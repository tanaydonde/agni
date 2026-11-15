/** `GET /api/property`: the scan's CoStar record and derived assumptions. */

export interface Fact {
  label: string
  value: string
  /** CoStar field path, e.g. `resoValues.structure.yearBuilt`. */
  source_field: string
}

export interface Derived<T> {
  value: T
  source_field: string
  rationale: string
}

export interface AlarmTimings {
  notice_hrr_kw: number
  same_room_seconds: number
  seconds_per_metre: number
}

export type FloorMaterial = 'carpet' | 'hardwood'

export interface VerificationItem {
  item: string
  reason: string
  source_field: string
}

export interface PropertyRecord {
  source: { provider: string; dataset: string; record_id: number; as_of: string | null }
  kind: 'residential' | 'commercial'
  scan: { model_id: string; description: string | null; partial: boolean; captured: string | null }
  name: string | null
  address: string | null
  city: string | null
  state: string | null
  latitude: number | null
  longitude: number | null
  tenants: { name: string | null; industry: string | null; employees: number | null }[]
  conflicts: { label: string; values: Fact[] }[]
  facts: Fact[]
}

export interface FireProfile {
  occupant_context: Derived<string>
  occupant_count: Derived<number>
  alarm: Derived<AlarmTimings>
  injured_fraction: Derived<number>
  floor_material: Derived<FloorMaterial>
  verification: VerificationItem[]
}

export interface PropertyBrief {
  record: PropertyRecord
  profile: FireProfile
  /** Whether the server can run Gemini's context-aware `spread` placement. */
  gemini_placement: boolean
}
