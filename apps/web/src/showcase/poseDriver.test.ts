import * as THREE from 'three'
import { describe, expect, it } from 'vitest'
import { easePose } from './poseDriver'

const pose = (x: number, yaw = 0, pitch = 0) => ({ position: new THREE.Vector3(x, 0, 0), yaw, pitch })

describe('easePose', () => {
  it('closes most of the gap within half a second, without overshooting', () => {
    const current = pose(0)
    const target = pose(1, 0.5, -0.2)
    for (let t = 0; t < 0.5; t += 1 / 60) easePose(current, target, 1 / 60)
    expect(current.position.x).toBeGreaterThan(0.9)
    expect(current.position.x).toBeLessThanOrEqual(1)
    expect(current.yaw).toBeGreaterThan(0.45)
    expect(current.pitch).toBeLessThan(-0.18)
  })

  it('turns the short way across ±π', () => {
    const current = pose(0, Math.PI - 0.1)
    easePose(current, pose(0, -Math.PI + 0.1), 1 / 60)
    expect(current.yaw).toBeGreaterThan(Math.PI - 0.1)
  })

  it('cuts to a far-off target, and to any target given unlimited time', () => {
    const far = pose(0)
    easePose(far, pose(10, 1, 0.3), 1 / 60)
    expect(far).toMatchObject({ yaw: 1, pitch: 0.3 })
    expect(far.position.x).toBe(10)

    const near = pose(0)
    easePose(near, pose(1, 1), Infinity)
    expect(near.position.x).toBe(1)
    expect(near.yaw).toBeCloseTo(1)
  })
})
