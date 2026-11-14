import type { SimulationFrame } from './types'

/**
 * The newest frame at or before `seconds`, or undefined before the first.
 * Frames are ordered with strictly increasing times.
 */
export function frameAtTime(frames: SimulationFrame[], seconds: number): SimulationFrame | undefined {
  let low = 0
  let high = frames.length - 1
  let answer: SimulationFrame | undefined
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
