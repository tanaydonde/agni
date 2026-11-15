// Hose streams, drawn inside Showcase's own three.js scene. As in fire.ts,
// everything three.js here must come from Showcase's copy (`context.three`).

import type * as THREE from 'three'
import { createWaterRenderer, type WaterRenderer, type WaterStream } from '../fire/waterRenderer'
import { Z_UP_TO_Y_UP } from '../scene/placement'
import type { MpSdk, SceneComponent } from './sdk'

interface WaterInputs {
  streams: WaterStream[]
}

const COMPONENT_NAME = 'tflop.water'

/**
 * Hands the streams being sprayed to the water and keeps it moving. The
 * water is depth-tested, so walls and furniture hide what is behind them.
 */
class WaterComponent implements SceneComponent<WaterInputs> {
  inputs: WaterInputs = { streams: [] }
  declare outputs: { objectRoot?: THREE.Object3D | null }
  declare context: SceneComponent['context']
  private water?: WaterRenderer

  onInit() {
    const three = this.context!.three
    const root = new three.Group()
    root.rotation.set(...Z_UP_TO_Y_UP)
    this.water = createWaterRenderer(three)
    root.add(this.water.object)
    this.outputs.objectRoot = root
    this.water.setStreams(this.inputs.streams)
  }

  onInputsUpdated() {
    this.water?.setStreams(this.inputs.streams)
  }

  /** `deltaMs`: milliseconds since the last tick. */
  onTick(deltaMs: number) {
    this.water?.tick(Math.min(deltaMs / 1000, 0.1))
  }

  onDestroy() {
    this.water?.dispose()
  }
}

export interface ShowcaseWater {
  update(streams: WaterStream[]): void
  dispose(): void
}

/** Adds the hose streams to Showcase's scene. */
export async function mountWater(sdk: MpSdk, streams: WaterStream[]): Promise<ShowcaseWater> {
  const registration = await sdk.Scene.register(COMPONENT_NAME, () => new WaterComponent() as never)
  const [object] = await sdk.Scene.createObjects(1)
  const component = object.addNode().addComponent<WaterInputs>(COMPONENT_NAME, { streams })
  object.start()
  return {
    update(next) {
      component.inputs.streams = next
    },
    dispose() {
      object.stop()
      registration?.dispose()
    },
  }
}
