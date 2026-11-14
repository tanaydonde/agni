import { Canvas } from '@react-three/fiber'
import { useMemo, useRef, useState, type ReactNode } from 'react'
import { SceneNotFoundError, sceneMeshUrl, useScene, useSceneMeshAvailable } from '../scene/api'
import { CameraController } from '../scene/CameraController'
import { buildFloorGeometry } from '../scene/geometry'
import { SceneModel } from '../scene/SceneModel'
import type { Floor } from '../scene/types'

interface SceneViewportProps {
  sceneId: string
}

/**
 * The 3D scene viewport. Renders the prepared scene's first floor with rooms,
 * using Matterport's defurnished mesh when it has been imported.
 */
export function SceneViewport({ sceneId }: SceneViewportProps) {
  const { data: scene, error, isPending } = useScene(sceneId)
  const { data: meshAvailable } = useSceneMeshAvailable(sceneId)

  if (!sceneId) {
    return (
      <ViewportFrame>
        <ViewportMessage>
          No scene selected. Set <code className="font-mono">MATTERPORT_MODEL_ID</code> in the
          repo-root <code className="font-mono">.env</code>.
        </ViewportMessage>
      </ViewportFrame>
    )
  }

  const floor = scene?.floors.find((candidate) => candidate.rooms.length > 0)

  return (
    <ViewportFrame>
      {floor ? (
        <SceneCanvas floor={floor} meshUrl={meshAvailable ? sceneMeshUrl(sceneId) : undefined} />
      ) : isPending ? (
        <ViewportMessage>Loading scene…</ViewportMessage>
      ) : error instanceof SceneNotFoundError ? (
        <ViewportMessage>
          No prepared scene for <code className="font-mono">{sceneId}</code>. From the repo root, run{' '}
          <code className="font-mono">cargo run -p import-scene</code>
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
function SceneCanvas({ floor, meshUrl }: { floor: Floor; meshUrl?: string }) {
  const focusTarget = useRef<HTMLDivElement>(null)
  const [active, setActive] = useState(false)
  const geometry = useMemo(() => buildFloorGeometry(floor), [floor])

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
        <CameraController element={focusTarget} geometry={geometry} onActiveChange={setActive} />
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

/** Marks the middle of the view, which pinch and orbit aim at. */
function Crosshair() {
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute top-1/2 left-1/2 size-1.5 -translate-1/2 rounded-full bg-white ring-1 ring-ink/60"
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
  [['Esc'], 'Release cursor'],
]

function ControlsHint({ active }: { active: boolean }) {
  if (!active) {
    return (
      <div className="pointer-events-none absolute bottom-3 left-3 rounded-md border border-hairline bg-white px-2.5 py-1.5 text-xs text-ink-secondary">
        Click to explore · Esc to release
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
