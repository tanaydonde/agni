// Fire cells as plain boxes to draw, shared by the editor canvas and the
// Matterport views. Placeholder look: swap for a shader or particles later.

import type { FurnitureObject, Quaternion, Vec3 } from '../scene/types'
import type { SimulationFrame } from './types'

export const FIRE_COLOR = '#ff6a1a'
/** Opacity of cells that are heating but not yet burning. */
export const HEATING_OPACITY = 0.25
/** Grows cells a little so they don't z-fight with furniture faces. */
const CELL_INFLATE = 1.01
/** Floor cells are 2 cm thin; lift them clear of the floor mesh. */
const FLOOR_CELL_LIFT_M = 0.01
const IDENTITY: Quaternion = { x: 0, y: 0, z: 0, w: 1 }

/** One unit box to draw, in canonical Z-up coordinates. */
export interface FireCellBox {
  id: string
  burning: boolean
  position: Vec3
  rotation: Quaternion
  /** Size of the unit box. */
  scale: Vec3
}

/**
 * The heating and burning cells of `frame`. Heating cells show where the
 * fire is about to spread, so they're only drawn while something burns: once
 * the fire is out, nothing is drawn. Furniture cell sizes are in the
 * object's local axes, so those cells take the object's rotation.
 */
export function fireCellBoxes(
  frame: SimulationFrame | undefined,
  objects: Pick<FurnitureObject, 'id' | 'transform'>[],
): FireCellBox[] {
  if (!frame || !frame.surface_fire.some((node) => node.state === 'burning')) return []
  const rotations = new Map(objects.map((object) => [object.id, object.transform.rotation]))
  return frame.surface_fire.flatMap((node) => {
    if (node.state !== 'burning' && node.state !== 'heating') return []
    const { x, y, z } = node.position_m
    const { width_m, depth_m, height_m } = node.dimensions_m
    const objectId = node.source.type === 'furniture' ? node.source.object_id : null
    const furniture = objectId !== null
    return [
      {
        id: node.id,
        burning: node.state === 'burning',
        position: { x, y, z: furniture ? z : z + FLOOR_CELL_LIFT_M },
        rotation: (objectId !== null && rotations.get(objectId)) || IDENTITY,
        scale: { x: width_m * CELL_INFLATE, y: depth_m * CELL_INFLATE, z: height_m * CELL_INFLATE },
      },
    ]
  })
}

/** IDs of the furniture that has fire cells (heating or burning) in `frame`. */
export function burningObjectIds(frame: SimulationFrame | undefined): Set<string> {
  return new Set(
    frame?.surface_fire.flatMap((node) => (node.source.type === 'furniture' ? [node.source.object_id] : [])),
  )
}
