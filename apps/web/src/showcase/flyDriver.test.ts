import * as THREE from 'three'
import { describe, expect, it } from 'vitest'
import { type FlyState, flyStep, nearestSweep, WALK_SPEED } from './flyDriver'

const fresh = (yaw = 0, pitch = 0): FlyState => ({
  position: new THREE.Vector3(),
  velocity: new THREE.Vector3(),
  yaw,
  pitch,
})

/** Steps `seconds` worth of 60 fps frames. */
function run(state: FlyState, keys: string[], seconds: number) {
  const held = new Set(keys)
  for (let t = 0; t < seconds; t += 1 / 60) flyStep(state, held, 1 / 60)
}

describe('flyStep', () => {
  it('walks forward along -Z at yaw 0, at walking speed once eased in', () => {
    const state = fresh()
    run(state, ['KeyW'], 2)
    expect(state.velocity.z).toBeCloseTo(-WALK_SPEED, 3)
    expect(state.position.x).toBeCloseTo(0, 6)
    expect(state.position.z).toBeLessThan(-2.5)
  })

  it('walks level even when looking down', () => {
    const state = fresh(0, -1.2)
    run(state, ['KeyW'], 1)
    expect(state.position.y).toBeCloseTo(0, 6)
    expect(state.position.z).toBeLessThan(0)
  })

  it('strafes right along +X and turns with yaw', () => {
    const right = fresh()
    run(right, ['KeyD'], 1)
    expect(right.position.x).toBeGreaterThan(0)

    // Facing +X (yaw -90°), W walks along +X.
    const turned = fresh(-Math.PI / 2)
    run(turned, ['KeyW'], 1)
    expect(turned.position.x).toBeGreaterThan(0)
    expect(Math.abs(turned.position.z)).toBeLessThan(1e-6)
  })

  it('goes straight up and down with E and Q', () => {
    const state = fresh(0.7, -0.4)
    run(state, ['KeyE'], 1)
    expect(state.position.y).toBeGreaterThan(0)
    expect(Math.hypot(state.position.x, state.position.z)).toBeLessThan(1e-6)
  })

  it('moves faster with Shift, and no faster on diagonals', () => {
    const boosted = fresh()
    run(boosted, ['KeyW', 'ShiftLeft'], 2)
    expect(boosted.velocity.length()).toBeCloseTo(WALK_SPEED * 3, 2)

    const diagonal = fresh()
    run(diagonal, ['KeyW', 'KeyD'], 2)
    expect(diagonal.velocity.length()).toBeCloseTo(WALK_SPEED, 3)
  })

  it('eases to a stop after the keys are released', () => {
    const state = fresh()
    run(state, ['KeyW'], 1)
    const stoppedAt = state.position.clone()
    run(state, [], 2)
    expect(state.velocity.length()).toBe(0)
    expect(state.position.distanceTo(stoppedAt)).toBeLessThan(0.3)
  })

  it('caps a long frame so a stall does not jump the camera', () => {
    const state = fresh()
    state.velocity.set(0, 0, -WALK_SPEED)
    flyStep(state, new Set(['KeyW']), 5)
    expect(state.position.length()).toBeLessThanOrEqual(WALK_SPEED * 0.1 + 1e-9)
  })
})

describe('nearestSweep', () => {
  it('picks the closest scan, or null with none', () => {
    const sweeps = [
      { sid: 'far', position: { x: 10, y: 1.5, z: 0 } },
      { sid: 'near', position: { x: 1, y: 1.5, z: 1 } },
    ]
    expect(nearestSweep(sweeps, new THREE.Vector3(0, 1.5, 0))).toBe('near')
    expect(nearestSweep([], new THREE.Vector3())).toBeNull()
  })
})
