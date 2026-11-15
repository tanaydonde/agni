import { useEffect, useMemo, useState } from 'react'
import { PlaybackBar } from '../components/PlaybackBar'
import { type LoadedScene, SceneGate } from '../components/SceneViewport'
import { PrimaryButton, StageMessage } from '../components/ViewportFrame'
import { firefighterStartSeconds, firstPersonPose } from '../demo/cameras'
import { HeatmapView } from '../demo/HeatmapView'
import { leadRoute, routeRibbon } from '../demo/route'
import { DemoShell, Switch } from '../demo/shared'
import { useDemoPlayback, useFloorGeometry } from '../demo/useDemoPlayback'
import { DEFAULT_BATCH_SIMULATION_COUNT } from '../fire/api'
import { useFireStore } from '../fire/fireStore'
import { agentsAtTime, agentTimeline, leadFirefighter } from '../fire/frames'
import type { AgentTimelineFrame } from '../fire/types'
import type { ModelPlacement } from '../scene/modelChoice'
import { ShowcaseViewport } from '../showcase/ShowcaseViewport'

type Tab = 'heatmap' | 'optimal'

const TABS = [
  ['heatmap', 'Heatmap'],
  ['optimal', 'Optimal route'],
] as const

/** The furnished scan shows the real furniture; no stand-in models. */
const NO_MODELS: ModelPlacement[] = []

/**
 * The demo: run the simulations here, then see where fire is likely
 * (Heatmap) and ride along with the firefighter through the top scenario
 * (Optimal route). The Methods page shows the same run.
 */
export function DemoPage() {
  const loading = useFireStore((state) => state.status === 'loading')
  const hasBatch = useFireStore((state) => state.batch !== null)
  const startBatch = useFireStore((state) => state.startBatch)

  return (
    <DemoShell
      label="Demo"
      actions={
        <>
          <PrimaryButton onClick={() => void startBatch()} disabled={loading}>
            {loading ? 'Running…' : hasBatch ? 'Run simulation again' : 'Run simulation'}
          </PrimaryButton>
          <p className="text-xs text-muted">
            Runs {DEFAULT_BATCH_SIMULATION_COUNT} fire scenarios and keeps the most representative one.
          </p>
        </>
      }
    >
      <SceneGate sceneId={__SCENE_ID__}>{(loaded) => <DemoViews {...loaded} />}</SceneGate>
    </DemoShell>
  )
}

function DemoViews({ scene, floor, meshUrl }: LoadedScene) {
  useDemoPlayback()
  const { status, batch, run, error, startBatch, pause, setClockStart } = useFireStore()
  const geometry = useFloorGeometry(floor)
  const [tab, setTab] = useState<Tab>('heatmap')
  // Matterport's player loads the first time the route is opened, then stays.
  const [routeOpened, setRouteOpened] = useState(false)
  const show = (next: Tab) => {
    if (next === 'optimal') setRouteOpened(true)
    setTab(next)
  }
  // The heatmap has no clock: a run (new, or left playing) waits for the route.
  useEffect(() => {
    if (tab === 'heatmap') pause()
  }, [tab, batch, pause])
  // The route plays from the firefighter's first step, not from the call.
  const routeStart = useMemo(() => (run ? firefighterStartSeconds(agentTimeline(run)) : 0), [run])
  useEffect(() => {
    if (tab !== 'optimal') return
    setClockStart(routeStart)
    return () => setClockStart(0)
  }, [tab, routeStart, setClockStart])

  if (status === 'loading') {
    return (
      <StageMessage title="Running the simulation…">
        Simulating {DEFAULT_BATCH_SIMULATION_COUNT} fires with occupants and a firefighter.
      </StageMessage>
    )
  }
  const ensemble = batch?.fire_ensemble
  if (!batch || !ensemble || !run) {
    return status === 'error' ? (
      <StageMessage title="The simulation couldn't be completed">
        <span className="text-danger">{error}</span>
        <PrimaryButton onClick={() => void startBatch()}>Try again</PrimaryButton>
      </StageMessage>
    ) : (
      <StageMessage title="No simulation yet">
        Run the simulation to see where fire is likely and the firefighter&apos;s route through the top
        scenario.
        <PrimaryButton onClick={() => void startBatch()}>Run simulation</PrimaryButton>
      </StageMessage>
    )
  }

  return (
    <>
      {tab === 'heatmap' && (
        <HeatmapView scene={scene} floor={floor} geometry={geometry} meshUrl={meshUrl} ensemble={ensemble} />
      )}
      {routeOpened && (
        <div className={`absolute inset-0 ${tab === 'optimal' ? '' : 'invisible'}`}>
          <FirstPersonRoute scene={scene} floor={floor} frames={agentTimeline(run)} />
          {tab === 'optimal' && <PlaybackBar label="Top scenario" />}
        </div>
      )}
      <div className="pointer-events-none absolute inset-x-3 top-3 z-30 flex justify-start">
        <Switch label="Demo view" options={TABS} value={tab} onChange={show} />
      </div>
    </>
  )
}

/**
 * The top scenario from the lead firefighter's eyes, in the furnished scan
 * with the fire and people drawn in. The firefighter's own body is left out.
 * An arrow on the floor shows the route still ahead of them.
 */
function FirstPersonRoute({
  scene,
  floor,
  frames,
}: Pick<LoadedScene, 'scene' | 'floor'> & { frames: AgentTimelineFrame[] }) {
  const clockSeconds = useFireStore((state) => state.clockSeconds)
  const pose = useMemo(() => firstPersonPose(frames, clockSeconds), [frames, clockSeconds])
  const leadId = useMemo(() => leadFirefighter(agentsAtTime(frames, 0))?.id, [frames])
  const route = useMemo(() => routeRibbon(leadRoute(frames)), [frames])

  return (
    <ShowcaseViewport
      scene={scene}
      floor={floor}
      models={NO_MODELS}
      version="furnished"
      cameraPose={pose}
      hiddenAgentId={leadId}
      route={route}
      routeSeconds={clockSeconds}
    />
  )
}
