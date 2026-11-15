// Our placed furniture, drawn inside Showcase's own three.js scene (read-only).
// Showcase requires every three.js object it renders to come from its copy of
// three (`context.three`), so nothing here may use our `three` import at runtime.

import type * as THREE from 'three'
import { categoryColor, EDGE_COLOR } from '../scene/furnitureStyle'
import { Z_UP_TO_Y_UP } from '../scene/placement'
import { isScannedObject } from '../scene/scanned'
import type { Dimensions3, Floor, FurnitureAsset, Quaternion, Vec3 } from '../scene/types'
import type { MpSdk, SceneComponent } from './sdk'

/** One box to draw, in canonical Z-up coordinates. */
export interface FurnitureItem {
  id: string
  position: Vec3
  rotation: Quaternion
  scale: Vec3
  size: Dimensions3
  color: string
  /** Has fire cells; drawn see-through so they show (walls still hide them). */
  burning: boolean
}

/**
 * The floor's placed furniture as boxes, the way the editor draws them.
 * Scanned furniture is left out: real models stand in for it here.
 */
export function furnitureItems(
  floor: Pick<Floor, 'objects'>,
  assets: FurnitureAsset[],
  burning: ReadonlySet<string> = new Set(),
): FurnitureItem[] {
  const assetsById = new Map(assets.map((asset) => [asset.id, asset]))
  return floor.objects.flatMap((object) => {
    const asset = assetsById.get(object.asset_id)
    if (!asset || isScannedObject(object)) return []
    return [
      {
        id: object.id,
        position: object.transform.position_m,
        rotation: object.transform.rotation,
        scale: object.transform.scale,
        size: asset.dimensions_m,
        color: categoryColor(asset.category),
        burning: burning.has(object.id),
      },
    ]
  })
}

interface FurnitureInputs {
  items: FurnitureItem[]
}

const COMPONENT_NAME = 'tflop.furniture'
/** Same as the editor's burning furniture. */
const BURNING_OPACITY = 0.3

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
      const material = new three.MeshBasicMaterial(
        item.burning
          ? { color: item.color, transparent: true, opacity: BURNING_OPACITY, depthWrite: false }
          : { color: item.color },
      )
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
