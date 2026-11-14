import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import { useFireFrame } from '../fire/fireStore'
import { useEditorStore } from './editorStore'
import { categoryColor, EDGE_COLOR, INVALID_COLOR, SELECTED_COLOR } from './furnitureStyle'
import { FURNITURE_GROUP, Z_UP_TO_Y_UP, zRotation } from './placement'
import type { Floor, FurnitureAsset, Quaternion, Vec3 } from './types'

const IDENTITY: Quaternion = { x: 0, y: 0, z: 0, w: 1 }
const UNIT_SCALE: Vec3 = { x: 1, y: 1, z: 1 }
const noRaycast = () => {}

interface FurnitureProps {
  floor: Floor
  assets: FurnitureAsset[]
}

/**
 * Placed furniture on one floor, plus the translucent preview of an asset
 * being dragged in from the palette. Until assets have GLB models, each is a
 * box of its catalog dimensions standing on its origin (the bottom center).
 */
export function Furniture({ floor, assets }: FurnitureProps) {
  const selectedId = useEditorStore((state) => state.selectedObjectId)
  const drag = useEditorStore((state) => state.drag)
  const preview = useEditorStore((state) => state.preview)
  const placing = useEditorStore((state) => state.placing)
  // The world's verdict on the current drag or palette placement.
  const invalid = useEditorStore((state) => state.liveCheck?.valid === false)
  const assetsById = useMemo(() => new Map(assets.map((asset) => [asset.id, asset])), [assets])
  const ghostAsset = placing ? assetsById.get(placing.assetId) : undefined
  // Furniture with fire cells turns see-through, so the cells inside show.
  const fireFrame = useFireFrame()
  const onFire = useMemo(
    () =>
      new Set(
        fireFrame?.surface_fire.flatMap((node) => (node.source.type === 'furniture' ? [node.source.object_id] : [])),
      ),
    [fireFrame],
  )

  return (
    <group name={FURNITURE_GROUP} rotation={Z_UP_TO_Y_UP}>
      {floor.objects.map((object) => {
        const asset = assetsById.get(object.asset_id)
        if (!asset) return null
        const dragged = drag?.objectId === object.id ? preview : null
        return (
          <FurnitureBox
            key={object.id}
            objectId={object.id}
            asset={asset}
            position={dragged?.position ?? object.transform.position_m}
            rotation={dragged ? zRotation(dragged.yaw) : object.transform.rotation}
            scale={object.transform.scale}
            selected={object.id === selectedId}
            invalid={invalid && Boolean(dragged)}
            onFire={onFire.has(object.id)}
          />
        )
      })}
      {ghostAsset && placing?.position && (
        <FurnitureBox
          asset={ghostAsset}
          position={placing.position}
          rotation={IDENTITY}
          scale={UNIT_SCALE}
          ghost
          invalid={invalid}
        />
      )}
    </group>
  )
}

interface FurnitureBoxProps {
  /** Set on placed objects so clicks can find them; absent on the ghost. */
  objectId?: string
  asset: FurnitureAsset
  position: Vec3
  rotation: Quaternion
  scale: Vec3
  selected?: boolean
  ghost?: boolean
  /** The world would reject this placement. */
  invalid?: boolean
  /** Has fire cells; drawn see-through so they show. */
  onFire?: boolean
}

function FurnitureBox({
  objectId,
  asset,
  position,
  rotation,
  scale,
  selected = false,
  ghost = false,
  invalid = false,
  onFire = false,
}: FurnitureBoxProps) {
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
      <mesh geometry={box} userData={objectId ? { objectId } : {}} raycast={ghost ? noRaycast : undefined}>
        <meshStandardMaterial
          color={invalid ? INVALID_COLOR : categoryColor(asset.category)}
          transparent={ghost || invalid || onFire}
          opacity={onFire ? 0.3 : ghost || invalid ? 0.6 : 1}
          depthWrite={!onFire}
          emissive={invalid ? INVALID_COLOR : selected ? SELECTED_COLOR : '#000000'}
          emissiveIntensity={invalid ? 0.4 : selected ? 0.35 : 0}
        />
      </mesh>
      <lineSegments geometry={edges} raycast={noRaycast}>
        <lineBasicMaterial color={invalid ? INVALID_COLOR : selected ? SELECTED_COLOR : EDGE_COLOR} />
      </lineSegments>
    </group>
  )
}
