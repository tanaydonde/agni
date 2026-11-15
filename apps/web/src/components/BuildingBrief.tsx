import { useState } from 'react'
import type { Derived, PropertyBrief } from '../property/types'

interface BuildingBriefProps {
  brief: PropertyBrief
  /** Whether runs use the derived assumptions below. */
  applied: boolean
  onAppliedChange: (applied: boolean) => void
}

/**
 * The scan's CoStar facts, the scenario assumptions derived from them, and
 * what a crew still has to verify. Every line names its CoStar field.
 */
export function BuildingBrief({ brief, applied, onAppliedChange }: BuildingBriefProps) {
  const [open, setOpen] = useState(true)
  const { record, profile } = brief
  const title = record.name ?? record.address ?? 'Property'
  const place = [record.city, record.state].filter(Boolean).join(', ')

  return (
    <section className="flex max-h-[45vh] flex-none flex-col overflow-hidden rounded-lg border border-hairline bg-white text-xs">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="flex items-start justify-between gap-2 border-b border-hairline px-3 py-2 text-left"
        aria-expanded={open}
      >
        <span className="flex flex-col">
          <span className="text-sm font-medium text-ink">{title}</span>
          <span className="text-muted">
            {place} · CoStar {record.source.dataset} #{record.source.record_id}
            {record.source.as_of && ` · ${record.source.as_of}`}
          </span>
        </span>
        <span className="text-muted">{open ? '▾' : '▸'}</span>
      </button>

      {open && (
        <div className="flex flex-col gap-3 overflow-y-auto px-3 py-2">
          <p className="text-ink-secondary">
            Scan: <span className="text-ink">{record.scan.description ?? record.scan.model_id}</span>
            {record.scan.partial && ' (one space, not the whole property)'}
          </p>

          <label className="flex items-center gap-1.5 text-ink">
            <input
              type="checkbox"
              checked={applied}
              onChange={(event) => onAppliedChange(event.target.checked)}
            />
            Use CoStar assumptions in runs
            <span className="text-muted">(off: server defaults)</span>
          </label>

          <Section title="Scenario assumptions" hint="Derived from CoStar · unverified">
            <Assumption label="Occupants" derived={profile.occupant_count} />
            <Assumption label="Who" derived={profile.occupant_context} />
            <Assumption
              label="Alarm"
              derived={profile.alarm}
              value={`noticed at ${profile.alarm.value.notice_hrr_kw} kW, ${profile.alarm.value.same_room_seconds}s to react`}
            />
            <Assumption
              label="Need help out"
              derived={profile.injured_fraction}
              value={`${Math.round(profile.injured_fraction.value * 100)}%`}
            />
            <Assumption label="Floor" derived={profile.floor_material} />
          </Section>

          <Section title="Crew must verify">
            <ul className="flex flex-col gap-1.5">
              {profile.verification.map((item) => (
                <li key={item.item} className="flex gap-1.5" title={item.reason}>
                  <span aria-hidden className="text-danger">●</span>
                  <span>
                    <span className="text-ink">{item.item}</span>{' '}
                    <span className="text-muted">{item.reason}</span>
                  </span>
                </li>
              ))}
            </ul>
          </Section>

          <Section title="CoStar facts">
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
              {record.facts.map((fact, index) => (
                <div key={`${fact.label}-${index}`} className="contents">
                  <dt className="text-muted">{fact.label}</dt>
                  <dd className="text-ink" title={`CoStar · ${fact.source_field}`}>
                    {fact.value}
                  </dd>
                </div>
              ))}
            </dl>
          </Section>

          {record.tenants.length > 0 && (
            <Section title="Current tenants" hint="Context only, not a headcount">
              <ul className="flex flex-col gap-0.5">
                {record.tenants.slice(0, 8).map((tenant, index) => (
                  <li key={index} className="flex justify-between gap-2">
                    <span className="text-ink">{tenant.name ?? 'Unknown tenant'}</span>
                    <span className="text-muted">
                      {[tenant.industry, tenant.employees && `${tenant.employees} staff`]
                        .filter(Boolean)
                        .join(' · ')}
                    </span>
                  </li>
                ))}
              </ul>
            </Section>
          )}
        </div>
      )}
    </section>
  )
}

function Section({ title, hint, children }: { title: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <h3 className="flex items-baseline justify-between font-medium text-ink">
        {title}
        {hint && <span className="font-normal text-muted">{hint}</span>}
      </h3>
      {children}
    </div>
  )
}

function Assumption<T>({ label, derived, value }: { label: string; derived: Derived<T>; value?: string }) {
  return (
    <div className="flex flex-col" title={`CoStar · ${derived.source_field}`}>
      <span>
        <span className="text-muted">{label}: </span>
        <span className="text-ink">{value ?? String(derived.value)}</span>
      </span>
      <span className="text-muted">{derived.rationale}</span>
    </div>
  )
}
