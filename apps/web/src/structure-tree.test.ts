import { describe, expect, it } from 'vitest'
import { dropReparents, groupKeyFromLabel } from './structure-tree'

const groups = [
  { id: 'outdoor' },
  { id: 'tents', parent: 'outdoor' },
  { id: 'bags', parent: 'outdoor' },
  { id: 'building' },
]

describe('structure tree moves', () => {
  it('turns a label into a group key', () => {
    expect(groupKeyFromLabel('Canopy')).toBe('canopy')
    expect(groupKeyFromLabel('  New group  ')).toBe('new_group')
  })

  it('refuses a drop onto the node or one of its descendants', () => {
    expect(dropReparents(groups, 'outdoor', 'outdoor')).toBe(false)
    expect(dropReparents(groups, 'outdoor', 'tents')).toBe(false)
    expect(dropReparents(groups, 'tents', 'building')).toBe(true)
    expect(dropReparents(groups, 'bags', 'tents')).toBe(true)
  })
})
