import { useThree } from '@react-three/fiber'
import { useEffect, useRef, type RefObject } from 'react'
import * as THREE from 'three'
import { checkEdit, useWorldEdit } from './api'
import { useEditorStore, type DragPreview, type GizmoDrag } from './editorStore'
import { createLiveChecker, editKey } from './liveCheck'
import {
  FURNITURE_GROUP,
  GIZMO_GROUP,
  constrainedMove,
  floorZ,
  ndcFromClient,
  rayFloorHit,
  rotationDrag,
  yawDegrees,
  type GizmoHandle,
} from './placement'
import type { EditCommand, Floor } from './types'
import { describeWorldError } from './worldErrors'

/** Shift while turning snaps the heading to multiples of this. */
const ROTATION_SNAP_DEGREES = 15

/** The edit that would commit `preview`, or null if nothing changed. */
function dragEdit(drag: GizmoDrag, preview: DragPreview): EditCommand | null {
  if (drag.handle === 'rotate') {
    return preview.yaw === drag.startYaw
      ? null
      : { type: 'rotate_object', object_id: drag.objectId, rotation_z_degrees: preview.yaw }
  }
  return preview.position.x === drag.start.x && preview.position.y === drag.start.y
    ? null
    : { type: 'move_object', object_id: drag.objectId, position_m: preview.position }
}

interface SelectionControllerProps {
  /** The element wrapping the canvas; it also holds the pointer lock in fly mode. */
  element: RefObject<HTMLElement | null>
  floor: Floor
}

/**
 * Editing in edit mode, i.e. whenever the cursor is free (fly mode, with the
 * pointer locked, never edits):
 * - click furniture to select it; click anything else to clear the selection
 * - drag an arrow or the square to slide the selection across the floor, or
 *   the ring to turn it (hold Shift to snap to 15°)
 * - Delete/Backspace removes the selection; Esc cancels a drag or deselects
 *
 * Drags follow where the cursor ray meets the floor plane, so objects can't
 * leave the floor. While dragging, the world dry-runs the move (room, wall,
 * overlap, and doorway rules) so the object turns red where it can't go;
 * releasing there puts it back. Option and Control clicks are left to the
 * camera.
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
    const checker = createLiveChecker(checkEdit, (key, response) => {
      store().setLiveCheck({
        key,
        valid: response.valid,
        message: response.error ? describeWorldError(response.error) : null,
      })
      get().invalidate()
    })

    /** Points the raycaster through the cursor. */
    const castAt = (event: { clientX: number; clientY: number }) => {
      const { camera, gl } = get()
      ndcFromClient(event.clientX, event.clientY, gl.domElement.getBoundingClientRect(), ndc)
      raycaster.setFromCamera(ndc, camera)
      return raycaster
    }

    const handleUnder = (hits: THREE.Intersection[]) =>
      hits.find((hit) => hit.object.userData.handle)?.object.userData.handle as GizmoHandle | undefined

    const endDrag = (commit: boolean, pointerId?: number) => {
      if (pointerId !== undefined && el.hasPointerCapture(pointerId)) el.releasePointerCapture(pointerId)
      checker.cancel()
      const { drag, preview, liveCheck, showNotice } = store()
      if (!drag) return
      el.style.cursor = ''
      const edit = commit && preview ? dragEdit(drag, preview) : null
      const knownInvalid = edit && liveCheck && !liveCheck.valid && liveCheck.key === editKey(edit)
      if (edit && knownInvalid) {
        // The world already said no: put the object back and say why.
        showNotice(liveCheck.message ?? "Can't go there")
        store().endDrag()
      } else if (edit) {
        editRef.current(edit)
        // Let the patched scene render before dropping the preview, so the
        // object doesn't flash back for a frame.
        setTimeout(() => store().endDrag(), 0)
      } else {
        store().endDrag()
      }
      get().invalidate()
    }

    const onPointerDown = (event: PointerEvent) => {
      if (isLocked() || event.button !== 0 || event.altKey || event.ctrlKey || event.metaKey) return
      const { scene } = get()
      const ray = castAt(event)
      const hits = ray.intersectObjects(scene.children, true)
      const { selectedObjectId, select, startDrag } = store()

      // Handles draw on top of everything, so they win even when behind furniture.
      const handle = selectedObjectId ? handleUnder(hits) : undefined
      if (handle) {
        const current = floorRef.current
        const object = current.objects.find((candidate) => candidate.id === selectedObjectId)
        const startHit = rayFloorHit(ray.ray, floorZ(current))
        if (object && startHit) {
          event.preventDefault()
          el.setPointerCapture(event.pointerId)
          el.style.cursor = 'grabbing'
          startDrag({
            objectId: object.id,
            handle,
            start: object.transform.position_m,
            startYaw: yawDegrees(object.transform.rotation),
            startHit,
          })
          get().invalidate()
        }
        return
      }

      // Otherwise select whatever solid surface is first under the cursor:
      // furniture selects it; a wall or the floor clears the selection.
      const first = hits.find((hit) => hit.object instanceof THREE.Mesh)
      const objectId = first?.object.userData.objectId
      select(typeof objectId === 'string' ? objectId : null)
    }

    const onPointerMove = (event: PointerEvent) => {
      if (isLocked()) return
      const { drag, preview, updateDrag, setHoveredHandle } = store()

      if (drag) {
        const z = floorZ(floorRef.current)
        const hit = rayFloorHit(castAt(event).ray, z)
        if (!hit || !preview) return
        const next: DragPreview =
          drag.handle === 'rotate'
            ? {
                position: drag.start,
                yaw: rotationDrag(
                  drag.start,
                  drag.startHit,
                  hit,
                  drag.startYaw,
                  event.shiftKey ? ROTATION_SNAP_DEGREES : undefined,
                ),
              }
            : { position: constrainedMove(drag.start, drag.startHit, hit, drag.handle, z), yaw: preview.yaw }
        updateDrag(next)
        const edit = dragEdit(drag, next)
        if (edit) {
          checker.request(edit)
        } else {
          checker.cancel()
          store().setLiveCheck(null) // back where it started: always fine
        }
        get().invalidate()
        return
      }

      // Hover feedback; only the gizmo and furniture are tested, so this stays
      // cheap with a large scanned mesh in the scene.
      if (event.buttons !== 0) return
      const { scene } = get()
      const targets = [scene.getObjectByName(GIZMO_GROUP), scene.getObjectByName(FURNITURE_GROUP)].filter(
        (target): target is THREE.Object3D => target !== undefined,
      )
      const hits = castAt(event).intersectObjects(targets, true)
      const handle = handleUnder(hits) ?? null
      setHoveredHandle(handle)
      const overFurniture = hits.some((hit) => typeof hit.object.userData.objectId === 'string')
      el.style.cursor = handle ? 'grab' : overFurniture ? 'pointer' : ''
    }

    const onPointerUp = (event: PointerEvent) => {
      if (event.button === 0) endDrag(true, event.pointerId)
    }

    const onPointerCancel = (event: PointerEvent) => endDrag(false, event.pointerId)

    const onKeyDown = (event: KeyboardEvent) => {
      if (isLocked() || event.metaKey || event.ctrlKey || event.altKey) return
      const target = event.target as HTMLElement | null
      if (target?.closest('input, textarea, [contenteditable="true"]')) return // typing in the chat
      const { selectedObjectId, drag, placing, select } = store()

      if (event.key === 'Escape') {
        if (drag) endDrag(false)
        else if (!placing) select(null)
        return
      }
      if (event.key !== 'Delete' && event.key !== 'Backspace') return
      const object = floorRef.current.objects.find((candidate) => candidate.id === selectedObjectId)
      if (!object || drag || object.mobility === 'fixed') return
      event.preventDefault()
      editRef.current({ type: 'remove_object', object_id: object.id })
      select(null)
    }

    // Entering fly mode cancels any drag and clears hover feedback.
    const onPointerLockChange = () => {
      if (!isLocked()) return
      endDrag(false)
      store().setHoveredHandle(null)
      el.style.cursor = ''
    }

    el.addEventListener('pointerdown', onPointerDown)
    el.addEventListener('pointermove', onPointerMove)
    el.addEventListener('pointerup', onPointerUp)
    el.addEventListener('pointercancel', onPointerCancel)
    window.addEventListener('keydown', onKeyDown)
    document.addEventListener('pointerlockchange', onPointerLockChange)
    return () => {
      checker.cancel()
      el.removeEventListener('pointerdown', onPointerDown)
      el.removeEventListener('pointermove', onPointerMove)
      el.removeEventListener('pointerup', onPointerUp)
      el.removeEventListener('pointercancel', onPointerCancel)
      window.removeEventListener('keydown', onKeyDown)
      document.removeEventListener('pointerlockchange', onPointerLockChange)
    }
  }, [element, get])

  return null
}
