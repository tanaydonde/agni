import { useQuery } from '@tanstack/react-query'
import { SCENE_SCHEMA_VERSION, type Scene } from './types'

export class SceneNotFoundError extends Error {
  readonly sceneId: string

  constructor(sceneId: string) {
    super(`No prepared scene for ${sceneId}`)
    this.sceneId = sceneId
  }
}

export async function fetchScene(sceneId: string): Promise<Scene> {
  const response = await fetch(`/api/scenes/${encodeURIComponent(sceneId)}`)
  if (response.status === 404) throw new SceneNotFoundError(sceneId)
  if (!response.ok) throw new Error(`Loading scene ${sceneId} failed (${response.status})`)

  const scene = (await response.json()) as Scene
  if (scene.schema_version !== SCENE_SCHEMA_VERSION) {
    throw new Error(
      `Scene ${sceneId} uses schema v${scene.schema_version}; this app reads v${SCENE_SCHEMA_VERSION}`,
    )
  }
  return scene
}

export function useScene(sceneId: string) {
  return useQuery({
    queryKey: ['scene', sceneId],
    queryFn: () => fetchScene(sceneId),
    retry: (count, error) => !(error instanceof SceneNotFoundError) && count < 2,
    staleTime: Infinity,
    enabled: sceneId !== '',
  })
}

export function sceneMeshUrl(sceneId: string): string {
  return `/api/scenes/${encodeURIComponent(sceneId)}/mesh.obj`
}

/** Whether the scene's defurnished mesh has been imported. */
export function useSceneMeshAvailable(sceneId: string) {
  return useQuery({
    queryKey: ['scene-mesh', sceneId],
    queryFn: async () => (await fetch(sceneMeshUrl(sceneId), { method: 'HEAD' })).ok,
    staleTime: Infinity,
    enabled: sceneId !== '',
  })
}
