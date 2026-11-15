// The firefighter's route as an arrow on the floor, drawn inside Showcase's
// own three.js scene (everything three.js comes from `context.three`).

import type * as THREE from 'three'
import type { RouteRibbon } from '../demo/route'
import { createRouteRenderer, type RouteRenderer } from '../demo/routeRenderer'
import { Z_UP_TO_Y_UP } from '../scene/placement'
import type { MpSdk, SceneComponent } from './sdk'

interface RouteInputs {
  ribbon: RouteRibbon | null
  /** The playback clock: the route already walked is not drawn. */
  seconds: number
}

const COMPONENT_NAME = 'tflop.route'
class RouteComponent implements SceneComponent<RouteInputs> {
  inputs: RouteInputs = { ribbon: null, seconds: 0 }
  declare outputs: { objectRoot?: THREE.Object3D | null }
  declare context: SceneComponent['context']
  private route?: RouteRenderer

  onInit() {
    const three = this.context!.three
    const root = new three.Group()
    root.rotation.set(...Z_UP_TO_Y_UP)
    this.route = createRouteRenderer(three)
    root.add(this.route.object)
    this.outputs.objectRoot = root
    this.onInputsUpdated()
  }

  onInputsUpdated() {
    this.route?.setRoute(this.inputs.ribbon)
    this.route?.setClock(this.inputs.seconds)
  }

  /** `deltaMs`: milliseconds since the last tick. */
  onTick(deltaMs: number) {
    this.route?.tick(Math.min(deltaMs / 1000, 0.1))
  }

  onDestroy() {
    this.route?.dispose()
  }
}

export interface ShowcaseRoute {
  update(ribbon: RouteRibbon | null, seconds: number): void
  dispose(): void
}

/** Adds the route arrow to Showcase's scene. */
export async function mountRoute(sdk: MpSdk, ribbon: RouteRibbon | null, seconds: number): Promise<ShowcaseRoute> {
  const registration = await sdk.Scene.register(COMPONENT_NAME, () => new RouteComponent() as never)
  const [object] = await sdk.Scene.createObjects(1)
  const component = object.addNode().addComponent<RouteInputs>(COMPONENT_NAME, { ribbon, seconds })
  object.start()
  return {
    update(nextRibbon, nextSeconds) {
      component.inputs.ribbon = nextRibbon
      component.inputs.seconds = nextSeconds
    },
    dispose() {
      object.stop()
      registration?.dispose()
    },
  }
}
