import { beforeEach, describe, expect, it } from 'vitest'
import { agentRequestBody } from './agent-stream'
import { copy } from './i18n'
import { publishFrame, publishSelection, resetUiContext } from './ui-context'

describe('copy', () => {
  beforeEach(() => resetUiContext())

  it('keeps the agent fallback in one place', () => {
    expect(copy.agentUnavailable).toContain('OPENAI_API_KEY')
    expect(copy.commandPlaceholder).toBe('Search, open views, or ask AI')
  })

  it('sends the marked object with the question', () => {
    publishFrame({
      workspace: { id: 'ws', name: 'Enrich objects' },
      panes: [{ id: 'table', type: 'object_table', title: 'Objects', focused: true }],
      focused: 'table',
      language: 'en',
    })
    publishSelection('table', [{
      id: '018f1000-0000-7000-8000-000000000011',
      key: 'SYN-000001',
      name: 'Insulation 2',
      type: 'article',
      version: '2',
    }])
    const body = agentRequestBody([{ role: 'user', content: 'Which product is selected?' }])
    expect(body.stream).toBe(true)
    expect(body.context.selection[0]?.key).toBe('SYN-000001')
    expect(body.context.selection[0]?.name).toBe('Insulation 2')
    expect(body.context.selection[0]?.version).toBe('2')
    expect(body.context.language).toBe('en')
  })
})
