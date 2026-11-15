// Free flying inside Showcase. Showcase's public camera API can only stand
// the camera at scans, so this drives its three.js camera directly: Showcase's
// own controls are switched off (the hidden `R3F.controlsToggle` /
// `navigationToggle`), and a Scene component rewrites the camera every frame,
// after Showcase's own camera update. Relies on Bundle 26.5.4 internals: the
// camera's parent ("CameraRig") holds the position and the camera holds the
// rotation. `sdk.Camera.pose` does not follow what we set.

import * as THREE from 'three'
import { clampPitch, orientation } from '../scene/cameraMath'
import type { CameraPose } from '../scene/cameraMath'
import type { CameraDriver } from './cameraDriver'
import { MODE, type MpSdk, readMap, SWEEP_TRANSITION, type SceneComponent, type SweepData } from './sdk'

/** Walking speed in m/s; Shift multiplies it by BOOST. */
export const WALK_SPEED = 1.5
const BOOST = 3
/** How quickly movement speeds up and slows down (1/s), as in the editor. */
const RESPONSIVENESS = 10
/** Radians turned per pixel of pointer movement, as in the editor. */
const LOOK_RATE = 0.0025
/** Longest frame step we integrate, so a stall doesn't jump the camera. */
const MAX_STEP_S = 0.1
/** How often to check for a nearer scan (ms). */
const SWEEP_CHECK_MS = 250
/** How long Showcase gets to start our camera component. */
const START_TIMEOUT_MS = 5000

const MOVE_KEYS = new Set(['KeyW', 'KeyA', 'KeyS', 'KeyD', 'KeyQ', 'KeyE', 'ShiftLeft', 'ShiftRight'])

export interface FlyState {
  position: THREE.Vector3
  velocity: THREE.Vector3
  yaw: number
  pitch: number
}

/**
 * Advances the camera by `dt` seconds for the held keys. W/S and A/D walk
 * level relative to where the camera faces, whatever its pitch; Q/E go
 * straight down/up. Speed eases in and out.
 */
export function flyStep(state: FlyState, held: ReadonlySet<string>, dt: number, speed = WALK_SPEED): void {
  const step = Math.min(dt, MAX_STEP_S)
  const axis = (positive: string, negative: string) => (held.has(positive) ? 1 : 0) - (held.has(negative) ? 1 : 0)
  const forward = axis('KeyW', 'KeyS')
  const right = axis('KeyD', 'KeyA')
  const sin = Math.sin(state.yaw)
  const cos = Math.cos(state.yaw)
  // Yaw 0 looks along -Z; right is +X.
  const target = new THREE.Vector3(-sin * forward + cos * right, axis('KeyE', 'KeyQ'), -cos * forward - sin * right)
  if (target.lengthSq() > 0) {
    const boost = held.has('ShiftLeft') || held.has('ShiftRight') ? BOOST : 1
    target.normalize().multiplyScalar(speed * boost)
  }
  state.velocity.lerp(target, 1 - Math.exp(-RESPONSIVENESS * step))
  if (target.lengthSq() === 0 && state.velocity.lengthSq() < 1e-6) state.velocity.set(0, 0, 0)
  state.position.addScaledVector(state.velocity, step)
}

/** The scan nearest `position`, if any (Showcase coordinates). */
export function nearestSweep(sweeps: Pick<SweepData, 'sid' | 'position'>[], position: THREE.Vector3): string | null {
  let best: string | null = null
  let bestDistance = Infinity
  for (const { sid, position: p } of sweeps) {
    const distance = (p.x - position.x) ** 2 + (p.y - position.y) ** 2 + (p.z - position.z) ** 2
    if (distance < bestDistance) {
      bestDistance = distance
      best = sid
    }
  }
  return best
}

type FlySdk = Pick<MpSdk, 'Scene' | 'Sweep' | 'Mode' | 'R3F'>

/**
 * Takes Showcase's camera away from Showcase. `pose` is filled with where
 * Showcase had the camera; from then on `step` is called every frame (with
 * the seconds since the last) to move `pose`, which is then written to the
 * camera. Rejects if this Showcase won't hand its camera over. `release`
 * gives it back, standing at the scan nearest `pose`.
 */
export async function takeOverCamera(
  sdk: FlySdk,
  componentName: string,
  pose: CameraPose,
  step: (seconds: number) => void,
): Promise<{ release(): void }> {
  const r3f = sdk.R3F
  if (!r3f) throw new Error('This Showcase build has no camera override.')

  const sweeps = Object.values(await readMap(sdk.Sweep.data)).filter((sweep) => sweep.enabled)
  let mode: string | null = null
  let currentSweep: string | null = null
  let sweepMoving = false
  let lastSweepCheck = 0
  const rotation = new THREE.Quaternion()

  const subscriptions = [
    sdk.Mode.current.subscribe((value) => (mode = value)),
    sdk.Sweep.current.subscribe((sweep) => (currentSweep = sweep?.sid || null)),
  ]

  /**
   * Inside mode paints the current scan's panorama onto the mesh, so keep the
   * current scan the nearest one or the walls smear. The jump is instant and
   * doesn't disturb the camera, which we rewrite every frame anyway.
   */
  const followNearestSweep = () => {
    const now = performance.now()
    if (mode !== MODE.INSIDE || sweepMoving || now - lastSweepCheck < SWEEP_CHECK_MS) return
    lastSweepCheck = now
    const nearest = nearestSweep(sweeps, pose.position)
    if (!nearest || nearest === currentSweep) return
    sweepMoving = true
    sdk.Sweep.moveTo(nearest, { transition: SWEEP_TRANSITION.INSTANT })
      .catch(() => {})
      .finally(() => (sweepMoving = false))
  }

  let started: () => void = () => {}
  const running = new Promise<void>((resolve) => (started = resolve))

  // Showcase's camera (from its own copy of three) and the rig holding its position.
  let camera: THREE.Camera | null = null
  let rig: THREE.Object3D | null = null

  const component = (): SceneComponent => ({
    onInit(this: SceneComponent) {
      camera = this.context?.camera ?? null
      rig = camera?.parent ?? null
      if (!camera || !rig) return
      // Start from wherever Showcase put the camera.
      pose.position.copy(rig.position)
      const { x, y, z, w } = camera.quaternion
      const euler = new THREE.Euler().setFromQuaternion(new THREE.Quaternion(x, y, z, w), 'YXZ')
      pose.yaw = euler.y
      pose.pitch = clampPitch(euler.x)
      started()
    },
    // Showcase's delta is in milliseconds.
    onTick(deltaMs: number) {
      if (!camera || !rig) return
      step(deltaMs / 1000)
      rig.position.set(pose.position.x, pose.position.y, pose.position.z)
      orientation(pose.yaw, pose.pitch, rotation)
      camera.quaternion.set(rotation.x, rotation.y, rotation.z, rotation.w)
      followNearestSweep()
    },
  })

  const registration = await sdk.Scene.register(componentName, component as () => SceneComponent<never>)
  const [object] = await sdk.Scene.createObjects(1)
  object.addNode().addComponent(componentName)
  object.start()

  const stop = () => {
    object.stop()
    registration?.dispose()
    subscriptions.forEach((subscription) => subscription.cancel())
  }
  const timeout = new Promise<never>((_, reject) =>
    setTimeout(() => reject(new Error('Showcase did not start the camera component.')), START_TIMEOUT_MS),
  )
  try {
    await Promise.race([running, timeout])
    await Promise.all([r3f.controlsToggle(false), r3f.navigationToggle(false)])
  } catch (error) {
    stop()
    throw error
  }

  return {
    release() {
      stop()
      // Hand control back to Showcase, standing at a real scan so its own
      // idea of the camera matches what's on screen again.
      void Promise.all([r3f.controlsToggle(true), r3f.navigationToggle(true)])
        .then(() => {
          const nearest = nearestSweep(sweeps, pose.position)
          if (nearest) return sdk.Sweep.moveTo(nearest, { transition: SWEEP_TRANSITION.FLY })
        })
        .catch(() => {})
    },
  }
}

/**
 * Takes over Showcase's camera for free flying. Rejects if this Showcase
 * won't hand it over, so the caller can fall back to hopping between scans.
 */
export async function createFlyDriver(sdk: FlySdk, componentName = 'tflop.fly-camera'): Promise<CameraDriver> {
  const held = new Set<string>()
  const state: FlyState = { position: new THREE.Vector3(), velocity: new THREE.Vector3(), yaw: 0, pitch: 0 }
  const takeover = await takeOverCamera(sdk, componentName, state, (seconds) => flyStep(state, held, seconds))
  // Nothing is held yet, so this is still where Showcase had the camera.
  const home = { position: state.position.clone(), yaw: state.yaw, pitch: state.pitch }
  const goHome = () => {
    state.position.copy(home.position)
    state.velocity.set(0, 0, 0)
    state.yaw = home.yaw
    state.pitch = home.pitch
  }

  return {
    keyDown(code) {
      if (!MOVE_KEYS.has(code)) return false
      held.add(code)
      return true
    },
    keyUp(code) {
      held.delete(code)
    },
    releaseAll() {
      held.clear()
    },
    look(dx, dy) {
      state.yaw -= dx * LOOK_RATE
      state.pitch = clampPitch(state.pitch - dy * LOOK_RATE)
    },
    home: goHome,
    place(pose) {
      home.position.copy(pose.position)
      home.yaw = pose.yaw
      home.pitch = clampPitch(pose.pitch)
      goHome()
    },
    dispose() {
      held.clear()
      takeover.release()
    },
  }
}
