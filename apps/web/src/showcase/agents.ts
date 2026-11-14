// Agent ovals drawn inside Showcase's own three.js scene: occupants white,
// firefighters orange.

import type * as THREE from 'three'
import { AGENT_COLORS } from '../fire/frames'
import type { AgentFrame } from '../fire/types'
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
  private owned: { dispose(): void }[] = []

  onInit() {
    const three = this.context!.three
    this.root = new three.Group()
    this.root.rotation.set(...Z_UP_TO_Y_UP)
    this.outputs.objectRoot = this.root
    this.build()
  }

  onInputsUpdated() {
    this.build()
  }

  onDestroy() {
    this.clear()
  }

  private build() {
    const three = this.context?.three
    if (!three || !this.root) return
    this.clear()
    const geometry = new three.SphereGeometry(1, 16, 12)
    const materials = {
      occupant: new three.MeshBasicMaterial({ color: AGENT_COLORS.occupant }),
      firefighter: new three.MeshBasicMaterial({ color: AGENT_COLORS.firefighter }),
    }
    this.owned.push(geometry, materials.occupant, materials.firefighter)

    for (const agent of this.inputs.agents) {
      const mesh = new three.Mesh(geometry, materials[agent.role.type])
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

  private clear() {
    this.root?.clear()
    for (const resource of this.owned) resource.dispose()
    this.owned = []
  }
}

export interface ShowcaseAgents {
  update(agents: AgentFrame[]): void
  dispose(): void
}

export async function mountAgents(sdk: MpSdk, agents: AgentFrame[]): Promise<ShowcaseAgents> {
  const registration = await sdk.Scene.register(COMPONENT_NAME, () => new AgentComponent() as never)
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
