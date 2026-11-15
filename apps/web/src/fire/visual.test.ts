import * as THREE from 'three'
import { describe, expect, it } from 'vitest'
import { createFireVisualResources, setFireVisualCell, setFireVisualTime } from './visual'

describe('procedural fire visuals', () => {
  it('builds a Z-up unit volume backed by filtered 3D noise', () => {
    const resources = createFireVisualResources(THREE)
    const positions = resources.flameGeometry.getAttribute('position')
    const zValues = Array.from({ length: positions.count }, (_, index) => positions.getZ(index))

    expect(Math.min(...zValues)).toBe(0)
    expect(Math.max(...zValues)).toBe(1)
    expect(resources.noiseTexture.image.width).toBe(32)
    expect(resources.noiseTexture.image.height).toBe(32)
    expect(resources.noiseTexture.image.depth).toBe(32)
    resources.dispose()
  })

  it('updates shared animation and per-cell uniforms', () => {
    const resources = createFireVisualResources(THREE)
    setFireVisualTime(resources, 4.5)
    setFireVisualCell(resources.flameMaterial, 0.2, 0.7, 0.6, { x: 1, y: 2, z: 3 })

    expect(resources.flameMaterial.uniforms.uTime.value).toBe(4.5)
    expect(resources.flameMaterial.uniforms.uSeed.value).toBe(0.2)
    expect(resources.flameMaterial.uniforms.uIntensity.value).toBe(0.7)
    expect(resources.flameMaterial.uniforms.uDensity.value).toBe(0.6)
    expect(resources.flameMaterial.uniforms.uCameraLocal.value.toArray()).toEqual([1, 2, 3])
    resources.dispose()
  })
})
