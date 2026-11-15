import { useState, type KeyboardEvent } from 'react'
import {
  MAX_OCCUPANT_COUNT,
  MIN_OCCUPANT_COUNT,
  clampOccupantCount,
  type ScenarioOptions,
} from '../fire/api'

interface ActionBarProps {
  canRunSimulation: boolean
  hasSelectedOrigin: boolean
  onRunSimulation?: () => void
  /** Who will be in the building when the simulation runs. */
  scenario: ScenarioOptions
  onScenarioChange: (scenario: Partial<ScenarioOptions>) => void
}

export function ActionBar({
  canRunSimulation,
  hasSelectedOrigin,
  onRunSimulation,
  scenario,
  onScenarioChange,
}: ActionBarProps) {
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
      <button
        type="button"
        disabled={!canRunSimulation}
        onClick={onRunSimulation}
        title={
          canRunSimulation
            ? 'Generate occupants and start a fire at the selected furniture'
            : 'Select a piece of furniture in the editor to start a fire there'
        }
        className="h-8 shrink-0 rounded-md border border-control-border bg-white px-4 text-sm font-medium text-ink hover:bg-canvas disabled:cursor-not-allowed disabled:text-placeholder disabled:hover:bg-white"
      >
        {hasSelectedOrigin ? 'Run simulation' : 'Select fire origin'}
      </button>
      <div className="flex items-center gap-3 text-xs text-ink-secondary">
        <OccupantCountInput
          value={scenario.occupantCount}
          onChange={(occupantCount) => onScenarioChange({ occupantCount })}
        />
        <label
          className="flex items-center gap-1.5"
          title="Pack people into the largest rooms so they queue at the doors. Off: Gemini spreads them out."
        >
          <input
            type="checkbox"
            checked={scenario.layout === 'crowded'}
            onChange={(event) =>
              onScenarioChange({ layout: event.target.checked ? 'crowded' : 'spread' })
            }
          />
          Crowded
        </label>
      </div>
      {!hasSelectedOrigin && (
        <p className="text-xs text-muted">
          In Building → Editor, click a piece of furniture to choose where the single fire starts.
        </p>
      )}
    </div>
  )
}

/**
 * A number box for how many occupants to spawn. Typing edits a local draft
 * (so the box can be cleared and retyped); Enter or leaving the box commits
 * it, clamped to the allowed range, and anything unparseable reverts.
 */
function OccupantCountInput({ value, onChange }: { value: number; onChange: (count: number) => void }) {
  const [draft, setDraft] = useState<string | null>(null)

  const commit = () => {
    if (draft === null) return
    const parsed = Number.parseFloat(draft)
    if (Number.isFinite(parsed)) onChange(clampOccupantCount(parsed))
    setDraft(null)
  }

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') commit()
    else if (event.key === 'Escape') setDraft(null)
  }

  return (
    <label className="flex items-center gap-1.5" title={`${MIN_OCCUPANT_COUNT}–${MAX_OCCUPANT_COUNT} occupants`}>
      Occupants
      <input
        type="number"
        inputMode="numeric"
        min={MIN_OCCUPANT_COUNT}
        max={MAX_OCCUPANT_COUNT}
        step={1}
        value={draft ?? String(value)}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={onKeyDown}
        className="w-14 rounded border border-control-border bg-white px-1.5 py-0.5 text-ink"
      />
    </label>
  )
}
