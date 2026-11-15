import { useThree } from '@react-three/fiber'
import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import type { HeatmapImage } from '../fire/heatmap'
import { Z_UP_TO_Y_UP } from './placement'

const UNIT_SQUARE = new THREE.PlaneGeometry(1, 1)
const noRaycast = () => {}

/**
 * The fire heatmap as one image laid flat on the floor plan, a pixel per
 * cell. Drawn over everything: seen from above, heat on a piece of furniture
 * must not be hidden by the furniture's box. Each cell's own opacity lets
 * the plan and furniture show through.
 */
export function FireHeatmap({ image, floorZ }: { image: HeatmapImage; floorZ: number }) {
  const invalidate = useThree((state) => state.invalidate)
  const texture = useMemo(() => {
    const texture = new THREE.DataTexture(image.data, image.width, image.height, THREE.RGBAFormat)
    // Crisp cells, not blurred across neighbours.
    texture.magFilter = THREE.NearestFilter
    texture.minFilter = THREE.NearestFilter
    texture.colorSpace = THREE.SRGBColorSpace
    texture.needsUpdate = true
    return texture
  }, [image])
  useEffect(() => () => texture.dispose(), [texture])
  useEffect(() => invalidate(), [texture, invalidate])

  return (
    <group name="fire-heatmap" rotation={Z_UP_TO_Y_UP}>
      <mesh
        geometry={UNIT_SQUARE}
        position={[image.originX + image.widthM / 2, image.originY + image.heightM / 2, floorZ]}
        scale={[image.widthM, image.heightM, 1]}
        renderOrder={10}
        raycast={noRaycast}
      >
        <meshBasicMaterial map={texture} transparent depthTest={false} depthWrite={false} toneMapped={false} />
      </mesh>
    </group>
  )
}
