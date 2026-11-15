// Body models for simulation agents (occupants and firefighters), reduced to
// plain arrays so both our renderer and Showcase (which has its own copy of
// three.js) can build meshes from them.
//
// Each model is normalized so its footprint is a circle of radius 1 centred
// on the origin, standing on z = 0, Z-up and facing +X. Scaling it uniformly
// by an agent's `radius_m` would make the body exactly fill the simulation's
// collision circle; renderers scale by `agentScale` instead, which draws it
// AGENT_DRAW_SCALE times larger.

import * as THREE from 'three'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'
import type { AgentFrame } from '../fire/types'

export type AgentKind = AgentFrame['role']['type']

export const AGENT_MODEL_URLS: Record<AgentKind, string> = {
  occupant: '/models/agents/occupant-blob.glb',
  firefighter: '/models/agents/firefighter-blob.glb',
}

/**
 * How much larger agents are drawn than their collision circle. Visual only:
 * a hitbox this size (0.875 m across for a 0.25 m occupant) wouldn't fit
 * through most scanned doorways, so the simulation keeps its own radius and
 * blobs overlap walls and each other a little.
 */
export const AGENT_DRAW_SCALE = 1.75

/** Uniform scale for a normalized body to draw an agent of `radiusM`. */
export function agentScale(radiusM: number): number {
  return radiusM * AGENT_DRAW_SCALE
}

/** Below this speed an agent keeps facing where it last walked. */
const TURN_MIN_SPEED_M_PER_S = 0.05

export interface BodyPart {
  name: string
  positions: Float32Array
  normals: Float32Array
  index: Uint16Array | Uint32Array | null
  /** sRGB hex, e.g. `#0a4070`. */
  color: string
  roughness: number
  metalness: number
  doubleSided: boolean
}

export interface BodyModel {
  /** Z-up, facing +X, footprint radius 1, standing on z = 0. */
  parts: BodyPart[]
  /** Height in footprint radii: rendered height is `radius_m * heightPerRadius`. */
  heightPerRadius: number
}

/**
 * Normalizes parts given in glTF world space (Y-up, front +Z): turns them
 * Z-up facing +X, centres the footprint, puts the lowest point on the floor,
 * and scales so the farthest vertex from the vertical axis is 1 away.
 */
export function normalizeBody(parts: BodyPart[]): BodyModel {
  // glTF (x, y, z) → (z, x, y): a cyclic swap, so a proper rotation.
  const turned = parts.map((part) => ({
    ...part,
    positions: cycleAxes(part.positions),
    normals: cycleAxes(part.normals),
  }))

  const min = [Infinity, Infinity, Infinity]
  const max = [-Infinity, -Infinity, -Infinity]
  for (const { positions } of turned) {
    for (let i = 0; i < positions.length; i += 3) {
      for (let axis = 0; axis < 3; axis++) {
        min[axis] = Math.min(min[axis], positions[i + axis])
        max[axis] = Math.max(max[axis], positions[i + axis])
      }
    }
  }
  if (!Number.isFinite(min[0])) throw new Error('Agent model has no vertices')
  const centerX = (min[0] + max[0]) / 2
  const centerY = (min[1] + max[1]) / 2

  let radius = 0
  for (const { positions } of turned) {
    for (let i = 0; i < positions.length; i += 3) {
      radius = Math.max(radius, Math.hypot(positions[i] - centerX, positions[i + 1] - centerY))
    }
  }
  if (radius <= 0) throw new Error('Agent model has no footprint')

  for (const { positions } of turned) {
    for (let i = 0; i < positions.length; i += 3) {
      positions[i] = (positions[i] - centerX) / radius
      positions[i + 1] = (positions[i + 1] - centerY) / radius
      positions[i + 2] = (positions[i + 2] - min[2]) / radius
    }
  }

  return { parts: turned, heightPerRadius: (max[2] - min[2]) / radius }
}

function cycleAxes(source: Float32Array): Float32Array {
  const out = new Float32Array(source.length)
  for (let i = 0; i < source.length; i += 3) {
    out[i] = source[i + 2]
    out[i + 1] = source[i]
    out[i + 2] = source[i + 1]
  }
  return out
}

/** Every mesh under `root`, baked into world space, as plain arrays. */
export function extractParts(root: THREE.Object3D): BodyPart[] {
  root.updateMatrixWorld(true)
  const parts: BodyPart[] = []
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return
    const geometry = (object.geometry as THREE.BufferGeometry).clone().applyMatrix4(object.matrixWorld)
    if (!geometry.getAttribute('normal')) geometry.computeVertexNormals()
    const material = (Array.isArray(object.material) ? object.material[0] : object.material) as
      | THREE.MeshStandardMaterial
      | undefined
    parts.push({
      name: object.name,
      positions: Float32Array.from(geometry.getAttribute('position').array),
      normals: Float32Array.from(geometry.getAttribute('normal').array),
      index: geometry.index ? (geometry.index.array.slice() as Uint16Array | Uint32Array) : null,
      color: `#${material?.color?.getHexString() ?? 'ffffff'}`,
      roughness: material?.roughness ?? 0.8,
      metalness: material?.metalness ?? 0,
      doubleSided: material?.side === THREE.DoubleSide,
    })
    geometry.dispose()
  })
  return parts
}

const loading = new Map<AgentKind, Promise<BodyModel>>()

/** The body for `kind`, loaded once and shared. A failed load is retried next call. */
export function loadAgentModel(kind: AgentKind): Promise<BodyModel> {
  let model = loading.get(kind)
  if (!model) {
    model = new GLTFLoader()
      .loadAsync(AGENT_MODEL_URLS[kind])
      .then((gltf) => normalizeBody(extractParts(gltf.scene)))
      .catch((error: unknown) => {
        loading.delete(kind)
        throw error
      })
    loading.set(kind, model)
  }
  return model
}

/** Every agent body that loads, by kind. Kinds that fail are logged and left out. */
export async function loadAgentModels(): Promise<Partial<Record<AgentKind, BodyModel>>> {
  const kinds = Object.keys(AGENT_MODEL_URLS) as AgentKind[]
  const loaded = await Promise.all(
    kinds.map((kind) =>
      loadAgentModel(kind).then(
        (model) => [kind, model] as const,
        (error: unknown) => {
          console.error(`The ${kind} model failed to load; drawing ovals`, error)
          return null
        },
      ),
    ),
  )
  return Object.fromEntries(loaded.filter((entry) => entry !== null))
}

/**
 * A body part as a three.js geometry, built with whichever copy of three.js is
 * given. The typed buffer attributes copy the arrays into that copy's own
 * realm: Showcase's three.js lives in an iframe, and its renderer checks
 * `array instanceof Float32Array`, which is false for an array from our
 * window, so passing ours straight through crashes Showcase's render loop.
 */
export function partGeometry(three: typeof THREE, part: BodyPart): THREE.BufferGeometry {
  const geometry = new three.BufferGeometry()
  geometry.setAttribute('position', new three.Float32BufferAttribute(part.positions, 3))
  geometry.setAttribute('normal', new three.Float32BufferAttribute(part.normals, 3))
  if (part.index) {
    geometry.setIndex(
      part.index.BYTES_PER_ELEMENT === 4
        ? new three.Uint32BufferAttribute(part.index, 1)
        : new three.Uint16BufferAttribute(part.index, 1),
    )
  }
  return geometry
}

/** How an agent is highlighted: held up in a queue, or squeezing past someone. */
export type AgentTint = 'none' | 'queued' | 'squeezing'

/** Glow added to a body for each highlight: red for stuck, amber for pushing past. */
export const TINT_COLORS: Record<Exclude<AgentTint, 'none'>, string> = {
  queued: '#dc2626',
  squeezing: '#f59e0b',
}

/** One of something for every tint. */
export function byTint<T>(make: (tint: AgentTint) => T): Record<AgentTint, T> {
  return { none: make('none'), queued: make('queued'), squeezing: make('squeezing') }
}

export function agentTint(agent: AgentFrame): AgentTint {
  return agent.movement === 'queued' || agent.movement === 'squeezing' ? agent.movement : 'none'
}

export function partMaterial(
  three: typeof THREE,
  part: BodyPart,
  tint: AgentTint = 'none',
): THREE.MeshStandardMaterial {
  const material = new three.MeshStandardMaterial({
    color: part.color,
    roughness: part.roughness,
    metalness: part.metalness,
    side: part.doubleSided ? three.DoubleSide : three.FrontSide,
  })
  if (tint !== 'none') {
    material.emissive = new three.Color(TINT_COLORS[tint])
    material.emissiveIntensity = 0.55
  }
  return material
}

/**
 * How an agent's body is posed: standing, lying hurt on the floor, or
 * carried over a firefighter's shoulder.
 */
export type AgentPose = 'standing' | 'lying' | 'carried'

/** A carried occupant is drawn this high, and this much smaller, beside their carrier. */
export const CARRY_HEIGHT_M = 1.0
export const CARRY_SCALE = 0.7

export function agentPose(agent: AgentFrame): AgentPose {
  if (agent.role.type !== 'occupant') return 'standing'
  if (agent.role.phase === 'injured') return 'lying'
  if (agent.role.phase === 'carried') return 'carried'
  return 'standing'
}

/** Half the spread of the drawn hose stream; matches the simulation's nozzle. */
const STREAM_HALF_ANGLE_RADIANS = (15 * Math.PI) / 180
const NOZZLE_HEIGHT_M = 1.1
const STREAM_TARGET_HEIGHT_M = 0.3

/**
 * Where to draw a firefighter's hose stream: a cone with its tip at the
 * nozzle, widening toward where it's aimed. For a unit cone pointing along
 * +Y with its tip at +Y (three.js's ConeGeometry), in Z-up world metres.
 * Null when the firefighter isn't spraying.
 */
export function nozzleStream(agent: AgentFrame): {
  position: [number, number, number]
  quaternion: [number, number, number, number]
  scale: [number, number, number]
} | null {
  if (agent.role.type !== 'firefighter' || !agent.role.nozzle_target_m) return null
  const target = agent.role.nozzle_target_m
  const start = {
    x: agent.position_m.x,
    y: agent.position_m.y,
    z: agent.position_m.z + NOZZLE_HEIGHT_M,
  }
  const end = { x: target.x, y: target.y, z: target.z + STREAM_TARGET_HEIGHT_M }
  const d = { x: end.x - start.x, y: end.y - start.y, z: end.z - start.z }
  const length = Math.hypot(d.x, d.y, d.z)
  if (length < 1e-6) return null
  const b = { x: d.x / length, y: d.y / length, z: d.z / length }
  // Turn -Y (away from the cone's tip) onto the stream direction, so the tip
  // sits at the nozzle: q = (a × b, 1 + a · b) for a = (0, -1, 0), normalized.
  const raw: [number, number, number, number] = [-b.z, 0, b.x, 1 - b.y]
  const norm = Math.hypot(...raw) || 1
  const radius = length * Math.tan(STREAM_HALF_ANGLE_RADIANS)
  return {
    position: [(start.x + end.x) / 2, (start.y + end.y) / 2, (start.z + end.z) / 2],
    quaternion: raw.map((component) => component / norm) as [number, number, number, number],
    scale: [radius, length, radius],
  }
}

/**
 * Which way each agent faces, as a turn about Z from +X: along its velocity
 * while walking, else wherever it faced before (or +X if never seen moving).
 */
export function nextHeadings(agents: AgentFrame[], previous: Map<string, number>): Map<string, number> {
  const headings = new Map<string, number>()
  for (const agent of agents) {
    const { x, y } = agent.velocity_m_per_s
    headings.set(
      agent.id,
      Math.hypot(x, y) >= TURN_MIN_SPEED_M_PER_S ? Math.atan2(y, x) : (previous.get(agent.id) ?? 0),
    )
  }
  return headings
}
