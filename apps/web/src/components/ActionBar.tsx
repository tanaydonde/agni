interface ActionBarProps {
  canRunSimulation: boolean
  onRunSimulation?: () => void
  onOptimize?: () => void
}

export function ActionBar({ canRunSimulation, onRunSimulation, onOptimize }: ActionBarProps) {
  return (
    <div className="flex gap-3">
      <button
        type="button"
        disabled={!canRunSimulation}
        onClick={onRunSimulation}
        title={
          canRunSimulation
            ? 'Start a fire at the selected furniture'
            : 'Select a piece of furniture in the editor to start a fire there'
        }
        className="h-10 flex-1 rounded-md border border-control-border bg-white font-medium text-ink hover:bg-canvas disabled:cursor-not-allowed disabled:text-placeholder disabled:hover:bg-white"
      >
        Run simulation
      </button>
      <button
        type="button"
        onClick={onOptimize}
        className="h-10 flex-1 rounded-md border border-accent bg-accent font-medium text-white hover:border-accent-strong hover:bg-accent-strong"
      >
        Optimize
      </button>
    </div>
  )
}
