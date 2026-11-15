import { Canvas } from '@react-three/fiber'
import { useMemo, useRef, useState, type ReactNode } from 'react'
import { useFireStore } from '../fire/fireStore'
import { usePlaybackClock } from '../fire/usePlaybackClock'
import {
  WorldServerUnavailableError,
  sceneMeshUrl,
  useDetectedFurniture,
  useFurnitureModels,
  useSceneMeshAvailable,
  useWorld,
} from '../scene/api'
import { CameraController } from '../scene/CameraController'
import { placeModels } from '../scene/modelChoice'
import { detectedInScene } from '../scene/scanned'
import { Furniture } from '../scene/Furniture'
import { buildFloorGeometry } from '../scene/geometry'
import { SceneModel } from '../scene/SceneModel'
import { SelectionController } from '../scene/SelectionController'
import { SurfaceFire } from '../scene/SurfaceFire'
import { SimulationAgents } from '../scene/SimulationAgents'
import type { DetectedObject, Floor, Scene } from '../scene/types'
import { ShowcaseViewport } from '../showcase/ShowcaseViewport'
import { ControlsHint, type Controls } from './ControlsHint'
import { FireControls } from './FireControls'
import { FlyIndicator } from './FlyIndicator'
import { ScenarioReviewControls } from './ScenarioReviewControls'

interface SceneViewportProps {
  /** Scene the dev setup expects; only used in setup instructions. */
  sceneId: string
}

/**
 * The 3D scene viewport. Renders the Rust world's first floor with rooms and
 * furniture, using Matterport's defurnished mesh when it has been imported.
 */
export function SceneViewport({ sceneId }: SceneViewportProps) {
  const { data: world, error, isPending } = useWorld()
  const scene = world?.scene
  const { data: meshAvailable } = useSceneMeshAvailable(scene?.id ?? '')
  const floor = scene?.floors.find((candidate) => candidate.rooms.length > 0)
  const scenePath = `data/prepared-scenes/${sceneId || '<model-id>'}.json`

  return (
    <ViewportFrame>
      {scene && floor ? (
        <SceneViews scene={scene} floor={floor} meshUrl={meshAvailable ? sceneMeshUrl(scene.id) : undefined} />
      ) : isPending ? (
        <ViewportMessage>Loading scene…</ViewportMessage>
      ) : error instanceof WorldServerUnavailableError ? (
        <ViewportMessage>
          The world server isn&apos;t running. From the repo root, run{' '}
          <code className="font-mono">cargo run -p tflop-server -- --scene {scenePath}</code>{' '}
          (import the scene first with <code className="font-mono">cargo run -p import-scene</code>).
        </ViewportMessage>
      ) : error ? (
        <ViewportMessage>{error.message}</ViewportMessage>
      ) : (
        <ViewportMessage>This scene has no floors with rooms.</ViewportMessage>
      )}
    </ViewportFrame>
  )
}

type View = 'editor' | 'textured' | 'furnished'
type PanelMode = 'building' | 'review'

/** Stable empty list, so hiding scanned furniture doesn't rebuild every render. */
const NO_DETECTED: DetectedObject[] = []

const VIEWS: [view: View, label: string][] = [
  ['editor', 'Editor'],
  ['textured', 'Textured'],
  ['furnished', 'Furnished'],
]

/**
 * The editor, and Matterport's textured player beside it, which the Textured
 * and Furnished views share: one shows the defurnished space, the other the
 * space as scanned, from the same spot. The editor stays mounted while hidden
 * so its camera and state survive a switch; the player loads the first time
 * it's shown and stays.
 */
function SceneViews({ scene, floor, meshUrl }: { scene: Scene; floor: Floor; meshUrl?: string }) {
  const [view, setView] = useState<View>('editor')
  const [mode, setMode] = useState<PanelMode>('building')
  const [texturedOpened, setTexturedOpened] = useState(false)
  const batch = useFireStore((state) => state.batch)
  const pause = useFireStore((state) => state.pause)
  const { data: detection } = useDetectedFurniture(scene.id)
  // Where the scene has them now, so moving or deleting a scanned piece in the
  // editor moves or removes its model in the Matterport views too.
  const scanned = useMemo(
    () =>
      detectedInScene(
        detection?.objects.filter((object) => object.floor_id === floor.id) ?? [],
        floor,
      ),
    [detection, floor],
  )
  const [showScanned, setShowScanned] = useState(true)
  const detected = showScanned ? scanned : NO_DETECTED
  const { data: furnitureModels } = useFurnitureModels()
  const { elevation_m, rooms } = floor
  const geometry = useMemo(() => buildFloorGeometry({ elevation_m, rooms }), [elevation_m, rooms])
  const models = useMemo(
    () =>
      placeModels(
        detected,
        furnitureModels ?? [],
        floor.rooms.flatMap((room) => room.boundary),
      ),
    [detected, furnitureModels, floor.rooms],
  )
  // Fire playback runs in whichever view is showing.
  usePlaybackClock()
  const show = (next: View) => {
    if (next !== 'editor') setTexturedOpened(true)
    setView(next)
  }
  const showMode = (next: PanelMode) => {
    pause()
    setMode(next)
  }
  const editorVisible = mode === 'building' && view === 'editor'

  return (
    <>
      <div className={`absolute inset-0 ${editorVisible ? '' : 'invisible'}`}>
        <SceneCanvas
          scene={scene}
          floor={floor}
          meshUrl={meshUrl}
          geometry={geometry}
          enabled={mode === 'building'}
          showSimulation={!batch}
        />
      </div>
      {texturedOpened && (
        <div
          className={`absolute inset-0 ${mode === 'building' && view !== 'editor' ? '' : 'invisible'}`}
        >
          <ShowcaseViewport
            scene={scene}
            floor={floor}
            models={models}
            version={view === 'furnished' ? 'furnished' : 'defurnished'}
          />
        </div>
      )}
      {mode === 'building' && !batch && <FireControls />}
      {mode === 'review' && (
        <ScenarioReviewControls
          scene={scene}
          floor={floor}
          geometry={geometry}
          meshUrl={meshUrl}
        />
      )}
      {/* One top bar: left, center and right slots share a row while there's
          room and wrap onto a second row (instead of overlapping) when narrow. */}
      <div className="pointer-events-none absolute inset-x-3 top-3 z-30 flex flex-wrap items-start gap-2 text-xs font-medium whitespace-nowrap">
        <div className="flex flex-1 basis-0 justify-start">
          <div
            role="tablist"
            aria-label="Panel mode"
            className="pointer-events-auto flex shrink-0 rounded-md border border-hairline bg-white p-0.5 shadow-[0_4px_12px_rgba(26,31,54,0.08)]"
          >
            <PanelModeButton active={mode === 'building'} onClick={() => showMode('building')}>
              Building
            </PanelModeButton>
            <PanelModeButton active={mode === 'review'} onClick={() => showMode('review')}>
              Scenario Review
            </PanelModeButton>
          </div>
        </div>
        {mode === 'building' && (
          <div
            role="radiogroup"
            aria-label="View"
            className="pointer-events-auto flex shrink-0 rounded-md border border-hairline bg-white p-0.5 shadow-[0_4px_12px_rgba(26,31,54,0.08)]"
          >
            {VIEWS.map(([value, label]) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={view === value}
                onClick={() => show(value)}
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
        )}
        {mode === 'building' && (
          <div className="flex flex-1 basis-0 justify-end">
            {/* The editor shows scanned furniture as scene objects (merged by
                the server); this toggle only affects the Matterport views. */}
            {scanned.length > 0 && view !== 'editor' && (
              <button
                type="button"
                aria-pressed={showScanned}
                onClick={() => setShowScanned((shown) => !shown)}
                className="pointer-events-auto shrink-0 rounded-md border border-hairline bg-white px-2.5 py-1 text-ink-secondary shadow-[0_4px_12px_rgba(26,31,54,0.08)] hover:text-ink aria-pressed:text-ink"
              >
                {showScanned ? 'Hide' : 'Show'} scanned furniture ({scanned.length})
              </button>
            )}
          </div>
        )}
      </div>
    </>
  )
}

/**
 * The canvas plus its controls. Clicking captures the pointer for flying
 * around; Esc gives it back.
 */
function SceneCanvas({
  scene,
  floor,
  meshUrl,
  geometry,
  enabled,
  showSimulation,
}: {
  scene: Scene
  floor: Floor
  meshUrl?: string
  geometry: ReturnType<typeof buildFloorGeometry>
  enabled: boolean
  showSimulation: boolean
}) {
  const focusTarget = useRef<HTMLDivElement>(null)
  const [active, setActive] = useState(false)
  return (
    <div
      ref={focusTarget}
      tabIndex={-1}
      aria-label="3D view. Click furniture to choose the fire origin; right-click to fly, Escape to stop."
      className="absolute inset-0 rounded-lg outline-none"
    >
      <Canvas frameloop="demand" dpr={[1, 2]} camera={{ fov: 50 }}>
        <hemisphereLight args={['#ffffff', '#d5dbe1', 1.6]} />
        <directionalLight position={[8, 14, 6]} intensity={1.4} />
        <SceneModel geometry={geometry} meshUrl={meshUrl} />
        <Furniture floor={floor} assets={scene.assets} />
        {showSimulation && <SurfaceFire floor={floor} />}
        {showSimulation && <SimulationAgents floor={floor} />}
        <CameraController
          element={focusTarget}
          geometry={geometry}
          onActiveChange={setActive}
          enabled={enabled}
        />
        {enabled && <SelectionController element={focusTarget} />}
      </Canvas>
      {enabled && active && <FlyIndicator />}
      {enabled && (
        <ControlsHint
          title={active ? 'Flying' : 'Editing'}
          controls={active ? FLY_CONTROLS : EDIT_CONTROLS}
        />
      )}
    </div>
  )
}

function PanelModeButton({
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
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={
        active
          ? 'rounded bg-ink px-3 py-1 text-white'
          : 'rounded px-3 py-1 text-ink-secondary hover:text-ink'
      }
    >
      {children}
    </button>
  )
}

const EDIT_CONTROLS: Controls = [
  [['Click'], 'Select fire origin'],
  [['Esc'], 'Deselect'],
  [['Pinch'], 'Move to cursor'],
  [['Swipe'], 'Look around'],
  [['⌥', 'Drag'], 'Orbit'],
  [['F'], 'Reset view'],
  [['Right-click'], 'Fly'],
]

const FLY_CONTROLS: Controls = [
  [['Move'], 'Look around'],
  [['W', 'A', 'S', 'D'], 'Fly'],
  [['Q', 'E'], 'Down / up'],
  [['Shift'], 'Faster'],
  [['Pinch'], 'Move to center'],
  [['Space', 'Move'], 'Pan'],
  [['⌥', 'Move'], 'Orbit'],
  [['F'], 'Reset view'],
  [['Esc'], 'Back to editing'],
]

function ViewportFrame({ children }: { children: ReactNode }) {
  return (
    <section
      aria-label="3D scene"
      className="relative min-w-0 flex-1 overflow-hidden rounded-lg border border-hairline bg-white"
    >
      {children}
    </section>
  )
}

function ViewportMessage({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <p className="max-w-md text-center leading-normal text-muted">{children}</p>
    </div>
  )
}
