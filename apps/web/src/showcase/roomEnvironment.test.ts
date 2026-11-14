import * as THREE from 'three'
import { describe, expect, it } from 'vitest'
import { createRoomEnvironment, disposeRoomEnvironment } from './roomEnvironment'

describe('createRoomEnvironment', () => {
  it('builds the room, its boxes, panels, and light from the given three', () => {
    const scene = createRoomEnvironment(THREE)
    const meshes: THREE.Mesh[] = []
    scene.traverse((object) => {
      if ((object as THREE.Mesh).isMesh) meshes.push(object as THREE.Mesh)
    })
    // Room + instanced boxes + six light panels.
    expect(meshes).toHaveLength(8)
    expect(meshes.filter((mesh) => (mesh as THREE.InstancedMesh).isInstancedMesh)[0]?.count).toBe(6)
    expect(scene.children.filter((child) => (child as THREE.PointLight).isPointLight)).toHaveLength(1)
    for (const mesh of meshes) expect(mesh).toBeInstanceOf(THREE.Mesh)
  })

  it('disposes every geometry and material', () => {
    const scene = createRoomEnvironment(THREE)
    const resources = new Set<THREE.BufferGeometry | THREE.Material>()
    scene.traverse((object) => {
      const mesh = object as THREE.Mesh
      if (mesh.isMesh) resources.add(mesh.geometry).add(mesh.material as THREE.Material)
    })
    let disposed = 0
    for (const resource of resources) resource.addEventListener('dispose', () => disposed++)
    disposeRoomEnvironment(scene)
    // One shared box geometry, room + box materials, six panel materials.
    expect(disposed).toBe(9)
  })
})
