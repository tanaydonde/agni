import { useThree } from '@react-three/fiber'
import { useEffect, useRef } from 'react'
import * as THREE from 'three'
import { checkEdit, useWorldEdit } from './api'
import { useEditorStore } from './editorStore'
import { createLiveChecker, editKey } from './liveCheck'
import { allObjectIds, assetById, floorZ, ndcFromClient, nextObjectId, rayFloorHit } from './placement'
import type { EditCommand, Floor, Scene, Vec3 } from './types'
import { describeWorldError } from './worldErrors'

/** Object ID used only to dry-run the preview; never added to the world. */
const PREVIEW_OBJECT_ID = '__placement_preview__'

interface PlacementControllerProps {
  scene: Scene
  floor: Floor
}

/**
 * Drives a drag from the asset palette into the view (edit mode only). While
 * an asset is held, the preview follows the floor under the cursor and the
 * world dry-runs adding it there, so the preview turns red where it can't go.
 * Releasing over a valid spot adds the object and selects it; releasing on an
 * invalid spot, outside the view, or pressing Esc cancels.
 */
export function PlacementController({ scene, floor }: PlacementControllerProps) {
  const get = useThree((state) => state.get)
  const edit = useWorldEdit()
  const placingAssetId = useEditorStore((state) => state.placing?.assetId)
  const latest = useRef({ scene, floor, edit })
  useEffect(() => {
    latest.current = { scene, floor, edit }
  }, [scene, floor, edit])

  useEffect(() => {
    if (!placingAssetId) return
    const raycaster = new THREE.Raycaster()
    const ndc = new THREE.Vector2()
    const store = useEditorStore.getState
    const checker = createLiveChecker(checkEdit, (key, response) => {
      store().setLiveCheck({
        key,
        valid: response.valid,
        message: response.error ? describeWorldError(response.error) : null,
      })
      get().invalidate()
    })

    const addEdit = (objectId: string, position: Vec3): EditCommand => ({
      type: 'add_object',
      object_id: objectId,
      asset_id: placingAssetId,
      floor_id: latest.current.floor.id,
      position_m: position,
      rotation_z_degrees: 0,
      mobility: 'movable',
    })

    /** The floor point under the cursor, or null when it's off the view or the floor. */
    const floorPointAt = (event: PointerEvent) => {
      const { camera, gl } = get()
      const rect = gl.domElement.getBoundingClientRect()
      const inside =
        event.clientX >= rect.left &&
        event.clientX <= rect.right &&
        event.clientY >= rect.top &&
        event.clientY <= rect.bottom
      // Placing is an edit-mode action; fly mode hides the cursor anyway.
      if (!inside || document.pointerLockElement) return null
      raycaster.setFromCamera(ndcFromClient(event.clientX, event.clientY, rect, ndc), camera)
      return rayFloorHit(raycaster.ray, floorZ(latest.current.floor))
    }

    const onPointerMove = (event: PointerEvent) => {
      const position = floorPointAt(event)
      store().setPlacingPosition(position)
      if (position) {
        checker.request(addEdit(PREVIEW_OBJECT_ID, position))
      } else {
        checker.cancel()
        store().setLiveCheck(null)
      }
      get().invalidate()
    }

    const onPointerUp = (event: PointerEvent) => {
      checker.cancel()
      const position = floorPointAt(event)
      const { scene: currentScene, edit: send } = latest.current
      const asset = assetById(currentScene, placingAssetId)
      const { liveCheck, showNotice } = store()
      const knownInvalid =
        position && liveCheck && !liveCheck.valid && liveCheck.key === editKey(addEdit(PREVIEW_OBJECT_ID, position))
      if (position && asset && knownInvalid) {
        showNotice(liveCheck.message ?? "Can't place it there")
        store().stopPlacing()
      } else if (position && asset) {
        const objectId = nextObjectId(asset.category, allObjectIds(currentScene))
        send(addEdit(objectId, position))
        store().select(objectId)
        // Keep the preview until the new object has rendered in its place.
        setTimeout(() => store().stopPlacing(), 0)
      } else {
        store().stopPlacing()
      }
      get().invalidate()
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') store().stopPlacing()
    }

    const previousCursor = document.body.style.cursor
    document.body.style.cursor = 'grabbing'
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
    window.addEventListener('keydown', onKeyDown)
    return () => {
      checker.cancel()
      document.body.style.cursor = previousCursor
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', onPointerUp)
      window.removeEventListener('keydown', onKeyDown)
    }
  }, [placingAssetId, get])

  return null
}
