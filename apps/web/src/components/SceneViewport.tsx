import { Canvas } from '@react-three/fiber'
import type { ReactNode } from 'react'
import { SceneNotFoundError, sceneMeshUrl, useScene, useSceneMeshAvailable } from '../scene/api'
import { SceneModel } from '../scene/SceneModel'

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
        <Canvas frameloop="demand" dpr={[1, 2]} camera={{ fov: 40 }}>
          <hemisphereLight args={['#ffffff', '#d5dbe1', 1.6]} />
          <directionalLight position={[8, 14, 6]} intensity={1.4} />
          <SceneModel floor={floor} meshUrl={meshAvailable ? sceneMeshUrl(sceneId) : undefined} />
        </Canvas>
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
