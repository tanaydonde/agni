import * as THREE from 'three'
import { describe, expect, it } from 'vitest'
import type { AgentFrame, SimulationFrame } from '../fire/types'
import { orientation } from '../scene/cameraMath'
import { EYE_HEIGHT_M, firefighterStartSeconds, firstPersonPose, vantagePose } from './cameras'

const firefighter = (
  x: number,
  y: number,
  velocity: [number, number] = [0, 0],
  nozzle?: { x: number; y: number; z: number },
): AgentFrame => ({
  id: 'engine-1/1',
  floor_id: 'floor-1',
  position_m: { x, y, z: 0 },
  velocity_m_per_s: { x: velocity[0], y: velocity[1], z: 0 },
  radius_m: 0.3,
  exposure: { heat: 0, toxic_gas: 0 },
  role: { type: 'firefighter', crew_id: 'engine-1', phase: nozzle ? 'suppressing' : 'advancing', nozzle_target_m: nozzle },
})

const frame = (time_seconds: number, ...agents: AgentFrame[]): SimulationFrame => ({
  time_seconds,
  surface_fire: [],
  rooms: [],
  fires: [],
  openings: [],
  agents,
})

/** The canonical (x, y) direction a pose faces, ignoring pitch. */
const facing = (yaw: number) => {
  const forward = new THREE.Vector3(0, 0, -1).applyQuaternion(orientation(yaw, 0))
  return { x: forward.x, y: -forward.z }
}

describe('firstPersonPose', () => {
  const walking = [
    frame(0, firefighter(0, 0, [1, 0])),
    frame(2, firefighter(2, 0, [0, 1])),
    frame(4, firefighter(2, 2, [0, 0])),
    frame(6, firefighter(2, 2, [0, 0])),
  ]

  it('sits at eye height where the firefighter stands', () => {
    const pose = firstPersonPose(walking, 1)!
    expect(pose.position.x).toBeCloseTo(1)
    expect(pose.position.y).toBeCloseTo(EYE_HEIGHT_M)
    expect(pose.position.z).toBeCloseTo(0)
  })

  it('faces where the firefighter is heading', () => {
    expect(facing(firstPersonPose(walking, 0)!.yaw).x).toBeCloseTo(1)
    expect(facing(firstPersonPose(walking, 2.5)!.yaw).y).toBeCloseTo(1)
  })

  it('keeps facing the way they last walked once they stop', () => {
    const stopped = facing(firstPersonPose(walking, 5)!.yaw)
    expect(stopped.x).toBeCloseTo(0)
    expect(stopped.y).toBeCloseTo(1)
  })

  it('looks down the hose while spraying', () => {
    const spraying = [frame(0, firefighter(0, 0, [1, 0], { x: 0, y: -3, z: 0 }))]
    const pose = firstPersonPose(spraying, 0)!
    expect(facing(pose.yaw).y).toBeCloseTo(-1)
    expect(pose.pitch).toBeLessThan(-0.3)
  })

  it('is null without a crew', () => {
    expect(firstPersonPose([frame(0)], 0)).toBeNull()
  })
})

describe('firefighterStartSeconds', () => {
  it('is the last still moment before the first step', () => {
    const frames = [frame(0), frame(2, firefighter(1, 1)), frame(4, firefighter(1, 1)), frame(6, firefighter(2, 1))]
    expect(firefighterStartSeconds(frames)).toBe(4)
  })

  it('is when they appear if they arrive already moving', () => {
    expect(firefighterStartSeconds([frame(0), frame(2, firefighter(1, 1, [1, 0]))])).toBe(2)
  })

  it('is 0 without a crew or if they never move', () => {
    expect(firefighterStartSeconds([frame(0), frame(2)])).toBe(0)
    expect(firefighterStartSeconds([frame(0, firefighter(1, 1)), frame(2, firefighter(1, 1))])).toBe(0)
  })
})

describe('vantagePose', () => {
  const sweep = (x: number, z: number) => ({ position: { x, y: 1.5, z } })
  const focus = { x: 0, y: 0, z: 0 }

  it('stands at the nearest scan that keeps its distance, looking at the focus', () => {
    const pose = vantagePose('scene', [sweep(1, 0), sweep(4, 0), sweep(9, 0)], focus)!
    expect(pose.position.x).toBe(4)
    expect(facing(pose.yaw).x).toBeCloseTo(-1)
    expect(pose.pitch).toBeLessThan(0)
  })

  it('prefers the farthest scan inside the room the focus is in', () => {
    const room = [
      { x: -1, y: -1 },
      { x: 3, y: -1 },
      { x: 3, y: 1 },
      { x: -1, y: 1 },
    ]
    const pose = vantagePose('scene', [sweep(1, 0), sweep(2, 0), sweep(4, 0)], focus, room)!
    expect(pose.position.x).toBe(2)
  })

  it('takes the farthest scan when all are close, and none when there are none', () => {
    expect(vantagePose('scene', [sweep(1, 0), sweep(2, 0)], focus)!.position.x).toBe(2)
    expect(vantagePose('scene', [], focus)).toBeNull()
  })
})
