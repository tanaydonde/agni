import { ImprovedNoise } from 'three/addons/math/ImprovedNoise.js'
import type * as THREE from 'three'

/**
 * Renderer-agnostic procedural fire resources. Matterport supplies its own
 * copy of three.js, so callers pass the THREE namespace that owns the scene.
 */
export interface FireVisualResources {
  flameGeometry: THREE.BufferGeometry
  flameMaterial: THREE.ShaderMaterial
  emberGeometry: THREE.BufferGeometry
  emberMaterial: THREE.ShaderMaterial
  noiseTexture: THREE.Data3DTexture
  dispose(): void
}

const NOISE_SIZE = 32
const RAY_STEPS = 14

/** Build filtered fBm once; each renderer uploads this same 32 KiB volume. */
function buildNoiseVolume(): Uint8Array {
  const data = new Uint8Array(NOISE_SIZE ** 3)
  const perlin = new ImprovedNoise()
  let index = 0
  for (let z = 0; z < NOISE_SIZE; z += 1) {
    for (let y = 0; y < NOISE_SIZE; y += 1) {
      for (let x = 0; x < NOISE_SIZE; x += 1) {
        let value = 0
        let amplitude = 0.58
        let normalizer = 0
        let frequency = 1.65
        for (let octave = 0; octave < 3; octave += 1) {
          const sample =
            perlin.noise(
              (x / NOISE_SIZE) * frequency,
              (y / NOISE_SIZE) * frequency,
              (z / NOISE_SIZE) * frequency,
            ) *
              0.5 +
            0.5
          value += sample * amplitude
          normalizer += amplitude
          frequency *= 2.03
          amplitude *= 0.48
        }
        data[index] = Math.round((value / normalizer) * 255)
        index += 1
      }
    }
  }
  return data
}

const NOISE_VOLUME = buildNoiseVolume()

const VOLUME_VERTEX_SHADER = /* glsl */ `
  precision highp float;

  in vec3 position;
  uniform mat4 modelViewMatrix;
  uniform mat4 projectionMatrix;
  out vec3 vLocalPosition;

  void main() {
    vLocalPosition = position;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`

const VOLUME_FRAGMENT_SHADER = /* glsl */ `
  precision highp float;
  precision highp sampler3D;

  uniform sampler3D uNoise;
  uniform float uTime;
  uniform float uSeed;
  uniform float uIntensity;
  uniform float uDensity;
  uniform vec3 uCameraLocal;

  in vec3 vLocalPosition;
  out vec4 outColor;

  vec2 intersectBox(vec3 origin, vec3 direction) {
    vec3 safeDirection = direction;
    safeDirection.x += direction.x >= 0.0 ? 0.00001 : -0.00001;
    safeDirection.y += direction.y >= 0.0 ? 0.00001 : -0.00001;
    safeDirection.z += direction.z >= 0.0 ? 0.00001 : -0.00001;
    vec3 inverseDirection = 1.0 / safeDirection;
    vec3 lower = (vec3(-0.5, -0.5, 0.0) - origin) * inverseDirection;
    vec3 upper = (vec3(0.5, 0.5, 1.0) - origin) * inverseDirection;
    vec3 nearer = min(lower, upper);
    vec3 farther = max(lower, upper);
    return vec2(max(max(nearer.x, nearer.y), nearer.z), min(min(farther.x, farther.y), farther.z));
  }

  float flameProfile(
    vec3 point,
    vec2 center,
    float heightScale,
    float widthScale,
    float turbulence
  ) {
    float localHeight = point.z / heightScale;
    float heightFade = 1.0 - smoothstep(0.82, 1.0, localHeight);
    float displacedHeight = localHeight +
      sqrt(max(localHeight, 0.0)) * mix(0.36, 0.66, uDensity) * turbulence;
    float width = mix(0.48 * widthScale, 0.012, pow(clamp(displacedHeight, 0.0, 1.0), 0.62));
    float radial = length(point.xy - center);
    return (1.0 - smoothstep(width * 0.7, width, radial)) * heightFade;
  }

  float flameField(vec3 point) {
    float z = point.z;
    float seedAngle = uSeed * 6.2831853;
    vec2 seedDirection = vec2(cos(seedAngle), sin(seedAngle));
    vec3 flow = vec3(point.xy * 1.86, z * 1.94 - uTime * 0.31);
    flow += vec3(uSeed * 3.7, uSeed * 5.3, uSeed * 2.1);

    float broadNoise = texture(uNoise, flow).r;
    float detailNoise = texture(uNoise, flow * 2.07 + vec3(0.31, 0.67, 0.19)).r;
    float turbulence = abs(broadNoise - 0.5) * 1.35 + abs(detailNoise - 0.5) * 0.65;
    vec2 warp = vec2(broadNoise, detailNoise) - 0.5;

    vec2 center = warp * mix(0.04, 0.24, z);
    float field = flameProfile(point, center, 1.0, mix(0.86, 1.08, uDensity), turbulence);
    // Turbulence shapes the outer flame, but this narrower core guarantees
    // that an actively burning component never disappears between frames.
    float stableCore = flameProfile(point, center * 0.35, 0.96, 0.68, turbulence * 0.24);
    field = max(field, stableCore * 0.88);
    float secondEnabled = smoothstep(0.24, 0.48, uDensity);
    float thirdEnabled = smoothstep(0.58, 0.84, uDensity);
    vec2 secondCenter = seedDirection * 0.18 + warp * 0.13;
    vec2 thirdCenter = vec2(-seedDirection.y, seedDirection.x) * 0.2 - warp * 0.1;
    float second = flameProfile(point, secondCenter, 0.78, 0.64, turbulence * 1.08);
    float third = flameProfile(point, thirdCenter, 0.62, 0.52, turbulence * 1.14);
    field = max(field, second * secondEnabled);
    field = max(field, third * thirdEnabled);

    float sideDistance = min(0.5 - abs(point.x), 0.5 - abs(point.y));
    float edgeFade = smoothstep(0.005, 0.1, sideDistance);
    float baseFade = smoothstep(0.0, 0.028, z);
    float topFade = 1.0 - smoothstep(0.92, 1.0, z);
    float breakup = smoothstep(0.18, 0.62, broadNoise + field * 0.48);
    float visibleDensity = max(field * breakup, stableCore * 0.72);
    return visibleDensity * edgeFade * baseFade * topFade * mix(0.78, 1.0, uIntensity);
  }

  vec3 flameColor(float amount, float height) {
    vec3 red = vec3(0.69, 0.012, 0.001);
    vec3 orange = vec3(1.0, 0.11, 0.002);
    vec3 yellow = vec3(1.0, 0.55, 0.025);
    vec3 whiteHot = vec3(1.0, 0.93, 0.5);
    float heat = clamp(amount * 0.6 + (1.0 - height) * 0.2, 0.0, 1.0);
    vec3 color = mix(red, orange, smoothstep(0.06, 0.34, heat));
    color = mix(color, yellow, smoothstep(0.42, 0.82, heat));
    color = mix(color, whiteHot, smoothstep(0.985, 1.0, heat) * 0.35);
    return color;
  }

  float randomJitter(vec2 pixel, float seed) {
    return fract(sin(dot(pixel, vec2(12.9898, 78.233)) + seed * 37.719) * 43758.5453);
  }

  void main() {
    vec3 rayDirection = normalize(vLocalPosition - uCameraLocal);
    vec2 hit = intersectBox(uCameraLocal, rayDirection);
    float rayStart = max(hit.x, 0.0);
    float rayEnd = hit.y;
    if (rayEnd <= rayStart) discard;

    float stepSize = (rayEnd - rayStart) / float(${RAY_STEPS});
    float jitter = randomJitter(gl_FragCoord.xy, uSeed);
    vec3 point = uCameraLocal + rayDirection * (rayStart + stepSize * jitter);
    vec4 accumulated = vec4(0.0);

    for (int stepIndex = 0; stepIndex < ${RAY_STEPS}; stepIndex++) {
      float amount = flameField(point);
      if (amount > 0.008) {
        float sampleAlpha = amount * mix(0.13, 0.23, uIntensity);
        vec3 sampleColor = flameColor(amount, point.z);
        accumulated.rgb += (1.0 - accumulated.a) * sampleColor * sampleAlpha;
        accumulated.a += (1.0 - accumulated.a) * sampleAlpha;
      }
      point += rayDirection * stepSize;
      if (accumulated.a > 0.94) break;
    }

    if (accumulated.a < 0.015) discard;
    vec3 straightColor = accumulated.rgb / max(accumulated.a, 0.001);
    float flicker = 0.96 + 0.04 * sin(uTime * 7.1 + uSeed * 17.0);
    outColor = vec4(straightColor * flicker, min(accumulated.a * 1.6, 1.0));
  }
`

const EMBER_VERTEX_SHADER = /* glsl */ `
  varying vec2 vUv;

  void main() {
    vUv = uv;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`

const EMBER_FRAGMENT_SHADER = /* glsl */ `
  uniform float uTime;
  uniform float uSeed;
  uniform float uIntensity;
  varying vec2 vUv;

  float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453123);
  }

  void main() {
    vec2 centered = (vUv - 0.5) * 2.0;
    float radius = length(centered);
    float glow = 1.0 - smoothstep(0.12, 1.0, radius);
    float speckle = smoothstep(0.7, 0.96, hash(floor(vUv * 12.0) + uSeed * 17.0));
    float pulse = 0.72 + 0.28 * sin(uTime * 2.6 + uSeed * 6.2831853);
    float alpha = glow * (0.12 + speckle * 0.38) * pulse * uIntensity;
    if (alpha < 0.015) discard;
    vec3 color = mix(vec3(0.55, 0.025, 0.005), vec3(1.0, 0.34, 0.015), speckle + glow * 0.35);
    gl_FragColor = vec4(color, alpha);
  }
`

function noiseTexture(three: typeof THREE): THREE.Data3DTexture {
  const texture = new three.Data3DTexture(NOISE_VOLUME, NOISE_SIZE, NOISE_SIZE, NOISE_SIZE)
  texture.format = three.RedFormat
  texture.minFilter = three.LinearFilter
  texture.magFilter = three.LinearFilter
  texture.wrapS = three.MirroredRepeatWrapping
  texture.wrapT = three.MirroredRepeatWrapping
  texture.wrapR = three.MirroredRepeatWrapping
  texture.unpackAlignment = 1
  texture.generateMipmaps = false
  texture.needsUpdate = true
  return texture
}

function emberMaterial(three: typeof THREE): THREE.ShaderMaterial {
  const material = new three.ShaderMaterial({
    uniforms: {
      uTime: { value: 0 },
      uSeed: { value: 0 },
      uIntensity: { value: 1 },
      uDensity: { value: 1 },
      uCameraLocal: { value: new three.Vector3() },
    },
    vertexShader: EMBER_VERTEX_SHADER,
    fragmentShader: EMBER_FRAGMENT_SHADER,
    transparent: true,
    depthWrite: false,
    depthTest: true,
    blending: three.AdditiveBlending,
    side: three.DoubleSide,
  })
  material.toneMapped = false
  return material
}

export function createFireVisualResources(three: typeof THREE): FireVisualResources {
  const noise = noiseTexture(three)
  // The simulation is Z-up. Move the unit box so it grows from z=0 to z=1;
  // its mesh is placed at the base of each connected burning component.
  const flame = new three.BoxGeometry(1, 1, 1)
  flame.translate(0, 0, 0.5)
  flame.computeBoundingSphere()
  const flameMaterial = new three.RawShaderMaterial({
    glslVersion: three.GLSL3,
    uniforms: {
      uNoise: { value: noise },
      uTime: { value: 0 },
      uSeed: { value: 0 },
      uIntensity: { value: 1 },
      uDensity: { value: 1 },
      uCameraLocal: { value: new three.Vector3() },
    },
    vertexShader: VOLUME_VERTEX_SHADER,
    fragmentShader: VOLUME_FRAGMENT_SHADER,
    transparent: true,
    depthWrite: false,
    depthTest: true,
    blending: three.NormalBlending,
    side: three.BackSide,
  })
  flameMaterial.toneMapped = false
  const ember = new three.PlaneGeometry(1, 1)
  const heatingMaterial = emberMaterial(three)
  return {
    flameGeometry: flame,
    flameMaterial,
    emberGeometry: ember,
    emberMaterial: heatingMaterial,
    noiseTexture: noise,
    dispose() {
      flame.dispose()
      flameMaterial.dispose()
      ember.dispose()
      heatingMaterial.dispose()
      noise.dispose()
    },
  }
}

/** Set shared animation time; per-cluster uniforms are set immediately before drawing. */
export function setFireVisualTime(resources: FireVisualResources, seconds: number): void {
  resources.flameMaterial.uniforms.uTime.value = seconds
  resources.emberMaterial.uniforms.uTime.value = seconds
}

export function setFireVisualCell(
  material: THREE.Material,
  seed: number,
  intensity: number,
  density: number,
  cameraLocal: { x: number; y: number; z: number },
): void {
  const shader = material as THREE.ShaderMaterial
  shader.uniforms.uSeed.value = seed
  shader.uniforms.uIntensity.value = intensity
  shader.uniforms.uDensity.value = density
  shader.uniforms.uCameraLocal.value.set(cameraLocal.x, cameraLocal.y, cameraLocal.z)
}
