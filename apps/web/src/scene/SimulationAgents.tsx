import { useThree } from '@react-three/fiber'
import { useEffect, useMemo, useState } from 'react'
import * as THREE from 'three'
import { useSimulationAgents } from '../fire/fireStore'
import { AGENT_COLORS, isShownAgent } from '../fire/frames'
import {
  type AgentKind,
  type BodyModel,
  agentScale,
  loadAgentModels,
  nextHeadings,
  partGeometry,
  partMaterial,
} from './agentModels'
import { Z_UP_TO_Y_UP } from './placement'
import type { Floor } from './types'

const BODY_HEIGHT_M = 1.7
const UNIT_SPHERE = new THREE.SphereGeometry(1, 16, 12)
const MATERIALS = {
  occupant: new THREE.MeshStandardMaterial({ color: AGENT_COLORS.occupant, roughness: 0.8 }),
  firefighter: new THREE.MeshStandardMaterial({ color: AGENT_COLORS.firefighter, roughness: 0.8 }),
}
const noRaycast = () => {}

/**
 * Simulation agents as their role's blob model (see ./agentModels), drawn
 * AGENT_DRAW_SCALE times their collision circle and turned to face where they
 * walk. Upright ovals stand in until a model loads, or if it fails to.
 */
export function SimulationAgents({ floor }: { floor: Floor }) {
  const simulationAgents = useSimulationAgents()
  const agents = useMemo(
    () =>
      simulationAgents.filter((agent) => isShownAgent(agent, floor.id)),
    [simulationAgents, floor.id],
  )
  const bodies = useAgentBodies()
  // Headings carry over between frames so a stopped agent keeps facing
  // where it walked.
  const [turned, setTurned] = useState(() => ({ agents, headings: nextHeadings(agents, new Map()) }))
  if (turned.agents !== agents) setTurned({ agents, headings: nextHeadings(agents, turned.headings) })
  const { headings } = turned
  const invalidate = useThree((state) => state.invalidate)
  useEffect(() => invalidate(), [agents, bodies, invalidate])

  return (
    <group name="simulation-agents" rotation={Z_UP_TO_Y_UP}>
      {agents.map((agent) => {
        const body = bodies[agent.role.type]
        return body ? (
          <group
            key={agent.id}
            name={agent.id}
            position={[agent.position_m.x, agent.position_m.y, agent.position_m.z]}
            rotation={[0, 0, headings.get(agent.id) ?? 0]}
            scale={agentScale(agent.radius_m)}
          >
            {body.map((part, index) => (
              <mesh key={index} geometry={part.geometry} material={part.material} raycast={noRaycast} />
            ))}
          </group>
        ) : (
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
        )
      })}
    </group>
  )
}

type BodyMeshes = { geometry: THREE.BufferGeometry; material: THREE.Material }[]

/** Each role's model as meshes, once loaded; a role is missing until then. */
function useAgentBodies(): Partial<Record<AgentKind, BodyMeshes>> {
  const [models, setModels] = useState<Partial<Record<AgentKind, BodyModel>>>({})
  useEffect(() => {
    let live = true
    void loadAgentModels().then((loaded) => live && setModels(loaded))
    return () => {
      live = false
    }
  }, [])

  const bodies = useMemo(
    () =>
      Object.fromEntries(
        Object.entries(models).map(([kind, model]) => [
          kind,
          model.parts.map((part) => ({
            geometry: partGeometry(THREE, part),
            material: partMaterial(THREE, part),
          })),
        ]),
      ) as Partial<Record<AgentKind, BodyMeshes>>,
    [models],
  )
  useEffect(
    () => () => {
      for (const body of Object.values(bodies)) {
        for (const part of body) {
          part.geometry.dispose()
          part.material.dispose()
        }
      }
    },
    [bodies],
  )
  return bodies
}
