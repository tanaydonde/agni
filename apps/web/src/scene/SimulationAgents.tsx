import { useThree } from '@react-three/fiber'
import { useEffect, useMemo, useState } from 'react'
import * as THREE from 'three'
import { useSimulationAgents } from '../fire/fireStore'
import { AGENT_COLORS, isShownAgent } from '../fire/frames'
import {
  type AgentKind,
  type AgentTint,
  type BodyModel,
  TINT_COLORS,
  agentScale,
  agentTint,
  byTint,
  loadAgentModels,
  nextHeadings,
  partGeometry,
  partMaterial,
} from './agentModels'
import { Z_UP_TO_Y_UP } from './placement'
import type { Floor } from './types'

const BODY_HEIGHT_M = 1.7
const UNIT_SPHERE = new THREE.SphereGeometry(1, 16, 12)
function ovalMaterials(color: string): Record<AgentTint, THREE.Material> {
  return byTint(
    (tint) =>
      new THREE.MeshStandardMaterial({
        color,
        roughness: 0.8,
        ...(tint === 'none' ? {} : { emissive: TINT_COLORS[tint], emissiveIntensity: 0.55 }),
      }),
  )
}
const MATERIALS = {
  occupant: ovalMaterials(AGENT_COLORS.occupant),
  firefighter: ovalMaterials(AGENT_COLORS.firefighter),
}
const noRaycast = () => {}

/**
 * Simulation agents as their role's blob model (see ./agentModels), drawn
 * AGENT_DRAW_SCALE times their collision circle and turned to face where they
 * walk. Upright ovals stand in until a model loads, or if it fails to.
 * People held up in the crowd glow red; people squeezing past glow amber.
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
        const tint = agentTint(agent)
        const body = bodies[agent.role.type]?.[tint]
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
            material={MATERIALS[agent.role.type][tint]}
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

/** Each role's model as meshes in every tint, once loaded; a role is missing until then. */
function useAgentBodies(): Partial<Record<AgentKind, Record<AgentTint, BodyMeshes>>> {
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
        Object.entries(models).map(([kind, model]) => {
          // Tints share each part's geometry and differ only in material.
          const geometries = model.parts.map((part) => partGeometry(THREE, part))
          return [
            kind,
            byTint((tint) =>
              model.parts.map((part, index) => ({
                geometry: geometries[index],
                material: partMaterial(THREE, part, tint),
              })),
            ),
          ]
        }),
      ) as Partial<Record<AgentKind, Record<AgentTint, BodyMeshes>>>,
    [models],
  )
  useEffect(
    () => () => {
      for (const tints of Object.values(bodies)) {
        for (const body of Object.values(tints)) {
          for (const part of body) {
            // Shared across tints; disposing twice is harmless.
            part.geometry.dispose()
            part.material.dispose()
          }
        }
      }
    },
    [bodies],
  )
  return bodies
}
