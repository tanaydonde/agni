import { useThree } from '@react-three/fiber'
import { useEffect } from 'react'
import * as THREE from 'three'
import { homePose, orientation } from './cameraMath'
import type { FloorGeometry } from './geometry'

/**
 * Holds the review camera at a fixed bird's-eye overview of the whole floor,
 * refitting whenever the floor or the view's size changes.
 */
export function ReviewCamera({ geometry }: { geometry: FloorGeometry }) {
  const get = useThree((state) => state.get)
  const size = useThree((state) => state.size)

  useEffect(() => {
    const { camera, invalidate } = get()
    if (!(camera instanceof THREE.PerspectiveCamera)) return
    const pose = homePose(geometry.bounds, geometry.floorZ, camera.fov, size.width / size.height)
    camera.position.copy(pose.position)
    orientation(pose.yaw, pose.pitch, camera.quaternion)
    camera.near = 0.05
    camera.far = Math.max(100, pose.distance * 10)
    camera.updateProjectionMatrix()
    invalidate()
  }, [geometry, get, size])

  return null
}
