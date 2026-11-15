import { OCCUPANT_COUNTS, type ScenarioOptions } from '../fire/api'

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
    <div className="flex flex-col gap-2">
      <div className="flex gap-3">
        <button
          type="button"
          disabled={!canRunSimulation}
          onClick={onRunSimulation}
          title={
            canRunSimulation
              ? 'Generate occupants and start a fire at the selected furniture'
              : 'Select a piece of furniture in the editor to start a fire there'
          }
          className="h-10 flex-1 rounded-md border border-control-border bg-white font-medium text-ink hover:bg-canvas disabled:cursor-not-allowed disabled:text-placeholder disabled:hover:bg-white"
        >
          {hasSelectedOrigin ? 'Run simulation' : 'Select fire origin'}
        </button>
      </div>
      {!hasSelectedOrigin && (
        <p className="text-xs leading-normal text-muted">
          In Building → Editor, click a piece of furniture to choose where the single fire starts.
        </p>
      )}
      <div className="flex items-center gap-3 text-xs text-ink-secondary">
        <label className="flex items-center gap-1.5">
          Occupants
          <select
            value={scenario.occupantCount}
            onChange={(event) => onScenarioChange({ occupantCount: Number(event.target.value) })}
            className="rounded border border-control-border bg-white px-1.5 py-0.5 text-ink"
          >
            {OCCUPANT_COUNTS.map((count) => (
              <option key={count} value={count}>
                {count}
              </option>
            ))}
          </select>
        </label>
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
    </div>
  )
}
