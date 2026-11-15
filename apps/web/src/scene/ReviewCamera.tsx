import { useThree } from '@react-three/fiber'
import { useEffect } from 'react'
import * as THREE from 'three'
import { orientation } from './cameraMath'
import { orbitPosition, orbitYawPitch, type Orbit } from './orbitCamera'

/** Points the review canvas's camera along the shared orbit (see useReviewOrbit). */
export function ReviewCamera({ orbit }: { orbit: Orbit | null }) {
  const get = useThree((state) => state.get)

  useEffect(() => {
    const { camera, invalidate } = get()
    if (!orbit || !(camera instanceof THREE.PerspectiveCamera)) return
    const { yaw, pitch } = orbitYawPitch(orbit)
    camera.position.copy(orbitPosition(orbit))
    orientation(yaw, pitch, camera.quaternion)
    camera.near = 0.05
    camera.far = Math.max(100, orbit.distance * 10)
    camera.updateProjectionMatrix()
    invalidate()
  }, [orbit, get])

  return null
}
