import { describe, expect, it } from 'vitest'
import { sv } from './i18n'

describe('copy', () => {
  it('keeps the agent fallback in one place', () => {
    expect(sv.agentUnavailable).toContain('OPENAI_API_KEY')
    expect(sv.loginHint).toContain('.env')
  })
})
