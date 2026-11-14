import { describe, expect, it } from 'vitest'
import { centralSweep } from './defurnish'

const sweep = (sid: string, x: number, z: number, enabled = true) => ({
  sid,
  enabled,
  position: { x, y: 1.5, z },
})

describe('centralSweep', () => {
  it('picks the enabled scan nearest the middle of all enabled scans', () => {
    const sweeps = [sweep('west', -10, 0), sweep('east', 10, 0), sweep('middle', 1, 0.5), sweep('north', 0, 10)]
    expect(centralSweep(sweeps)).toBe('middle')
  })

  it('ignores disabled scans', () => {
    const sweeps = [sweep('a', -1, 0), sweep('b', 1, 0), sweep('centre-but-off', 0, 0, false)]
    expect(['a', 'b']).toContain(centralSweep(sweeps))
  })

  it('returns null when there are no enabled scans', () => {
    expect(centralSweep([])).toBeNull()
    expect(centralSweep([sweep('off', 0, 0, false)])).toBeNull()
  })
})
