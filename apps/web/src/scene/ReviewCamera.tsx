import { useThree } from '@react-three/fiber'
import { useEffect, useRef } from 'react'
import * as THREE from 'three'
import type { ReviewCameraMode } from '../components/ScenarioReviewControls'
import type { AgentFrame } from '../fire/types'
import { homePose, orientation } from './cameraMath'
import type { FloorGeometry } from './geometry'

const CHASE_DISTANCE_M = 4
const CHASE_HEIGHT_M = 2.7
const LOOK_AHEAD_M = 1.4
const TARGET_HEIGHT_M = 1.1
const MIN_DIRECTION_SQ = 0.01

/** Drives the active Three.js camera from the lead firefighter or a fixed
 * whole-building overview while scenario review is open. */
export function ReviewCamera({
  mode,
  geometry,
  agents,
}: {
  mode: ReviewCameraMode
  geometry: FloorGeometry
  agents: AgentFrame[]
}) {
  const get = useThree((state) => state.get)
  const lastDirection = useRef(new THREE.Vector3(0, 0, -1))

  useEffect(() => {
    const { camera, size, invalidate } = get()
    if (!(camera instanceof THREE.PerspectiveCamera)) return

    if (mode === 'overview') {
      const pose = homePose(geometry.bounds, geometry.floorZ, camera.fov, size.width / size.height)
      camera.position.copy(pose.position)
      orientation(pose.yaw, pose.pitch, camera.quaternion)
      camera.near = 0.05
      camera.far = Math.max(100, pose.distance * 10)
      camera.updateProjectionMatrix()
      invalidate()
      return
    }

    const firefighters = agents
      .filter((agent) => agent.role.type === 'firefighter')
      .sort((left, right) => left.id.localeCompare(right.id))
    const leader =
      firefighters.find((agent) => agent.role.type === 'firefighter' && agent.role.phase !== 'exited') ??
      firefighters[0]
    if (!leader) return

    const position = new THREE.Vector3(
      leader.position_m.x,
      leader.position_m.z,
      -leader.position_m.y,
    )
    const direction = new THREE.Vector3(
      leader.velocity_m_per_s.x,
      leader.velocity_m_per_s.z,
      -leader.velocity_m_per_s.y,
    )
    if (direction.lengthSq() >= MIN_DIRECTION_SQ) lastDirection.current.copy(direction).normalize()
    const desired = position
      .clone()
      .addScaledVector(lastDirection.current, -CHASE_DISTANCE_M)
      .add(new THREE.Vector3(0, CHASE_HEIGHT_M, 0))
    if (camera.position.distanceToSquared(desired) > 100) camera.position.copy(desired)
    else camera.position.lerp(desired, 0.35)
    const target = position
      .clone()
      .addScaledVector(lastDirection.current, LOOK_AHEAD_M)
      .add(new THREE.Vector3(0, TARGET_HEIGHT_M, 0))
    camera.lookAt(target)
    camera.near = 0.05
    camera.far = 200
    camera.updateProjectionMatrix()
    invalidate()
  }, [agents, geometry, get, mode])

  return null
}
