// The fire's placeholder orange blocks, drawn inside Showcase's own three.js
// scene. As in furniture.ts, everything three.js here must come from
// Showcase's copy (`context.three`).

import type * as THREE from 'three'
import { FIRE_COLOR, HEATING_OPACITY, type FireCellBox } from '../fire/cells'
import { Z_UP_TO_Y_UP } from '../scene/placement'
import type { MpSdk, SceneComponent } from './sdk'

interface FireInputs {
  cells: FireCellBox[]
}

const COMPONENT_NAME = 'tflop.fire'

/**
 * Rebuilds the cell meshes whenever `cells` changes. Cells are depth-tested
 * like any other object, so walls (and opaque furniture models) hide the
 * fire behind them.
 */
class FireComponent implements SceneComponent<FireInputs> {
  inputs: FireInputs = { cells: [] }
  declare outputs: { objectRoot?: THREE.Object3D | null }
  declare context: SceneComponent['context']
  private root?: THREE.Group
  private box?: THREE.BoxGeometry
  private burning?: THREE.Material
  private heating?: THREE.Material

  onInit() {
    const three = this.context!.three
    this.root = new three.Group()
    this.root.rotation.set(...Z_UP_TO_Y_UP)
    this.box = new three.BoxGeometry(1, 1, 1)
    this.burning = new three.MeshBasicMaterial({ color: FIRE_COLOR })
    this.heating = new three.MeshBasicMaterial({
      color: FIRE_COLOR,
      transparent: true,
      opacity: HEATING_OPACITY,
      depthWrite: false,
    })
    this.outputs.objectRoot = this.root
    this.build()
  }

  onInputsUpdated() {
    this.build()
  }

  onDestroy() {
    this.root?.clear()
    this.box?.dispose()
    this.burning?.dispose()
    this.heating?.dispose()
  }

  private build() {
    const three = this.context?.three
    if (!three || !this.root || !this.box) return
    this.root.clear()
    for (const cell of this.inputs.cells) {
      const mesh = new three.Mesh(this.box, cell.burning ? this.burning : this.heating)
      mesh.name = cell.id
      mesh.position.set(cell.position.x, cell.position.y, cell.position.z)
      mesh.quaternion.set(cell.rotation.x, cell.rotation.y, cell.rotation.z, cell.rotation.w)
      mesh.scale.set(cell.scale.x, cell.scale.y, cell.scale.z)
      this.root.add(mesh)
    }
  }
}

export interface ShowcaseFire {
  update(cells: FireCellBox[]): void
  dispose(): void
}

/** Adds the fire to Showcase's scene. */
export async function mountFire(sdk: MpSdk, cells: FireCellBox[]): Promise<ShowcaseFire> {
  const registration = await sdk.Scene.register(COMPONENT_NAME, () => new FireComponent() as never)
  const [object] = await sdk.Scene.createObjects(1)
  const component = object.addNode().addComponent<FireInputs>(COMPONENT_NAME, { cells })
  object.start()
  return {
    update(next) {
      component.inputs.cells = next
    },
    dispose() {
      object.stop()
      registration?.dispose()
    },
  }
}
