import { Canvas } from '@react-three/fiber'
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useWorld } from '../scene/api'
import { burningObjectIds } from '../fire/cells'
import { agentsAtTime, agentTimeline, frameAtTime } from '../fire/frames'
import { representativeRuns, useFireStore } from '../fire/fireStore'
import type { SimulateFireResponse } from '../fire/types'
import { ReviewCamera } from '../scene/ReviewCamera'
import { Furniture } from '../scene/Furniture'
import { wheelPixels } from '../scene/cameraMath'
import type { FloorGeometry } from '../scene/geometry'
import type { ModelPlacement } from '../scene/modelChoice'
import { ORBIT_FOV_DEGREES, orbitHome, panOrbit, rotateOrbit, zoomOrbit, type Orbit } from '../scene/orbitCamera'
import { SceneModel } from '../scene/SceneModel'
import { SimulationAgentsView } from '../scene/SimulationAgents'
import { SurfaceFireView } from '../scene/SurfaceFire'
import type { Floor, Scene } from '../scene/types'
import { ShowcaseViewport } from '../showcase/ShowcaseViewport'
import { PlaybackBar } from './PlaybackBar'
import { PrimaryButton, StageMessage } from './ViewportFrame'

type ReviewView = 'mesh' | 'textured'

const REVIEW_VIEWS: [view: ReviewView, label: string][] = [
  ['mesh', '3D'],
  ['textured', 'Textured'],
]

/** How close and how far the review camera may zoom, relative to the start. */
const MIN_ZOOM = 0.15
const MAX_ZOOM = 3

interface ScenarioReviewControlsProps {
  scene: Scene
  floor: Floor
  geometry: FloorGeometry
  meshUrl?: string
}

/**
 * Scenario review: one representative run at a time, filling the view from
 * a bird's-eye orbit camera (drag to orbit, scroll to zoom, right-drag to
 * pan). Page through the runs with the arrows, the dots, or the ←/→ keys.
 * Every run shares the playback clock, and paging never moves it, so each
 * run is shown at the same moment (shorter runs hold their last frame).
 */
export function ScenarioReviewControls({
  scene,
  floor,
  geometry,
  meshUrl,
}: ScenarioReviewControlsProps) {
  const { status, batch, error, clockSeconds, startBatch, clear } = useFireStore()
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

  if (status === 'loading') {
    return <StageMessage title="Running scenarios…">Preparing four training views.</StageMessage>
  }

  if (status === 'error' && !batch) {
    return (
      <StageMessage title="Scenarios couldn't be completed">
        <span className="text-danger">{error}</span>
        <PrimaryButton onClick={() => void startBatch()}>Try again</PrimaryButton>
      </StageMessage>
    )
  }

  if (!batch || runs.length === 0) {
    return (
      <StageMessage title="No scenarios yet">
        Gemini will choose varied fire origins and occupant locations, then prepare four synchronized
        simulations for review.
        <PrimaryButton onClick={() => void startBatch()}>Run multiple simulations</PrimaryButton>
      </StageMessage>
    )
  }

  return (
    <>
      <div className="absolute inset-0 flex overscroll-none bg-canvas p-2 pt-12 pb-28">
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

      <PlaybackBar label="All scenarios" />
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
    () => agentsAtTime(agentTimeline(run), clockSeconds),
    [run, clockSeconds],
  )
  const timeline = useMemo(() => ({ frame, agents }), [frame, agents])
  const burning = useMemo(() => burningObjectIds(frame), [frame])
  const { orbit, resetOrbit, controls } = useReviewOrbit(geometry)

  return (
    <section
      aria-label={`Scenario ${position + 1} of ${count}`}
      aria-roledescription="slide"
      className="relative min-h-0 min-w-0 flex-1 overflow-hidden rounded-lg border border-hairline bg-white shadow-sm"
    >
      {/* One canvas for every run: paging swaps the data, not the WebGL context. */}
      <div className={`absolute inset-0 ${textured ? 'invisible' : ''}`}>
        <Canvas frameloop="demand" dpr={[1, 2]} camera={{ fov: ORBIT_FOV_DEGREES }}>
          <hemisphereLight args={['#ffffff', '#d5dbe1', 1.6]} />
          <directionalLight position={[8, 14, 6]} intensity={1.4} />
          <SceneModel geometry={geometry} meshUrl={meshUrl} />
          <Furniture floor={floor} assets={scene.assets} burning={burning} />
          <SurfaceFireView floor={floor} frame={frame} />
          <SimulationAgentsView floor={floor} simulationAgents={agents} />
          <ReviewCamera orbit={orbit} />
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
            orbit={orbit}
          />
        </div>
      )}

      {/* Camera input for both views: drag orbits, right-drag pans, scroll zooms. */}
      <div
        ref={controls}
        aria-label="Scenario view. Drag to orbit, right-drag to pan, scroll to zoom."
        className="absolute inset-0 cursor-grab touch-none active:cursor-grabbing"
      />

      <div className="pointer-events-none absolute inset-x-2 top-2 z-10 flex flex-wrap items-start justify-end gap-2">
        <div className="pointer-events-auto flex shrink-0 flex-wrap items-start justify-end gap-2">
          <button
            type="button"
            onClick={resetOrbit}
            className="rounded-md border border-hairline bg-white px-2.5 py-1 text-xs font-medium text-ink-secondary shadow-sm hover:text-ink"
          >
            Reset view
          </button>
          {viewSwitch}
          {pager}
        </div>
      </div>
    </section>
  )
}

/**
 * The review's orbit camera, shared by the 3D and textured views so switching
 * keeps the same view. It starts framed on the whole floor (as the 3D
 * overview always was) once the panel has a size, survives paging and
 * resizing, and returns there on `resetOrbit`. `controls` is the ref for the
 * element that takes the pointer: left-drag orbits, right-drag (or
 * Shift-drag) pans, the wheel or a pinch zooms.
 */
function useReviewOrbit(geometry: FloorGeometry) {
  const [orbit, setOrbit] = useState<Orbit | null>(null)
  const home = useRef<Orbit | null>(null)
  const size = useRef({ width: 0, height: 0 })

  const homeFor = useCallback(
    (width: number, height: number) =>
      orbitHome(geometry.bounds, geometry.floorZ, ORBIT_FOV_DEGREES, width / height),
    [geometry],
  )
  const resetOrbit = useCallback(() => {
    const { width, height } = size.current
    if (width === 0 || height === 0) return
    home.current = homeFor(width, height)
    setOrbit(home.current)
  }, [homeFor])
  // A new floor starts over from its own home view.
  useEffect(() => {
    resetOrbit()
  }, [resetOrbit])

  const controls = useCallback(
    (el: HTMLDivElement | null) => {
      if (!el) return
      const observer = new ResizeObserver(([entry]) => {
        const { width, height } = entry.contentRect
        const first = size.current.width === 0 || size.current.height === 0
        size.current = { width, height }
        if (first && width > 0 && height > 0) resetOrbit()
      })
      observer.observe(el)

      let drag: { mode: 'rotate' | 'pan'; x: number; y: number; pointerId: number } | null = null
      const onPointerDown = (event: PointerEvent) => {
        if (event.button !== 0 && event.button !== 2) return
        const pan = event.button === 2 || event.shiftKey || event.ctrlKey || event.metaKey
        drag = { mode: pan ? 'pan' : 'rotate', x: event.clientX, y: event.clientY, pointerId: event.pointerId }
        el.setPointerCapture(event.pointerId)
        event.preventDefault()
      }
      const onPointerMove = (event: PointerEvent) => {
        if (!drag || event.pointerId !== drag.pointerId) return
        const dx = event.clientX - drag.x
        const dy = event.clientY - drag.y
        drag.x = event.clientX
        drag.y = event.clientY
        const { mode } = drag
        setOrbit((current) =>
          current &&
          (mode === 'rotate'
            ? rotateOrbit(current, dx, dy)
            : panOrbit(current, dx, dy, ORBIT_FOV_DEGREES, size.current.height || 1)),
        )
      }
      const onPointerUp = (event: PointerEvent) => {
        if (!drag || event.pointerId !== drag.pointerId) return
        if (el.hasPointerCapture(event.pointerId)) el.releasePointerCapture(event.pointerId)
        drag = null
      }
      // Non-passive, so scrolling and pinching zoom the view, not the page.
      const onWheel = (event: WheelEvent) => {
        event.preventDefault()
        const { y } = wheelPixels(event)
        const base = home.current?.distance ?? 1
        setOrbit((current) => current && zoomOrbit(current, y, base * MIN_ZOOM, base * MAX_ZOOM))
      }
      const onContextMenu = (event: MouseEvent) => event.preventDefault()

      el.addEventListener('pointerdown', onPointerDown)
      el.addEventListener('pointermove', onPointerMove)
      el.addEventListener('pointerup', onPointerUp)
      el.addEventListener('pointercancel', onPointerUp)
      el.addEventListener('wheel', onWheel, { passive: false })
      el.addEventListener('contextmenu', onContextMenu)
      return () => {
        observer.disconnect()
        el.removeEventListener('pointerdown', onPointerDown)
        el.removeEventListener('pointermove', onPointerMove)
        el.removeEventListener('pointerup', onPointerUp)
        el.removeEventListener('pointercancel', onPointerUp)
        el.removeEventListener('wheel', onWheel)
        el.removeEventListener('contextmenu', onContextMenu)
      }
    },
    [resetOrbit],
  )

  return { orbit, resetOrbit, controls }
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
