import { describe, expect, it } from 'vitest'
import { homePose } from './cameraMath'
import {
  fovFromProjection,
  orbitForFov,
  orbitHome,
  orbitPosition,
  orbitYawPitch,
  panOrbit,
  rotateOrbit,
  zoomOrbit,
  type Orbit,
} from './orbitCamera'

const bounds = { min: { x: 0, y: 0 }, max: { x: 10, y: 6 }, height: 2.5 }

describe('orbitHome', () => {
  it('starts exactly where the 3D overview does', () => {
    const home = orbitHome(bounds, 0, 50, 16 / 9)
    const pose = homePose(bounds, 0, 50, 16 / 9)
    const position = orbitPosition(home)
    expect(position.x).toBeCloseTo(pose.position.x)
    expect(position.y).toBeCloseTo(pose.position.y)
    expect(position.z).toBeCloseTo(pose.position.z)
    const { yaw, pitch } = orbitYawPitch(home)
    expect(yaw).toBeCloseTo(pose.yaw)
    expect(pitch).toBeCloseTo(pose.pitch)
  })

  it('targets the middle of the floor', () => {
    const { target } = orbitHome(bounds, 0, 50, 1)
    expect(target.x).toBeCloseTo(5)
    expect(target.z).toBeCloseTo(-3) // canonical y = 3 → Three.js z = -3
  })
})

const base: Orbit = { target: { x: 0, y: 0, z: 0 }, azimuth: 0, elevation: Math.PI / 4, distance: 10 }

describe('orbit controls', () => {
  it('keeps the distance to the target while rotating, and clamps elevation', () => {
    const turned = rotateOrbit(base, 120, -40)
    expect(orbitPosition(turned).length()).toBeCloseTo(10)
    expect(rotateOrbit(base, 0, 10_000).elevation).toBeLessThan(Math.PI / 2)
    expect(rotateOrbit(base, 0, -10_000).elevation).toBeGreaterThan(0)
  })

  it('zooms out on wheel down and in on wheel up, within limits', () => {
    expect(zoomOrbit(base, 100, 1, 100).distance).toBeGreaterThan(10)
    expect(zoomOrbit(base, -100, 1, 100).distance).toBeLessThan(10)
    expect(zoomOrbit(base, 1e6, 1, 100).distance).toBe(100)
    expect(zoomOrbit(base, -1e6, 1, 100).distance).toBe(1)
  })

  it('pans along the ground so the scene follows the pointer', () => {
    // Camera at azimuth 0 sits on +z looking toward -z: screen right is +x.
    const right = panOrbit(base, 100, 0, 50, 800)
    expect(right.target.x).toBeLessThan(0) // target moves left, scene moves right
    expect(right.target.y).toBe(0)
    const down = panOrbit(base, 0, 100, 50, 800)
    expect(down.target.z).toBeLessThan(0) // target moves away, scene moves down
  })
})

describe('field of view', () => {
  it('reads the vertical field of view from a projection matrix', () => {
    // Showcase's dollhouse camera reported projection[5] = 1.3032 (about 75°).
    expect(fovFromProjection([0.53, 0, 0, 0, 0, 1.3032, 0, 0])).toBeCloseTo(75, 0)
    expect(fovFromProjection([1, 0, 0, 0, 0, 0])).toBeNull()
  })

  it('moves a wider camera closer so the scene fills the view the same way', () => {
    const wide = orbitForFov(base, 75)
    expect(wide.distance).toBeLessThan(base.distance)
    expect(wide.distance * Math.tan((75 * Math.PI) / 360)).toBeCloseTo(base.distance * Math.tan((50 * Math.PI) / 360))
    expect(orbitForFov(base, 50).distance).toBeCloseTo(base.distance)
  })
})
