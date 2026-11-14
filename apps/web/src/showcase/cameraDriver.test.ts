import { describe, expect, it, vi } from 'vitest'
import { createHopDriver } from './cameraDriver'
import type { Direction, MpSdk } from './sdk'

/** A fake SDK whose camera moves finish only when the test says so. */
function fakeSdk() {
  const moves: { direction: Direction; resolve: () => void; reject: () => void }[] = []
  const sdk = {
    Camera: {
      moveInDirection: vi.fn(
        (direction: Direction) =>
          new Promise<void>((resolve, reject) => {
            moves.push({ direction, resolve, reject: () => reject(new Error('no sweep')) })
          }),
      ),
    },
    Sweep: { moveTo: vi.fn(() => Promise.resolve('start')) },
    Mode: { moveTo: vi.fn(() => Promise.resolve('mode.inside')) },
  } as unknown as Pick<MpSdk, 'Camera' | 'Sweep' | 'Mode'>
  return { sdk, moves }
}

/** Lets the driver's promise callbacks run. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

describe('createHopDriver', () => {
  it('maps WASD and QE to Showcase directions', () => {
    const expected: [string, Direction][] = [
      ['KeyW', 'FORWARD'],
      ['KeyS', 'BACK'],
      ['KeyA', 'LEFT'],
      ['KeyD', 'RIGHT'],
      ['KeyE', 'UP'],
      ['KeyQ', 'DOWN'],
    ]
    for (const [code, direction] of expected) {
      const { sdk, moves } = fakeSdk()
      const driver = createHopDriver(sdk, null)
      expect(driver.keyDown(code)).toBe(true)
      expect(moves.map((move) => move.direction)).toEqual([direction])
    }
  })

  it('ignores keys it does not handle', () => {
    const { sdk, moves } = fakeSdk()
    const driver = createHopDriver(sdk, null)
    expect(driver.keyDown('KeyX')).toBe(false)
    expect(driver.keyDown('ArrowUp')).toBe(false)
    expect(moves).toHaveLength(0)
  })

  it('keeps one move in flight and repeats while the key is held', async () => {
    const { sdk, moves } = fakeSdk()
    const driver = createHopDriver(sdk, null)
    driver.keyDown('KeyW')
    driver.keyDown('KeyW') // auto-repeat while the first hop is still moving
    expect(moves).toHaveLength(1)

    moves[0].resolve()
    await settle()
    expect(moves).toHaveLength(2) // still held: hop again

    driver.keyUp('KeyW')
    moves[1].resolve()
    await settle()
    expect(moves).toHaveLength(2) // released: stop
  })

  it('still finishes a tap that is released mid-move', async () => {
    const { sdk, moves } = fakeSdk()
    const driver = createHopDriver(sdk, null)
    driver.keyDown('KeyD')
    driver.keyUp('KeyD')
    expect(moves).toHaveLength(1)
    moves[0].resolve()
    await settle()
    expect(moves).toHaveLength(1)
  })

  it('follows the most recently pressed held key', async () => {
    const { sdk, moves } = fakeSdk()
    const driver = createHopDriver(sdk, null)
    driver.keyDown('KeyW')
    driver.keyDown('KeyD')
    moves[0].resolve()
    await settle()
    expect(moves[1].direction).toBe('RIGHT')

    driver.keyUp('KeyD')
    moves[1].resolve()
    await settle()
    expect(moves[2].direction).toBe('FORWARD')
  })

  it('stops repeating a direction with no scan in it', async () => {
    const { sdk, moves } = fakeSdk()
    const driver = createHopDriver(sdk, null)
    driver.keyDown('KeyA')
    moves[0].reject()
    await settle()
    expect(moves).toHaveLength(1)

    driver.keyDown('KeyA') // pressing again tries again
    expect(moves).toHaveLength(2)
  })

  it('goes home to the start scan, or inside when there is none', () => {
    const withStart = fakeSdk()
    createHopDriver(withStart.sdk, 'sweep-1').home()
    expect(withStart.sdk.Sweep.moveTo).toHaveBeenCalledWith('sweep-1', { transition: 'transition.fly' })

    const withoutStart = fakeSdk()
    createHopDriver(withoutStart.sdk, null).home()
    expect(withoutStart.sdk.Mode.moveTo).toHaveBeenCalledWith('mode.inside')
  })

  it('does nothing after dispose', async () => {
    const { sdk, moves } = fakeSdk()
    const driver = createHopDriver(sdk, null)
    driver.keyDown('KeyW')
    driver.dispose()
    moves[0].resolve()
    await settle()
    driver.keyDown('KeyW')
    expect(moves).toHaveLength(1)
  })
})
