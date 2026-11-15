import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import { burningObjectIds } from '../fire/cells'
import { useFireFrame } from '../fire/fireStore'
import { useEditorStore } from './editorStore'
import { categoryColor, EDGE_COLOR, SELECTED_COLOR } from './furnitureStyle'
import { FURNITURE_GROUP, Z_UP_TO_Y_UP } from './placement'
import type { Floor, FurnitureAsset, Quaternion, Vec3 } from './types'

const noRaycast = () => {}

interface FurnitureProps {
  floor: Floor
  assets: FurnitureAsset[]
  /** Furniture with fire cells, e.g. for a run under review; defaults to the store's run. */
  burning?: ReadonlySet<string>
  /** Highlight the editor's selected piece (the fire origin). On by default. */
  highlightSelection?: boolean
  /**
   * Mark each piece as furniture over everything drawn on the plan (e.g. a
   * heatmap): diagonal hatching across its footprint, and its outline.
   */
  hatched?: boolean
}

/**
 * Placed furniture on one floor. The selected piece (the fire origin for a
 * single run) is highlighted. Until assets have GLB models, each is a box of
 * its catalog dimensions standing on its origin (the bottom center).
 */
export function Furniture({
  floor,
  assets,
  burning,
  highlightSelection = true,
  hatched = false,
}: FurnitureProps) {
  const selectedId = useEditorStore((state) => state.selectedObjectId)
  const assetsById = useMemo(() => new Map(assets.map((asset) => [asset.id, asset])), [assets])
  // Furniture with fire cells turns see-through so the cells inside it show;
  // walls stay opaque, so fire never shows through them.
  const fireFrame = useFireFrame()
  const storeBurning = useMemo(() => burningObjectIds(fireFrame), [fireFrame])
  const onFire = burning ?? storeBurning

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
            selected={highlightSelection && object.id === selectedId}
            onFire={onFire.has(object.id)}
            hatched={hatched}
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
  /** Has fire cells; drawn see-through so they show. */
  onFire?: boolean
  hatched?: boolean
}

function FurnitureBox({
  objectId,
  asset,
  position,
  rotation,
  scale,
  selected = false,
  onFire = false,
  hatched = false,
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
  const hatch = useMemo(
    () => (hatched ? hatchGeometry(width, depth, height, scale) : null),
    [hatched, width, depth, height, scale],
  )
  useEffect(() => () => hatch?.dispose(), [hatch])

  return (
    <group
      position={[position.x, position.y, position.z]}
      quaternion={[rotation.x, rotation.y, rotation.z, rotation.w]}
      scale={[scale.x, scale.y, scale.z]}
    >
      <mesh geometry={box} userData={{ objectId }}>
        <meshStandardMaterial
          color={categoryColor(asset.category)}
          transparent={onFire}
          opacity={onFire ? 0.3 : 1}
          depthWrite={!onFire}
          emissive={selected ? SELECTED_COLOR : '#000000'}
          emissiveIntensity={selected ? 0.35 : 0}
        />
      </mesh>
      {hatch && (
        <mesh geometry={hatch} renderOrder={11} raycast={noRaycast}>
          <meshBasicMaterial
            map={hatchTexture()}
            transparent
            opacity={0.6}
            depthTest={false}
            depthWrite={false}
            toneMapped={false}
          />
        </mesh>
      )}
      <lineSegments geometry={edges} raycast={noRaycast} renderOrder={hatched ? 11 : 0}>
        <lineBasicMaterial
          color={selected ? SELECTED_COLOR : EDGE_COLOR}
          depthTest={!hatched}
          transparent={hatched}
          opacity={hatched ? 0.7 : 1}
        />
      </lineSegments>
    </group>
  )
}

/** Gap between hatch lines, in metres. */
const HATCH_SPACING_M = 0.18

/**
 * The top of a box as a plane whose texture repeats every `HATCH_SPACING_M`
 * of the placed (scaled) piece, so every piece gets the same line spacing.
 */
function hatchGeometry(width: number, depth: number, height: number, scale: Vec3): THREE.PlaneGeometry {
  const plane = new THREE.PlaneGeometry(width, depth).translate(0, 0, height)
  const repeatU = (width * Math.abs(scale.x)) / HATCH_SPACING_M
  const repeatV = (depth * Math.abs(scale.y)) / HATCH_SPACING_M
  const uv = plane.getAttribute('uv')
  for (let index = 0; index < uv.count; index++) {
    uv.setXY(index, uv.getX(index) * repeatU, uv.getY(index) * repeatV)
  }
  return plane
}

let sharedHatch: THREE.CanvasTexture | null = null

/** One diagonal line per tile, joining up across tiles; made on first use. */
function hatchTexture(): THREE.CanvasTexture {
  if (sharedHatch) return sharedHatch
  const size = 64
  const canvas = document.createElement('canvas')
  canvas.width = size
  canvas.height = size
  const context = canvas.getContext('2d')
  if (context) {
    context.strokeStyle = EDGE_COLOR
    context.lineWidth = 7
    // The line and its neighbours one tile over, so the corners join up.
    for (const offset of [-size, 0, size]) {
      context.beginPath()
      context.moveTo(offset, size)
      context.lineTo(offset + size, 0)
      context.stroke()
    }
  }
  const texture = new THREE.CanvasTexture(canvas)
  texture.wrapS = THREE.RepeatWrapping
  texture.wrapT = THREE.RepeatWrapping
  texture.colorSpace = THREE.SRGBColorSpace
  texture.anisotropy = 4
  sharedHatch = texture
  return texture
}
