import type { ReactNode } from 'react'
import { PLAYBACK_RATES, playbackEnd, useFireStore } from '../fire/fireStore'

/**
 * The video-player bar along the bottom of a view: a time slider, restart,
 * ±10 s, play/pause and the speed buttons, all driving the shared playback
 * clock (fireStore).
 */
export function PlaybackBar({ label }: { label: string }) {
  const {
    status,
    clockSeconds,
    playbackRate,
    play,
    pause,
    restart,
    seekBy,
    setClock,
    setPlaybackRate,
    clockStartSeconds,
  } = useFireStore()
  const end = useFireStore(playbackEnd)

  return (
    <div className="absolute right-3 bottom-3 left-3 z-30 rounded-lg border border-hairline bg-white px-4 py-3 shadow-[0_4px_16px_rgba(26,31,54,0.12)]">
      <div className="mb-2 flex items-center gap-3">
        <span className="shrink-0 text-xs font-semibold text-ink">{label}</span>
        <input
          aria-label="Simulation time"
          type="range"
          min={clockStartSeconds}
          max={Math.max(end, clockStartSeconds + 0.1)}
          step={0.1}
          value={clockSeconds}
          onChange={(event) => setClock(Number(event.target.value))}
          className="min-w-0 flex-1 accent-accent"
        />
        <span className="shrink-0 font-mono text-xs text-ink-secondary tabular-nums">
          {Math.floor(clockSeconds)} s / {Math.floor(end)} s
        </span>
      </div>
      <div className="flex flex-wrap items-center justify-center gap-2 text-xs">
        <ControlButton label="Restart" onClick={restart}>
          Restart
        </ControlButton>
        <ControlButton label="Go back 10 seconds" onClick={() => seekBy(-10)}>
          −10s
        </ControlButton>
        <ControlButton
          label={status === 'playing' ? 'Pause' : 'Play'}
          onClick={status === 'playing' ? pause : play}
        >
          {status === 'playing' ? 'Pause' : 'Play'}
        </ControlButton>
        <ControlButton label="Go forward 10 seconds" onClick={() => seekBy(10)}>
          +10s
        </ControlButton>
        <div role="group" aria-label="Playback speed" className="ml-2 flex">
          {PLAYBACK_RATES.map((rate) => (
            <button
              key={rate}
              type="button"
              aria-pressed={playbackRate === rate}
              onClick={() => setPlaybackRate(rate)}
              className={`border border-control-border px-2 py-1 font-medium first:rounded-l last:rounded-r [&:not(:first-child)]:-ml-px ${
                playbackRate === rate ? 'bg-ink text-white' : 'text-ink hover:bg-canvas'
              }`}
            >
              {rate}×
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}

function ControlButton({
  label,
  onClick,
  children,
}: {
  label: string
  onClick: () => void
  children: ReactNode
}) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className="rounded border border-control-border bg-white px-2 py-1 font-medium text-ink hover:bg-canvas"
    >
      {children}
    </button>
  )
}
