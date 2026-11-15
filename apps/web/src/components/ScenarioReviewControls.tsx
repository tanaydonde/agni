import { Canvas } from '@react-three/fiber'
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useWorld } from '../scene/api'
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
import { SceneModel } from '../scene/SceneModel'
import { SimulationAgentsView } from '../scene/SimulationAgents'
import { SurfaceFireView } from '../scene/SurfaceFire'
import type { Floor, Scene } from '../scene/types'

export type ReviewCameraMode = 'follow' | 'overview'

interface ScenarioReviewControlsProps {
  scene: Scene
  floor: Floor
  geometry: FloorGeometry
  meshUrl?: string
}

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
  const [expandedPosition, setExpandedPosition] = useState<number | null>(null)
  const [cameraMode, setCameraMode] = useState<ReviewCameraMode>('follow')
  const revision = useWorld().data?.revision

  useEffect(() => {
    if (batch && revision !== undefined && revision !== batch.revision) clear()
  }, [batch, revision, clear])

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
        <RunButton startBatch={startBatch} label="Run simulations" />
      </ReviewMessage>
    )
  }

  const end = batchEndSeconds(batch)
  const activeExpandedPosition =
    expandedPosition !== null && expandedPosition < runs.length ? expandedPosition : null
  const expandedRun = activeExpandedPosition === null ? null : runs[activeExpandedPosition]

  return (
    <>
      <div className="absolute inset-0 grid gap-2 bg-canvas p-2 pt-12 pb-28">
        {expandedRun ? (
          <ReviewPanel
            run={expandedRun}
            position={activeExpandedPosition ?? 0}
            clockSeconds={clockSeconds}
            cameraMode={cameraMode}
            scene={scene}
            floor={floor}
            geometry={geometry}
            meshUrl={meshUrl}
            expanded
          />
        ) : (
          <div className="grid min-h-0 grid-cols-2 grid-rows-2 gap-2">
            {runs.map((run, position) => (
              <ReviewPanel
                key={`${position}:${run.results.scenario_id}`}
                run={run}
                position={position}
                clockSeconds={clockSeconds}
                cameraMode="overview"
                scene={scene}
                floor={floor}
                geometry={geometry}
                meshUrl={meshUrl}
                onExpand={() => {
                  setCameraMode('follow')
                  setExpandedPosition(position)
                }}
              />
            ))}
          </div>
        )}
      </div>

      {expandedRun && (
        <div className="absolute top-14 left-3 z-30 flex gap-2">
          <ControlButton label="Return to four scenarios" onClick={() => setExpandedPosition(null)}>
            ← Four scenarios
          </ControlButton>
          <div className="flex rounded-md border border-hairline bg-white p-0.5 text-xs font-medium shadow-sm">
            <ModeButton active={cameraMode === 'follow'} onClick={() => setCameraMode('follow')}>
              Follow firefighter
            </ModeButton>
            <ModeButton active={cameraMode === 'overview'} onClick={() => setCameraMode('overview')}>
              Overview
            </ModeButton>
          </div>
        </div>
      )}

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
  clockSeconds,
  cameraMode,
  scene,
  floor,
  geometry,
  meshUrl,
  expanded = false,
  onExpand,
}: {
  run: SimulateFireResponse
  position: number
  clockSeconds: number
  cameraMode: ReviewCameraMode
  scene: Scene
  floor: Floor
  geometry: FloorGeometry
  meshUrl?: string
  expanded?: boolean
  onExpand?: () => void
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
  const burning = frame?.surface_fire.filter((node) => node.state === 'burning').length ?? 0
  const activity = crewStatus(agents)

  return (
    <section
      aria-label={`Scenario ${position + 1}`}
      className="relative min-h-0 overflow-hidden rounded-lg border border-hairline bg-white shadow-sm"
    >
      <Canvas frameloop="demand" dpr={expanded ? [1, 2] : [1, 1.25]} camera={{ fov: 50 }}>
        <hemisphereLight args={['#ffffff', '#d5dbe1', 1.6]} />
        <directionalLight position={[8, 14, 6]} intensity={1.4} />
        <SceneModel geometry={geometry} meshUrl={meshUrl} />
        <Furniture floor={floor} assets={scene.assets} />
        <SurfaceFireView floor={floor} frame={frame} />
        <SimulationAgentsView floor={floor} simulationAgents={agents} />
        <ReviewCamera mode={cameraMode} geometry={geometry} agents={agents} />
      </Canvas>

      <div className="pointer-events-none absolute top-2 left-2 rounded-md bg-ink/85 px-2.5 py-1.5 text-xs text-white shadow-sm">
        <div className="font-semibold">Scenario {position + 1}</div>
        <div className="mt-0.5 text-white/80">
          Escaped {counts.exited}/{total} · Fire {burning} cells{activity ? ` · ${activity}` : ''}
        </div>
      </div>
      {!expanded && onExpand && (
        <button
          type="button"
          onClick={onExpand}
          className="absolute top-2 right-2 rounded-md border border-white/30 bg-ink/85 px-2.5 py-1.5 text-xs font-medium text-white shadow-sm hover:bg-ink"
        >
          Follow firefighter ↗
        </button>
      )}
    </section>
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

function ModeButton({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: ReactNode
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={
        active
          ? 'rounded bg-accent px-2.5 py-1 text-white'
          : 'rounded px-2.5 py-1 text-ink-secondary hover:text-ink'
      }
    >
      {children}
    </button>
  )
}
