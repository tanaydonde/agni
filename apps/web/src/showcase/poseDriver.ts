// A Showcase camera that goes where it is told instead of where the user
// steers: riding along with the firefighter, or sitting still on a wall. It
// takes the camera over the same way free flying does (flyDriver.ts).

import * as THREE from 'three'
import type { CameraPose } from '../scene/cameraMath'
import { nearestSweep, takeOverCamera } from './flyDriver'
import { type MpSdk, readMap, SWEEP_TRANSITION } from './sdk'

/** How quickly the camera closes on its target (1/s). */
const FOLLOW_RATE = 6
/** A target this far away is a cut (a seek, or the first pose), not a move. */
const SNAP_DISTANCE_M = 3

export interface PoseDriver {
  /** Where the camera should be. It eases there, or cuts if that is far away. */
  setTarget(pose: CameraPose): void
  dispose(): void
}

/**
 * Moves `pose` toward `target` over `seconds`: position and angles ease
 * exponentially, turning the short way round. A far-off target is jumped to.
 */
export function easePose(pose: CameraPose, target: CameraPose, seconds: number): void {
  if (pose.position.distanceTo(target.position) > SNAP_DISTANCE_M) {
    pose.position.copy(target.position)
    pose.yaw = target.yaw
    pose.pitch = target.pitch
    return
  }
  const blend = 1 - Math.exp(-FOLLOW_RATE * Math.max(0, seconds))
  pose.position.lerp(target.position, blend)
  // The short way round: the difference wrapped into (-π, π].
  const turn = Math.atan2(Math.sin(target.yaw - pose.yaw), Math.cos(target.yaw - pose.yaw))
  pose.yaw += turn * blend
  pose.pitch += (target.pitch - pose.pitch) * blend
}

type PoseSdk = Pick<MpSdk, 'Scene' | 'Sweep' | 'Mode' | 'R3F'>

/**
 * A camera driven by `setTarget`. If this Showcase won't hand its camera
 * over, it stands at the scan nearest the target instead, facing wherever
 * Showcase leaves it.
 */
export async function createPoseDriver(sdk: PoseSdk, componentName = 'tflop.pose-camera'): Promise<PoseDriver> {
  const pose: CameraPose = { position: new THREE.Vector3(), yaw: 0, pitch: 0 }
  let target: CameraPose | null = null
  let cut = true
  try {
    const takeover = await takeOverCamera(sdk, componentName, pose, (seconds) => {
      if (!target) return
      // The first target is a cut from wherever Showcase had the camera.
      easePose(pose, target, cut ? Infinity : seconds)
      cut = false
    })
    return {
      setTarget(next) {
        target = next
      },
      dispose: () => takeover.release(),
    }
  } catch (error) {
    console.warn('Camera override unavailable; the view will stand at the nearest scan instead.', error)
    return createSweepPoseDriver(sdk)
  }
}

/** The fallback: stand at whichever scan is nearest the target. */
async function createSweepPoseDriver(sdk: Pick<MpSdk, 'Sweep'>): Promise<PoseDriver> {
  const sweeps = Object.values(await readMap(sdk.Sweep.data)).filter((sweep) => sweep.enabled)
  let current: string | null = null
  let moving = false
  return {
    setTarget(next) {
      const nearest = nearestSweep(sweeps, next.position)
      if (!nearest || nearest === current || moving) return
      moving = true
      sdk.Sweep.moveTo(nearest, { transition: SWEEP_TRANSITION.FLY })
        .then(() => (current = nearest))
        .catch(() => {})
        .finally(() => (moving = false))
    },
    dispose() {},
  }
}
