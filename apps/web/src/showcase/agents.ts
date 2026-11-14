// Agents drawn inside Showcase's own three.js scene as their role's blob
// model (see ../scene/agentModels), with ovals until a model loads.

import type * as THREE from 'three'
import { AGENT_COLORS } from '../fire/frames'
import type { AgentFrame } from '../fire/types'
import {
  type AgentKind,
  type BodyModel,
  agentScale,
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
    materials: Record<AgentKind, THREE.Material>
  }
  /** Built with Showcase's three.js once the models arrive. */
  private bodies: Partial<Record<AgentKind, { geometry: THREE.BufferGeometry; material: THREE.Material }[]>> = {}
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
    this.oval = {
      geometry: new three.SphereGeometry(1, 16, 12),
      materials: {
        occupant: new three.MeshBasicMaterial({ color: AGENT_COLORS.occupant }),
        firefighter: new three.MeshBasicMaterial({ color: AGENT_COLORS.firefighter }),
      },
    }
    void this.models.then((models) => {
      if (this.destroyed) return
      // Lit by Showcase's scene environment (./environment).
      for (const [kind, model] of Object.entries(models) as [AgentKind, BodyModel][]) {
        this.bodies[kind] = model.parts.map((part) => ({
          geometry: partGeometry(three, part),
          material: partMaterial(three, part),
        }))
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
    for (const material of Object.values(this.oval?.materials ?? {})) material.dispose()
    for (const body of Object.values(this.bodies)) {
      for (const part of body) {
        part.geometry.dispose()
        part.material.dispose()
      }
    }
  }

  private build() {
    const three = this.context?.three
    if (!three || !this.root || !this.oval) return
    this.root.clear()
    this.headings = nextHeadings(this.inputs.agents, this.headings)

    for (const agent of this.inputs.agents) {
      const body = this.bodies[agent.role.type]
      if (body) {
        const group = new three.Group()
        group.name = agent.id
        group.position.set(agent.position_m.x, agent.position_m.y, agent.position_m.z)
        group.rotation.set(0, 0, this.headings.get(agent.id) ?? 0)
        group.scale.setScalar(agentScale(agent.radius_m))
        for (const part of body) group.add(new three.Mesh(part.geometry, part.material))
        this.root.add(group)
        continue
      }
      const mesh = new three.Mesh(this.oval.geometry, this.oval.materials[agent.role.type])
      mesh.name = agent.id
      mesh.position.set(
        agent.position_m.x,
        agent.position_m.y,
        agent.position_m.z + BODY_HEIGHT_M / 2,
      )
      mesh.scale.set(agent.radius_m, agent.radius_m, BODY_HEIGHT_M / 2)
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
