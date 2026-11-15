import { useMemo, useState } from 'react'
import { Link } from '../components/Link'
import { PlaybackBar } from '../components/PlaybackBar'
import { type LoadedScene, SceneCanvas, SceneGate } from '../components/SceneViewport'
import { StageMessage } from '../components/ViewportFrame'
import { DemoShell, Switch } from '../demo/shared'
import { useDemoPlayback, useFloorGeometry } from '../demo/useDemoPlayback'
import { useFireStore } from '../fire/fireStore'
import { fireOrigin } from '../fire/frames'
import type { ModelPlacement } from '../scene/modelChoice'
import { ShowcaseViewport } from '../showcase/ShowcaseViewport'

type World = 'basic' | 'furnished'

const WORLDS = [
  ['basic', 'Basic'],
  ['furnished', 'Furnished'],
] as const

const NO_MODELS: ModelPlacement[] = []

/**
 * The top scenario from the Demo page, in two worlds: the basic one (grey
 * rooms, furniture as boxes) to fly around freely, and the furnished scan,
 * starting at a fixed spot like a fly on the wall, with the fly controls to
 * find a better one. Both play on one clock.
 */
export function MethodsPage() {
  return (
    <DemoShell label="Methods">
      <SceneGate sceneId={__SCENE_ID__}>{(loaded) => <MethodsViews {...loaded} />}</SceneGate>
    </DemoShell>
  )
}

function MethodsViews({ scene, floor, meshUrl }: LoadedScene) {
  useDemoPlayback()
  const status = useFireStore((state) => state.status)
  const batch = useFireStore((state) => state.batch)
  const run = useFireStore((state) => state.run)
  const geometry = useFloorGeometry(floor)
  const [world, setWorld] = useState<World>('basic')
  // Matterport's player loads the first time Furnished is opened, then stays.
  const [furnishedOpened, setFurnishedOpened] = useState(false)
  const show = (next: World) => {
    if (next === 'furnished') setFurnishedOpened(true)
    setWorld(next)
  }
  const focus = useMemo(() => (run ? fireOrigin(run.results.frames) : null), [run])

  if (!batch || !run) {
    return (
      <StageMessage title={status === 'loading' ? 'Running the simulation…' : 'No simulation yet'}>
        {status === 'loading'
          ? 'The run will play here when it finishes.'
          : 'Methods plays the simulation from the Demo page. Run it there first.'}
        <Link to="/demo" className="font-medium text-accent hover:text-accent-strong">
          Go to Demo
        </Link>
      </StageMessage>
    )
  }

  return (
    <>
      {/* Both worlds end above the playback bar. */}
      <div className={`absolute inset-x-0 top-0 bottom-24 ${world === 'basic' ? '' : 'invisible'}`}>
        <SceneCanvas
          scene={scene}
          floor={floor}
          meshUrl={meshUrl}
          geometry={geometry}
          enabled={world === 'basic'}
          showSimulation
          selectable={false}
        />
      </div>
      {furnishedOpened && (
        <div className={`absolute inset-x-0 top-0 bottom-24 ${world === 'furnished' ? '' : 'invisible'}`}>
          <ShowcaseViewport
            scene={scene}
            floor={floor}
            models={NO_MODELS}
            version="furnished"
            vantageFocus={focus}
            flyFromVantage
          />
        </div>
      )}
      <PlaybackBar label="Top scenario" />
      <div className="pointer-events-none absolute inset-x-3 top-3 z-30 flex justify-center">
        <Switch label="World" options={WORLDS} value={world} onChange={show} />
      </div>
    </>
  )
}
