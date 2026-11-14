// Picks and places a furniture model for each piece of scanned furniture.
// Pure functions in the canonical Z-up world, so they can be tested without
// a renderer.

import type { BoundaryEdge, DetectedObject, Dimensions3, FurnitureModel, Vec3 } from './types'

/** A model standing in for one detected box. */
export interface ModelPlacement {
  objectId: string
  model: FurnitureModel
  /** Bottom centre of the box, Z-up. */
  position: Vec3
  /** Rotation about Z of the model's +X axis, from world +X. */
  yawDegrees: number
  /**
   * Stretch along the model's own axes (glTF: X width, Y height, Z depth)
   * so it fills the measured box exactly, like the hitbox it stands for.
   */
  scale: { x: number; y: number; z: number }
}

/**
 * The model for a detected box: one with the same label, the closest in
 * size. Null when no model has that label (beds, desks, unknown objects stay
 * boxes rather than become something they aren't).
 */
export function chooseModel(object: DetectedObject, models: FurnitureModel[]): FurnitureModel | null {
  let best: FurnitureModel | null = null
  let bestDistance = Infinity
  for (const model of models) {
    if (model.label !== object.label) continue
    const distance = sizeDistance(object.size_m, model.size_m)
    if (distance < bestDistance) {
      best = model
      bestDistance = distance
    }
  }
  return best
}

/**
 * How unlike two sizes are: summed absolute log ratios of long side, short
 * side, and height, so twice too big counts the same as half too small and
 * orientation doesn't matter.
 */
export function sizeDistance(a: Dimensions3, b: Dimensions3): number {
  const sorted = (size: Dimensions3) => [
    Math.max(size.width_m, size.depth_m),
    Math.min(size.width_m, size.depth_m),
    size.height_m,
  ]
  const [left, right] = [sorted(a), sorted(b)]
  return left.reduce((sum, value, axis) => sum + Math.abs(Math.log(Math.max(value, 0.01) / Math.max(right[axis], 0.01))), 0)
}

/**
 * Places `model` in `object`'s box. A box has no front, so the model's front
 * (+Z in glTF, which lands on the box's −depth side) is turned away from the
 * nearer wall: sofas and cabinets usually stand with their backs to one.
 */
export function placeModel(object: DetectedObject, model: FurnitureModel, walls: BoundaryEdge[]): ModelPlacement {
  const yaw = object.yaw_degrees
  const theta = (yaw * Math.PI) / 180
  const depthAxis = { x: -Math.sin(theta), y: Math.cos(theta) }
  const center = object.position_m
  const behindDefaultFront = distanceToWall(center, depthAxis, walls)
  const inFrontOfDefault = distanceToWall(center, { x: -depthAxis.x, y: -depthAxis.y }, walls)
  // Default front faces −depthAxis; turn around if the wall is on that side.
  const turnAround = inFrontOfDefault < behindDefaultFront

  return {
    objectId: object.id,
    model,
    position: center,
    yawDegrees: turnAround ? yaw + 180 : yaw,
    scale: {
      x: object.size_m.width_m / model.size_m.width_m,
      y: object.size_m.height_m / model.size_m.height_m,
      z: object.size_m.depth_m / model.size_m.depth_m,
    },
  }
}

/** Models for every detected box that has a sensible one. */
export function placeModels(
  objects: DetectedObject[],
  models: FurnitureModel[],
  walls: BoundaryEdge[],
): ModelPlacement[] {
  return objects.flatMap((object) => {
    const model = chooseModel(object, models)
    return model ? [placeModel(object, model, walls)] : []
  })
}

/** Distance along a ray in the plan to the nearest wall edge, or Infinity. */
export function distanceToWall(origin: Vec3, direction: { x: number; y: number }, walls: BoundaryEdge[]): number {
  let nearest = Infinity
  for (const wall of walls) {
    if (wall.kind !== 'wall') continue
    const ex = wall.end.x - wall.start.x
    const ey = wall.end.y - wall.start.y
    const denominator = direction.x * ey - direction.y * ex
    if (Math.abs(denominator) < 1e-12) continue
    const ox = wall.start.x - origin.x
    const oy = wall.start.y - origin.y
    const along = (ox * ey - oy * ex) / denominator
    const onEdge = (ox * direction.y - oy * direction.x) / denominator
    if (along > 0 && onEdge >= 0 && onEdge <= 1) nearest = Math.min(nearest, along)
  }
  return nearest
}
