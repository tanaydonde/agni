import { useThree } from '@react-three/fiber'
import { useEffect, type RefObject } from 'react'
import * as THREE from 'three'
import { useEditorStore } from './editorStore'
import { FURNITURE_GROUP, ndcFromClient } from './placement'

interface SelectionControllerProps {
  /** The element wrapping the canvas; it also holds the pointer lock in fly mode. */
  element: RefObject<HTMLElement | null>
}

/**
 * Picking the fire origin in edit mode, i.e. whenever the cursor is free (fly
 * mode, with the pointer locked, never selects):
 * - click furniture to select (highlight) it; click anything else to clear
 * - Esc clears the selection
 *
 * Selection never changes the scene. Option and Control clicks are left to
 * the camera.
 */
export function SelectionController({ element }: SelectionControllerProps) {
  const get = useThree((state) => state.get)

  useEffect(() => {
    const el = element.current
    if (!el) return
    const raycaster = new THREE.Raycaster()
    const ndc = new THREE.Vector2()
    const isLocked = () => document.pointerLockElement === el
    const store = useEditorStore.getState

    /** Points the raycaster through the cursor. */
    const castAt = (event: { clientX: number; clientY: number }) => {
      const { camera, gl } = get()
      ndcFromClient(event.clientX, event.clientY, gl.domElement.getBoundingClientRect(), ndc)
      raycaster.setFromCamera(ndc, camera)
      return raycaster
    }

    const onPointerDown = (event: PointerEvent) => {
      if (isLocked() || event.button !== 0 || event.altKey || event.ctrlKey || event.metaKey) return
      // Select whatever solid surface is first under the cursor: furniture
      // selects it; a wall or the floor clears the selection.
      const hits = castAt(event).intersectObjects(get().scene.children, true)
      const first = hits.find((hit) => hit.object instanceof THREE.Mesh)
      const objectId = first?.object.userData.objectId
      store().select(typeof objectId === 'string' ? objectId : null)
    }

    // Hover feedback; only furniture is tested, so this stays cheap with a
    // large scanned mesh in the scene.
    const onPointerMove = (event: PointerEvent) => {
      if (isLocked() || event.buttons !== 0) return
      const furniture = get().scene.getObjectByName(FURNITURE_GROUP)
      if (!furniture) return
      const hits = castAt(event).intersectObject(furniture, true)
      const overFurniture = hits.some((hit) => typeof hit.object.userData.objectId === 'string')
      el.style.cursor = overFurniture ? 'pointer' : ''
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (isLocked() || event.metaKey || event.ctrlKey || event.altKey) return
      const target = event.target as HTMLElement | null
      if (target?.closest('input, textarea, [contenteditable="true"]')) return // typing in a form field
      if (event.key === 'Escape') store().select(null)
    }

    const onPointerLockChange = () => {
      if (isLocked()) el.style.cursor = ''
    }

    el.addEventListener('pointerdown', onPointerDown)
    el.addEventListener('pointermove', onPointerMove)
    window.addEventListener('keydown', onKeyDown)
    document.addEventListener('pointerlockchange', onPointerLockChange)
    return () => {
      el.removeEventListener('pointerdown', onPointerDown)
      el.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('keydown', onKeyDown)
      document.removeEventListener('pointerlockchange', onPointerLockChange)
    }
  }, [element, get])

  return null
}
