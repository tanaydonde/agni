// Volumetric hose streams. Each stream is a box along its flight whose
// fragment shader marches the view ray through water shaped like a hose
// throws it: a tight jet at the nozzle that arcs under gravity, breaks into
// ropes, sheds a widening cone of spray, and bursts into mist where it lands.
// Written against a passed-in three.js, like volumeRenderer.ts.

import type * as THREE from 'three'
import type { Vec3 } from '../scene/types'
import { NOISE_GLSL, createNoiseTexture } from './noise'

/** One stream to draw, in canonical Z-up metres. */
export interface WaterStream {
  id: string
  nozzle: Vec3
  /** Where the water lands. */
  target: Vec3
}

/** How fast water leaves the nozzle. Sets the arc and how fast a stream starts and stops. */
const WATER_SPEED_M_PER_S = 11
const GRAVITY_M_PER_S2 = 9.81
/** The spray spreads to this half-angle: the cone the simulation wets. */
const SPRAY_HALF_ANGLE_RADIANS = (15 * Math.PI) / 180
/** Radius of the burst of mist where the water lands. */
const SPLASH_RADIUS_M = 0.6
/** Seconds for a stream to catch up with a firefighter's new aim. */
const AIM_SECONDS = 0.12
/** Seconds for the splash to build and to settle. */
const SPLASH_SECONDS = 0.3

const VERTEX_SHADER = /* glsl */ `
uniform vec3 uSize;
uniform vec3 uCenter;
varying vec3 vPosition;
varying vec3 vCamera;

void main() {
  // Both in the stream's own metres: origin at the nozzle, x toward the
  // target, z up (as near as the stream's tilt allows).
  vPosition = position * uSize + uCenter;
  vCamera = (inverse(modelMatrix) * vec4(cameraPosition, 1.0)).xyz * uSize + uCenter;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`

const FRAGMENT_SHADER = /* glsl */ `
${NOISE_GLSL}

#define STEPS 96

uniform vec3 uSize;
uniform vec3 uCenter;
uniform float uLength; // nozzle to target
uniform float uSag;    // how far the arc rises above the straight line
uniform float uHead;   // how far the front of the water has got
uniform float uTail;   // how far the back of the water has got, once shut off
uniform float uSplash; // 0–1: how much water is landing
uniform float uSpread; // tangent of the spray's half-angle
uniform float uSpeed;
uniform float uTime;
uniform mat4 projectionMatrix;
uniform mat4 modelViewMatrix;

varying vec3 vPosition;
varying vec3 vCamera;

/**
 * Water at a point: x is the solid jet, y the spray and mist. \`top\` is how
 * far up the stream's cross-section the point is (0 underside, 1 top), for
 * shading.
 */
vec2 waterAt(vec3 q, out float top) {
  top = 0.5;
  vec2 water = vec2(0.0);
  float s = clamp(q.x / uLength, 0.0, 1.0);

  // Only between the back and the front of the water.
  float flowing = smoothstep(uTail - 0.25, uTail, q.x) * (1.0 - smoothstep(uHead - 0.3, uHead, q.x));
  if (flowing > 0.0 && q.x > -0.02 && q.x < uLength + 0.1) {
    // The pattern rides along with the water.
    float along = q.x - uTime * uSpeed;
    // The jet rises, then falls onto the target, wavering more as it goes.
    vec2 waver = vec2(noise(vec3(along * 0.8, 3.1, 7.7)), noise(vec3(along * 0.8, 11.3, 1.9))) - 0.5;
    vec2 off = q.yz - vec2(0.0, uSag * 4.0 * s * (1.0 - s)) - waver * 0.24 * s;
    float distance = length(off);
    float sprayRadius = 0.03 + uSpread * q.x;

    if (distance < sprayRadius) {
      top = clamp(0.5 + 0.5 * off.y / max(distance, 0.001), 0.0, 1.0);

      // Solid at the nozzle; further out it tears into ropes and gaps.
      float jetRadius = 0.03 + 0.09 * s;
      float ropes = fbm(vec3(along * 2.6, off * 9.0));
      float torn = mix(1.0, smoothstep(0.36, 0.6, ropes), smoothstep(0.05, 0.6, s));
      water.x = smoothstep(jetRadius, jetRadius * 0.3, distance) * torn * flowing * smoothstep(-0.02, 0.02, q.x);

      // Spray flies out along the cone, so its streaks follow the cone's lines.
      float streaks = fbm(vec3(along * 1.3, off / sprayRadius * 3.2));
      float inCone = 1.0 - distance / sprayRadius;
      water.y = inCone * sqrt(inCone) * smoothstep(0.36, 0.7, streaks)
        * smoothstep(0.1, 0.7, q.x) * mix(1.0, 0.7, s) * flowing;
    }
  }

  // Where it lands: mist thrown up and outward.
  if (uSplash > 0.0) {
    vec3 fromTarget = q - vec3(uLength, 0.0, 0.0);
    float reach = length(fromTarget * vec3(1.0, 1.0, 0.85)) / ${SPLASH_RADIUS_M.toFixed(2)};
    if (reach < 1.0) {
      float billow = fbm(fromTarget * 4.5 - vec3(0.0, 0.0, uTime * 1.6) + vec3(uTime * 0.7));
      float burst = (1.0 - reach) * (1.0 - reach) * smoothstep(0.2, 0.6, billow) * uSplash;
      water.y += burst * 4.0;
      top = mix(top, clamp(0.5 + fromTarget.z, 0.0, 1.0), step(water.x, 0.001));
    }
  }
  return water;
}

void main() {
  // The box draws its back faces, so the camera may be inside it.
  vec3 direction = normalize(vPosition - vCamera);
  vec3 low = uCenter - uSize * 0.5;
  vec3 high = uCenter + uSize * 0.5;
  vec3 toLow = (low - vCamera) / direction;
  vec3 toHigh = (high - vCamera) / direction;
  vec3 nearest = min(toLow, toHigh);
  vec3 farthest = max(toLow, toHigh);
  float start = max(max(nearest.x, nearest.y), max(nearest.z, 0.0));
  float end = min(min(farthest.x, farthest.y), farthest.z);
  if (end <= start) discard;

  float stride = max((end - start) / float(STEPS), 0.008);
  float jitter = fract(52.9829189 * fract(dot(gl_FragCoord.xy, vec2(0.06711056, 0.00583715))));
  float t = start + stride * jitter;

  vec3 light = vec3(0.0);
  float through = 1.0;
  float firstHit = -1.0;

  for (int i = 0; i < STEPS; i++) {
    if (t > end || through < 0.02) break;
    vec3 q = vCamera + direction * t;
    float top;
    vec2 water = waterAt(q, top);

    if (water.x + water.y > 0.002) {
      // Lit from above: bright on top, deeper blue underneath.
      float jet = 1.0 - exp(-water.x * 60.0 * stride);
      float mist = 1.0 - exp(-water.y * 11.0 * stride);
      vec3 jetColor = mix(vec3(0.16, 0.45, 0.86), vec3(0.9, 0.97, 1.0), 0.25 + 0.75 * top);
      vec3 mistColor = mix(vec3(0.46, 0.68, 0.95), vec3(0.88, 0.95, 1.0), 0.3 + 0.7 * top);
      float opacity = 1.0 - (1.0 - jet) * (1.0 - mist);
      light += through * (jetColor * jet + mistColor * mist * (1.0 - jet));
      through *= 1.0 - opacity;
      if (firstHit < 0.0 && opacity > 0.004) firstHit = t;
    }
    t += stride;
  }

  float alpha = 1.0 - through;
  if (alpha < 0.004 || firstHit < 0.0) discard;

  // Depth of the nearest water on this ray, so walls in front of it hide it.
  vec3 hit = (vCamera + direction * firstHit - uCenter) / uSize;
  vec4 clip = projectionMatrix * modelViewMatrix * vec4(hit, 1.0);
  gl_FragDepth = clamp(clip.z / clip.w * 0.5 + 0.5, 0.0, 1.0);
  gl_FragColor = vec4(min(light, vec3(1.0)), alpha); // premultiplied
}
`

export interface WaterRenderer {
  /** Add to a group that is in canonical Z-up metres. */
  object: THREE.Object3D
  /** The streams being sprayed now. One that is left out is shut off and runs dry. */
  setStreams(streams: WaterStream[]): void
  /** Advances the water. False once there is nothing left to draw. */
  tick(deltaSeconds: number): boolean
  dispose(): void
}

interface Stream {
  mesh: THREE.Mesh
  uniforms: Record<string, { value: unknown }> & {
    uHead: { value: number }
    uTail: { value: number }
    uSplash: { value: number }
    uTime: { value: number }
  }
  /** Where it is drawn, easing toward `aim`. */
  nozzle: THREE.Vector3
  target: THREE.Vector3
  aim: { nozzle: Vec3; target: Vec3 }
  spraying: boolean
}

export function createWaterRenderer(three: typeof THREE): WaterRenderer {
  const group = new three.Group()
  group.name = 'water'
  const geometry = new three.BoxGeometry(1, 1, 1)
  const noise = createNoiseTexture(three)
  const streams = new Map<string, Stream>()
  const spread = Math.tan(SPRAY_HALF_ANGLE_RADIANS)
  // Scratch, reused every frame.
  const along = new three.Vector3()
  const up = new three.Vector3()
  const side = new three.Vector3()
  const basis = new three.Matrix4()
  const box = new three.Matrix4()

  function add(id: string, { nozzle, target }: WaterStream): Stream {
    const uniforms = {
      uNoise: { value: noise },
      uSize: { value: new three.Vector3(1, 1, 1) },
      uCenter: { value: new three.Vector3() },
      uLength: { value: 1 },
      uSag: { value: 0 },
      uHead: { value: 0 },
      uTail: { value: 0 },
      uSplash: { value: 0 },
      uSpread: { value: spread },
      uSpeed: { value: WATER_SPEED_M_PER_S },
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
    mesh.name = `${id}/stream`
    mesh.matrixAutoUpdate = false
    // Drawn after the flames it lands in.
    mesh.renderOrder = 2
    mesh.raycast = () => {}
    group.add(mesh)
    const stream: Stream = {
      mesh,
      uniforms,
      nozzle: new three.Vector3(nozzle.x, nozzle.y, nozzle.z),
      target: new three.Vector3(target.x, target.y, target.z),
      aim: { nozzle, target },
      spraying: true,
    }
    streams.set(id, stream)
    place(stream)
    return stream
  }

  /** Fits the stream's box around its flight and tells the shader its shape. */
  function place(stream: Stream) {
    const { uniforms, mesh } = stream
    along.subVectors(stream.target, stream.nozzle)
    const length = Math.max(along.length(), 0.05)
    along.divideScalar(length)
    // z: straight up, tilted just enough to be square to the stream.
    up.set(0, 0, 1).addScaledVector(along, -along.z)
    if (up.lengthSq() < 1e-6) up.set(1, 0, 0) // spraying straight up or down
    up.normalize()
    side.crossVectors(up, along)

    // The height a thrown stream rises above the straight line to its target.
    const sag = Math.min(0.6, (GRAVITY_M_PER_S2 * length ** 2) / (8 * WATER_SPEED_M_PER_S ** 2))
    const girth = Math.max(0.03 + spread * length, SPLASH_RADIUS_M) + 0.2
    const size = uniforms.uSize.value as THREE.Vector3
    const center = uniforms.uCenter.value as THREE.Vector3
    size.set(length + SPLASH_RADIUS_M + 0.2, 2 * girth, 2 * girth + sag)
    center.set(size.x / 2 - 0.1, 0, sag / 2)
    uniforms.uLength.value = length
    uniforms.uSag.value = sag

    basis.makeBasis(along, side, up).setPosition(stream.nozzle)
    box.makeTranslation(center.x, center.y, center.z).scale(size)
    mesh.matrix.multiplyMatrices(basis, box)
    mesh.matrixWorldNeedsUpdate = true
  }

  function remove(id: string, stream: Stream) {
    group.remove(stream.mesh)
    ;(stream.mesh.material as THREE.Material).dispose()
    streams.delete(id)
  }

  return {
    object: group,

    setStreams(next) {
      const spraying = new Set<string>()
      for (const stream of next) {
        spraying.add(stream.id)
        const existing = streams.get(stream.id)
        if (!existing) {
          add(stream.id, stream)
        } else {
          existing.aim = stream
          if (!existing.spraying) {
            // Turned back on before it ran dry: a fresh stream from the nozzle.
            existing.spraying = true
            existing.uniforms.uHead.value = 0
            existing.uniforms.uTail.value = 0
          }
        }
      }
      for (const [id, stream] of streams) {
        if (!spraying.has(id)) stream.spraying = false
      }
    },

    tick(deltaSeconds) {
      for (const [id, stream] of streams) {
        const { uniforms } = stream
        const length = uniforms.uLength.value as number
        uniforms.uTime.value += deltaSeconds
        const splashStep = deltaSeconds / SPLASH_SECONDS
        if (stream.spraying) {
          const ease = 1 - Math.exp(-deltaSeconds / AIM_SECONDS)
          const { nozzle, target } = stream.aim
          stream.nozzle.lerp(along.set(nozzle.x, nozzle.y, nozzle.z), ease)
          stream.target.lerp(along.set(target.x, target.y, target.z), ease)
          place(stream)
          uniforms.uHead.value = Math.min(length + 1, uniforms.uHead.value + WATER_SPEED_M_PER_S * deltaSeconds)
          const landing = uniforms.uHead.value >= length
          uniforms.uSplash.value = Math.min(1, Math.max(0, uniforms.uSplash.value + (landing ? splashStep : -splashStep)))
        } else {
          // Shut off: what has left the nozzle flies on and lands.
          uniforms.uTail.value += WATER_SPEED_M_PER_S * deltaSeconds
          if (uniforms.uTail.value >= length) {
            uniforms.uSplash.value = Math.max(0, uniforms.uSplash.value - splashStep)
            if (uniforms.uSplash.value === 0) remove(id, stream)
          }
        }
      }
      return streams.size > 0
    },

    dispose() {
      for (const [id, stream] of streams) remove(id, stream)
      geometry.dispose()
      noise.dispose()
    },
  }
}
