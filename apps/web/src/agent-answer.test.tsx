// @vitest-environment jsdom
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { renderAgentAnswer, stripAgentMarkdown } from './agent-answer'

describe('agent answer formatting', () => {
  it('keeps a space between sentences that models glue together', () => {
    // Mirrors the backend join: UI still shows whatever the API sends.
    const glued = "change.I couldn't create the proposal"
    const fixed = "change. I couldn't create the proposal"
    expect(glued.includes('change.I')).toBe(true)
    expect(fixed.includes('change. I')).toBe(true)
  })

  it('renders bold markdown instead of raw asterisks', () => {
    const { container } = render(<p data-testid="ai-answer">{renderAgentAnswer('The selected product is **Feta 36**.')}</p>)
    expect(screen.getByTestId('ai-answer').textContent).toBe('The selected product is Feta 36.')
    expect(container.querySelector('strong')?.textContent).toBe('Feta 36')
    expect(screen.getByTestId('ai-answer').textContent).not.toContain('**')
  })

  it('strips bold markers when a plain string is needed', () => {
    expect(stripAgentMarkdown('Hello **world**')).toBe('Hello world')
  })
})
