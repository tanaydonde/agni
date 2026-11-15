// Volumetric fire: a box around the fire whose fragment shader marches each
// view ray through the heat field (volume.ts) and builds flames out of
// rising noise. Written against a passed-in three.js so the editor canvas and
// Matterport's Showcase (which has its own copy) share it.

import type * as THREE from 'three'
import { blitField, unionGrid, voxelCount, type FireVolume, type VoxelGrid } from './volume'

/** Seconds to ease from one simulation frame's fire to the next. */
const BLEND_SECONDS = 0.4
const NOISE_SIZE = 32

const VERTEX_SHADER = /* glsl */ `
uniform vec3 uSize;
varying vec3 vPosition;
varying vec3 vCamera;

void main() {
  // Both in the box's own metres, centred on the box, z up.
  vPosition = position * uSize;
  vCamera = (inverse(modelMatrix) * vec4(cameraPosition, 1.0)).xyz * uSize;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`

const FRAGMENT_SHADER = /* glsl */ `
precision highp sampler3D;

#define STEPS 110

uniform sampler3D uField; // r, g: flame heat before and after; b, a: ember glow
uniform sampler3D uNoise;
uniform vec3 uSize;
uniform float uVoxel;
uniform float uBlend;
uniform float uTime;
uniform mat4 projectionMatrix;
uniform mat4 modelViewMatrix;

varying vec3 vPosition;
varying vec3 vCamera;

// Smooth value noise from a small tiling texture of random values.
float noise(vec3 x) {
  vec3 cell = floor(x);
  vec3 f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return texture(uNoise, (cell + f + 0.5) / ${NOISE_SIZE}.0).r;
}

float fbm(vec3 p) {
  float sum = 0.5 * noise(p);
  sum += 0.25 * noise(p * 2.03 + vec3(17.1, 3.7, 9.2));
  sum += 0.125 * noise(p * 4.11 + vec3(5.3, 41.9, 23.6));
  return sum / 0.875;
}

/** Flame heat (x) and ember glow (y) at a point, faded out at the box's faces. */
vec2 heatAt(vec3 p) {
  vec3 uvw = p / uSize + 0.5;
  vec4 field = texture(uField, uvw);
  vec3 inside = min(uvw, 1.0 - uvw) * uSize / (2.0 * uVoxel);
  float edge = clamp(min(inside.x, min(inside.y, inside.z)), 0.0, 1.0);
  return vec2(mix(field.r, field.g, uBlend), mix(field.b, field.a, uBlend)) * edge;
}

vec3 flameColor(float heat) {
  vec3 color = mix(vec3(0.62, 0.05, 0.0), vec3(1.0, 0.36, 0.02), smoothstep(0.0, 0.3, heat));
  color = mix(color, vec3(1.0, 0.74, 0.16), smoothstep(0.25, 0.62, heat));
  return mix(color, vec3(1.0, 0.96, 0.78), smoothstep(0.6, 1.0, heat));
}

void main() {
  // The box draws its back faces, so the camera may be inside it.
  vec3 direction = normalize(vPosition - vCamera);
  vec3 halfSize = uSize * 0.5;
  vec3 toLow = (-halfSize - vCamera) / direction;
  vec3 toHigh = (halfSize - vCamera) / direction;
  vec3 nearest = min(toLow, toHigh);
  vec3 farthest = max(toLow, toHigh);
  float start = max(max(nearest.x, nearest.y), max(nearest.z, 0.0));
  float end = min(min(farthest.x, farthest.y), farthest.z);
  if (end <= start) discard;

  float stride = max((end - start) / float(STEPS), uVoxel * 0.3);
  // Start each pixel's ray at a different offset: banding becomes fine grain.
  float jitter = fract(52.9829189 * fract(dot(gl_FragCoord.xy, vec2(0.06711056, 0.00583715))));
  float t = start + stride * jitter;

  vec3 light = vec3(0.0);
  float through = 1.0; // how much of what's behind still shows
  float firstHit = -1.0;

  for (int i = 0; i < STEPS; i++) {
    if (t > end || through < 0.02) break;
    vec3 p = vCamera + direction * t;
    vec2 heat = heatAt(p);

    if (heat.x > 0.03) {
      // Flames lean and curl more the higher they are above their fuel.
      vec3 drift = vec3(p.xy * 1.6, p.z * 1.1 - uTime * 1.5);
      vec3 sway = vec3(noise(drift), noise(drift + vec3(31.4, 7.7, 13.1)), 0.5) - 0.5;
      float lifted = heatAt(p + sway * (0.14 + 0.45 * (1.0 - heat.x))).x;

      // Noise streaming upward eats into the heat: little near the fuel,
      // where the fire is solid, and more with height, which tears the top
      // into tongues.
      float turbulence = fbm(vec3(p.xy * 3.8, p.z * 1.5 - uTime * 3.0) + sway * 1.6);
      float flame = lifted * 1.55 - turbulence * (1.6 - 0.75 * lifted);

      if (flame > 0.0) {
        // Thin at the edges, so the bright core shows through the red fringe.
        float density = smoothstep(0.0, 0.7, flame);
        vec3 color = flameColor(flame * 0.8);
        float opacity = 1.0 - exp(-density * 6.0 * stride);
        light += through * color * (opacity + density * density * stride * 1.8);
        through *= 1.0 - opacity;
        if (firstHit < 0.0 && opacity > 0.005) firstHit = t;
      } else {
        // Thin soot where the heat is too torn to burn.
        float soot = smoothstep(-0.35, 0.0, flame) * smoothstep(0.05, 0.3, lifted);
        float opacity = 1.0 - exp(-soot * 0.5 * stride);
        light += through * vec3(0.1, 0.09, 0.085) * opacity;
        through *= 1.0 - opacity;
      }
    }

    if (heat.y > 0.03) {
      // Smouldering: a dim glow that crawls over what is about to catch.
      float glow = smoothstep(0.35, 0.8, fbm(vec3(p.xy * 5.0, p.z * 5.0 + uTime * 0.6)));
      float opacity = 1.0 - exp(-heat.y * (0.25 + glow) * 1.3 * stride);
      light += through * mix(vec3(0.75, 0.16, 0.02), vec3(1.0, 0.45, 0.06), glow) * opacity;
      through *= 1.0 - opacity;
      if (firstHit < 0.0 && opacity > 0.01) firstHit = t;
    }

    t += stride;
  }

  float alpha = 1.0 - through;
  if (alpha < 0.004 || firstHit < 0.0) discard;

  // Depth of the nearest fire on this ray, so walls in front of the fire
  // hide it and walls behind it don't.
  vec4 clip = projectionMatrix * modelViewMatrix * vec4((vCamera + direction * firstHit) / uSize, 1.0);
  gl_FragDepth = clamp(clip.z / clip.w * 0.5 + 0.5, 0.0, 1.0);
  gl_FragColor = vec4(min(light, vec3(1.0)), alpha); // premultiplied
}
`

export interface FireVolumeRenderer {
  /** Add to a group that is in canonical Z-up metres. */
  object: THREE.Object3D
  /** The fire to ease towards; null lets it die down. */
  setVolume(volume: FireVolume | null): void
  /** Advances the flames. False once there is nothing left to draw. */
  tick(deltaSeconds: number): boolean
  dispose(): void
}

/** Stride of the field texture: flame before, flame after, ember before, ember after. */
const CHANNELS = 4

export function createFireVolumeRenderer(three: typeof THREE): FireVolumeRenderer {
  const geometry = new three.BoxGeometry(1, 1, 1)
  const noise = noiseTexture(three)
  const uniforms = {
    uField: { value: null as THREE.Data3DTexture | null },
    uNoise: { value: noise },
    uSize: { value: new three.Vector3(1, 1, 1) },
    uVoxel: { value: 1 },
    uBlend: { value: 1 },
    uTime: { value: 0 },
  }
  const material = new three.ShaderMaterial({
    uniforms,
    vertexShader: VERTEX_SHADER,
    fragmentShader: FRAGMENT_SHADER,
    side: three.BackSide,
    transparent: true,
    premultipliedAlpha: true,
    depthWrite: false,
  })
  const mesh = new three.Mesh(geometry, material)
  mesh.name = 'fire-volume'
  mesh.visible = false
  // Drawn after the see-through furniture it burns inside.
  mesh.renderOrder = 1
  mesh.raycast = () => {}

  /** What the texture holds, and the grid of the last volume set (null: dying down). */
  let shown: { grid: VoxelGrid; data: Uint8Array } | null = null
  let targetGrid: VoxelGrid | null = null

  function clear() {
    uniforms.uField.value?.dispose()
    uniforms.uField.value = null
    shown = null
    targetGrid = null
    mesh.visible = false
  }

  function upload(grid: VoxelGrid, data: Uint8Array) {
    const [nx, ny, nz] = grid.size
    uniforms.uField.value?.dispose()
    const texture = new three.Data3DTexture(data, nx, ny, nz)
    texture.format = three.RGBAFormat
    texture.type = three.UnsignedByteType
    texture.minFilter = three.LinearFilter
    texture.magFilter = three.LinearFilter
    texture.unpackAlignment = 1
    texture.needsUpdate = true
    uniforms.uField.value = texture

    const v = grid.voxelM
    uniforms.uVoxel.value = v
    uniforms.uSize.value.set(nx * v, ny * v, nz * v)
    mesh.scale.copy(uniforms.uSize.value)
    mesh.position.set(
      (grid.origin[0] + nx / 2) * v,
      (grid.origin[1] + ny / 2) * v,
      (grid.origin[2] + nz / 2) * v,
    )
    shown = { grid, data }
    mesh.visible = true
  }

  return {
    object: mesh,

    setVolume(volume) {
      if (!volume && !shown) return
      // Where the fire is right now, mid-blend, becomes the new starting point.
      const blend = uniforms.uBlend.value
      const previous = shown
      const carried = previous && (!volume || volume.voxelM === previous.grid.voxelM)
      let grid: VoxelGrid = volume ?? previous!.grid
      // Keep the last fire's box too, so flames that just went out fade
      // rather than vanish.
      if (volume && carried && targetGrid) grid = unionGrid(volume, targetGrid)

      const data = new Uint8Array(voxelCount(grid) * CHANNELS)
      if (volume) {
        blitField(volume.flame, volume, data, grid, 1, CHANNELS)
        blitField(volume.ember, volume, data, grid, 3, CHANNELS)
      }
      if (previous && carried) {
        const count = voxelCount(previous.grid)
        const flame = new Uint8Array(count)
        const ember = new Uint8Array(count)
        for (let i = 0; i < count; i++) {
          const at = i * CHANNELS
          flame[i] = previous.data[at] + (previous.data[at + 1] - previous.data[at]) * blend
          ember[i] = previous.data[at + 2] + (previous.data[at + 3] - previous.data[at + 2]) * blend
        }
        blitField(flame, previous.grid, data, grid, 0, CHANNELS)
        blitField(ember, previous.grid, data, grid, 2, CHANNELS)
        uniforms.uBlend.value = 0
      } else if (volume) {
        // A different voxel size can't be blended from: show the new fire
        // at once (or, with no fire before it, let it grow from nothing).
        uniforms.uBlend.value = previous ? 1 : 0
      }
      targetGrid = volume
      upload(grid, data)
    },

    tick(deltaSeconds) {
      if (!shown) return false
      uniforms.uTime.value += deltaSeconds
      uniforms.uBlend.value = Math.min(1, uniforms.uBlend.value + deltaSeconds / BLEND_SECONDS)
      if (!targetGrid && uniforms.uBlend.value >= 1) clear()
      return shown !== null
    },

    dispose() {
      clear()
      geometry.dispose()
      material.dispose()
      noise.dispose()
    },
  }
}

/** A tiling cube of random bytes, the same every time. */
function noiseTexture(three: typeof THREE): THREE.Data3DTexture {
  const data = new Uint8Array(NOISE_SIZE ** 3)
  let state = 0x9e3779b9
  for (let i = 0; i < data.length; i++) {
    // mulberry32
    state = (state + 0x6d2b79f5) | 0
    let mixed = Math.imul(state ^ (state >>> 15), 1 | state)
    mixed = (mixed + Math.imul(mixed ^ (mixed >>> 7), 61 | mixed)) ^ mixed
    data[i] = (mixed ^ (mixed >>> 14)) >>> 24
  }
  const texture = new three.Data3DTexture(data, NOISE_SIZE, NOISE_SIZE, NOISE_SIZE)
  texture.format = three.RedFormat
  texture.type = three.UnsignedByteType
  texture.minFilter = three.LinearFilter
  texture.magFilter = three.LinearFilter
  texture.wrapS = texture.wrapT = texture.wrapR = three.RepeatWrapping
  texture.unpackAlignment = 1
  texture.needsUpdate = true
  return texture
}
