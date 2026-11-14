// Human-readable explanations for world API errors (crates/world-core
// `WorldApiError`, serialized as `{ code, ...details }`).

import type { WorldApiErrorBody } from './types'

/** Batch errors wrap the failing edit's error; return that inner error. */
export function unwrapWorldError(body: WorldApiErrorBody): WorldApiErrorBody {
  let current = body
  while (current.code === 'batch_edit_failed' && isErrorBody(current.reason)) current = current.reason
  return current
}

/** A short sentence explaining why the world rejected an edit. */
export function describeWorldError(body: WorldApiErrorBody): string {
  const error = unwrapWorldError(body)
  switch (error.code) {
    case 'object_outside_rooms':
      return 'Not inside a room'
    case 'object_outside_room':
      return "Doesn't fit inside the room"
    case 'furniture_overlap':
      return `Overlaps ${String(error.conflicting_object_id ?? 'other furniture')}`
    case 'wall_collision':
      return 'Hits a wall'
    case 'opening_clearance_blocked':
      return 'Blocks a doorway'
    case 'unsupported_furniture_rotation':
      return 'Furniture must stay upright'
    case 'object_fixed':
      return 'This item is fixed in place'
    case 'stale_revision':
      return 'The scene changed elsewhere; reloaded it'
    case 'object_not_found':
      return 'That item no longer exists'
    default:
      return `Edit rejected (${error.code})`
  }
}

function isErrorBody(value: unknown): value is WorldApiErrorBody {
  return typeof value === 'object' && value !== null && typeof (value as { code?: unknown }).code === 'string'
}
