// Draws a route arrow (route.ts) on the floor. Written against a passed-in
// three.js so it can live inside Matterport's Showcase, which has its own copy.

import type * as THREE from 'three'
import type { RouteRibbon } from './route'

const ROUTE_COLOR = '#22d36f'

const VERTEX_SHADER = /* glsl */ `
attribute float aAcross;
attribute float aAlong;
attribute float aSeconds;
varying float vAcross;
varying float vAlong;
varying float vSeconds;

void main() {
  vAcross = aAcross;
  vAlong = aAlong;
  vSeconds = aSeconds;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`

const FRAGMENT_SHADER = /* glsl */ `
uniform vec3 uColor;
uniform float uNow;
uniform float uTime;
varying float vAcross;
varying float vAlong;
varying float vSeconds;

void main() {
  // Only the route still ahead, starting just behind the firefighter.
  float ahead = smoothstep(-0.4, 0.4, vSeconds - uNow);
  if (ahead < 0.01) discard;
  // Chevrons that run along the route the way to walk it.
  float phase = fract((vAlong + abs(vAcross) * 0.9) / 0.7 - uTime * 1.1);
  float chevron = smoothstep(0.0, 0.08, phase) * (1.0 - smoothstep(0.4, 0.48, phase));
  gl_FragColor = vec4(mix(uColor, vec3(1.0), chevron * 0.75), 0.92 * ahead);
}
`

export interface RouteRenderer {
  /** Add to a group that is in canonical Z-up metres. */
  object: THREE.Object3D
  setRoute(ribbon: RouteRibbon | null): void
  /** The playback clock: the route already walked is not drawn. */
  setClock(seconds: number): void
  /** Moves the chevrons along. */
  tick(deltaSeconds: number): void
  dispose(): void
}

/**
 * The arrow is depth-tested, so furniture and walls hide the route behind
 * them, and it writes depth, so where the route retraces itself only the
 * part walked sooner shows.
 */
export function createRouteRenderer(three: typeof THREE): RouteRenderer {
  const uniforms = { uColor: { value: new three.Color(ROUTE_COLOR) }, uNow: { value: 0 }, uTime: { value: 0 } }
  const material = new three.ShaderMaterial({
    uniforms,
    vertexShader: VERTEX_SHADER,
    fragmentShader: FRAGMENT_SHADER,
    side: three.DoubleSide,
    transparent: true,
    depthFunc: three.LessDepth,
  })
  const mesh = new three.Mesh(new three.BufferGeometry(), material)
  mesh.name = 'route'
  mesh.visible = false
  mesh.frustumCulled = false
  mesh.raycast = () => {}
  let shown: RouteRibbon | null = null

  return {
    object: mesh,

    setRoute(ribbon) {
      if (ribbon === shown) return
      shown = ribbon
      mesh.geometry.dispose()
      const geometry = new three.BufferGeometry()
      if (ribbon) {
        // These copy into arrays made by `three`'s own window. Showcase's
        // three.js rejects typed arrays from ours: they fail its `instanceof`.
        geometry.setAttribute('position', new three.Float32BufferAttribute(ribbon.positions, 3))
        geometry.setAttribute('aAcross', new three.Float32BufferAttribute(ribbon.across, 1))
        geometry.setAttribute('aAlong', new three.Float32BufferAttribute(ribbon.along, 1))
        geometry.setAttribute('aSeconds', new three.Float32BufferAttribute(ribbon.seconds, 1))
        geometry.setIndex(new three.Uint32BufferAttribute(ribbon.index, 1))
      }
      mesh.geometry = geometry
      mesh.visible = ribbon !== null
    },

    setClock(seconds) {
      uniforms.uNow.value = seconds
    },

    tick(deltaSeconds) {
      uniforms.uTime.value += deltaSeconds
    },

    dispose() {
      mesh.geometry.dispose()
      material.dispose()
    },
  }
}
