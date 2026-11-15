import { useThree } from '@react-three/fiber'
import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import { FIRE_COLOR, HEATING_OPACITY, fireCellBoxes, type FireCellBox } from '../fire/cells'
import { useFireFrame } from '../fire/fireStore'
import type { SimulationFrame } from '../fire/types'
import { Z_UP_TO_Y_UP } from './placement'
import type { Floor } from './types'

const UNIT_BOX = new THREE.BoxGeometry(1, 1, 1)
const BURNING = new THREE.MeshBasicMaterial({ color: FIRE_COLOR })
const HEATING = new THREE.MeshBasicMaterial({
  color: FIRE_COLOR,
  transparent: true,
  opacity: HEATING_OPACITY,
  depthWrite: false,
})
const noRaycast = () => {}

/**
 * The fire at the playback clock, drawn on one floor as orange blocks. Cells
 * ignore the pointer, so they never get in the way of selecting furniture.
 */
export function SurfaceFire({ floor }: { floor: Floor }) {
  const frame = useFireFrame()
  return <SurfaceFireView floor={floor} frame={frame} />
}

/** Surface fire for an explicitly supplied review timeline. */
export function SurfaceFireView({
  floor,
  frame,
}: {
  floor: Floor
  frame: SimulationFrame | undefined
}) {
  const boxes = useMemo(() => fireCellBoxes(frame, floor.objects), [frame, floor.objects])
  // The canvas renders on demand; the clock runs outside it (usePlaybackClock).
  const invalidate = useThree((state) => state.invalidate)
  useEffect(() => invalidate(), [boxes, invalidate])

  return (
    <group name="surface-fire" rotation={Z_UP_TO_Y_UP}>
      {boxes.map((box) => (
        <FireCell key={box.id} box={box} />
      ))}
    </group>
  )
}

function FireCell({ box }: { box: FireCellBox }) {
  const { position: p, rotation: q, scale: s } = box
  return (
    <mesh
      geometry={UNIT_BOX}
      material={box.burning ? BURNING : HEATING}
      position={[p.x, p.y, p.z]}
      quaternion={[q.x, q.y, q.z, q.w]}
      scale={[s.x, s.y, s.z]}
      raycast={noRaycast}
    />
  )
}
