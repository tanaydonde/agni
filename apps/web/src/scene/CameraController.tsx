import { useFrame, useThree } from '@react-three/fiber'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, type RefObject } from 'react'
import * as THREE from 'three'
import {
  type CameraPose,
  clampPitch,
  flySpeed,
  homePose,
  orbit,
  orientation,
  pinchTravel,
  wheelPixels,
  worldPerPixel,
} from './cameraMath'
import type { FloorGeometry } from './geometry'
import { ndcFromClient } from './placement'

/** Radians turned per pixel of pointer movement. */
const LOOK_RATE = 0.0025
/** Radians orbited per pixel of pointer movement with Option held. */
const ORBIT_RATE = 0.005
/** Speed multiplier while Shift is held. */
const BOOST = 3
/** How quickly flying speeds up and slows down (1/s). */
const RESPONSIVENESS = 10
/** Assumed distance to the target when nothing is in the middle of the view. */
const FALLBACK_DISTANCE_M = 5
const NEAR_M = 0.05

const FLY_KEYS = new Set(['KeyW', 'KeyA', 'KeyS', 'KeyD', 'KeyQ', 'KeyE', 'ShiftLeft', 'ShiftRight'])
const SCREEN_CENTER = new THREE.Vector2(0, 0)

interface CameraControllerProps {
  /** Focusable element wrapping the canvas. Right-clicking it captures the pointer. */
  element: RefObject<HTMLElement | null>
  geometry: FloorGeometry
  /** Called when fly mode starts (pointer captured) or ends (pointer released). */
  onActiveChange?: (active: boolean) => void
  /** False while another controller owns the Three.js camera. */
  enabled?: boolean
}

/**
 * Camera modeled on Unity's Scene view, built for a trackpad. It has two modes:
 *
 * - Edit mode (default; cursor free, left clicks belong to the editor): pinch
 *   moves toward the point under the cursor, a two-finger swipe looks around,
 *   and Option + drag orbits around the point under the cursor.
 * - Fly mode: right-click (or Control-click) captures the pointer; Esc or
 *   another right-click releases it. Moving the pointer looks around, WASD/QE
 *   fly, pinch moves toward the center of the view, Space + move pans, and
 *   Option + move orbits.
 *
 * F returns to the overview in either mode.
 */
export function CameraController({
  element,
  geometry,
  onActiveChange,
  enabled = true,
}: CameraControllerProps) {
  const get = useThree((state) => state.get)
  const pose = useRef<CameraPose | null>(null)
  const keys = useRef(new Set<string>())
  const velocity = useRef(new THREE.Vector3())
  const speed = useMemo(() => flySpeed(geometry.bounds), [geometry])
  const onActiveChangeRef = useRef(onActiveChange)
  useEffect(() => {
    onActiveChangeRef.current = onActiveChange
  }, [onActiveChange])

  /** Writes the pose to the camera and requests a frame. */
  const apply = useCallback(() => {
    const { camera, invalidate } = get()
    if (!pose.current) return
    camera.position.copy(pose.current.position)
    orientation(pose.current.yaw, pose.current.pitch, camera.quaternion)
    invalidate()
  }, [get])

  const goHome = useCallback(() => {
    const { camera, size } = get()
    if (!(camera instanceof THREE.PerspectiveCamera)) return
    const home = homePose(geometry.bounds, geometry.floorZ, camera.fov, size.width / size.height)
    pose.current = home
    velocity.current.set(0, 0, 0)
    camera.near = NEAR_M
    camera.far = Math.max(100, home.distance * 10)
    camera.updateProjectionMatrix()
    apply()
  }, [apply, geometry, get])

  useLayoutEffect(goHome, [goHome])

  // Review cameras overwrite the actual camera but not this controller's
  // stored pose. Restore it when Building mode becomes active again.
  useEffect(() => {
    if (enabled) apply()
  }, [apply, enabled])

  useEffect(() => {
    if (!enabled) return
    const el = element.current
    if (!el) return
    const isActive = () => document.pointerLockElement === el
    const raycaster = new THREE.Raycaster()
    // Pan distance and orbit pivot are fixed when their modifier goes down.
    let panDistance: number | null = null
    let orbitPivot: THREE.Vector3 | null = null
    /** Edit mode's Option + drag orbit: the pivot under the cursor at press. */
    let editOrbitPivot: THREE.Vector3 | null = null

    const cursorNdc = (event: { clientX: number; clientY: number }) =>
      ndcFromClient(event.clientX, event.clientY, get().gl.domElement.getBoundingClientRect())

    /** First visible surface at `ndc` (default: the middle of the view), if any. */
    const surfaceAt = (ndc: THREE.Vector2 = SCREEN_CENTER) => {
      const { camera, scene } = get()
      raycaster.setFromCamera(ndc, camera)
      const hit = raycaster
        .intersectObjects(scene.children, true)
        .find((intersection) => intersection.object instanceof THREE.Mesh)
      return { ray: raycaster.ray.clone(), hit }
    }

    const reset = () => {
      keys.current.clear()
      panDistance = null
      orbitPivot = null
      editOrbitPivot = null
    }

    const onPointerDown = (event: PointerEvent) => {
      // Right-click, or Control-click on a Mac, toggles fly mode.
      if (event.button === 2 || (event.button === 0 && event.ctrlKey)) {
        event.preventDefault()
        if (isActive()) document.exitPointerLock()
        // Chrome refuses a re-lock for about a second after Esc; the next click works.
        else Promise.resolve(el.requestPointerLock()).catch(() => {})
        return
      }
      if (isActive() || event.button !== 0 || !event.altKey) return
      // Edit mode: Option + drag orbits around what's under the cursor.
      const { ray, hit } = surfaceAt(cursorNdc(event))
      editOrbitPivot = hit?.point ?? ray.at(FALLBACK_DISTANCE_M, new THREE.Vector3())
      el.setPointerCapture(event.pointerId)
    }

    const onPointerUp = (event: PointerEvent) => {
      if (editOrbitPivot && el.hasPointerCapture(event.pointerId)) el.releasePointerCapture(event.pointerId)
      editOrbitPivot = null
    }

    const onContextMenu = (event: MouseEvent) => event.preventDefault()

    const onPointerLockChange = () => {
      reset()
      onActiveChangeRef.current?.(isActive())
    }

    const onPointerMove = (event: PointerEvent) => {
      const p = pose.current
      if (!p) return
      if (!isActive()) {
        if (editOrbitPivot && event.buttons & 1) {
          pose.current = orbit(p, editOrbitPivot, -event.movementX * ORBIT_RATE, -event.movementY * ORBIT_RATE)
          apply()
        }
        return
      }
      const dx = event.movementX
      const dy = event.movementY

      if (keys.current.has('Space')) {
        // Drag the world: what's in the middle of the view moves with the pointer.
        if (panDistance === null) panDistance = surfaceAt().hit?.distance ?? FALLBACK_DISTANCE_M
        const { camera, size } = get()
        const fov = camera instanceof THREE.PerspectiveCamera ? camera.fov : 50
        const scale = worldPerPixel(panDistance, fov, size.height)
        const rotation = orientation(p.yaw, p.pitch)
        p.position
          .addScaledVector(new THREE.Vector3(1, 0, 0).applyQuaternion(rotation), -dx * scale)
          .addScaledVector(new THREE.Vector3(0, 1, 0).applyQuaternion(rotation), dy * scale)
      } else if (event.altKey) {
        if (!orbitPivot) {
          const { ray, hit } = surfaceAt()
          orbitPivot = hit?.point ?? ray.at(FALLBACK_DISTANCE_M, new THREE.Vector3())
        }
        pose.current = orbit(p, orbitPivot, -dx * ORBIT_RATE, -dy * ORBIT_RATE)
      } else {
        orbitPivot = null
        p.yaw -= dx * LOOK_RATE
        p.pitch = clampPitch(p.pitch - dy * LOOK_RATE)
      }
      apply()
    }

    const onWheel = (event: WheelEvent) => {
      // Always swallow wheel/pinch over the viewport so the page never zooms
      // or swipes back.
      event.preventDefault()
      const p = pose.current
      if (!p) return
      const delta = wheelPixels(event)

      if (event.ctrlKey) {
        // Chrome and Firefox report trackpad pinch as ctrl + wheel. Fly mode
        // aims at the crosshair; edit mode at the cursor.
        const { ray, hit } = surfaceAt(isActive() ? SCREEN_CENTER : cursorNdc(event))
        const travel = pinchTravel(hit?.distance ?? FALLBACK_DISTANCE_M, delta.y)
        p.position.addScaledVector(ray.direction, travel)
      } else {
        // A two-finger swipe looks around too.
        p.yaw -= delta.x * LOOK_RATE
        p.pitch = clampPitch(p.pitch - delta.y * LOOK_RATE)
      }
      apply()
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey) return // leave browser shortcuts alone
      if (event.code === 'KeyF') {
        goHome()
      } else if (!isActive()) {
        return
      } else if (event.code === 'Space') {
        event.preventDefault()
        keys.current.add('Space')
      } else if (event.code === 'AltLeft' || event.code === 'AltRight') {
        event.preventDefault()
      } else if (FLY_KEYS.has(event.code)) {
        keys.current.add(event.code)
        get().invalidate() // start the fly loop
      }
    }

    const onKeyUp = (event: KeyboardEvent) => {
      keys.current.delete(event.code)
      if (event.code === 'Space') panDistance = null
      if (event.code === 'AltLeft' || event.code === 'AltRight') orbitPivot = null
    }

    el.addEventListener('pointerdown', onPointerDown)
    el.addEventListener('pointerup', onPointerUp)
    el.addEventListener('contextmenu', onContextMenu)
    el.addEventListener('pointermove', onPointerMove)
    el.addEventListener('wheel', onWheel, { passive: false })
    el.addEventListener('keydown', onKeyDown)
    el.addEventListener('keyup', onKeyUp)
    document.addEventListener('pointerlockchange', onPointerLockChange)
    return () => {
      el.removeEventListener('pointerdown', onPointerDown)
      el.removeEventListener('pointerup', onPointerUp)
      el.removeEventListener('contextmenu', onContextMenu)
      el.removeEventListener('pointermove', onPointerMove)
      el.removeEventListener('wheel', onWheel)
      el.removeEventListener('keydown', onKeyDown)
      el.removeEventListener('keyup', onKeyUp)
      document.removeEventListener('pointerlockchange', onPointerLockChange)
      if (isActive()) document.exitPointerLock()
    }
  }, [apply, element, enabled, get, goHome])

  useFrame((_, rawDelta) => {
    if (!enabled) return
    const p = pose.current
    if (!p) return
    // After idle time (frameloop="demand") the first delta can be huge.
    const delta = Math.min(rawDelta, 1 / 30)
    const held = keys.current
    const axis = (positive: string, negative: string) =>
      (held.has(positive) ? 1 : 0) - (held.has(negative) ? 1 : 0)

    // Fly along the view direction (including pitch), like Unity; Q/E use world up.
    const rotation = orientation(p.yaw, p.pitch)
    const target = new THREE.Vector3()
      .addScaledVector(new THREE.Vector3(0, 0, -1).applyQuaternion(rotation), axis('KeyW', 'KeyS'))
      .addScaledVector(new THREE.Vector3(1, 0, 0).applyQuaternion(rotation), axis('KeyD', 'KeyA'))
      .addScaledVector(new THREE.Vector3(0, 1, 0), axis('KeyE', 'KeyQ'))
    if (target.lengthSq() > 0) {
      const boost = held.has('ShiftLeft') || held.has('ShiftRight') ? BOOST : 1
      target.normalize().multiplyScalar(speed * boost)
    }

    const v = velocity.current
    v.lerp(target, 1 - Math.exp(-RESPONSIVENESS * delta))
    if (target.lengthSq() === 0 && v.lengthSq() < 1e-4) {
      v.set(0, 0, 0)
      return // at rest: stop requesting frames
    }
    p.position.addScaledVector(v, delta)
    apply() // also requests the next frame
  })

  return null
}
