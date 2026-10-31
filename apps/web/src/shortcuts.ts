/**
 * Typed shortcut register — single source for chords, scopes, and command links.
 * Bind with react-hotkeys-hook; conflict-check overlapping scopes that share a chord.
 */

export type ShortcutScope = 'app' | 'layout' | 'table'

export type Shortcut = {
  id: string
  /** Display + conflict key, e.g. Ctrl+K, Alt+←, Ctrl+Shift+→ */
  chord: string
  scopes: ShortcutScope[]
  description: string
  commandId?: string
  /**
   * Keys for react-hotkeys-hook (comma-joined). Prefer explicit ctrl/meta — not `mod` —
   * so Mac and Windows match today's shell behaviour.
   */
  hotkeys: string[]
  /** When true, ignore the event if Shift is held (Alt+arrows vs Alt+Shift view focus). */
  rejectShift?: boolean
  /** Skip while the command palette is open or a generic input is focused (layout chords). */
  layoutOnly?: boolean
}

export const shortcuts: Shortcut[] = [
  {
    id: 'palette.search',
    chord: 'Ctrl+K',
    scopes: ['app'],
    description: 'Open the command palette',
    commandId: 'palette:search',
    hotkeys: ['ctrl+k', 'meta+k'],
  },
  {
    id: 'palette.ai',
    chord: 'Ctrl+J',
    scopes: ['app'],
    description: 'Open the AI palette',
    hotkeys: ['ctrl+j', 'meta+j'],
  },
  {
    id: 'review.save',
    chord: 'Ctrl+S',
    scopes: ['app'],
    description: 'Save review drafts',
    hotkeys: ['ctrl+s', 'meta+s'],
  },
  {
    id: 'feedback.send',
    chord: 'Ctrl+Shift+B',
    scopes: ['app'],
    description: 'Send feedback',
    commandId: 'feedback:send',
    hotkeys: ['ctrl+shift+b', 'meta+shift+b'],
  },
  {
    id: 'help.open',
    chord: 'F1',
    scopes: ['app'],
    description: 'Show keyboard shortcuts',
    hotkeys: ['f1'],
  },
  {
    id: 'help.close',
    chord: 'Escape',
    scopes: ['app'],
    description: 'Close help and menus',
    hotkeys: ['escape'],
  },
  ...workspaceNumberShortcuts(),
  {
    id: 'layout.focus.left',
    chord: 'Alt+←',
    scopes: ['layout'],
    description: 'Focus the neighbour on the left',
    commandId: 'layout:focus:left',
    hotkeys: ['alt+left'],
    rejectShift: true,
    layoutOnly: true,
  },
  {
    id: 'layout.focus.right',
    chord: 'Alt+→',
    scopes: ['layout'],
    description: 'Focus the neighbour on the right',
    commandId: 'layout:focus:right',
    hotkeys: ['alt+right'],
    rejectShift: true,
    layoutOnly: true,
  },
  {
    id: 'layout.focus.up',
    chord: 'Alt+↑',
    scopes: ['layout'],
    description: 'Focus the neighbour above',
    commandId: 'layout:focus:up',
    hotkeys: ['alt+up'],
    rejectShift: true,
    layoutOnly: true,
  },
  {
    id: 'layout.focus.down',
    chord: 'Alt+↓',
    scopes: ['layout'],
    description: 'Focus the neighbour below',
    commandId: 'layout:focus:down',
    hotkeys: ['alt+down'],
    rejectShift: true,
    layoutOnly: true,
  },
  {
    id: 'layout.move.left',
    chord: 'Ctrl+Alt+←',
    scopes: ['layout'],
    description: 'Move the focused view left',
    commandId: 'layout:move_view_left',
    hotkeys: ['ctrl+alt+left', 'meta+alt+left'],
    layoutOnly: true,
  },
  {
    id: 'layout.move.right',
    chord: 'Ctrl+Alt+→',
    scopes: ['layout'],
    description: 'Move the focused view right',
    commandId: 'layout:move_view_right',
    hotkeys: ['ctrl+alt+right', 'meta+alt+right'],
    layoutOnly: true,
  },
  {
    id: 'layout.move.up',
    chord: 'Ctrl+Alt+↑',
    scopes: ['layout'],
    description: 'Move the focused view up',
    commandId: 'layout:move_view_up',
    hotkeys: ['ctrl+alt+up', 'meta+alt+up'],
    layoutOnly: true,
  },
  {
    id: 'layout.move.down',
    chord: 'Ctrl+Alt+↓',
    scopes: ['layout'],
    description: 'Move the focused view down',
    commandId: 'layout:move_view_down',
    hotkeys: ['ctrl+alt+down', 'meta+alt+down'],
    layoutOnly: true,
  },
  {
    id: 'layout.resize.left',
    chord: 'Ctrl+Alt+Shift+←',
    scopes: ['layout'],
    description: 'Resize the focused split left',
    hotkeys: ['ctrl+alt+shift+left', 'meta+alt+shift+left'],
    layoutOnly: true,
  },
  {
    id: 'layout.resize.right',
    chord: 'Ctrl+Alt+Shift+→',
    scopes: ['layout'],
    description: 'Resize the focused split right',
    hotkeys: ['ctrl+alt+shift+right', 'meta+alt+shift+right'],
    layoutOnly: true,
  },
  {
    id: 'layout.resize.up',
    chord: 'Ctrl+Alt+Shift+↑',
    scopes: ['layout'],
    description: 'Resize the focused split up',
    hotkeys: ['ctrl+alt+shift+up', 'meta+alt+shift+up'],
    layoutOnly: true,
  },
  {
    id: 'layout.resize.down',
    chord: 'Ctrl+Alt+Shift+↓',
    scopes: ['layout'],
    description: 'Resize the focused split down',
    hotkeys: ['ctrl+alt+shift+down', 'meta+alt+shift+down'],
    layoutOnly: true,
  },
  {
    id: 'layout.zoom',
    chord: 'Ctrl+Alt+Z',
    scopes: ['layout'],
    description: 'Maximize the focused pane',
    commandId: 'layout:zoom',
    hotkeys: ['ctrl+alt+z', 'meta+alt+z'],
    layoutOnly: true,
  },
  {
    id: 'layout.close',
    chord: 'Ctrl+Alt+W',
    scopes: ['layout'],
    description: 'Close the focused view',
    commandId: 'layout:close_view',
    hotkeys: ['ctrl+alt+w', 'meta+alt+w'],
    layoutOnly: true,
  },
  {
    id: 'table.column.move-left',
    chord: 'Ctrl+Shift+←',
    scopes: ['table'],
    description: 'Move the active column left',
    commandId: 'column:move-left',
    // Explicit Control only — not meta — so Mac word-selection chords stay free.
    hotkeys: ['ctrl+shift+left'],
    layoutOnly: true,
  },
  {
    id: 'table.column.move-right',
    chord: 'Ctrl+Shift+→',
    scopes: ['table'],
    description: 'Move the active column right',
    commandId: 'column:move-right',
    hotkeys: ['ctrl+shift+right'],
    layoutOnly: true,
  },
]

function workspaceNumberShortcuts(): Shortcut[] {
  return Array.from({ length: 9 }, (_, index) => {
    const n = index + 1
    return {
      id: `workspace.${n}`,
      chord: `Ctrl+${n}`,
      scopes: ['app'] as ShortcutScope[],
      description: `Open workspace ${n}`,
      hotkeys: [`ctrl+${n}`, `meta+${n}`],
    }
  })
}

export function shortcutById(id: string, list: Shortcut[] = shortcuts): Shortcut | undefined {
  return list.find((item) => item.id === id)
}

export function chordForCommand(commandId: string, list: Shortcut[] = shortcuts): string | undefined {
  return list.find((item) => item.commandId === commandId)?.chord
}

export type ShortcutConflict = { a: string; b: string; chord: string; scopes: ShortcutScope[] }

function scopesOverlap(left: ShortcutScope[], right: ShortcutScope[]) {
  return left.some((scope) => right.includes(scope))
}

/** Conflicts when the same chord is registered on overlapping scopes. */
export function findShortcutConflicts(list: Shortcut[] = shortcuts): ShortcutConflict[] {
  const found: ShortcutConflict[] = []
  for (let i = 0; i < list.length; i += 1) {
    for (let j = i + 1; j < list.length; j += 1) {
      const a = list[i]!
      const b = list[j]!
      if (a.chord !== b.chord) continue
      if (!scopesOverlap(a.scopes, b.scopes)) continue
      found.push({
        a: a.id,
        b: b.id,
        chord: a.chord,
        scopes: a.scopes.filter((scope) => b.scopes.includes(scope)),
      })
    }
  }
  return found
}

export function assertNoShortcutConflicts(list: Shortcut[] = shortcuts) {
  const conflicts = findShortcutConflicts(list)
  if (conflicts.length === 0) return
  const lines = conflicts.map((item) => `${item.a} vs ${item.b} on ${item.chord} [${item.scopes.join(',')}]`)
  throw new Error(`Shortcut conflicts:\n${lines.join('\n')}`)
}

const SCOPE_HEADINGS: Record<ShortcutScope, string> = {
  app: 'App',
  layout: 'Layout',
  table: 'Table',
}

/** Markdown reference generated from the typed registry (docs/shortcuts.md). */
export function formatShortcutsMarkdown(list: Shortcut[] = shortcuts): string {
  const lines: string[] = [
    '# Keyboard shortcuts',
    '',
    'Generated from `apps/web/src/shortcuts.ts`. Do not edit by hand — run `node apps/web/scripts/gen-shortcuts-md.mjs`.',
    '',
  ]
  for (const scope of ['app', 'layout', 'table'] as ShortcutScope[]) {
    const rows = list.filter((item) => item.scopes.includes(scope))
    if (rows.length === 0) continue
    lines.push(`## ${SCOPE_HEADINGS[scope]}`, '')
    lines.push('| Chord | Action |')
    lines.push('| --- | --- |')
    for (const item of rows) {
      lines.push(`| ${item.chord} | ${item.description} |`)
    }
    lines.push('')
  }
  return `${lines.join('\n').trimEnd()}\n`
}
