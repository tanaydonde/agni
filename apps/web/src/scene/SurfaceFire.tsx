import { useFrame, useThree } from '@react-three/fiber'
import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import { fireCellBoxes } from '../fire/cells'
import { useFireFrame } from '../fire/fireStore'
import type { SimulationFrame } from '../fire/types'
import { fireVolume } from '../fire/volume'
import { createFireVolumeRenderer } from '../fire/volumeRenderer'
import { Z_UP_TO_Y_UP } from './placement'
import type { Floor } from './types'

/** Longest step the flames take in one frame, so they don't jump after a pause. */
const MAX_TICK_SECONDS = 0.1

/**
 * The fire at the playback clock, drawn on one floor as volumetric flames.
 * They ignore the pointer, so they never get in the way of selecting furniture.
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
  const volume = useMemo(() => fireVolume(fireCellBoxes(frame, floor.objects)), [frame, floor.objects])
  const fire = useMemo(() => createFireVolumeRenderer(THREE), [])
  useEffect(() => () => fire.dispose(), [fire])

  // The canvas renders on demand; the clock runs outside it (usePlaybackClock).
  const invalidate = useThree((state) => state.invalidate)
  useEffect(() => {
    fire.setVolume(volume)
    invalidate()
  }, [fire, volume, invalidate])
  // Flames move on their own, so keep drawing for as long as there are any.
  useFrame((_, delta) => {
    if (fire.tick(Math.min(delta, MAX_TICK_SECONDS))) invalidate()
  })

  return (
    <group name="surface-fire" rotation={Z_UP_TO_Y_UP}>
      <primitive object={fire.object} />
    </group>
  )
}
