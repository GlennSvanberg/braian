import { describe, expect, it } from 'vitest'
import { askIndex, visibleRows, type PaletteItem } from './palette-model'

const catalog: PaletteItem[] = [
  { id: 'view:detail', group: 'views', title: 'Detail', subtitle: 'Open the detail view' },
  { id: 'object:history', group: 'object', title: 'History', subtitle: 'On the selected object' },
]

describe('palette rows', () => {
  it('keeps Ask AI last and inside the same list', () => {
    const rows = visibleRows('hej', catalog, [], {})
    expect(rows.at(-1)?.id).toBe('ask')
    expect(rows.at(-1)?.title).toContain('Ask AI')
    expect(rows.at(-1)?.subtitle).toBe('')
    expect(rows.every((row) => row.group !== 'ask' || row.id === 'ask')).toBe(true)
    expect(askIndex(rows)).toBe(rows.length - 1)
  })

  it('puts search hits above the ask row with a middle dot', () => {
    const rows = visibleRows('talt', [], [{ id: '1', group: 'search', title: 'ART-TALT-1 · Kupol', subtitle: 'artikel' }], {})
    const hit = rows.find((row) => row.group === 'search')
    expect(hit?.title).toContain('·')
    expect(rows.at(-1)?.group).toBe('ask')
  })
})
