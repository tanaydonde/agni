// A neutral indoor scene to bake into an environment map, ported from
// three.js's examples/jsm/environments/RoomEnvironment.js (MIT License,
// Copyright 2010-2025 three.js authors). The original imports our copy of
// three, but Showcase can only render objects made with its own copy, so this
// version builds everything from the namespace it is given.

import type * as THREE from 'three'

type Three = typeof THREE

/** [position, rotation about Y, scale] of the six furniture-like boxes. */
const BOXES: [THREE.Vector3Tuple, number, THREE.Vector3Tuple][] = [
  [[-10.906, 2.009, 1.846], -0.195, [2.328, 7.905, 4.651]],
  [[-5.607, -0.754, -0.758], 0.994, [1.97, 1.534, 3.955]],
  [[6.167, 0.857, 7.803], 0.561, [3.927, 6.285, 3.687]],
  [[-2.017, 0.018, 6.124], 0.333, [2.002, 4.566, 2.064]],
  [[2.291, -0.756, -2.621], -0.286, [1.546, 1.552, 1.496]],
  [[-2.193, -0.369, -5.547], 0.516, [3.875, 3.487, 2.986]],
]

/** [intensity, position, scale] of the emissive panels acting as area lights. */
const PANELS: [number, THREE.Vector3Tuple, THREE.Vector3Tuple][] = [
  [50, [-16.116, 14.37, 8.208], [0.1, 2.428, 2.739]],
  [50, [-16.109, 18.021, -8.207], [0.1, 2.425, 2.751]],
  [17, [14.904, 12.198, -1.832], [0.15, 4.265, 6.331]],
  [43, [-0.462, 8.89, 14.52], [4.38, 5.441, 0.088]],
  [20, [3.235, 11.486, -12.541], [2.5, 2.0, 0.1]],
  [100, [0.0, 20.0, 0.0], [1.0, 0.1, 1.0]],
]

/** A grey room with a few boxes and bright panels, built from `three`. */
export function createRoomEnvironment(three: Three): THREE.Scene {
  const scene = new three.Scene()
  scene.name = 'RoomEnvironment'
  scene.position.y = -3.5

  const geometry = new three.BoxGeometry()
  geometry.deleteAttribute('uv')

  const mainLight = new three.PointLight(0xffffff, 900, 28, 2)
  mainLight.position.set(0.418, 16.199, 0.3)
  scene.add(mainLight)

  const room = new three.Mesh(geometry, new three.MeshStandardMaterial({ side: three.BackSide }))
  room.position.set(-0.757, 13.219, 0.717)
  room.scale.set(31.713, 28.305, 28.591)
  scene.add(room)

  const boxes = new three.InstancedMesh(geometry, new three.MeshStandardMaterial(), BOXES.length)
  const transform = new three.Object3D()
  BOXES.forEach(([position, yaw, scale], index) => {
    transform.position.set(...position)
    transform.rotation.set(0, yaw, 0)
    transform.scale.set(...scale)
    transform.updateMatrix()
    boxes.setMatrixAt(index, transform.matrix)
  })
  scene.add(boxes)

  for (const [intensity, position, scale] of PANELS) {
    // Emissive only: a flat light source in the reflections.
    const material = new three.MeshLambertMaterial({ color: 0x000000, emissive: 0xffffff, emissiveIntensity: intensity })
    const panel = new three.Mesh(geometry, material)
    panel.position.set(...position)
    panel.scale.set(...scale)
    scene.add(panel)
  }

  return scene
}

/** Frees the geometry and materials of a scene from `createRoomEnvironment`. */
export function disposeRoomEnvironment(scene: THREE.Scene): void {
  const resources = new Set<{ dispose(): void }>()
  scene.traverse((object) => {
    const mesh = object as THREE.Mesh
    if (!mesh.isMesh) return
    resources.add(mesh.geometry)
    for (const material of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
      resources.add(material)
    }
  })
  for (const resource of resources) resource.dispose()
}
