// Where the demo's two Matterport cameras stand: riding with the lead
// firefighter (first person), and a fixed spot to watch from (fly on the
// wall). Pure functions of the timeline; poses are in Three.js Y-up space,
// which is also Showcase's.

import * as THREE from 'three'
import { agentsAtTime, leadFirefighter } from '../fire/frames'
import type { AgentFrame, AgentTimelineFrame } from '../fire/types'
import { type CameraPose, yawPitchFor } from '../scene/cameraMath'
import type { Point2 } from '../scene/geometry'
import { toThree } from '../scene/placement'
import type { Vec3 } from '../scene/types'

export const EYE_HEIGHT_M = 1.6
/** The view faces where the firefighter will be this many seconds on. */
const LOOK_AHEAD_SECONDS = 1
/** Under this distance (m) or speed (m/s) the firefighter counts as standing still. */
const STILL = 0.05
/** Walking, the eyes are tipped slightly down, toward the floor ahead. */
const WALKING_PITCH = -0.12
/** The hose is aimed at the base of the fire. */
const NOZZLE_TARGET_HEIGHT_M = 0.3

/**
 * The lead firefighter's view at `seconds`: at eye height where they stand,
 * facing where they're heading, or at what they're spraying. Standing still,
 * they keep facing the way they last walked. Null when the run has no crew.
 */
export function firstPersonPose(frames: AgentTimelineFrame[], seconds: number): CameraPose | null {
  const lead = leadFirefighter(agentsAtTime(frames, seconds))
  if (!lead) return null
  const eye = { ...lead.position_m, z: lead.position_m.z + EYE_HEIGHT_M }
  const position = toThree(eye)

  if (lead.role.type === 'firefighter' && lead.role.nozzle_target_m) {
    const target = { ...lead.role.nozzle_target_m, z: lead.position_m.z + NOZZLE_TARGET_HEIGHT_M }
    const aim = toThree(target).sub(position)
    if (Math.hypot(aim.x, aim.z) > STILL) return { position, ...yawPitchFor(aim) }
  }

  const heading = headingAt(frames, seconds, lead)
  return { position, yaw: heading === null ? 0 : yawFor(heading), pitch: WALKING_PITCH }
}

/**
 * When the lead firefighter starts moving: the last moment they stand still
 * before their first step (or when they first appear, already moving). The
 * route plays from here, skipping the wait for the crew to arrive. 0 if they
 * never move or the run has no crew.
 */
export function firefighterStartSeconds(frames: AgentTimelineFrame[]): number {
  let previous: { seconds: number; position: Vec3 } | null = null
  for (const frame of frames) {
    const lead = leadFirefighter(frame.agents)
    if (!lead) continue
    const { position_m: position, velocity_m_per_s: velocity } = lead
    if (previous && Math.hypot(position.x - previous.position.x, position.y - previous.position.y) > STILL) {
      return previous.seconds
    }
    if (Math.hypot(velocity.x, velocity.y) > STILL) return frame.time_seconds
    previous = { seconds: frame.time_seconds, position }
  }
  return 0
}

/**
 * The horizontal direction (canonical x, y) the lead is heading at `seconds`:
 * toward where they'll be shortly, else the way they last moved before now,
 * else the way they first move after. Null if they never move.
 */
function headingAt(frames: AgentTimelineFrame[], seconds: number, lead: AgentFrame): { x: number; y: number } | null {
  const ahead = leadFirefighter(agentsAtTime(frames, seconds + LOOK_AHEAD_SECONDS))
  if (ahead) {
    const step = { x: ahead.position_m.x - lead.position_m.x, y: ahead.position_m.y - lead.position_m.y }
    if (Math.hypot(step.x, step.y) > STILL) return step
  }
  const velocityIn = (frame: AgentTimelineFrame) => {
    const { x, y } = frame.agents.find((agent) => agent.id === lead.id)?.velocity_m_per_s ?? { x: 0, y: 0 }
    return Math.hypot(x, y) > STILL ? { x, y } : null
  }
  const before = frames.filter((frame) => frame.time_seconds <= seconds)
  for (let index = before.length - 1; index >= 0; index--) {
    const velocity = velocityIn(before[index])
    if (velocity) return velocity
  }
  for (const frame of frames.slice(before.length)) {
    const velocity = velocityIn(frame)
    if (velocity) return velocity
  }
  return null
}

/** Camera yaw that faces along a canonical horizontal direction. */
function yawFor({ x, y }: { x: number; y: number }): number {
  return yawPitchFor(toThree({ x, y, z: 0 })).yaw
}

/** A hand-picked spot to watch a scene from, in canonical Z-up metres. */
export interface Vantage {
  position_m: Vec3
  look_at_m: Vec3
}

/**
 * Hand-picked vantage points by scene ID. A scene without one gets a scan
 * position chosen by `vantagePose`.
 */
export const VANTAGE_OVERRIDES: Record<string, Vantage> = {}

/** The nearest a chosen scan may be to what it watches, so the view takes in the room. */
const MIN_VANTAGE_DISTANCE_M = 2.5

/**
 * A fixed place to watch `focus` from: the scene's hand-picked vantage, else
 * a scan position (scans are where the photo textures are sharpest, and are
 * always inside). Of the scans inside `room` (the outline of the room the
 * focus is in), the one farthest from the focus, for the widest view of that
 * room. With none there, the scan nearest the focus that still stands back
 * from it, or failing that the farthest scan. Null with no scans at all.
 */
export function vantagePose(
  sceneId: string,
  sweeps: { position: { x: number; y: number; z: number } }[],
  focus: Vec3,
  room: Point2[] = [],
): CameraPose | null {
  const override = VANTAGE_OVERRIDES[sceneId]
  if (override) {
    const position = toThree(override.position_m)
    return { position, ...yawPitchFor(toThree(override.look_at_m).sub(position)) }
  }
  const target = toThree(focus)
  const candidates = sweeps.map(({ position: p }) => {
    const position = new THREE.Vector3(p.x, p.y, p.z)
    return { position, distance: Math.hypot(position.x - target.x, position.z - target.z) }
  })
  if (candidates.length === 0) return null
  const farthest = (among: typeof candidates) =>
    among.reduce((best, candidate) => (candidate.distance > best.distance ? candidate : best))
  // Three.js (x, z) → canonical (x, -z).
  const inRoom = candidates.filter(({ position: p }) => insidePolygon({ x: p.x, y: -p.z }, room))
  const standingBack = candidates.filter((candidate) => candidate.distance >= MIN_VANTAGE_DISTANCE_M)
  const { position } = inRoom.length
    ? farthest(inRoom)
    : standingBack.length
      ? standingBack.reduce((best, candidate) => (candidate.distance < best.distance ? candidate : best))
      : farthest(candidates)
  const look = target.clone().sub(position)
  return { position, ...(look.lengthSq() > 0 ? yawPitchFor(look) : { yaw: 0, pitch: 0 }) }
}

/** Whether `point` is inside the closed outline `polygon` (even-odd rule). */
function insidePolygon(point: Point2, polygon: Point2[]): boolean {
  let inside = false
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const a = polygon[i]
    const b = polygon[j]
    if (a.y > point.y !== b.y > point.y && point.x < ((b.x - a.x) * (point.y - a.y)) / (b.y - a.y) + a.x) {
      inside = !inside
    }
  }
  return inside
}
