import { describe, expect, it } from 'vitest'
import {
  deletionNeedsMove,
  groupKeysToDelete,
  moveTargets,
  renameOnKey,
  structureKeyFromName,
} from './structure-menu'

const nodes = [
  { key: 'outdoor' },
  { key: 'tents', parentKey: 'outdoor' },
  { key: 'bags', parentKey: 'tents' },
  { key: 'building' },
]

describe('structure menu', () => {
  it('deletes a group together with the groups under it', () => {
    expect(groupKeysToDelete(nodes, 'outdoor')).toEqual(['outdoor', 'tents', 'bags'])
    expect(groupKeysToDelete(nodes, 'building')).toEqual(['building'])
  })

  it('refuses a move into the node or one of its descendants', () => {
    expect(moveTargets(nodes, 'outdoor')).toEqual(['building'])
    expect(moveTargets(nodes, 'tents')).toEqual(['outdoor', 'building'])
    expect(moveTargets(nodes, 'building')).toEqual(['outdoor', 'tents', 'bags'])
  })

  it('asks for a destination only when objects are in the way', () => {
    expect(deletionNeedsMove(0)).toBe(false)
    expect(deletionNeedsMove(2)).toBe(true)
  })

  it('derives a key from the display name and commits rename from the keyboard', () => {
    expect(structureKeyFromName(' eCom ')).toBe('ecom')
    expect(structureKeyFromName('CO2 footprint')).toBe('co2_footprint')
    expect(structureKeyFromName('   ')).toBe('')
    expect(renameOnKey('Enter')).toBe('commit')
    expect(renameOnKey('Escape')).toBe('cancel')
    expect(renameOnKey('a')).toBe('type')
  })
})
