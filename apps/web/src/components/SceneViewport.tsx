import { Canvas } from '@react-three/fiber'
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import {
  WorldServerUnavailableError,
  sceneMeshUrl,
  useDetectedFurniture,
  useSceneMeshAvailable,
  useWorld,
} from '../scene/api'
import { CameraController } from '../scene/CameraController'
import { DetectedFurniture } from '../scene/DetectedFurniture'
import { useEditorStore } from '../scene/editorStore'
import { Furniture } from '../scene/Furniture'
import { buildFloorGeometry } from '../scene/geometry'
import { PlacementController } from '../scene/PlacementController'
import { SceneModel } from '../scene/SceneModel'
import { SelectionController } from '../scene/SelectionController'
import { TransformGizmo } from '../scene/TransformGizmo'
import type { DetectedObject, Floor, Scene } from '../scene/types'
import { ShowcaseViewport } from '../showcase/ShowcaseViewport'
import { ControlsHint, type Controls } from './ControlsHint'
import { FlyIndicator } from './FlyIndicator'

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
  const [texturedOpened, setTexturedOpened] = useState(false)
  const { data: detection } = useDetectedFurniture(scene.id)
  const scanned = useMemo(
    () => detection?.objects.filter((object) => object.floor_id === floor.id) ?? [],
    [detection, floor.id],
  )
  const [showScanned, setShowScanned] = useState(true)
  const detected = showScanned ? scanned : NO_DETECTED
  const show = (next: View) => {
    if (next !== 'editor') setTexturedOpened(true)
    setView(next)
  }

  return (
    <>
      <div className={`absolute inset-0 ${view === 'editor' ? '' : 'invisible'}`}>
        <SceneCanvas scene={scene} floor={floor} meshUrl={meshUrl} detected={detected} />
      </div>
      {texturedOpened && (
        <div className={`absolute inset-0 ${view === 'editor' ? 'invisible' : ''}`}>
          <ShowcaseViewport
            scene={scene}
            floor={floor}
            detected={detected}
            version={view === 'furnished' ? 'furnished' : 'defurnished'}
          />
        </div>
      )}
      <div
        role="radiogroup"
        aria-label="View"
        className="absolute top-3 left-1/2 flex -translate-x-1/2 rounded-md border border-hairline bg-white p-0.5 text-xs font-medium shadow-[0_4px_12px_rgba(26,31,54,0.08)]"
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
      {scanned.length > 0 && (
        <button
          type="button"
          aria-pressed={showScanned}
          onClick={() => setShowScanned((shown) => !shown)}
          className="absolute top-3 right-3 rounded-md border border-hairline bg-white px-2.5 py-1 text-xs font-medium text-ink-secondary shadow-[0_4px_12px_rgba(26,31,54,0.08)] hover:text-ink aria-pressed:text-ink"
        >
          {showScanned ? 'Hide' : 'Show'} scanned furniture ({scanned.length})
        </button>
      )}
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
  detected,
}: {
  scene: Scene
  floor: Floor
  meshUrl?: string
  detected: DetectedObject[]
}) {
  const focusTarget = useRef<HTMLDivElement>(null)
  const [active, setActive] = useState(false)
  // Depend on the room data only: furniture edits replace `floor` but must
  // not rebuild the room or reset the camera.
  const { elevation_m, rooms } = floor
  const geometry = useMemo(() => buildFloorGeometry({ elevation_m, rooms }), [elevation_m, rooms])

  return (
    <div
      ref={focusTarget}
      tabIndex={-1}
      aria-label="3D view. Click to select furniture; right-click to fly, Escape to stop."
      className="absolute inset-0 rounded-lg outline-none"
    >
      <Canvas frameloop="demand" dpr={[1, 2]} camera={{ fov: 50 }}>
        <hemisphereLight args={['#ffffff', '#d5dbe1', 1.6]} />
        <directionalLight position={[8, 14, 6]} intensity={1.4} />
        <SceneModel geometry={geometry} meshUrl={meshUrl} />
        <DetectedFurniture objects={detected} assets={scene.assets} />
        <Furniture floor={floor} assets={scene.assets} />
        <TransformGizmo floor={floor} />
        <CameraController element={focusTarget} geometry={geometry} onActiveChange={setActive} />
        <SelectionController element={focusTarget} floor={floor} />
        <PlacementController scene={scene} floor={floor} />
      </Canvas>
      {active && <FlyIndicator />}
      <PlacementStatus />
      <ControlsHint
        title={active ? 'Flying' : 'Editing'}
        controls={active ? FLY_CONTROLS : EDIT_CONTROLS}
      />
    </div>
  )
}

const NOTICE_MS = 3500

/**
 * Says why the furniture being dragged can't go where it is (live, from the
 * world's dry run), or why the last edit was rejected (briefly).
 */
function PlacementStatus() {
  const liveIssue = useEditorStore((state) =>
    (state.drag || state.placing) && state.liveCheck && !state.liveCheck.valid
      ? (state.liveCheck.message ?? "Can't go here")
      : null,
  )
  const notice = useEditorStore((state) => state.notice)
  const dismissNotice = useEditorStore((state) => state.dismissNotice)

  useEffect(() => {
    if (!notice) return
    const timer = setTimeout(() => dismissNotice(notice.id), NOTICE_MS)
    return () => clearTimeout(timer)
  }, [notice, dismissNotice])

  const text = liveIssue ?? notice?.text
  if (!text) return null
  return (
    <div
      role="status"
      className="pointer-events-none absolute top-14 left-1/2 -translate-x-1/2 rounded-md border border-danger/30 bg-white px-3 py-1.5 text-xs font-medium text-danger shadow-[0_4px_12px_rgba(26,31,54,0.08)]"
    >
      {text}
    </div>
  )
}

const EDIT_CONTROLS: Controls = [
  [['Click'], 'Select'],
  [['Drag', 'Arrow'], 'Move along X / Y'],
  [['Drag', 'Square'], 'Move freely'],
  [['Drag', 'Ring'], 'Rotate (⇧ snaps 15°)'],
  [['Del'], 'Delete selected'],
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
