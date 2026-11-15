import { useQuery } from '@tanstack/react-query'
import {
  DETECTION_SCHEMA_VERSION,
  SCENE_SCHEMA_VERSION,
  type Detection,
  type FurnitureModel,
  type GetSceneResponse,
  type WorldApiErrorBody,
} from './types'

export const WORLD_QUERY_KEY = ['world'] as const

/** The Rust world server (`cargo run -p tflop-server`) isn't reachable. */
export class WorldServerUnavailableError extends Error {
  constructor() {
    super('The world server is not running')
  }
}

/** A world API call failed with a structured `{ code, ... }` error. */
export class WorldApiError extends Error {
  readonly status: number
  readonly body: WorldApiErrorBody

  constructor(status: number, body: WorldApiErrorBody) {
    super(`World API error ${status}: ${body.code}`)
    this.status = status
    this.body = body
  }
}

export async function fetchWorld(): Promise<GetSceneResponse> {
  const response = await fetch('/api/world/scene').catch(() => null)
  // The Vite proxy answers 5xx when the Rust server is down.
  if (!response || response.status >= 500) throw new WorldServerUnavailableError()
  if (!response.ok) throw new Error(`Loading the scene failed (${response.status})`)

  const world = (await response.json()) as GetSceneResponse
  if (world.scene.schema_version !== SCENE_SCHEMA_VERSION) {
    throw new Error(
      `Scene ${world.scene.id} uses schema v${world.scene.schema_version}; this app reads v${SCENE_SCHEMA_VERSION}`,
    )
  }
  return world
}

/** The authoritative scene and its revision, from the Rust world. */
export function useWorld() {
  return useQuery({
    queryKey: WORLD_QUERY_KEY,
    queryFn: fetchWorld,
    retry: (count, error) => !(error instanceof WorldServerUnavailableError) && count < 2,
    staleTime: Infinity,
  })
}

/**
 * Furniture detected in the scan, or null if `detect-furniture` hasn't been
 * run for this scene.
 */
export function useDetectedFurniture(sceneId: string) {
  return useQuery({
    queryKey: ['detected-furniture', sceneId],
    queryFn: async (): Promise<Detection | null> => {
      const response = await fetch(`/api/scenes/${encodeURIComponent(sceneId)}/detected.json`)
      if (response.status === 404) return null
      if (!response.ok) throw new Error(`Loading detected furniture failed (${response.status})`)
      const detection = (await response.json()) as Detection
      if (detection.schema_version !== DETECTION_SCHEMA_VERSION) {
        throw new Error(
          `Detected furniture uses schema v${detection.schema_version}; this app reads v${DETECTION_SCHEMA_VERSION}`,
        )
      }
      return detection
    },
    staleTime: Infinity,
    enabled: sceneId !== '',
  })
}

/**
 * The normalized furniture models (`pnpm --filter web normalize-furniture`),
 * or none if they haven't been prepared. The model files aren't in Git.
 */
export function useFurnitureModels() {
  return useQuery({
    queryKey: ['furniture-models'],
    queryFn: async (): Promise<FurnitureModel[]> => {
      const response = await fetch('/models/furniture/index.json')
      // Vite answers a missing file with its HTML page, not a 404.
      if (!response.ok || !response.headers.get('content-type')?.includes('json')) return []
      const index = (await response.json()) as { models: FurnitureModel[] }
      return index.models
    },
    staleTime: Infinity,
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
