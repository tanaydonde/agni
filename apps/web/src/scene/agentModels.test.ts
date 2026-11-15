/// <reference types="node" />
import { readFileSync } from 'node:fs'
import { runInNewContext } from 'node:vm'
import * as THREE from 'three'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'
import { describe, expect, it } from 'vitest'
import type { AgentFrame } from '../fire/types'
import {
  AGENT_MODEL_URLS,
  agentPose,
  agentScale,
  nozzleStream,
  type BodyModel,
  type BodyPart,
  extractParts,
  nextHeadings,
  normalizeBody,
  partGeometry,
} from './agentModels'

/** Match OCCUPANT_RADIUS_M and FIREFIGHTER_RADIUS_M in crates/simulation-core/src/agent.rs. */
const OCCUPANT_RADIUS_M = 0.25
const FIREFIGHTER_RADIUS_M = 0.3

function part(positions: number[]): BodyPart {
  return {
    name: 'part',
    positions: Float32Array.from(positions),
    normals: new Float32Array(positions.length),
    index: null,
    color: '#ffffff',
    roughness: 1,
    metalness: 0,
    doubleSided: false,
  }
}

function extent(model: BodyModel) {
  let radius = 0
  const min = [Infinity, Infinity, Infinity]
  const max = [-Infinity, -Infinity, -Infinity]
  for (const { positions } of model.parts) {
    for (let i = 0; i < positions.length; i += 3) {
      radius = Math.max(radius, Math.hypot(positions[i], positions[i + 1]))
      for (let axis = 0; axis < 3; axis++) {
        min[axis] = Math.min(min[axis], positions[i + axis])
        max[axis] = Math.max(max[axis], positions[i + axis])
      }
    }
  }
  return { radius, min, max }
}

describe('normalizeBody', () => {
  it('centres the footprint at radius 1, stands it on z = 0, and turns glTF Y-up to Z-up', () => {
    // A glTF-space (Y-up) box, 4 wide (x), 6 tall (y), 2 deep (z), off-centre and below the floor.
    const model = normalizeBody([
      part([10, -1, 5, 14, -1, 7, 10, 5, 7, 14, 5, 5]),
    ])
    const { radius, min, max } = extent(model)
    expect(radius).toBeCloseTo(1)
    expect(min[2]).toBeCloseTo(0)
    // glTF height becomes Z; 6 tall over a footprint radius of hypot(2, 1).
    expect(max[2]).toBeCloseTo(6 / Math.hypot(2, 1))
    expect(model.heightPerRadius).toBeCloseTo(6 / Math.hypot(2, 1))
    // glTF front (+Z, depth) becomes +X.
    expect(max[0] - min[0]).toBeCloseTo(2 / Math.hypot(2, 1))
  })
})

describe.each([
  { kind: 'occupant' as const, radiusM: OCCUPANT_RADIUS_M },
  { kind: 'firefighter' as const, radiusM: FIREFIGHTER_RADIUS_M },
])('the $kind blob', ({ kind, radiusM }) => {
  const file = readFileSync(new URL(`../../public${AGENT_MODEL_URLS[kind]}`, import.meta.url))
  const load = () =>
    new GLTFLoader()
      .parseAsync(file.buffer.slice(file.byteOffset, file.byteOffset + file.byteLength), '')
      .then((gltf) => normalizeBody(extractParts(gltf.scene)))

  it('fills exactly the simulation collision circle when scaled by radius_m', async () => {
    const model = await load()
    const { radius, min } = extent(model)
    expect(radius * radiusM).toBeCloseTo(radiusM, 6)
    expect(min[2]).toBeCloseTo(0, 6)
    // Body plus both eyes.
    expect(model.parts).toHaveLength(3)
  })

  it('is drawn 1.75 times its collision circle', async () => {
    const model = await load()
    const scale = agentScale(radiusM)
    expect(extent(model).radius * scale).toBeCloseTo(1.75 * radiusM, 6)
    // The blob is about 2.5 footprint radii tall: 1.1 m for an occupant, 1.3 m for a firefighter.
    expect(model.heightPerRadius).toBeGreaterThan(2.3)
    expect(model.heightPerRadius).toBeLessThan(2.7)
  })

  it('faces +X: the eyes are on the front', async () => {
    const model = await load()
    const eyes = model.parts.filter((candidate) => candidate.name.startsWith('Eye'))
    expect(eyes).toHaveLength(2)
    for (const eye of eyes) {
      let x = 0
      for (let i = 0; i < eye.positions.length; i += 3) x += eye.positions[i]
      expect(x / (eye.positions.length / 3)).toBeGreaterThan(0.4)
    }
  })
})

describe('agentPose', () => {
  const occupant = (phase: 'injured' | 'carried' | 'moving_to_exit') =>
    ({ role: { type: 'occupant', phase } }) as AgentFrame

  it('lays injured people down and puts carried people over a shoulder', () => {
    expect(agentPose(occupant('injured'))).toBe('lying')
    expect(agentPose(occupant('carried'))).toBe('carried')
    expect(agentPose(occupant('moving_to_exit'))).toBe('standing')
  })
})

describe('nozzleStream', () => {
  const firefighter = (target?: { x: number; y: number; z: number }) =>
    ({
      position_m: { x: 0, y: 0, z: 0 },
      role: { type: 'firefighter', crew_id: 'engine-1', phase: 'suppressing', nozzle_target_m: target },
    }) as AgentFrame

  it('draws nothing unless the hose is aimed', () => {
    expect(nozzleStream(firefighter())).toBeNull()
  })

  it('runs from the nozzle to the target, tip at the nozzle, widening with distance', () => {
    const stream = nozzleStream(firefighter({ x: 4, y: 0, z: 0.8 }))!
    // Nozzle at 1.1 m, target 0.3 m above its fire: both at 1.1 m here.
    expect(stream.position).toEqual([2, 0, 1.1])
    expect(stream.scale[1]).toBeCloseTo(4)
    expect(stream.scale[0]).toBeCloseTo(4 * Math.tan((15 * Math.PI) / 180))
    // The cone's tip (+Y) turned toward the nozzle (-X).
    const tip = new THREE.Vector3(0, 1, 0).applyQuaternion(
      new THREE.Quaternion(...stream.quaternion),
    )
    expect(tip.x).toBeCloseTo(-1)
    expect(tip.y).toBeCloseTo(0)
  })
})

describe('nextHeadings', () => {
  const agent = (id: string, vx: number, vy: number) =>
    ({ id, velocity_m_per_s: { x: vx, y: vy, z: 0 } }) as AgentFrame

  it('faces along the velocity while walking', () => {
    const headings = nextHeadings([agent('a', 0, 1)], new Map())
    expect(headings.get('a')).toBeCloseTo(Math.PI / 2)
  })

  it('keeps the last heading when standing still', () => {
    const headings = nextHeadings([agent('a', 0, 0), agent('b', 0, 0)], new Map([['a', 1.2]]))
    expect(headings.get('a')).toBe(1.2)
    expect(headings.get('b')).toBe(0)
  })
})

describe('partGeometry', () => {
  it("copies arrays into the target three.js's realm", () => {
    // Showcase's three.js runs in an iframe; its renderer rejects typed arrays
    // that fail `instanceof` there. Here the part comes from another realm and
    // our THREE plays Showcase's.
    const foreign = runInNewContext(
      '({ positions: new Float32Array(9), normals: new Float32Array(9), index: new Uint16Array([0, 1, 2]) })',
    ) as Pick<BodyPart, 'positions' | 'normals' | 'index'>
    expect(foreign.positions instanceof Float32Array).toBe(false)

    const geometry = partGeometry(THREE, { ...part([]), ...foreign })
    expect(geometry.getAttribute('position').array).toBeInstanceOf(Float32Array)
    expect(geometry.getAttribute('normal').array).toBeInstanceOf(Float32Array)
    expect(geometry.index?.array).toBeInstanceOf(Uint16Array)
  })
})
