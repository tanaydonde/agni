import type { Vec3 } from '../scene/types'
import type {
  AgentFrame,
  AgentTimelineFrame,
  FirefighterPhase,
  SimulateFireResponse,
  SimulationFrame,
} from './types'

/**
 * The newest frame at or before `seconds`, or undefined before the first.
 * Frames are ordered with strictly increasing times.
 */
export function frameAtTime<Frame extends { time_seconds: number }>(
  frames: Frame[],
  seconds: number,
): Frame | undefined {
  let low = 0
  let high = frames.length - 1
  let answer: Frame | undefined
  while (low <= high) {
    const middle = Math.floor((low + high) / 2)
    if (frames[middle].time_seconds <= seconds) {
      answer = frames[middle]
      low = middle + 1
    } else {
      high = middle - 1
    }
  }
  return answer
}

/** Body colors: occupants white, firefighters orange. */
export const AGENT_COLORS = { occupant: '#ffffff', firefighter: '#f97316' } as const

/** Whether to draw an agent on `floorId`: anyone who hasn't left the building. */
export function isShownAgent(agent: AgentFrame, floorId: string): boolean {
  return agent.floor_id === floorId && agent.role.phase !== 'exited'
}

const CREW_PHASE_LABELS: Record<FirefighterPhase, string> = {
  staging: 'arriving',
  advancing: 'moving in',
  suppressing: 'spraying',
  rescuing: 'reaching someone',
  carrying: 'carrying someone out',
  withdrawing: 'leaving',
  exited: 'out',
  incapacitated: 'down',
}

/** The first crew's leader (`{crew_id}/1` of the first crew), if there is a crew. */
export function leadFirefighter(agents: AgentFrame[]): AgentFrame | undefined {
  return agents
    .filter((agent) => agent.role.type === 'firefighter')
    .sort((left, right) => left.id.localeCompare(right.id))[0]
}

/** Where the fire starts: the first cell to burn and its room, or null if nothing does. */
export function fireOrigin(frames: SimulationFrame[]): { position_m: Vec3; room_id: string } | null {
  for (const frame of frames) {
    const node = frame.surface_fire.find((candidate) => candidate.state === 'burning')
    if (node) return { position_m: node.position_m, room_id: node.room_id }
  }
  return null
}

/** What the first crew's leader is doing, for the playback bar, or null with no crew. */
export function crewStatus(agents: AgentFrame[]): string | null {
  const leader = leadFirefighter(agents)
  if (!leader || leader.role.type !== 'firefighter') return null
  return `Crew ${CREW_PHASE_LABELS[leader.role.phase]}`
}

/**
 * Where everyone is in `run`: every simulation step when the server sent
 * them (`agent_frames`), else the playback frames.
 */
export function agentTimeline(run: SimulateFireResponse): AgentTimelineFrame[] {
  return run.agent_frames ?? run.results.frames
}

/** Linearly interpolated agent positions at the playback clock. Fire remains
 * frame-stepped, but people move smoothly between authoritative Rust frames.
 * Pass `agentTimeline(run)` so the straight lines stay short enough to keep
 * people inside rooms. */
export function agentsAtTime(frames: AgentTimelineFrame[], seconds: number): AgentFrame[] {
  const lower = frameAtTime(frames, seconds)
  if (!lower) return []
  const lowerIndex = frames.indexOf(lower)
  const upper = frames[lowerIndex + 1]
  if (!upper || seconds <= lower.time_seconds) return lower.agents
  const fraction = Math.min(
    1,
    (seconds - lower.time_seconds) / (upper.time_seconds - lower.time_seconds),
  )
  const upperById = new Map(upper.agents.map((agent) => [agent.id, agent]))

  return lower.agents.map((agent) => {
    const next = upperById.get(agent.id)
    if (!next) return agent
    return {
      ...agent,
      position_m: lerpVector(agent.position_m, next.position_m, fraction),
      velocity_m_per_s: lerpVector(agent.velocity_m_per_s, next.velocity_m_per_s, fraction),
      exposure: {
        heat: lerp(agent.exposure.heat, next.exposure.heat, fraction),
        toxic_gas: lerp(agent.exposure.toxic_gas, next.exposure.toxic_gas, fraction),
      },
    }
  })
}

/** How many occupants started injured, and how many of them are out. */
export function rescueCounts(agents: AgentFrame[]): { rescued: number; injured: number } {
  let rescued = 0
  let injured = 0
  for (const agent of agents) {
    if (agent.role.type !== 'occupant' || !agent.role.injured) continue
    injured++
    if (agent.role.phase === 'exited') rescued++
  }
  return { rescued, injured }
}

/** How many people inside are held up: queued, or squeezing past someone. */
export function heldUpCount(agents: AgentFrame[]): number {
  return agents.filter(
    (agent) =>
      agent.role.phase !== 'exited' &&
      (agent.movement === 'queued' || agent.movement === 'squeezing'),
  ).length
}

export function occupantCounts(agents: AgentFrame[]): { inside: number; exited: number } {
  let inside = 0
  let exited = 0
  for (const agent of agents) {
    if (agent.role.type !== 'occupant') continue
    if (agent.role.phase === 'exited') exited++
    else inside++
  }
  return { inside, exited }
}

function lerpVector(
  left: { x: number; y: number; z: number },
  right: { x: number; y: number; z: number },
  fraction: number,
) {
  return {
    x: lerp(left.x, right.x, fraction),
    y: lerp(left.y, right.y, fraction),
    z: lerp(left.z, right.z, fraction),
  }
}

function lerp(left: number, right: number, fraction: number): number {
  return left + (right - left) * fraction
}
