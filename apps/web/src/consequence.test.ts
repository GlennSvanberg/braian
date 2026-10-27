import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

describe('consequence dialog', () => {
  it('closes on Escape without discarding the change', () => {
    const source = readFileSync(new URL('./consequence.tsx', import.meta.url), 'utf8')
    const handler = source.slice(source.indexOf("event.key === 'Escape'"), source.indexOf("event.key === 'Escape'") + 180)
    expect(handler).toContain('setId(null)')
    expect(handler).not.toContain('discard')
  })
})
