import { useFrame } from '@react-three/fiber'
import { useRef, type ReactNode } from 'react'
import * as THREE from 'three'
import { useEditorStore } from './editorStore'
import { GIZMO_GROUP, Z_UP_TO_Y_UP, type GizmoHandle } from './placement'
import type { Floor } from './types'

/** Gizmo size relative to its distance from the camera, so it looks constant on screen. */
const SCREEN_SCALE = 0.13
/** Raise the gizmo slightly so it doesn't z-fight the floor. */
const LIFT_M = 0.01
const RENDER_ON_TOP = 999
/** Ring radius, in gizmo units; just outside the arrow tips. */
const RING_RADIUS = 1.4

const COLORS: Record<GizmoHandle, string> = {
  x: '#e5484d',
  y: '#30a46c',
  plane: '#8b8d98',
  rotate: '#3e63dd',
}
const ACTIVE_COLOR = '#f5c400'

/**
 * Unity-style handles for the selected object, lying on the floor in the
 * canonical Z-up frame:
 * - red arrow: slide along +X
 * - green arrow: slide along +Y
 * - grey square: slide freely across the floor
 * - blue ring: turn about the vertical axis
 *
 * There's no vertical arrow or tilt ring because furniture always stands
 * upright on the floor. Handles draw on top of everything and carry
 * `userData.handle` for picking (see SelectionController).
 */
export function TransformGizmo({ floor }: { floor: Floor }) {
  const selectedId = useEditorStore((state) => state.selectedObjectId)
  const drag = useEditorStore((state) => state.drag)
  const preview = useEditorStore((state) => state.preview)
  const hovered = useEditorStore((state) => state.hoveredHandle)
  const placing = useEditorStore((state) => state.placing)
  const root = useRef<THREE.Group>(null)
  const worldPosition = useRef(new THREE.Vector3())

  useFrame(({ camera }) => {
    const group = root.current
    if (!group) return
    group.getWorldPosition(worldPosition.current)
    group.scale.setScalar(camera.position.distanceTo(worldPosition.current) * SCREEN_SCALE)
  })

  const object = floor.objects.find((candidate) => candidate.id === selectedId)
  if (!object || object.mobility !== 'movable' || placing) return null
  const position = (drag?.objectId === object.id && preview?.position) || object.transform.position_m
  const active = drag?.handle ?? hovered

  return (
    <group rotation={Z_UP_TO_Y_UP}>
      <group ref={root} name={GIZMO_GROUP} position={[position.x, position.y, position.z + LIFT_M]}>
        {/* Arrows are built along +Y; turning -90° about Z points one along +X. */}
        <group rotation={[0, 0, -Math.PI / 2]}>
          <Arrow handle="x" active={active === 'x'} />
        </group>
        <Arrow handle="y" active={active === 'y'} />
        <PlaneHandle active={active === 'plane'} />
        <RotateRing active={active === 'rotate'} />
      </group>
    </group>
  )
}

function Arrow({ handle, active }: { handle: GizmoHandle; active: boolean }) {
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
      <HitArea handle={handle} position={[0, 0.65, 0]}>
        <cylinderGeometry args={[0.1, 0.1, 1.1, 8]} />
      </HitArea>
    </group>
  )
}

function PlaneHandle({ active }: { active: boolean }) {
  return (
    <mesh position={[0.28, 0.28, 0]} userData={{ handle: 'plane' }} renderOrder={RENDER_ON_TOP}>
      <planeGeometry args={[0.3, 0.3]} />
      <OnTopMaterial color={active ? ACTIVE_COLOR : COLORS.plane} opacity={active ? 0.75 : 0.45} />
    </mesh>
  )
}

/** A ring flat on the floor; dragging around it turns the object. */
function RotateRing({ active }: { active: boolean }) {
  return (
    <group>
      {/* TorusGeometry lies in the XY plane: the floor, in this frame. */}
      <mesh renderOrder={RENDER_ON_TOP}>
        <torusGeometry args={[RING_RADIUS, active ? 0.035 : 0.022, 8, 96]} />
        <OnTopMaterial color={active ? ACTIVE_COLOR : COLORS.rotate} />
      </mesh>
      <HitArea handle="rotate">
        <torusGeometry args={[RING_RADIUS, 0.1, 6, 48]} />
      </HitArea>
    </group>
  )
}

/** An invisible, fatter mesh that makes a thin handle easy to grab. */
function HitArea({
  handle,
  position,
  children,
}: {
  handle: GizmoHandle
  position?: [number, number, number]
  children: ReactNode
}) {
  return (
    <mesh position={position} userData={{ handle }}>
      {children}
      <meshBasicMaterial transparent opacity={0} depthWrite={false} depthTest={false} side={THREE.DoubleSide} />
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
