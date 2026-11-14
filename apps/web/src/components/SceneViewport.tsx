import { Canvas } from '@react-three/fiber'
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { WorldServerUnavailableError, sceneMeshUrl, useSceneMeshAvailable, useWorld } from '../scene/api'
import { CameraController } from '../scene/CameraController'
import { useEditorStore } from '../scene/editorStore'
import { Furniture } from '../scene/Furniture'
import { buildFloorGeometry } from '../scene/geometry'
import { PlacementController } from '../scene/PlacementController'
import { SceneModel } from '../scene/SceneModel'
import { SelectionController } from '../scene/SelectionController'
import { TransformGizmo } from '../scene/TransformGizmo'
import type { Floor, Scene } from '../scene/types'

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
        <SceneCanvas
          scene={scene}
          floor={floor}
          meshUrl={meshAvailable ? sceneMeshUrl(scene.id) : undefined}
        />
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

/**
 * The canvas plus its controls. Clicking captures the pointer for flying
 * around; Esc gives it back.
 */
function SceneCanvas({ scene, floor, meshUrl }: { scene: Scene; floor: Floor; meshUrl?: string }) {
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
        <Furniture floor={floor} assets={scene.assets} />
        <TransformGizmo floor={floor} />
        <CameraController element={focusTarget} geometry={geometry} onActiveChange={setActive} />
        <SelectionController element={focusTarget} floor={floor} />
        <PlacementController scene={scene} floor={floor} />
      </Canvas>
      {active && (
        <>
          <div
            aria-hidden
            className="pointer-events-none absolute inset-0 rounded-lg ring-2 ring-inset ring-accent"
          />
          <Crosshair />
        </>
      )}
      <PlacementStatus />
      <ControlsHint active={active} />
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
      className="pointer-events-none absolute top-3 left-1/2 -translate-x-1/2 rounded-md border border-danger/30 bg-white px-3 py-1.5 text-xs font-medium text-danger shadow-[0_4px_12px_rgba(26,31,54,0.08)]"
    >
      {text}
    </div>
  )
}

/** Marks the middle of the view in fly mode, which pinch and orbit aim at. */
function Crosshair() {
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute top-1/2 left-1/2 size-1.5 -translate-1/2 rounded-full bg-white ring-1 ring-ink/60"
    />
  )
}

type Controls = [keys: string[], action: string][]

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

function ControlsHint({ active }: { active: boolean }) {
  const controls = active ? FLY_CONTROLS : EDIT_CONTROLS
  return (
    <div className="pointer-events-none absolute bottom-3 left-3 flex flex-col gap-2 rounded-lg border border-hairline bg-white px-3 py-2.5 text-xs text-ink-secondary shadow-[0_4px_12px_rgba(26,31,54,0.08)]">
      <span className="font-medium text-ink">{active ? 'Flying' : 'Editing'}</span>
      <div className="grid grid-cols-[auto_auto] items-center gap-x-3 gap-y-1.5">
        {controls.map(([keys, action]) => (
          <div key={action} className="contents">
            <span className="flex gap-1">
              {keys.map((key) => (
                <kbd
                  key={key}
                  className="rounded border border-control-border bg-canvas px-1.5 font-mono text-[11px] leading-[18px] text-ink"
                >
                  {key}
                </kbd>
              ))}
            </span>
            <span>{action}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

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
