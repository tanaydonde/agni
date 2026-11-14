import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import { categoryColor, EDGE_COLOR } from './furnitureStyle'
import { Z_UP_TO_Y_UP } from './placement'
import type { DetectedObject, FurnitureAsset } from './types'

const noRaycast = () => {}
/** One shared unit cube, standing on its bottom face; boxes scale it. */
const UNIT_BOX = new THREE.BoxGeometry(1, 1, 1).translate(0, 0, 0.5)
const UNIT_EDGES = new THREE.EdgesGeometry(UNIT_BOX)

interface DetectedFurnitureProps {
  objects: DetectedObject[]
  assets: FurnitureAsset[]
}

/**
 * Furniture found in the scan, drawn as translucent boxes of their measured
 * size, colored like the catalog asset closest to them in size. Read-only: they
 * ignore the pointer, so they never get in the way of editing.
 */
export function DetectedFurniture({ objects, assets }: DetectedFurnitureProps) {
  const categories = useMemo(() => new Map(assets.map((asset) => [asset.id, asset.category])), [assets])

  return (
    <group name="detected-furniture" rotation={Z_UP_TO_Y_UP}>
      {objects.map((object) => (
        <DetectedBox
          key={object.id}
          object={object}
          color={categoryColor((object.asset_id && categories.get(object.asset_id)) || '')}
        />
      ))}
    </group>
  )
}

function DetectedBox({ object, color }: { object: DetectedObject; color: string }) {
  const { position_m: position, size_m: size } = object
  const material = useMemo(
    () => new THREE.MeshStandardMaterial({ color, transparent: true, opacity: 0.55, depthWrite: false }),
    [color],
  )
  useEffect(() => () => material.dispose(), [material])

  return (
    <group
      name={object.id}
      position={[position.x, position.y, position.z]}
      rotation={[0, 0, THREE.MathUtils.degToRad(object.yaw_degrees)]}
      scale={[size.width_m, size.depth_m, size.height_m]}
    >
      <mesh geometry={UNIT_BOX} material={material} raycast={noRaycast} />
      <lineSegments geometry={UNIT_EDGES} raycast={noRaycast}>
        <lineBasicMaterial color={EDGE_COLOR} />
      </lineSegments>
    </group>
  )
}
