// Agents drawn inside Showcase's own three.js scene as their role's blob
// model (see ../scene/agentModels), with ovals until a model loads. People
// held up in the crowd glow red; people squeezing past glow amber. Their hose
// streams are drawn by ./water.

import type * as THREE from 'three'
import { AGENT_COLORS } from '../fire/frames'
import type { AgentFrame } from '../fire/types'
import {
  type AgentKind,
  type AgentTint,
  type BodyModel,
  TINT_COLORS,
  CARRY_HEIGHT_M,
  CARRY_SCALE,
  agentPose,
  agentScale,
  agentTint,
  byTint,
  loadAgentModels,
  nextHeadings,
  partGeometry,
  partMaterial,
} from '../scene/agentModels'
import { Z_UP_TO_Y_UP } from '../scene/placement'
import type { MpSdk, SceneComponent } from './sdk'

const COMPONENT_NAME = 'tflop.agents'
const BODY_HEIGHT_M = 1.7

interface AgentInputs {
  agents: AgentFrame[]
}

class AgentComponent implements SceneComponent<AgentInputs> {
  inputs: AgentInputs = { agents: [] }
  declare outputs: { objectRoot?: THREE.Object3D | null }
  declare context: SceneComponent['context']
  private root?: THREE.Group
  private oval?: {
    geometry: THREE.SphereGeometry
    materials: Record<AgentKind, Record<AgentTint, THREE.Material>>
  }
  /** Built with Showcase's three.js once the models arrive, in every tint. */
  private bodies: Partial<
    Record<AgentKind, Record<AgentTint, { geometry: THREE.BufferGeometry; material: THREE.Material }[]>>
  > = {}
  private headings = new Map<string, number>()
  private destroyed = false
  private readonly models: Promise<Partial<Record<AgentKind, BodyModel>>>

  constructor(models: Promise<Partial<Record<AgentKind, BodyModel>>>) {
    this.models = models
  }

  onInit() {
    const three = this.context!.three
    this.root = new three.Group()
    this.root.rotation.set(...Z_UP_TO_Y_UP)
    this.outputs.objectRoot = this.root
    const ovals = (color: string): Record<AgentTint, THREE.Material> =>
      byTint(
        (tint) => new three.MeshBasicMaterial({ color: tint === 'none' ? color : TINT_COLORS[tint] }),
      )
    this.oval = {
      geometry: new three.SphereGeometry(1, 16, 12),
      materials: {
        occupant: ovals(AGENT_COLORS.occupant),
        firefighter: ovals(AGENT_COLORS.firefighter),
      },
    }
    void this.models.then((models) => {
      if (this.destroyed) return
      // Lit by Showcase's scene environment (./environment).
      for (const [kind, model] of Object.entries(models) as [AgentKind, BodyModel][]) {
        const geometries = model.parts.map((part) => partGeometry(three, part))
        this.bodies[kind] = byTint((tint) =>
          model.parts.map((part, index) => ({
            geometry: geometries[index],
            material: partMaterial(three, part, tint),
          })),
        )
      }
      this.build()
    })
    this.build()
  }

  onInputsUpdated() {
    this.build()
  }

  onDestroy() {
    this.destroyed = true
    this.root?.clear()
    this.oval?.geometry.dispose()
    for (const tints of Object.values(this.oval?.materials ?? {})) {
      for (const material of Object.values(tints)) material.dispose()
    }
    for (const tints of Object.values(this.bodies)) {
      for (const body of Object.values(tints)) {
        for (const part of body) {
          part.geometry.dispose()
          part.material.dispose()
        }
      }
    }
  }

  private build() {
    const three = this.context?.three
    if (!three || !this.root || !this.oval) return
    this.root.clear()
    this.headings = nextHeadings(this.inputs.agents, this.headings)

    for (const agent of this.inputs.agents) {
      const tint = agentTint(agent)
      const pose = agentPose(agent)
      const lift = pose === 'carried' ? CARRY_HEIGHT_M : 0
      const shrink = pose === 'carried' ? CARRY_SCALE : 1
      const body = this.bodies[agent.role.type]?.[tint]
      if (body) {
        const group = new three.Group()
        group.name = agent.id
        group.position.set(agent.position_m.x, agent.position_m.y, agent.position_m.z + lift)
        group.rotation.set(0, 0, this.headings.get(agent.id) ?? 0)
        group.scale.setScalar(agentScale(agent.radius_m) * shrink)
        // Lying hurt or carried: the body on its side, resting on the floor.
        const posed = new three.Group()
        if (pose !== 'standing') {
          posed.position.set(0, 0, 1)
          posed.rotation.set(0, Math.PI / 2, 0)
        }
        for (const part of body) posed.add(new three.Mesh(part.geometry, part.material))
        group.add(posed)
        this.root.add(group)
        continue
      }
      const mesh = new three.Mesh(this.oval.geometry, this.oval.materials[agent.role.type][tint])
      mesh.name = agent.id
      if (pose === 'standing') {
        mesh.position.set(
          agent.position_m.x,
          agent.position_m.y,
          agent.position_m.z + BODY_HEIGHT_M / 2,
        )
        mesh.scale.set(agent.radius_m, agent.radius_m, BODY_HEIGHT_M / 2)
      } else {
        mesh.position.set(
          agent.position_m.x,
          agent.position_m.y,
          agent.position_m.z + lift + agent.radius_m,
        )
        mesh.scale.set(
          (BODY_HEIGHT_M / 2) * shrink,
          agent.radius_m * shrink,
          agent.radius_m * shrink,
        )
      }
      this.root.add(mesh)
    }
  }
}

export interface ShowcaseAgents {
  update(agents: AgentFrame[]): void
  dispose(): void
}

export async function mountAgents(sdk: MpSdk, agents: AgentFrame[]): Promise<ShowcaseAgents> {
  // Never rejects: a model that fails is logged and its role keeps ovals.
  const models = loadAgentModels()
  const registration = await sdk.Scene.register(COMPONENT_NAME, () => new AgentComponent(models) as never)
  const [object] = await sdk.Scene.createObjects(1)
  const component = object.addNode().addComponent<AgentInputs>(COMPONENT_NAME, { agents })
  object.start()
  return {
    update(next) {
      component.inputs.agents = next
    },
    dispose() {
      object.stop()
      registration?.dispose()
    },
  }
}
