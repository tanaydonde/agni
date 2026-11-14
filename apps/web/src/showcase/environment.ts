// Image-based lighting for our furniture models inside Showcase: a neutral
// indoor environment baked into Showcase's scene, so PBR materials (many of
// the models are fully metallic) reflect something instead of rendering
// black. The scan's own lighting is baked into its photos and is unaffected.

import type * as THREE from 'three'
import { createRoomEnvironment, disposeRoomEnvironment } from './roomEnvironment'
import type { MpSdk, SceneComponent } from './sdk'

const COMPONENT_NAME = 'tflop.environment'

/** How strongly the environment lights the models. */
export const ENVIRONMENT_INTENSITY = 1

/** Sets Showcase's `scene.environment` while it exists; restores it after. */
class EnvironmentComponent implements SceneComponent {
  // Showcase fills this in before onInit. No `inputs`: an empty object makes
  // addComponent throw (see SceneComponent).
  declare context: SceneComponent['context']
  private texture?: THREE.Texture
  private previous?: { environment: THREE.Texture | null; intensity: number }

  onInit() {
    const { three, renderer, scene } = this.context!
    const room = createRoomEnvironment(three)
    const generator = new three.PMREMGenerator(renderer)
    this.texture = generator.fromScene(room, 0.04).texture
    generator.dispose()
    disposeRoomEnvironment(room)

    this.previous = { environment: scene.environment, intensity: scene.environmentIntensity }
    scene.environment = this.texture
    scene.environmentIntensity = ENVIRONMENT_INTENSITY
  }

  onDestroy() {
    const scene = this.context?.scene
    if (scene && this.previous) {
      scene.environment = this.previous.environment
      scene.environmentIntensity = this.previous.intensity
    }
    this.texture?.dispose()
  }
}

/** Lights Showcase's scene with the environment until disposed. */
export async function mountEnvironment(sdk: MpSdk): Promise<{ dispose(): void }> {
  const registration = await sdk.Scene.register(COMPONENT_NAME, () => new EnvironmentComponent() as never)
  const [object] = await sdk.Scene.createObjects(1)
  object.addNode().addComponent(COMPONENT_NAME)
  object.start()
  return {
    dispose() {
      object.stop()
      registration?.dispose()
    },
  }
}
