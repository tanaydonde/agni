import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback } from 'react'
import { useEditorStore } from './editorStore'
import { applyEditLocally } from './placement'
import { describeWorldError } from './worldErrors'
import {
  DETECTION_SCHEMA_VERSION,
  SCENE_SCHEMA_VERSION,
  type ApplyEditsRequest,
  type Detection,
  type FurnitureModel,
  type CheckEditsRequest,
  type CheckEditsResponse,
  type EditCommand,
  type EditResponse,
  type GetSceneResponse,
  type WorldApiErrorBody,
} from './types'

export const WORLD_QUERY_KEY = ['world'] as const
const WORLD_EDIT_KEY = ['world-edit'] as const

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

async function postEdit(request: ApplyEditsRequest): Promise<EditResponse> {
  const response = await fetch('/api/world/apply_edits', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  }).catch(() => null)
  if (!response) throw new WorldServerUnavailableError()
  const body = await response.json().catch(() => null)
  if (!response.ok) {
    throw new WorldApiError(response.status, body ?? { code: 'unknown' })
  }
  return body as EditResponse
}

/** Dry-runs one edit: would the world accept it right now? Changes nothing. */
export async function checkEdit(edit: EditCommand): Promise<CheckEditsResponse> {
  const request: CheckEditsRequest = { edits: [edit] }
  const response = await fetch('/api/world/check_edits', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  })
  if (!response.ok) throw new Error(`Checking the edit failed (${response.status})`)
  return (await response.json()) as CheckEditsResponse
}

/**
 * Returns a function that sends one edit to the world. The cached scene is
 * patched synchronously, so furniture released from a drag doesn't jump back
 * while the request is in flight; the server's scene (which snaps furniture
 * to the floor) replaces it afterwards. Edits run one at a time so each
 * carries the revision its predecessor produced.
 */
export function useWorldEdit(): (edit: EditCommand) => void {
  const queryClient = useQueryClient()

  const { mutate } = useMutation({
    mutationKey: WORLD_EDIT_KEY,
    scope: { id: 'world-edit' },
    mutationFn: async (edit: EditCommand) => {
      const current = queryClient.getQueryData<GetSceneResponse>(WORLD_QUERY_KEY)
      if (!current) throw new Error('The scene has not loaded yet')
      return postEdit({ expected_revision: current.revision, edits: [edit] })
    },
    onSuccess: (response) => {
      queryClient.setQueryData<GetSceneResponse>(WORLD_QUERY_KEY, (world) =>
        world && { ...world, revision: response.revision },
      )
    },
    onError: (error) => {
      // Placement rules, or stale_revision (someone else, e.g. the agent,
      // edited first): the refetch below drops the optimistic change.
      useEditorStore
        .getState()
        .showNotice(
          error instanceof WorldApiError
            ? describeWorldError(error.body)
            : error instanceof WorldServerUnavailableError
              ? "Couldn't reach the world server"
              : 'The edit failed',
        )
    },
    onSettled: () => {
      // Refetch once the last queued edit finishes, so later optimistic edits
      // aren't briefly overwritten by an older server scene.
      if (queryClient.isMutating({ mutationKey: WORLD_EDIT_KEY }) <= 1) {
        return queryClient.invalidateQueries({ queryKey: WORLD_QUERY_KEY })
      }
    },
  })

  return useCallback(
    (edit: EditCommand) => {
      void queryClient.cancelQueries({ queryKey: WORLD_QUERY_KEY })
      queryClient.setQueryData<GetSceneResponse>(WORLD_QUERY_KEY, (world) =>
        world && { ...world, scene: applyEditLocally(world.scene, edit) },
      )
      mutate(edit)
    },
    [mutate, queryClient],
  )
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
