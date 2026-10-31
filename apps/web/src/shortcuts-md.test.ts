import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { formatShortcutsMarkdown, shortcuts } from './shortcuts'

const root = join(dirname(fileURLToPath(import.meta.url)), '../../..')
const docPath = join(root, 'docs/shortcuts.md')

describe('docs/shortcuts.md', () => {
  it('matches formatShortcutsMarkdown() from the registry', () => {
    const expected = formatShortcutsMarkdown(shortcuts)
    if (process.env.BRAIAN_WRITE_SHORTCUTS_MD === '1') {
      writeFileSync(docPath, expected)
    }
    const actual = readFileSync(docPath, 'utf8')
    expect(actual).toBe(expected)
  })

  it('lists Ctrl+Shift column moves and keeps Ctrl+K on palette:search', () => {
    const md = formatShortcutsMarkdown(shortcuts)
    expect(md).toContain('| Ctrl+Shift+← | Move the active column left |')
    expect(md).toContain('| Ctrl+Shift+→ | Move the active column right |')
    expect(md).toContain('| Ctrl+K | Open the command palette |')
    expect(md).not.toMatch(/Sign out|themeDark|themeLight/i)
  })
})
