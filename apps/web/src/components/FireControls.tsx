import { useEffect } from 'react'
import { crewStatus, heldUpCount, occupantCounts } from '../fire/frames'
import { PLAYBACK_RATES, endSeconds, useFireStore, useSimulationAgents } from '../fire/fireStore'
import { useWorld } from '../scene/api'

/**
 * Playback for the latest fire run, in the corner of the editor: play or
 * pause, restart, clear, and the simulated time. Hidden with no run.
 */
export function FireControls() {
  const {
    status,
    objectId,
    run,
    clockSeconds,
    error,
    playbackRate,
    play,
    pause,
    restart,
    clear,
    setPlaybackRate,
  } = useFireStore()
  const agents = useSimulationAgents()
  const counts = occupantCounts(agents)
  const heldUp = heldUpCount(agents)
  const crew = crewStatus(agents)
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
        <span>Generating occupants and simulating fire and crew at {objectId}…</span>
      ) : status === 'error' ? (
        <span className="text-danger">{error}</span>
      ) : (
        <>
          <span className="font-medium text-ink">
            Inside {counts.inside} · Exited {counts.exited}
            {heldUp > 0 && (
              <span className="text-danger" title="Queued, or squeezing past someone">
                {` · Held up ${heldUp}`}
              </span>
            )}
            {crew && ` · ${crew}`}
          </span>
          <span className="font-mono text-ink tabular-nums">
            {Math.floor(clockSeconds)} s / {Math.floor(endSeconds(run))} s
          </span>
          <ControlButton onClick={status === 'playing' ? pause : play}>
            {status === 'playing' ? 'Pause' : 'Play'}
          </ControlButton>
          <ControlButton onClick={restart}>Restart</ControlButton>
          <div role="group" aria-label="Playback speed" className="flex">
            {PLAYBACK_RATES.map((rate) => (
              <button
                key={rate}
                type="button"
                aria-pressed={playbackRate === rate}
                onClick={() => setPlaybackRate(rate)}
                className={`border border-control-border px-1.5 py-0.5 font-medium first:rounded-l last:rounded-r [&:not(:first-child)]:-ml-px ${
                  playbackRate === rate ? 'bg-ink text-white' : 'text-ink hover:bg-canvas'
                }`}
              >
                {rate}×
              </button>
            ))}
          </div>
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
