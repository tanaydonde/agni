import { useFrame } from '@react-three/fiber'
import { useRef } from 'react'
import * as THREE from 'three'
import { useEditorStore } from './editorStore'
import { Z_UP_TO_Y_UP, type MoveHandle } from './placement'
import type { Floor } from './types'

/** Gizmo size relative to its distance from the camera, so it looks constant on screen. */
const SCREEN_SCALE = 0.13
/** Raise the gizmo slightly so it doesn't z-fight the floor. */
const LIFT_M = 0.01
const RENDER_ON_TOP = 999

const COLORS: Record<MoveHandle, string> = { x: '#e5484d', y: '#30a46c', plane: '#3e63dd' }
const ACTIVE_COLOR = '#f5c400'

const SCREEN_CENTER = new THREE.Vector2(0, 0)

/**
 * Unity-style move handles for the selected object, in the canonical Z-up
 * frame: a red arrow along +X, a green arrow along +Y, and a blue square for
 * free movement across the floor. There's no vertical arrow because furniture
 * always stands on the floor. Handles draw on top of everything and carry
 * `userData.handle` for picking (see SelectionController).
 */
export function MoveGizmo({ floor }: { floor: Floor }) {
  const selectedId = useEditorStore((state) => state.selectedObjectId)
  const drag = useEditorStore((state) => state.drag)
  const dragPosition = useEditorStore((state) => state.dragPosition)
  const hovered = useEditorStore((state) => state.hoveredHandle)
  const placing = useEditorStore((state) => state.placing)
  const root = useRef<THREE.Group>(null)
  const raycaster = useRef(new THREE.Raycaster())
  const worldPosition = useRef(new THREE.Vector3())

  const object = floor.objects.find((candidate) => candidate.id === selectedId)
  const visible = Boolean(object && object.mobility === 'movable' && !placing)

  useFrame(({ camera }) => {
    const group = root.current
    if (!group) {
      useEditorStore.getState().setHoveredHandle(null)
      return
    }
    group.getWorldPosition(worldPosition.current)
    group.scale.setScalar(camera.position.distanceTo(worldPosition.current) * SCREEN_SCALE)

    // Highlight the handle under the crosshair while flying (pointer locked).
    const store = useEditorStore.getState()
    let handle: MoveHandle | null = null
    if (document.pointerLockElement && !store.drag) {
      raycaster.current.setFromCamera(SCREEN_CENTER, camera)
      const hit = raycaster.current.intersectObject(group, true).find((h) => h.object.userData.handle)
      handle = (hit?.object.userData.handle as MoveHandle | undefined) ?? null
    }
    store.setHoveredHandle(handle)
  })

  if (!object || !visible) return null
  const position = (drag?.objectId === object.id && dragPosition) || object.transform.position_m
  const active = drag?.handle ?? hovered

  return (
    <group rotation={Z_UP_TO_Y_UP}>
      <group ref={root} position={[position.x, position.y, position.z + LIFT_M]}>
        {/* Arrows are built along +Y; turning -90° about Z points one along +X. */}
        <group rotation={[0, 0, -Math.PI / 2]}>
          <Arrow handle="x" active={active === 'x'} />
        </group>
        <Arrow handle="y" active={active === 'y'} />
        <PlaneHandle active={active === 'plane'} />
      </group>
    </group>
  )
}

function Arrow({ handle, active }: { handle: MoveHandle; active: boolean }) {
  const color = active ? ACTIVE_COLOR : COLORS[handle]
  return (
    <group>
      <mesh position={[0, 0.55, 0]} renderOrder={RENDER_ON_TOP}>
        <cylinderGeometry args={[0.025, 0.025, 0.8, 12]} />
        <OnTopMaterial color={color} />
      </mesh>
      <mesh position={[0, 1.06, 0]} renderOrder={RENDER_ON_TOP}>
        <coneGeometry args={[0.08, 0.22, 16]} />
        <OnTopMaterial color={color} />
      </mesh>
      {/* Invisible, fatter hit area so the arrow is easy to grab. */}
      <mesh position={[0, 0.65, 0]} userData={{ handle }}>
        <cylinderGeometry args={[0.1, 0.1, 1.1, 8]} />
        <meshBasicMaterial transparent opacity={0} depthWrite={false} depthTest={false} />
      </mesh>
    </group>
  )
}

function PlaneHandle({ active }: { active: boolean }) {
  return (
    <mesh position={[0.28, 0.28, 0]} userData={{ handle: 'plane' }} renderOrder={RENDER_ON_TOP}>
      <planeGeometry args={[0.3, 0.3]} />
      <OnTopMaterial color={active ? ACTIVE_COLOR : COLORS.plane} opacity={active ? 0.7 : 0.4} />
    </mesh>
  )
}

function OnTopMaterial({ color, opacity = 1 }: { color: string; opacity?: number }) {
  return (
    <meshBasicMaterial
      color={color}
      transparent
      opacity={opacity}
      depthTest={false}
      depthWrite={false}
      side={THREE.DoubleSide}
      toneMapped={false}
    />
  )
}
