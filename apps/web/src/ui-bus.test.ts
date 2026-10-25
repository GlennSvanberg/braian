import { describe, expect, it } from 'vitest'
import { applyUiResult, onUi } from './ui-bus'

describe('ui bus', () => {
  it('applies a registry result to listeners', () => {
    const seen: string[] = []
    const stop = onUi((event) => seen.push(`${event.name}:${String(event.args.q ?? '')}`))
    expect(applyUiResult({ ui: true, name: 'ui.filter', args: { q: 'talt' } })).toBe(true)
    expect(applyUiResult({ reply: 'no' })).toBe(false)
    stop()
    expect(seen).toEqual(['ui.filter:talt'])
  })
})
