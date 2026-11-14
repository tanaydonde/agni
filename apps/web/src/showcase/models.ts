// Furniture models drawn inside Showcase with its own glTF loader
// (`mp.gltfLoader`), so they use Showcase's copy of three.js.

import type { ModelPlacement } from '../scene/modelChoice'
import type { MpSdk, SceneObject } from './sdk'

const GLTF_LOADER = 'mp.gltfLoader'
/** Showcase's stock ambient and directional lights. */
const LIGHTS = 'mp.lights'

/** Node transform in Showcase's Y-up world for a Z-up placement. */
export function showcaseTransform(placement: ModelPlacement) {
  const { position, yawDegrees, scale } = placement
  // Z-up (x, y, z) is Y-up (x, z, −y); a turn about Z is the same turn about Y.
  const half = (yawDegrees * Math.PI) / 360
  return {
    position: { x: position.x, y: position.z, z: -position.y },
    quaternion: { x: 0, y: Math.sin(half), z: 0, w: Math.cos(half) },
    scale,
  }
}

export interface ShowcaseModels {
  update(placements: ModelPlacement[]): void
  dispose(): void
}

/**
 * Adds the placed models to Showcase. Showcase nodes can't take new
 * components once started, so each update replaces the whole set.
 */
export function mountModels(sdk: MpSdk, initial: ModelPlacement[]): ShowcaseModels {
  let current: SceneObject | null = null
  let generation = 0
  let disposed = false

  const build = async (placements: ModelPlacement[]) => {
    const mine = ++generation
    const [object] = placements.length > 0 ? await sdk.Scene.createObjects(1) : [null]
    // A newer update or dispose arrived while this one was being created.
    if (disposed || mine !== generation) {
      object?.stop()
      return
    }
    current?.stop()
    current = object
    if (!object) return
    // Showcase's scene has no lights of its own, and glTF materials are lit:
    // without these the models render black.
    object.addNode().addComponent(LIGHTS)
    for (const placement of placements) {
      const node = object.addNode()
      node.addComponent(GLTF_LOADER, {
        url: new URL(placement.model.url, window.location.origin).href,
        // Clicks should still walk through the space, not hit our models.
        colliderEnabled: false,
      })
      const { position, quaternion, scale } = showcaseTransform(placement)
      node.position.set(position.x, position.y, position.z)
      node.quaternion.set(quaternion.x, quaternion.y, quaternion.z, quaternion.w)
      node.scale.set(scale.x, scale.y, scale.z)
    }
    object.start()
  }

  void build(initial)
  return {
    update(placements) {
      void build(placements)
    },
    dispose() {
      disposed = true
      current?.stop()
      current = null
    },
  }
}
