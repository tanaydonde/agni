// Our placed furniture, drawn inside Showcase's own three.js scene (read-only).
// Showcase requires every three.js object it renders to come from its copy of
// three (`context.three`), so nothing here may use our `three` import at runtime.

import type * as THREE from 'three'
import { categoryColor, EDGE_COLOR } from '../scene/furnitureStyle'
import { Z_UP_TO_Y_UP, zRotation } from '../scene/placement'
import type { DetectedObject, Dimensions3, Floor, FurnitureAsset, Quaternion, Vec3 } from '../scene/types'
import type { MpSdk, SceneComponent } from './sdk'

/** One box to draw, in canonical Z-up coordinates. */
export interface FurnitureItem {
  id: string
  position: Vec3
  rotation: Quaternion
  scale: Vec3
  size: Dimensions3
  color: string
  /** Below 1 draws the box see-through, like scanned furniture in the editor. */
  opacity?: number
}

/** The floor's placed furniture as boxes, the way the editor draws them. */
export function furnitureItems(floor: Pick<Floor, 'objects'>, assets: FurnitureAsset[]): FurnitureItem[] {
  const assetsById = new Map(assets.map((asset) => [asset.id, asset]))
  return floor.objects.flatMap((object) => {
    const asset = assetsById.get(object.asset_id)
    if (!asset) return []
    return [
      {
        id: object.id,
        position: object.transform.position_m,
        rotation: object.transform.rotation,
        scale: object.transform.scale,
        size: asset.dimensions_m,
        color: categoryColor(asset.category),
      },
    ]
  })
}

/** Furniture detected in the scan, as see-through boxes of its measured size. */
export function detectedFurnitureItems(objects: DetectedObject[], assets: FurnitureAsset[]): FurnitureItem[] {
  const categories = new Map(assets.map((asset) => [asset.id, asset.category]))
  return objects.map((object) => ({
    id: object.id,
    position: object.position_m,
    rotation: zRotation(object.yaw_degrees),
    scale: { x: 1, y: 1, z: 1 },
    size: object.size_m,
    color: categoryColor((object.asset_id && categories.get(object.asset_id)) || ''),
    opacity: DETECTED_OPACITY,
  }))
}

/** Matches the editor's scanned-furniture boxes. */
const DETECTED_OPACITY = 0.55

interface FurnitureInputs {
  items: FurnitureItem[]
}

const COMPONENT_NAME = 'tflop.furniture'

/** Rebuilds a group of boxes whenever its `items` input changes. */
class FurnitureComponent implements SceneComponent<FurnitureInputs> {
  inputs: FurnitureInputs = { items: [] }
  // Showcase fills these in before onInit.
  declare outputs: { objectRoot?: THREE.Object3D | null }
  declare context: SceneComponent['context']
  private root?: THREE.Group
  private owned: { dispose(): void }[] = []

  onInit() {
    const three = this.context!.three
    this.root = new three.Group()
    // Same Z-up → Y-up turn as the editor's furniture group.
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
    const edgeMaterial = new three.LineBasicMaterial({ color: EDGE_COLOR })
    this.owned.push(edgeMaterial)
    for (const item of this.inputs.items) {
      const { width_m: width, depth_m: depth, height_m: height } = item.size
      // Bottom face at the origin, i.e. standing on the floor.
      const box = new three.BoxGeometry(width, depth, height).translate(0, 0, height / 2)
      const edges = new three.EdgesGeometry(box)
      const opacity = item.opacity ?? 1
      const material = new three.MeshBasicMaterial({
        color: item.color,
        transparent: opacity < 1,
        opacity,
        depthWrite: opacity >= 1,
      })
      this.owned.push(box, edges, material)

      const group = new three.Group()
      group.name = item.id
      group.position.set(item.position.x, item.position.y, item.position.z)
      group.quaternion.set(item.rotation.x, item.rotation.y, item.rotation.z, item.rotation.w)
      group.scale.set(item.scale.x, item.scale.y, item.scale.z)
      group.add(new three.Mesh(box, material), new three.LineSegments(edges, edgeMaterial))
      this.root.add(group)
    }
  }

  private clear() {
    this.root?.clear()
    for (const resource of this.owned) resource.dispose()
    this.owned = []
  }
}

export interface ShowcaseFurniture {
  update(items: FurnitureItem[]): void
  dispose(): void
}

/** Adds the furniture to Showcase's scene. */
export async function mountFurniture(sdk: MpSdk, items: FurnitureItem[]): Promise<ShowcaseFurniture> {
  const registration = await sdk.Scene.register(COMPONENT_NAME, () => new FurnitureComponent() as never)
  const [object] = await sdk.Scene.createObjects(1)
  const component = object.addNode().addComponent<FurnitureInputs>(COMPONENT_NAME, { items })
  object.start()
  return {
    update(next) {
      component.inputs.items = next
    },
    dispose() {
      object.stop()
      registration?.dispose()
    },
  }
}
