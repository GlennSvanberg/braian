import { describe, expect, it } from 'vitest'
import {
  assertNoShortcutConflicts,
  chordForCommand,
  findShortcutConflicts,
  shortcuts,
  type Shortcut,
} from './shortcuts'

describe('shortcut registry', () => {
  it('has no conflicts among registered shortcuts', () => {
    expect(findShortcutConflicts(shortcuts)).toEqual([])
    expect(() => assertNoShortcutConflicts(shortcuts)).not.toThrow()
  })

  it('fails when two shortcuts share a chord in overlapping scopes', () => {
    const colliding: Shortcut[] = [
      {
        id: 'a',
        chord: 'Ctrl+K',
        scopes: ['app'],
        description: 'A',
        hotkeys: ['ctrl+k'],
      },
      {
        id: 'b',
        chord: 'Ctrl+K',
        scopes: ['app', 'layout'],
        description: 'B',
        hotkeys: ['ctrl+k'],
      },
    ]
    const conflicts = findShortcutConflicts(colliding)
    expect(conflicts).toHaveLength(1)
    expect(conflicts[0]?.chord).toBe('Ctrl+K')
    expect(() => assertNoShortcutConflicts(colliding)).toThrow(/Shortcut conflicts/)
  })

  it('allows the same chord in non-overlapping scopes', () => {
    const ok: Shortcut[] = [
      { id: 'a', chord: 'Ctrl+L', scopes: ['app'], description: 'A', hotkeys: ['ctrl+l'] },
      { id: 'b', chord: 'Ctrl+L', scopes: ['table'], description: 'B', hotkeys: ['ctrl+l'] },
    ]
    expect(findShortcutConflicts(ok)).toEqual([])
  })

  it('maps command ids to display chords (Ctrl+K is palette:search only)', () => {
    expect(chordForCommand('palette:search')).toBe('Ctrl+K')
    expect(chordForCommand('layout:add-view')).toBeUndefined()
    expect(chordForCommand('layout:focus:left')).toBe('Alt+←')
  })

  it('binds Ctrl+Shift column moves (Control only) and exposes command chords', () => {
    expect(chordForCommand('column:move-left')).toBe('Ctrl+Shift+←')
    expect(chordForCommand('column:move-right')).toBe('Ctrl+Shift+→')
    const left = shortcuts.find((item) => item.id === 'table.column.move-left')
    expect(left?.hotkeys).toEqual(['ctrl+shift+left'])
    expect(left?.hotkeys.some((key) => key.includes('meta'))).toBe(false)
  })
})

  it('maps #141 Ctrl+K commands into the registry without chord conflicts', () => {
    expect(chordForCommand('agent:open')).toBe('Ctrl+J')
    expect(chordForCommand('theme:toggle')).toBeUndefined()
    expect(chordForCommand('session:sign-out')).toBeUndefined()
    const commandIds = shortcuts.map((item) => item.commandId).filter(Boolean)
    expect(commandIds.filter((id) => id === 'agent:open')).toHaveLength(1)
    expect(findShortcutConflicts(shortcuts)).toEqual([])
  })

