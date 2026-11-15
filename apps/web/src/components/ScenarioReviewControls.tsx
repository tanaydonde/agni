import { Canvas } from '@react-three/fiber'
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useWorld } from '../scene/api'
import { burningObjectIds } from '../fire/cells'
import { agentsAtTime, crewStatus, frameAtTime, occupantCounts } from '../fire/frames'
import {
  PLAYBACK_RATES,
  batchEndSeconds,
  representativeRuns,
  useFireStore,
} from '../fire/fireStore'
import type { SimulateFireResponse } from '../fire/types'
import { ReviewCamera } from '../scene/ReviewCamera'
import { Furniture } from '../scene/Furniture'
import type { FloorGeometry } from '../scene/geometry'
import type { ModelPlacement } from '../scene/modelChoice'
import { SceneModel } from '../scene/SceneModel'
import { SimulationAgentsView } from '../scene/SimulationAgents'
import { SurfaceFireView } from '../scene/SurfaceFire'
import type { Floor, Scene } from '../scene/types'
import { ShowcaseViewport } from '../showcase/ShowcaseViewport'

type ReviewView = 'mesh' | 'textured'

const REVIEW_VIEWS: [view: ReviewView, label: string][] = [
  ['mesh', '3D'],
  ['textured', 'Textured'],
]

/** A scroll gesture pages once; it ends after this long without wheel events. */
const WHEEL_GESTURE_IDLE_MS = 300
/** Ignore tiny wheel deltas (trackpad jitter). */
const MIN_WHEEL_DELTA = 4

interface ScenarioReviewControlsProps {
  scene: Scene
  floor: Floor
  geometry: FloorGeometry
  meshUrl?: string
}

/**
 * Scenario review: one representative run at a time, filling the view from
 * a fixed bird's-eye overview. Page through the runs with the arrows, the
 * dots, a scroll gesture over the view, or the ←/→ keys. Every run shares
 * the playback clock, and paging never moves it, so each run is shown at
 * the same moment (shorter runs hold their last frame).
 */
export function ScenarioReviewControls({
  scene,
  floor,
  geometry,
  meshUrl,
}: ScenarioReviewControlsProps) {
  const {
    status,
    batch,
    error,
    clockSeconds,
    playbackRate,
    startBatch,
    play,
    pause,
    restart,
    seekBy,
    setClock,
    setPlaybackRate,
    clear,
  } = useFireStore()
  const runs = useMemo(() => representativeRuns(batch), [batch])
  const revision = useWorld().data?.revision
  const canPage = runs.length > 1
  // The page is local (not the store's representative, which clamps the
  // shared clock) and starts over at the first run for each new batch.
  const [page, setPage] = useState({ batch, position: 0 })
  const position = page.batch === batch ? Math.min(page.position, Math.max(runs.length - 1, 0)) : 0
  const showRun = useCallback(
    (next: number) => {
      const count = runs.length
      if (count > 0) setPage({ batch, position: ((next % count) + count) % count })
    },
    [batch, runs.length],
  )
  // 3D mesh or Matterport's textured dollhouse. The textured player loads the
  // first time it's shown and then stays, so switching back is instant.
  const [view, setView] = useState<ReviewView>('mesh')
  const [texturedOpened, setTexturedOpened] = useState(false)
  const showView = (next: ReviewView) => {
    if (next === 'textured') setTexturedOpened(true)
    setView(next)
  }
  const previousRun = useCallback(() => showRun(position - 1), [showRun, position])
  const nextRun = useCallback(() => showRun(position + 1), [showRun, position])

  useEffect(() => {
    if (batch && revision !== undefined && revision !== batch.revision) clear()
  }, [batch, revision, clear])

  // ←/→ (and Page Up/Down) page through the runs, except while typing or
  // dragging the time slider.
  useEffect(() => {
    if (!canPage) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return
      const target = event.target as HTMLElement | null
      if (target?.closest('input, textarea, select, [contenteditable="true"]')) return
      if (event.key === 'ArrowRight' || event.key === 'PageDown') nextRun()
      else if (event.key === 'ArrowLeft' || event.key === 'PageUp') previousRun()
      else return
      event.preventDefault()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [canPage, nextRun, previousRun])

  // One page per scroll gesture, so a trackpad fling doesn't skip runs. A
  // gesture is a run of wheel events less than WHEEL_GESTURE_IDLE_MS apart,
  // judged by the events' own timestamps so slow frames can't split it. The
  // listener is non-passive so it can stop a pinch (ctrl+wheel) from zooming
  // the page and a sideways swipe from navigating back; pinches never page.
  const paging = useRef({ canPage, nextRun, previousRun })
  useEffect(() => {
    paging.current = { canPage, nextRun, previousRun }
  })
  const lastWheelAt = useRef(Number.NEGATIVE_INFINITY)
  const wheelArea = useCallback((el: HTMLDivElement | null) => {
    if (!el) return
    const onWheel = (event: globalThis.WheelEvent) => {
      event.preventDefault()
      if (event.ctrlKey) return
      const { canPage: pageable, nextRun: next, previousRun: previous } = paging.current
      if (!pageable) return
      const delta = Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX
      if (Math.abs(delta) < MIN_WHEEL_DELTA) return
      const gestureInProgress = event.timeStamp - lastWheelAt.current < WHEEL_GESTURE_IDLE_MS
      lastWheelAt.current = event.timeStamp
      if (gestureInProgress) return
      if (delta > 0) next()
      else previous()
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [])

  if (status === 'loading') {
    return <ReviewMessage title="Running scenarios…">Preparing four training views.</ReviewMessage>
  }

  if (status === 'error' && !batch) {
    return (
      <ReviewMessage title="Scenarios couldn't be completed">
        <span className="text-danger">{error}</span>
        <RunButton startBatch={startBatch} label="Try again" />
      </ReviewMessage>
    )
  }

  if (!batch || runs.length === 0) {
    return (
      <ReviewMessage title="No scenarios yet">
        Gemini will choose varied fire origins and occupant locations, then prepare four synchronized
        simulations for review.
        <RunButton startBatch={startBatch} label="Run multiple simulations" />
      </ReviewMessage>
    )
  }

  const end = batchEndSeconds(batch)

  return (
    <>
      <div ref={wheelArea} className="absolute inset-0 flex overscroll-none bg-canvas p-2 pt-12 pb-28">
        <ReviewPanel
          run={runs[position]}
          position={position}
          count={runs.length}
          clockSeconds={clockSeconds}
          scene={scene}
          floor={floor}
          geometry={geometry}
          meshUrl={meshUrl}
          textured={view === 'textured'}
          texturedOpened={texturedOpened}
          viewSwitch={<ViewSwitch view={view} onChange={showView} />}
          pager={
            canPage && (
              <Pager
                position={position}
                count={runs.length}
                onPrevious={previousRun}
                onNext={nextRun}
                onSelect={showRun}
              />
            )
          }
        />
      </div>

      <GlobalPlaybackControls
        status={status}
        clockSeconds={clockSeconds}
        end={end}
        playbackRate={playbackRate}
        play={play}
        pause={pause}
        restart={restart}
        seekBy={seekBy}
        setClock={setClock}
        setPlaybackRate={setPlaybackRate}
      />
    </>
  )
}

function ReviewPanel({
  run,
  position,
  count,
  clockSeconds,
  scene,
  floor,
  geometry,
  meshUrl,
  textured,
  texturedOpened,
  viewSwitch,
  pager,
}: {
  run: SimulateFireResponse
  position: number
  count: number
  clockSeconds: number
  scene: Scene
  floor: Floor
  geometry: FloorGeometry
  meshUrl?: string
  /** Show Matterport's textured dollhouse instead of the 3D mesh. */
  textured: boolean
  /** Whether the textured player has been opened (it stays mounted after). */
  texturedOpened: boolean
  viewSwitch: ReactNode
  pager?: ReactNode
}) {
  const frame = useMemo(
    () => frameAtTime(run.results.frames, clockSeconds),
    [run.results.frames, clockSeconds],
  )
  const agents = useMemo(
    () => agentsAtTime(run.results.frames, clockSeconds),
    [run.results.frames, clockSeconds],
  )
  const counts = occupantCounts(agents)
  const total = counts.inside + counts.exited
  const burningCells = frame?.surface_fire.filter((node) => node.state === 'burning').length ?? 0
  const activity = crewStatus(agents)
  const timeline = useMemo(() => ({ frame, agents }), [frame, agents])
  const burning = useMemo(() => burningObjectIds(frame), [frame])
  const stats = `Escaped ${counts.exited}/${total} · Fire ${burningCells} cells${activity ? ` · ${activity}` : ''}`

  return (
    <section
      aria-label={`Scenario ${position + 1} of ${count}`}
      aria-roledescription="slide"
      className="relative min-h-0 min-w-0 flex-1 overflow-hidden rounded-lg border border-hairline bg-white shadow-sm"
    >
      {/* One canvas for every run: paging swaps the data, not the WebGL context. */}
      <div className={`absolute inset-0 ${textured ? 'invisible' : ''}`}>
        <Canvas frameloop="demand" dpr={[1, 2]} camera={{ fov: 50 }}>
          <hemisphereLight args={['#ffffff', '#d5dbe1', 1.6]} />
          <directionalLight position={[8, 14, 6]} intensity={1.4} />
          <SceneModel geometry={geometry} meshUrl={meshUrl} />
          <Furniture floor={floor} assets={scene.assets} burning={burning} />
          <SurfaceFireView floor={floor} frame={frame} />
          <SimulationAgentsView floor={floor} simulationAgents={agents} />
          <ReviewCamera geometry={geometry} />
        </Canvas>
      </div>
      {texturedOpened && (
        <div className={`absolute inset-0 ${textured ? '' : 'invisible'}`}>
          <ShowcaseViewport
            scene={scene}
            floor={floor}
            models={NO_MODELS}
            version="defurnished"
            timeline={timeline}
            birdsEye
          />
        </div>
      )}

      {/* One header row, so the stats and the pager wrap instead of overlapping. */}
      <div className="pointer-events-none absolute inset-x-2 top-2 z-10 flex flex-wrap items-start justify-between gap-2">
        <div
          title={stats}
          className="min-w-0 max-w-full rounded-md bg-ink/85 px-2.5 py-1.5 text-xs text-white shadow-sm"
        >
          <div className="truncate">{stats}</div>
        </div>
        <div className="pointer-events-auto ml-auto flex shrink-0 items-start gap-2">
          {viewSwitch}
          {pager}
        </div>
      </div>
    </section>
  )
}

/** Stable empty list: the review's textured view draws no scanned-furniture models. */
const NO_MODELS: ModelPlacement[] = []

/** 3D mesh or Matterport's textured dollhouse, styled like the Building view switch. */
function ViewSwitch({ view, onChange }: { view: ReviewView; onChange: (view: ReviewView) => void }) {
  return (
    <div
      role="radiogroup"
      aria-label="Review view"
      className="flex rounded-md border border-hairline bg-white p-0.5 text-xs font-medium shadow-sm"
    >
      {REVIEW_VIEWS.map(([value, label]) => (
        <button
          key={value}
          type="button"
          role="radio"
          aria-checked={view === value}
          onClick={() => onChange(value)}
          className={
            view === value
              ? 'rounded bg-accent px-2.5 py-1 text-white'
              : 'rounded px-2.5 py-1 text-ink-secondary hover:text-ink'
          }
        >
          {label}
        </button>
      ))}
    </div>
  )
}

/** ‹ 1 / 4 › plus a dot per run; the dots jump straight to a run. */
function Pager({
  position,
  count,
  onPrevious,
  onNext,
  onSelect,
}: {
  position: number
  count: number
  onPrevious: () => void
  onNext: () => void
  onSelect: (position: number) => void
}) {
  return (
    <div
      role="group"
      aria-label="Scenarios"
      className="flex items-center gap-1 rounded-md border border-hairline bg-white p-0.5 text-xs font-medium text-ink shadow-sm"
    >
      <button
        type="button"
        aria-label="Previous scenario"
        onClick={onPrevious}
        className="rounded px-2 py-1 hover:bg-canvas"
      >
        ‹
      </button>
      <span className="min-w-10 text-center tabular-nums" aria-live="polite">
        {position + 1} / {count}
      </span>
      <div className="flex items-center gap-1 px-1">
        {Array.from({ length: count }, (_, index) => (
          <button
            key={index}
            type="button"
            aria-label={`Show scenario ${index + 1}`}
            aria-current={index === position ? 'true' : undefined}
            onClick={() => onSelect(index)}
            className={`size-2 rounded-full ${index === position ? 'bg-accent' : 'bg-control-border hover:bg-ink-secondary'}`}
          />
        ))}
      </div>
      <button
        type="button"
        aria-label="Next scenario"
        onClick={onNext}
        className="rounded px-2 py-1 hover:bg-canvas"
      >
        ›
      </button>
    </div>
  )
}

function GlobalPlaybackControls({
  status,
  clockSeconds,
  end,
  playbackRate,
  play,
  pause,
  restart,
  seekBy,
  setClock,
  setPlaybackRate,
}: {
  status: string
  clockSeconds: number
  end: number
  playbackRate: number
  play: () => void
  pause: () => void
  restart: () => void
  seekBy: (seconds: number) => void
  setClock: (seconds: number) => void
  setPlaybackRate: (rate: number) => void
}) {
  return (
    <div className="absolute right-3 bottom-3 left-3 z-30 rounded-lg border border-hairline bg-white px-4 py-3 shadow-[0_4px_16px_rgba(26,31,54,0.12)]">
      <div className="mb-2 flex items-center gap-3">
        <span className="shrink-0 text-xs font-semibold text-ink">All scenarios</span>
        <input
          aria-label="Simulation time"
          type="range"
          min={0}
          max={Math.max(end, 0.1)}
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
        <ControlButton label="Restart all scenarios" onClick={restart}>
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

function ReviewMessage({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="absolute inset-0 z-10 flex items-center justify-center bg-white/85 p-8">
      <div className="flex max-w-sm flex-col items-center gap-3 text-center text-sm leading-normal text-muted">
        <h2 className="text-base font-semibold text-ink">{title}</h2>
        <div className="flex flex-col items-center gap-3">{children}</div>
      </div>
    </div>
  )
}

function RunButton({ startBatch, label }: { startBatch: () => Promise<void>; label: string }) {
  return (
    <button
      type="button"
      onClick={() => void startBatch()}
      className="rounded-md border border-accent bg-accent px-4 py-2 font-medium text-white hover:border-accent-strong hover:bg-accent-strong"
    >
      {label}
    </button>
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
