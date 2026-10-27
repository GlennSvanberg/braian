import { describe, expect, it } from 'vitest'
import { modelCommandTitle, withModelCommand } from './model-command'
import type { PaletteItem } from './palette-model'

const ask: PaletteItem = { id: 'ask', group: 'ask', title: 'Ask AI', subtitle: 'Stays in this panel' }

describe('model commands', () => {
  it('names an attribute draft and keeps Ask AI last', () => {
    const hint = modelCommandTitle('add attribute weight (kg) to Drill')
    expect(hint?.title).toBe('Add attribute weight (kg) to Drill')
    const rows = withModelCommand('add attribute weight (kg) to Drill', [ask])
    expect(rows[0]?.sourceId).toBe('model:command')
    expect(rows[0]?.title).toContain('weight (kg)')
    expect(rows.at(-1)?.id).toBe('ask')
  })

  it('ignores ordinary search text', () => {
    expect(modelCommandTitle('camp')).toBeNull()
  })

  it('hints group commands without stealing a class move', () => {
    expect(modelCommandTitle('add group canopy under camp')?.title).toBe('Add group canopy under camp')
    expect(modelCommandTitle('move group camp to timber')?.title).toBe('Move group camp to timber')
    expect(modelCommandTitle('move bag to timber')?.title).toBe('Move bag to timber')
    const rows = withModelCommand('save assortment Summer tents', [ask])
    expect(rows[0]?.sourceId).toBe('assortment:save')
    expect(rows[0]?.title).toBe('Save assortment Summer tents')
  })
})
