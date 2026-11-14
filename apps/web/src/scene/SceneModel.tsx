import { useLoader, useThree } from '@react-three/fiber'
import {
  Component,
  Suspense,
  useEffect,
  useLayoutEffect,
  useMemo,
  type ReactNode,
} from 'react'
import * as THREE from 'three'
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js'
import { mergeGeometries } from 'three/examples/jsm/utils/BufferGeometryUtils.js'
import { buildFloorGeometry, type FloorGeometry, type Point2, type WallPiece } from './geometry'
import type { Floor } from './types'

/** Rotates the canonical Z-up world into Three.js Y-up: (x, y, z) → (x, z, -y). */
const Z_UP_TO_Y_UP: [number, number, number] = [-Math.PI / 2, 0, 0]

const COLORS = {
  mesh: '#e9ecf0',
  floor: '#eef1f5',
  wall: '#ffffff',
  window: '#9ecbf0',
  separator: '#a3acb9',
}

interface SceneModelProps {
  floor: Floor
  /** Matterport's defurnished mesh, shown instead of the plain room shell. */
  meshUrl?: string
}

export function SceneModel({ floor, meshUrl }: SceneModelProps) {
  const geometry = useMemo(() => buildFloorGeometry(floor), [floor])
  const shell = <RoomShell geometry={geometry} />

  return (
    <>
      {meshUrl ? (
        // Show the shell while the mesh downloads, and keep it if loading fails.
        <FallbackOnError fallback={shell}>
          <Suspense fallback={shell}>
            <DefurnishedMesh url={meshUrl} />
          </Suspense>
        </FallbackOnError>
      ) : (
        shell
      )}
      <FrameFloor geometry={geometry} />
    </>
  )
}

/**
 * Matterport's defurnished mesh: the scanned space with furniture removed,
 * untextured, in the canonical Z-up frame. Its faces point into the rooms, so
 * culling back faces drops ceilings and far-side walls for a dollhouse view.
 */
function DefurnishedMesh({ url }: { url: string }) {
  const object = useLoader(OBJLoader, url)
  const geometries = useMemo(
    () =>
      object.children
        .filter((child): child is THREE.Mesh => child instanceof THREE.Mesh)
        .map((child) => child.geometry as THREE.BufferGeometry),
    [object],
  )

  return (
    <group rotation={Z_UP_TO_Y_UP}>
      {geometries.map((geometry) => (
        <mesh key={geometry.uuid} geometry={geometry}>
          <meshStandardMaterial color={COLORS.mesh} flatShading />
        </mesh>
      ))}
    </group>
  )
}

/** The plain room shell built from the structured walls and floors. */
function RoomShell({ geometry }: { geometry: FloorGeometry }) {
  const floorMesh = useOwnedGeometry(() => floorGeometry(geometry), geometry)
  const wallMesh = useOwnedGeometry(() => boxesGeometry(geometry.walls), geometry)
  const windowMesh = useOwnedGeometry(() => boxesGeometry(geometry.windows), geometry)
  const separatorLines = useOwnedGeometry(() => separatorGeometry(geometry), geometry)

  return (
    <group rotation={Z_UP_TO_Y_UP}>
      {floorMesh && (
        <mesh geometry={floorMesh}>
          <meshStandardMaterial color={COLORS.floor} side={THREE.DoubleSide} />
        </mesh>
      )}
      {wallMesh && (
        <mesh geometry={wallMesh}>
          <meshStandardMaterial color={COLORS.wall} />
        </mesh>
      )}
      {windowMesh && (
        <mesh geometry={windowMesh}>
          <meshStandardMaterial color={COLORS.window} transparent opacity={0.45} />
        </mesh>
      )}
      {separatorLines && (
        <lineSegments geometry={separatorLines}>
          <lineBasicMaterial color={COLORS.separator} />
        </lineSegments>
      )}
    </group>
  )
}

class FallbackOnError extends Component<{ fallback: ReactNode; children: ReactNode }> {
  state = { failed: false }

  static getDerivedStateFromError() {
    return { failed: true }
  }

  componentDidCatch(error: unknown) {
    console.error('Defurnished mesh failed to load; showing the room shell instead.', error)
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children
  }
}

/** Places the camera at a three-quarter view that fits the whole floor. */
function FrameFloor({ geometry }: { geometry: FloorGeometry }) {
  const getState = useThree((state) => state.get)
  const size = useThree((state) => state.size)

  useLayoutEffect(() => {
    const { camera, invalidate } = getState()
    if (!(camera instanceof THREE.PerspectiveCamera)) return
    const { min, max, height } = geometry.bounds
    const center = new THREE.Vector3(
      (min.x + max.x) / 2,
      geometry.floorZ + height / 2,
      -(min.y + max.y) / 2, // world y → three -z
    )
    const radius = Math.hypot(max.x - min.x, max.y - min.y, height) / 2

    const vFov = THREE.MathUtils.degToRad(camera.fov)
    const hFov = 2 * Math.atan(Math.tan(vFov / 2) * (size.width / size.height))
    const distance = (radius / Math.sin(Math.min(vFov, hFov) / 2)) * 1.05

    const elevation = THREE.MathUtils.degToRad(50)
    const azimuth = THREE.MathUtils.degToRad(35)
    camera.position.set(
      center.x + distance * Math.cos(elevation) * Math.sin(azimuth),
      center.y + distance * Math.sin(elevation),
      center.z + distance * Math.cos(elevation) * Math.cos(azimuth),
    )
    camera.near = Math.max(0.01, distance / 100)
    camera.far = distance * 10
    camera.lookAt(center)
    camera.updateProjectionMatrix()
    invalidate()
  }, [getState, geometry, size])

  return null
}

function floorGeometry({ roomOutlines, floorZ }: FloorGeometry): THREE.BufferGeometry | null {
  const parts = roomOutlines.map(([outer, ...holes]) => {
    const shape = new THREE.Shape(outer.map(toVector2))
    shape.holes = holes.map((hole) => new THREE.Path(hole.map(toVector2)))
    return new THREE.ShapeGeometry(shape).translate(0, 0, floorZ)
  })
  return merge(parts)
}

function boxesGeometry(pieces: WallPiece[]): THREE.BufferGeometry | null {
  const matrix = new THREE.Matrix4()
  const rotation = new THREE.Quaternion()
  const zAxis = new THREE.Vector3(0, 0, 1)
  const parts = pieces.map((piece) => {
    rotation.setFromAxisAngle(zAxis, piece.angle)
    matrix.compose(new THREE.Vector3(...piece.center), rotation, new THREE.Vector3(1, 1, 1))
    return new THREE.BoxGeometry(piece.length, piece.thickness, piece.height).applyMatrix4(matrix)
  })
  return merge(parts)
}

function separatorGeometry({ separators, floorZ }: FloorGeometry): THREE.BufferGeometry | null {
  if (separators.length === 0) return null
  const z = floorZ + 0.005 // just above the floor to avoid z-fighting
  const positions = separators.flatMap(([a, b]) => [a.x, a.y, z, b.x, b.y, z])
  return new THREE.BufferGeometry().setAttribute(
    'position',
    new THREE.Float32BufferAttribute(positions, 3),
  )
}

function merge(parts: THREE.BufferGeometry[]): THREE.BufferGeometry | null {
  if (parts.length === 0) return null
  const merged = mergeGeometries(parts)
  parts.forEach((part) => part.dispose())
  return merged
}

/** Builds a GPU geometry from `source` and disposes it when `source` changes. */
function useOwnedGeometry(
  build: () => THREE.BufferGeometry | null,
  source: FloorGeometry,
): THREE.BufferGeometry | null {
  // eslint-disable-next-line react-hooks/exhaustive-deps -- rebuilt only when the source changes
  const geometry = useMemo(build, [source])
  useEffect(() => () => geometry?.dispose(), [geometry])
  return geometry
}

function toVector2(point: Point2): THREE.Vector2 {
  return new THREE.Vector2(point.x, point.y)
}
