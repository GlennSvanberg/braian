import { describe, expect, it } from 'vitest'
import { dockTab } from './layout'

describe('dockTab', () => {
  it('splits a pane to the right and keeps the moved tab', () => {
    const next = dockTab({ tabs: ['a', 'b'], active: 'a' }, 'b', 'right', 'a')
    expect(next).toEqual({
      direction: 'row',
      ratio: 0.5,
      first: { tabs: ['a'], active: 'a' },
      second: { tabs: ['b'], active: 'b' },
    })
  })
})
