import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createLiveChecker, editKey } from './liveCheck'
import type { CheckEditsResponse, EditCommand } from './types'

const move = (x: number): EditCommand => ({
  type: 'move_object',
  object_id: 'desk-1',
  position_m: { x, y: 0, z: 0 },
})
const VALID: CheckEditsResponse = { valid: true, error: null }

describe('createLiveChecker', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('sends only the newest edit and never overlaps requests', async () => {
    const resolvers: (() => void)[] = []
    const sent: EditCommand[] = []
    const check = vi.fn((edit: EditCommand) => {
      sent.push(edit)
      return new Promise<CheckEditsResponse>((resolve) => resolvers.push(() => resolve(VALID)))
    })
    const results: string[] = []
    const checker = createLiveChecker(check, (key) => results.push(key), 50)

    checker.request(move(1))
    await vi.advanceTimersByTimeAsync(0)
    checker.request(move(2))
    checker.request(move(3)) // replaces 2 while 1 is in flight
    await vi.advanceTimersByTimeAsync(200)
    expect(sent).toEqual([move(1)])

    resolvers[0]()
    await vi.advanceTimersByTimeAsync(200)
    expect(sent).toEqual([move(1), move(3)])
    resolvers[1]()
    await vi.advanceTimersByTimeAsync(0)
    expect(results).toEqual([editKey(move(1)), editKey(move(3))])
  })

  it('ignores answers that arrive after cancel', async () => {
    let resolve = () => {}
    const check = () => new Promise<CheckEditsResponse>((r) => (resolve = () => r(VALID)))
    const onResult = vi.fn()
    const checker = createLiveChecker(check, onResult, 50)

    checker.request(move(1))
    await vi.advanceTimersByTimeAsync(0)
    checker.cancel()
    resolve()
    await vi.advanceTimersByTimeAsync(0)
    expect(onResult).not.toHaveBeenCalled()
  })
})
