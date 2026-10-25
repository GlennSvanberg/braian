import { describe, expect, it } from 'vitest'
import { copy } from './i18n'

describe('copy', () => {
  it('keeps the agent fallback in one place', () => {
    expect(copy.agentUnavailable).toContain('OPENAI_API_KEY')
    expect(copy.loginHint).toContain('.env')
  })
})
