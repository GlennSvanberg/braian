import { describe, expect, it } from 'vitest'
import { mergeFrecency, rankByFrecency, score, touch, type FrecencyEntry } from './frecency'

function memory() {
  const data = new Map<string, string>()
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value)
    },
    removeItem: (key: string) => {
      data.delete(key)
    },
    clear: () => data.clear(),
    key: () => null,
    length: 0,
  } as Storage
}

describe('frecency', () => {
  it('ranks a recently used command above an unused one', () => {
    const store = memory()
    const now = 1_700_000_000_000
    touch('ui.filter', now, store)
    touch('ui.filter', now + 10, store)
    const ranked = rankByFrecency(
      [
        { id: 'ui.set_theme', title: 'Theme' },
        { id: 'ui.filter', title: 'Filter' },
      ],
      now + 10,
      store,
    )
    expect(ranked[0]?.id).toBe('ui.filter')
    expect(score({ uses: 2, last: now } satisfies FrecencyEntry, now)).toBeGreaterThan(1)
  })

  it('keeps the higher use count when the server copy arrives', () => {
    const merged = mergeFrecency({ 'ui.filter': { uses: 1, last: 10 } }, { 'ui.filter': { uses: 4, last: 20 } })
    expect(merged['ui.filter']?.uses).toBe(4)
  })
})
