// Noise shared by the volumetric shaders (fire, water): a small tiling cube
// of random values, and the GLSL that reads it as smooth noise.

import type * as THREE from 'three'

const NOISE_SIZE = 32

/** GLSL `noise` and `fbm` (both 0–1), reading a `uNoise` sampler3D. */
export const NOISE_GLSL = /* glsl */ `
precision highp sampler3D;
uniform sampler3D uNoise;

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
`

/** The texture for `uNoise`: a tiling cube of random bytes, the same every time. */
export function createNoiseTexture(three: typeof THREE): THREE.Data3DTexture {
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
