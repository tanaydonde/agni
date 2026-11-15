import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import { useEditorStore } from './editorStore'
import { categoryColor, EDGE_COLOR, SELECTED_COLOR } from './furnitureStyle'
import { FURNITURE_GROUP, Z_UP_TO_Y_UP } from './placement'
import type { Floor, FurnitureAsset, Quaternion, Vec3 } from './types'

const noRaycast = () => {}

interface FurnitureProps {
  floor: Floor
  assets: FurnitureAsset[]
}

/**
 * Placed furniture on one floor. The selected piece (the fire origin for a
 * single run) is highlighted. Until assets have GLB models, each is a box of
 * its catalog dimensions standing on its origin (the bottom center).
 */
export function Furniture({ floor, assets }: FurnitureProps) {
  const selectedId = useEditorStore((state) => state.selectedObjectId)
  const assetsById = useMemo(() => new Map(assets.map((asset) => [asset.id, asset])), [assets])

  return (
    <group name={FURNITURE_GROUP} rotation={Z_UP_TO_Y_UP}>
      {floor.objects.map((object) => {
        const asset = assetsById.get(object.asset_id)
        if (!asset) return null
        return (
          <FurnitureBox
            key={object.id}
            objectId={object.id}
            asset={asset}
            position={object.transform.position_m}
            rotation={object.transform.rotation}
            scale={object.transform.scale}
            selected={object.id === selectedId}
          />
        )
      })}
    </group>
  )
}

interface FurnitureBoxProps {
  /** Lets clicks find the object. */
  objectId: string
  asset: FurnitureAsset
  position: Vec3
  rotation: Quaternion
  scale: Vec3
  selected?: boolean
}

function FurnitureBox({ objectId, asset, position, rotation, scale, selected = false }: FurnitureBoxProps) {
  const { width_m: width, depth_m: depth, height_m: height } = asset.dimensions_m
  // Shift the box up so its bottom face sits at the origin, i.e. on the floor.
  const box = useMemo(
    () => new THREE.BoxGeometry(width, depth, height).translate(0, 0, height / 2),
    [width, depth, height],
  )
  const edges = useMemo(() => new THREE.EdgesGeometry(box), [box])
  useEffect(
    () => () => {
      box.dispose()
      edges.dispose()
    },
    [box, edges],
  )

  return (
    <group
      position={[position.x, position.y, position.z]}
      quaternion={[rotation.x, rotation.y, rotation.z, rotation.w]}
      scale={[scale.x, scale.y, scale.z]}
    >
      <mesh geometry={box} userData={{ objectId }}>
        <meshStandardMaterial
          color={categoryColor(asset.category)}
          emissive={selected ? SELECTED_COLOR : '#000000'}
          emissiveIntensity={selected ? 0.35 : 0}
        />
      </mesh>
      <lineSegments geometry={edges} raycast={noRaycast}>
        <lineBasicMaterial color={selected ? SELECTED_COLOR : EDGE_COLOR} />
      </lineSegments>
    </group>
  )
}
