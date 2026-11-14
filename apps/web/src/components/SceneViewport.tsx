import { Canvas } from '@react-three/fiber'
import { useMemo, useRef, useState, type ReactNode } from 'react'
import { WorldServerUnavailableError, sceneMeshUrl, useSceneMeshAvailable, useWorld } from '../scene/api'
import { CameraController } from '../scene/CameraController'
import { useEditorStore } from '../scene/editorStore'
import { Furniture } from '../scene/Furniture'
import { buildFloorGeometry } from '../scene/geometry'
import { MoveGizmo } from '../scene/MoveGizmo'
import { PlacementController } from '../scene/PlacementController'
import { SceneModel } from '../scene/SceneModel'
import { SelectionController } from '../scene/SelectionController'
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
      aria-label="3D view. Click to explore; press Escape to stop."
      className={`absolute inset-0 rounded-lg outline-none ${active ? '' : 'cursor-pointer'}`}
    >
      <Canvas frameloop="demand" dpr={[1, 2]} camera={{ fov: 50 }}>
        <hemisphereLight args={['#ffffff', '#d5dbe1', 1.6]} />
        <directionalLight position={[8, 14, 6]} intensity={1.4} />
        <SceneModel geometry={geometry} meshUrl={meshUrl} />
        <Furniture floor={floor} assets={scene.assets} />
        <MoveGizmo floor={floor} />
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
      <ControlsHint active={active} />
    </div>
  )
}

/**
 * Marks the middle of the view, which clicks, pinch, and orbit aim at. During
 * a gizmo drag it follows the virtual cursor instead.
 */
function Crosshair() {
  const offset = useEditorStore((state) => state.cursorOffset)
  return (
    <div
      aria-hidden
      style={{ left: `calc(50% + ${offset.x}px)`, top: `calc(50% + ${offset.y}px)` }}
      className="pointer-events-none absolute size-1.5 -translate-1/2 rounded-full bg-white ring-1 ring-ink/60"
    />
  )
}

const CONTROLS: [keys: string[], action: string][] = [
  [['Move'], 'Look around'],
  [['W', 'A', 'S', 'D'], 'Fly'],
  [['Q', 'E'], 'Down / up'],
  [['Shift'], 'Faster'],
  [['Pinch'], 'Move to center'],
  [['Space', 'Move'], 'Pan'],
  [['⌥', 'Move'], 'Orbit'],
  [['F'], 'Reset view'],
  [['Click'], 'Select furniture'],
  [['Drag', 'Arrow'], 'Move along floor'],
  [['Del'], 'Delete selected'],
  [['Esc'], 'Release cursor'],
]

function ControlsHint({ active }: { active: boolean }) {
  if (!active) {
    return (
      <div className="pointer-events-none absolute bottom-3 left-3 rounded-md border border-hairline bg-white px-2.5 py-1.5 text-xs text-ink-secondary">
        Click to explore · Esc to release · Drag furniture in from the panel
      </div>
    )
  }
  return (
    <div className="pointer-events-none absolute bottom-3 left-3 grid grid-cols-[auto_auto] items-center gap-x-3 gap-y-1.5 rounded-lg border border-hairline bg-white px-3 py-2.5 text-xs text-ink-secondary shadow-[0_4px_12px_rgba(26,31,54,0.08)]">
      {CONTROLS.map(([keys, action]) => (
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
