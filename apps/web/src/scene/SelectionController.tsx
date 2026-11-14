import { useThree } from '@react-three/fiber'
import { useEffect, useRef, type RefObject } from 'react'
import * as THREE from 'three'
import { useWorldEdit } from './api'
import { useEditorStore } from './editorStore'
import { constrainedMove, floorZ, rayFloorHit, type MoveHandle } from './placement'
import type { Floor } from './types'

interface SelectionControllerProps {
  /** The focusable element that holds the pointer lock (see CameraController). */
  element: RefObject<HTMLElement | null>
  floor: Floor
}

/**
 * Crosshair editing while the pointer is locked: click to select the
 * furniture under the crosshair (or clear the selection), press on a gizmo
 * handle and move to drag the object across the floor, and press Delete or
 * Backspace to remove it.
 *
 * The cursor is hidden while locked, so a drag moves a virtual cursor (which
 * the crosshair follows) and casts rays from it. Positions come from where
 * that ray meets the floor plane, so the object can't leave the floor.
 */
export function SelectionController({ element, floor }: SelectionControllerProps) {
  const get = useThree((state) => state.get)
  const edit = useWorldEdit()
  const floorRef = useRef(floor)
  const editRef = useRef(edit)
  useEffect(() => {
    floorRef.current = floor
    editRef.current = edit
  }, [floor, edit])

  useEffect(() => {
    const el = element.current
    if (!el) return
    const raycaster = new THREE.Raycaster()
    const ndc = new THREE.Vector2()
    const isLocked = () => document.pointerLockElement === el
    const store = useEditorStore.getState

    /** Points the raycaster through a spot `offset` pixels from the view center. */
    const castFrom = (offset: { x: number; y: number }) => {
      const { camera, size } = get()
      ndc.set(offset.x / (size.width / 2), -offset.y / (size.height / 2))
      raycaster.setFromCamera(ndc, camera)
      return raycaster
    }

    const endDrag = (commit: boolean) => {
      const { drag, dragPosition } = store()
      if (!drag) return
      const moved =
        dragPosition && (dragPosition.x !== drag.start.x || dragPosition.y !== drag.start.y)
      if (commit && moved) {
        editRef.current({ type: 'move_object', object_id: drag.objectId, position_m: dragPosition })
        // Let the patched scene render before dropping the drag preview, so
        // the object doesn't flash back to its old spot for a frame.
        setTimeout(() => store().endDrag(), 0)
      } else {
        store().endDrag()
      }
      get().invalidate()
    }

    const onPointerDown = (event: PointerEvent) => {
      if (!isLocked() || event.button !== 0) return
      const { scene } = get()
      const hits = castFrom({ x: 0, y: 0 }).intersectObjects(scene.children, true)
      const { selectedObjectId, select, startDrag } = store()

      // Handles draw on top of everything, so they win even when behind furniture.
      const handleHit = selectedObjectId && hits.find((hit) => hit.object.userData.handle)
      if (handleHit) {
        const current = floorRef.current
        const object = current.objects.find((candidate) => candidate.id === selectedObjectId)
        const startHit = rayFloorHit(raycaster.ray, floorZ(current))
        if (object && startHit) {
          startDrag({
            objectId: object.id,
            handle: handleHit.object.userData.handle as MoveHandle,
            start: object.transform.position_m,
            startHit,
          })
          get().invalidate()
        }
        return
      }

      // Otherwise select whatever solid surface is first under the crosshair:
      // furniture selects it; a wall or the floor clears the selection.
      const first = hits.find((hit) => hit.object instanceof THREE.Mesh)
      const objectId = first?.object.userData.objectId
      select(typeof objectId === 'string' ? objectId : null)
    }

    const onPointerMove = (event: PointerEvent) => {
      const { drag, cursorOffset, updateDrag } = store()
      if (!drag || !isLocked()) return
      const { size, invalidate } = get()
      const offset = {
        x: THREE.MathUtils.clamp(cursorOffset.x + event.movementX, -size.width / 2, size.width / 2),
        y: THREE.MathUtils.clamp(cursorOffset.y + event.movementY, -size.height / 2, size.height / 2),
      }
      const z = floorZ(floorRef.current)
      const hit = rayFloorHit(castFrom(offset).ray, z)
      const position = hit ? constrainedMove(drag.start, drag.startHit, hit, drag.handle, z) : store().dragPosition
      if (position) updateDrag(position, offset)
      invalidate()
    }

    const onPointerUp = (event: PointerEvent) => {
      if (event.button === 0) endDrag(true)
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (!isLocked() || event.metaKey || event.ctrlKey) return
      if (event.code !== 'Delete' && event.code !== 'Backspace') return
      const { selectedObjectId, drag, select } = store()
      const object = floorRef.current.objects.find((candidate) => candidate.id === selectedObjectId)
      if (!object || drag || object.mobility === 'fixed') return
      event.preventDefault()
      editRef.current({ type: 'remove_object', object_id: object.id })
      select(null)
    }

    // Losing the lock mid-drag (Esc) cancels the move.
    const onPointerLockChange = () => {
      if (!isLocked()) endDrag(false)
    }

    el.addEventListener('pointerdown', onPointerDown)
    el.addEventListener('pointermove', onPointerMove)
    el.addEventListener('pointerup', onPointerUp)
    el.addEventListener('keydown', onKeyDown)
    document.addEventListener('pointerlockchange', onPointerLockChange)
    return () => {
      el.removeEventListener('pointerdown', onPointerDown)
      el.removeEventListener('pointermove', onPointerMove)
      el.removeEventListener('pointerup', onPointerUp)
      el.removeEventListener('keydown', onKeyDown)
      document.removeEventListener('pointerlockchange', onPointerLockChange)
    }
  }, [element, get])

  return null
}
