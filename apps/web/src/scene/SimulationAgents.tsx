import { useThree } from '@react-three/fiber'
import { useEffect, useMemo } from 'react'
import * as THREE from 'three'
import { useSimulationAgents } from '../fire/fireStore'
import { AGENT_COLORS, isShownAgent } from '../fire/frames'
import { Z_UP_TO_Y_UP } from './placement'
import type { Floor } from './types'

const BODY_HEIGHT_M = 1.7
const UNIT_SPHERE = new THREE.SphereGeometry(1, 16, 12)
const MATERIALS = {
  occupant: new THREE.MeshStandardMaterial({ color: AGENT_COLORS.occupant, roughness: 0.8 }),
  firefighter: new THREE.MeshStandardMaterial({ color: AGENT_COLORS.firefighter, roughness: 0.8 }),
}
const noRaycast = () => {}

/** Simulation agents as upright ovals: occupants white, firefighters orange. */
export function SimulationAgents({ floor }: { floor: Floor }) {
  const simulationAgents = useSimulationAgents()
  const agents = useMemo(
    () =>
      simulationAgents.filter((agent) => isShownAgent(agent, floor.id)),
    [simulationAgents, floor.id],
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
          material={MATERIALS[agent.role.type]}
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
