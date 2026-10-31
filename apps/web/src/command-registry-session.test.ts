import { describe, expect, it } from 'vitest'
import { registryCommands } from './command-registry'
import { chordForCommand, findShortcutConflicts, shortcuts } from './shortcuts'

describe('session commands in the registries', () => {
  it('registers theme, agent, and sign-out once each', () => {
    const ids = ['theme:toggle', 'agent:open', 'session:sign-out']
    for (const id of ids) {
      expect(registryCommands.filter((command) => command.id === id)).toHaveLength(1)
    }
    expect(new Set(registryCommands.map((command) => command.id)).size).toBe(registryCommands.length)
  })

  it('keeps shortcut chords conflict-free with agent:open linked', () => {
    expect(chordForCommand('agent:open')).toBe('Ctrl+J')
    expect(findShortcutConflicts(shortcuts)).toEqual([])
  })
})
