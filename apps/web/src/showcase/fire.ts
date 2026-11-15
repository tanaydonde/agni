// The volumetric fire, drawn inside Showcase's own three.js scene. As in
// furniture.ts, everything three.js here must come from Showcase's copy
// (`context.three`).

import type * as THREE from 'three'
import type { FireVolume } from '../fire/volume'
import { createFireVolumeRenderer, type FireVolumeRenderer } from '../fire/volumeRenderer'
import { Z_UP_TO_Y_UP } from '../scene/placement'
import type { MpSdk, SceneComponent } from './sdk'

interface FireInputs {
  volume: FireVolume | null
}

const COMPONENT_NAME = 'tflop.fire'

/**
 * Hands each new `volume` to the flames and keeps them moving. The flames
 * are depth-tested, so walls (and opaque furniture models) hide the fire
 * behind them.
 */
class FireComponent implements SceneComponent<FireInputs> {
  inputs: FireInputs = { volume: null }
  declare outputs: { objectRoot?: THREE.Object3D | null }
  declare context: SceneComponent['context']
  private fire?: FireVolumeRenderer

  onInit() {
    const three = this.context!.three
    const root = new three.Group()
    root.rotation.set(...Z_UP_TO_Y_UP)
    this.fire = createFireVolumeRenderer(three)
    root.add(this.fire.object)
    this.outputs.objectRoot = root
    this.fire.setVolume(this.inputs.volume)
  }

  onInputsUpdated() {
    this.fire?.setVolume(this.inputs.volume)
  }

  /** `deltaMs`: milliseconds since the last tick. */
  onTick(deltaMs: number) {
    this.fire?.tick(Math.min(deltaMs / 1000, 0.1))
  }

  onDestroy() {
    this.fire?.dispose()
  }
}

export interface ShowcaseFire {
  update(volume: FireVolume | null): void
  dispose(): void
}

/** Adds the fire to Showcase's scene. */
export async function mountFire(sdk: MpSdk, volume: FireVolume | null): Promise<ShowcaseFire> {
  const registration = await sdk.Scene.register(COMPONENT_NAME, () => new FireComponent() as never)
  const [object] = await sdk.Scene.createObjects(1)
  const component = object.addNode().addComponent<FireInputs>(COMPONENT_NAME, { volume })
  object.start()
  return {
    update(next) {
      component.inputs.volume = next
    },
    dispose() {
      object.stop()
      registration?.dispose()
    },
  }
}
