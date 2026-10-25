import { describe, expect, it } from 'vitest'
import { modelCommandTitle, withModelCommand } from './model-command'
import type { PaletteItem } from './palette-model'

const ask: PaletteItem = { id: 'ask', group: 'ask', title: 'Ask AI', subtitle: 'Stays in this panel' }

describe('model commands', () => {
  it('names a power-tool draft and keeps Ask AI last', () => {
    const hint = modelCommandTitle('set up a model for power tools')
    expect(hint?.title).toBe('Draft a model for power tools')
    const rows = withModelCommand('add attribute weight (kg) to Drill', [ask])
    expect(rows[0]?.sourceId).toBe('model:command')
    expect(rows[0]?.title).toContain('weight (kg)')
    expect(rows.at(-1)?.id).toBe('ask')
  })

  it('ignores ordinary search text', () => {
    expect(modelCommandTitle('talt')).toBeNull()
  })
})
