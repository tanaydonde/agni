import { useThree } from '@react-three/fiber'
import { useEffect } from 'react'
import * as THREE from 'three'
import { orientation, topDownZoom } from './cameraMath'
import type { FloorGeometry } from './geometry'

/** How far above the floor the camera sits; orthographic, so only clipping cares. */
const HEIGHT_M = 100

/**
 * A fixed bird's-eye camera for an orthographic canvas: straight down on the
 * whole floor, canonical +Y at the top of the screen. It takes no input.
 */
export function TopDownCamera({ geometry }: { geometry: FloorGeometry }) {
  const get = useThree((state) => state.get)
  const size = useThree((state) => state.size)

  useEffect(() => {
    const { camera, invalidate } = get()
    if (!(camera instanceof THREE.OrthographicCamera)) return
    const { min, max } = geometry.bounds
    // Canonical (x, y) → Three.js (x, -z).
    camera.position.set((min.x + max.x) / 2, geometry.floorZ + HEIGHT_M, -(min.y + max.y) / 2)
    orientation(0, -Math.PI / 2, camera.quaternion)
    camera.zoom = topDownZoom(geometry.bounds, size.width, size.height)
    camera.near = 0.1
    camera.far = HEIGHT_M * 2
    camera.updateProjectionMatrix()
    invalidate()
  }, [geometry, size, get])

  return null
}
