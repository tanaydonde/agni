import { describe, expect, it } from 'vitest'
import { centralSweep, viewFor } from './defurnish'

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

describe('viewFor', () => {
  const views = [
    { id: 'layered', type: 'viewtype.layeredbase' },
    { id: 'empty', type: 'viewtype.defurnish' },
    { id: 'scan', type: 'viewtype.base' },
  ]

  it('finds the defurnished view', () => {
    expect(viewFor(views, 'defurnished')?.id).toBe('empty')
  })

  it('prefers the plain base view for the furnished space', () => {
    expect(viewFor(views, 'furnished')?.id).toBe('scan')
    expect(viewFor(views.slice(0, 2), 'furnished')?.id).toBe('layered')
  })

  it('returns undefined when the space lacks the view', () => {
    expect(viewFor([{ type: 'viewtype.base' }], 'defurnished')).toBeUndefined()
  })
})
