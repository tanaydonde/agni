import { DIRECTION, type Direction, MODE, type MpSdk, SWEEP_TRANSITION } from './sdk'

/**
 * Keyboard camera control for the Showcase view. Swappable: today's driver
 * hops between scans; a free-flying one can replace it if Showcase lets us
 * drive its camera directly.
 */
export interface CameraDriver {
  /** Returns true when the key is one the driver handles (so the caller can preventDefault). */
  keyDown(code: string): boolean
  keyUp(code: string): void
  /** Back to where the tour started. */
  home(): void
  dispose(): void
}

/** W/A/S/D walk; Q/E go down/up (between floors where there are any). */
export const HOP_KEYS: Readonly<Record<string, Direction>> = {
  KeyW: DIRECTION.FORWARD,
  KeyS: DIRECTION.BACK,
  KeyA: DIRECTION.LEFT,
  KeyD: DIRECTION.RIGHT,
  KeyE: DIRECTION.UP,
  KeyQ: DIRECTION.DOWN,
}

type HopSdk = Pick<MpSdk, 'Camera' | 'Sweep' | 'Mode'>

/**
 * Showcase can only stand the camera at scan positions, so each key press
 * hops to the next scan in that direction. Holding a key keeps hopping, one
 * move at a time; the most recently pressed held key wins. A direction with
 * no scan in it stops repeating until the key is pressed again.
 */
export function createHopDriver(sdk: HopSdk, startSweep: string | null): CameraDriver {
  const held: Direction[] = []
  let moving = false
  let disposed = false

  const release = (direction: Direction) => {
    const index = held.indexOf(direction)
    if (index >= 0) held.splice(index, 1)
  }

  const pump = () => {
    if (moving || disposed) return
    const direction = held.at(-1)
    if (!direction) return
    moving = true
    sdk.Camera.moveInDirection(direction)
      .catch(() => release(direction)) // nowhere to go that way
      .finally(() => {
        moving = false
        pump()
      })
  }

  return {
    keyDown(code) {
      const direction = HOP_KEYS[code]
      if (!direction) return false
      if (!held.includes(direction)) held.push(direction)
      pump()
      return true
    },
    keyUp(code) {
      const direction = HOP_KEYS[code]
      if (direction) release(direction)
    },
    home() {
      held.length = 0
      const back = startSweep
        ? sdk.Sweep.moveTo(startSweep, { transition: SWEEP_TRANSITION.FLY })
        : sdk.Mode.moveTo(MODE.INSIDE)
      back.catch(() => {})
    },
    dispose() {
      disposed = true
      held.length = 0
    },
  }
}
