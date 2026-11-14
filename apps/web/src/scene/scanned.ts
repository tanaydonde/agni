// Scanned furniture lives in two places: the detection file (what the scan
// measured, used to pick real models for the Matterport views) and, merged by
// the server, the scene as `scan-<detected id>` objects that can be edited and
// can burn. These helpers keep the two in step.

import { yawDegrees } from './placement'
import type { DetectedObject, Floor, FurnitureObject } from './types'

/** Matches `SCANNED_OBJECT_PREFIX` in crates/furniture-detect. */
export const SCANNED_OBJECT_PREFIX = 'scan-'

export function isScannedObject(object: Pick<FurnitureObject, 'id'>): boolean {
  return object.id.startsWith(SCANNED_OBJECT_PREFIX)
}

/**
 * The detected objects as they stand in the scene now: moved or turned where
 * the scene moved them, and dropped if the scene deleted them. Detections the
 * server didn't merge (an older server, or no catalog match) pass through.
 */
export function detectedInScene(detected: DetectedObject[], floor: Pick<Floor, 'objects'>): DetectedObject[] {
  const scanned = new Map(floor.objects.filter(isScannedObject).map((object) => [object.id, object]))
  if (scanned.size === 0) return detected
  return detected.flatMap((object) => {
    const placed = scanned.get(`${SCANNED_OBJECT_PREFIX}${object.id}`)
    if (!placed) return []
    return [{ ...object, position_m: placed.transform.position_m, yaw_degrees: yawDegrees(placed.transform.rotation) }]
  })
}
