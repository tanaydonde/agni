import { useThree } from '@react-three/fiber'
import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import { useFireFrame } from '../fire/fireStore'
import { Z_UP_TO_Y_UP } from './placement'
import type { Floor } from './types'

const BODY_HEIGHT_M = 1.7
const UNIT_SPHERE = new THREE.SphereGeometry(1, 16, 12)
const WHITE = new THREE.MeshStandardMaterial({ color: '#ffffff', roughness: 0.8 })
const noRaycast = () => {}

/** Stationary simulation agents, drawn as white upright ovals. */
export function SimulationAgents({ floor }: { floor: Floor }) {
  const frame = useFireFrame()
  const agents = useMemo(
    () => frame?.agents.filter((agent) => agent.floor_id === floor.id) ?? [],
    [frame, floor.id],
  )
  const invalidate = useThree((state) => state.invalidate)
  useEffect(() => invalidate(), [agents, invalidate])

  return (
    <group name="simulation-agents" rotation={Z_UP_TO_Y_UP}>
      {agents.map((agent) => (
        <mesh
          key={agent.id}
          name={agent.id}
          geometry={UNIT_SPHERE}
          material={WHITE}
          position={[
            agent.position_m.x,
            agent.position_m.y,
            agent.position_m.z + BODY_HEIGHT_M / 2,
          ]}
          scale={[agent.radius_m, agent.radius_m, BODY_HEIGHT_M / 2]}
          raycast={noRaycast}
        />
      ))}
    </group>
  )
}
