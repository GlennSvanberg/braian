import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { copy } from './i18n'

describe('group wording', () => {
  it('keeps the word class out of user-facing copy', () => {
    for (const [key, value] of Object.entries(copy)) {
      expect(value, key).not.toMatch(/\bclass(es)?\b/i)
    }
  })

  it('shows the key warning in the consequence dialog', () => {
    const source = readFileSync(new URL('./consequence.tsx', import.meta.url), 'utf8')
    expect(source).toContain('data-testid="key-warning"')
    expect(source).toContain('consequence.warnings')
  })
})
