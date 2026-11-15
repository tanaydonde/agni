import { useFrame, useThree } from '@react-three/fiber'
import { useEffect, useMemo, useState } from 'react'
import * as THREE from 'three'
import { useSimulationAgents } from '../fire/fireStore'
import { AGENT_COLORS, isShownAgent, leadFirefighter } from '../fire/frames'
import type { AgentFrame } from '../fire/types'
import { createWaterRenderer } from '../fire/waterRenderer'
import {
  type AgentKind,
  type AgentPose,
  type AgentTint,
  type BodyModel,
  CARRY_HEIGHT_M,
  CARRY_SCALE,
  TINT_COLORS,
  agentPose,
  agentScale,
  agentTint,
  byTint,
  loadAgentModels,
  nextHeadings,
  hoseStream,
  partGeometry,
  partMaterial,
} from './agentModels'
import { Z_UP_TO_Y_UP } from './placement'
import type { Floor } from './types'

const BODY_HEIGHT_M = 1.7
const UNIT_SPHERE = new THREE.SphereGeometry(1, 16, 12)
/** Longest step the water takes in one frame, so it doesn't jump after a pause. */
const MAX_TICK_SECONDS = 0.1
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
/** Turns a standing body (Z-up, footprint radius 1) onto its side, resting on the floor. */
const ON_ITS_SIDE: [number, number, number] = [0, Math.PI / 2, 0]

/**
 * Simulation agents as their role's blob model (see ./agentModels), drawn
 * AGENT_DRAW_SCALE times their collision circle and turned to face where they
 * walk. Upright ovals stand in until a model loads, or if it fails to.
 * People held up in the crowd glow red; people squeezing past glow amber.
 * Injured people lie on the floor; people being carried lie across their
 * firefighter's shoulder. A spraying firefighter throws a stream of water (HoseStreams).
 */
export function SimulationAgents({
  floor,
  hideLeadFirefighter = false,
}: {
  floor: Floor
  hideLeadFirefighter?: boolean
}) {
  const simulationAgents = useSimulationAgents()
  return (
    <SimulationAgentsView
      floor={floor}
      simulationAgents={simulationAgents}
      hideLeadFirefighter={hideLeadFirefighter}
    />
  )
}

/** Agent rendering for an explicitly supplied review timeline. */
export function SimulationAgentsView({
  floor,
  simulationAgents,
  hideLeadFirefighter = false,
}: {
  floor: Floor
  simulationAgents: AgentFrame[]
  hideLeadFirefighter?: boolean
}) {
  const agents = useMemo(
    () =>
      simulationAgents.filter((agent) => isShownAgent(agent, floor.id)),
    [simulationAgents, floor.id],
  )
  const bodies = useAgentBodies()
  const leadFirefighterId = hideLeadFirefighter ? leadFirefighter(agents)?.id : undefined
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
        if (agent.id === leadFirefighterId) return null
        const tint = agentTint(agent)
        const pose = agentPose(agent)
        const body = bodies[agent.role.type]?.[tint]
        const lift = pose === 'carried' ? CARRY_HEIGHT_M : 0
        const position: [number, number, number] = [
          agent.position_m.x,
          agent.position_m.y,
          agent.position_m.z + lift,
        ]
        const shrink = pose === 'carried' ? CARRY_SCALE : 1
        return body ? (
          <group
            key={agent.id}
            name={agent.id}
            position={position}
            rotation={[0, 0, headings.get(agent.id) ?? 0]}
            scale={agentScale(agent.radius_m) * shrink}
          >
            <group {...sideways(pose)}>
              {body.map((part, index) => (
                <mesh key={index} geometry={part.geometry} material={part.material} raycast={noRaycast} />
              ))}
            </group>
          </group>
        ) : (
          <mesh
            key={agent.id}
            name={agent.id}
            geometry={UNIT_SPHERE}
            material={MATERIALS[agent.role.type][tint]}
            position={[
              position[0],
              position[1],
              position[2] + (pose === 'standing' ? BODY_HEIGHT_M / 2 : agent.radius_m),
            ]}
            scale={
              pose === 'standing'
                ? [agent.radius_m, agent.radius_m, BODY_HEIGHT_M / 2]
                : [(BODY_HEIGHT_M / 2) * shrink, agent.radius_m * shrink, agent.radius_m * shrink]
            }
            raycast={noRaycast}
          />
        )
      })}
      <HoseStreams agents={agents} />
    </group>
  )
}

/** The water every spraying firefighter in `agents` throws. Goes inside a Z-up group. */
function HoseStreams({ agents }: { agents: AgentFrame[] }) {
  const streams = useMemo(() => agents.flatMap((agent) => hoseStream(agent) ?? []), [agents])
  const water = useMemo(() => createWaterRenderer(THREE), [])
  useEffect(() => () => water.dispose(), [water])

  const invalidate = useThree((state) => state.invalidate)
  useEffect(() => {
    water.setStreams(streams)
    invalidate()
  }, [water, streams, invalidate])
  // Water moves on its own, so keep drawing for as long as any is in the air.
  useFrame((_, delta) => {
    if (water.tick(Math.min(delta, MAX_TICK_SECONDS))) invalidate()
  })

  return <primitive object={water.object} />
}

/** Lays a body on its side for lying and carried poses; a standing body is left alone. */
function sideways(pose: AgentPose): {
  position?: [number, number, number]
  rotation?: [number, number, number]
} {
  return pose === 'standing' ? {} : { position: [0, 0, 1], rotation: ON_ITS_SIDE }
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
