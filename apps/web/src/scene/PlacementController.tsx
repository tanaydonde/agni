import { useThree } from '@react-three/fiber'
import { useEffect, useRef } from 'react'
import * as THREE from 'three'
import { useWorldEdit } from './api'
import { useEditorStore } from './editorStore'
import { allObjectIds, assetById, floorZ, nextObjectId, rayFloorHit } from './placement'
import type { Floor, Scene } from './types'

interface PlacementControllerProps {
  scene: Scene
  floor: Floor
}

/**
 * Drives a drag from the asset palette into the view. While an asset is held,
 * the preview follows the floor under the cursor; releasing over the view
 * adds the object there and selects it. Releasing elsewhere, or Esc, cancels.
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

    /** The floor point under the cursor, or null when it's off the view or the floor. */
    const floorPointAt = (event: PointerEvent) => {
      const { camera, gl } = get()
      const rect = gl.domElement.getBoundingClientRect()
      const inside =
        event.clientX >= rect.left &&
        event.clientX <= rect.right &&
        event.clientY >= rect.top &&
        event.clientY <= rect.bottom
      if (!inside) return null
      ndc.set(
        ((event.clientX - rect.left) / rect.width) * 2 - 1,
        -((event.clientY - rect.top) / rect.height) * 2 + 1,
      )
      raycaster.setFromCamera(ndc, camera)
      return rayFloorHit(raycaster.ray, floorZ(latest.current.floor))
    }

    const onPointerMove = (event: PointerEvent) => {
      store().setPlacingPosition(floorPointAt(event))
      get().invalidate()
    }

    const onPointerUp = (event: PointerEvent) => {
      const position = floorPointAt(event)
      const { scene: currentScene, floor: currentFloor, edit: send } = latest.current
      const asset = assetById(currentScene, placingAssetId)
      if (position && asset) {
        const objectId = nextObjectId(asset.category, allObjectIds(currentScene))
        send({
          type: 'add_object',
          object_id: objectId,
          asset_id: asset.id,
          floor_id: currentFloor.id,
          position_m: position,
          rotation_z_degrees: 0,
          mobility: 'movable',
        })
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
      document.body.style.cursor = previousCursor
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', onPointerUp)
      window.removeEventListener('keydown', onKeyDown)
    }
  }, [placingAssetId, get])

  return null
}
