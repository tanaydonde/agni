import { describe, expect, it } from 'vitest'
import { describeWorldError, unwrapWorldError } from './worldErrors'

describe('world errors', () => {
  it('unwraps batch failures to the failing edit', () => {
    const inner = { code: 'wall_collision', object_id: 'desk-1', room_id: 'r', edge_id: 'e' }
    expect(unwrapWorldError({ code: 'batch_edit_failed', edit_index: 0, reason: inner })).toEqual(inner)
  })

  it('explains placement rejections', () => {
    expect(
      describeWorldError({
        code: 'batch_edit_failed',
        edit_index: 0,
        reason: { code: 'furniture_overlap', object_id: 'desk-1', conflicting_object_id: 'chair-2' },
      }),
    ).toBe('Overlaps chair-2')
    expect(describeWorldError({ code: 'opening_clearance_blocked' })).toBe('Blocks a doorway')
    expect(describeWorldError({ code: 'something_new' })).toBe('Edit rejected (something_new)')
  })
})
