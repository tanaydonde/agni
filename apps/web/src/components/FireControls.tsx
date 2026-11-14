import { useEffect } from 'react'
import { endSeconds, useFireStore } from '../fire/fireStore'
import { useWorld } from '../scene/api'

/**
 * Playback for the latest fire run, in the corner of the editor: play or
 * pause, restart, clear, and the simulated time. Hidden with no run.
 */
export function FireControls() {
  const { status, objectId, run, clockSeconds, error, play, pause, restart, clear } = useFireStore()
  const revision = useWorld().data?.revision

  // Furniture moved since the run: its fire would float in the wrong place.
  useEffect(() => {
    if (run && revision !== undefined && revision !== run.revision) clear()
  }, [run, revision, clear])

  if (status === 'idle') return null

  return (
    <div
      role="group"
      aria-label="Fire simulation"
      className="absolute right-3 bottom-3 flex items-center gap-2 rounded-lg border border-hairline bg-white px-3 py-2 text-xs text-ink-secondary shadow-[0_4px_12px_rgba(26,31,54,0.08)]"
    >
      <span aria-hidden className="size-2 rounded-full bg-[#ff6a1a]" />
      {status === 'loading' ? (
        <span>Simulating fire at {objectId}…</span>
      ) : status === 'error' ? (
        <span className="text-danger">{error}</span>
      ) : (
        <>
          <span className="font-mono text-ink tabular-nums">
            {Math.floor(clockSeconds)} s / {Math.floor(endSeconds(run))} s
          </span>
          <ControlButton onClick={status === 'playing' ? pause : play}>
            {status === 'playing' ? 'Pause' : 'Play'}
          </ControlButton>
          <ControlButton onClick={restart}>Restart</ControlButton>
        </>
      )}
      {status !== 'loading' && <ControlButton onClick={clear}>Clear</ControlButton>}
    </div>
  )
}

function ControlButton({ onClick, children }: { onClick: () => void; children: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded border border-control-border px-2 py-0.5 font-medium text-ink hover:bg-canvas"
    >
      {children}
    </button>
  )
}
