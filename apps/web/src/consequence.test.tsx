// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Json } from './api'
import { ConsequenceHost } from './consequence'
import { publishProposal } from './proposal-bus'

const api = vi.hoisted(() => vi.fn())

vi.mock('./api', async () => {
  const actual = await vi.importActual<typeof import('./api')>('./api')
  return { ...actual, api }
})

const proposal: Json = {
  title: 'Delete type',
  source: 'user',
  rows: [],
  consequence: {
    lines: ['Delete type recipe'],
    warnings: [],
    samples: [],
    jobs: [],
    definition_errors: [],
    destructive: true,
    confirm_name: 'Recipe',
    affected_objects: 0,
  },
}

function Host() {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  return (
    <QueryClientProvider client={client}>
      <button type="button" data-testid="open-delete">Delete</button>
      <ConsequenceHost session={{ setProposalId: () => undefined, push: () => undefined }} />
    </QueryClientProvider>
  )
}

describe('consequence dialog', () => {
  beforeEach(() => {
    api.mockReset()
    api.mockImplementation((path: string) => {
      if (path === '/v1/definitions') return Promise.resolve({ languages: ['en'], types: [] })
      if (String(path).startsWith('/v1/proposals/')) return Promise.resolve(proposal)
      return Promise.resolve({})
    })
  })

  afterEach(() => {
    cleanup()
  })

  it('closes on Escape without discarding the change and returns focus', async () => {
    render(<Host />)
    const opener = screen.getByTestId('open-delete')
    opener.focus()
    publishProposal('proposal-1')
    const dialog = await screen.findByRole('dialog')
    expect(screen.getByTestId('consequence')).toBeTruthy()
    screen.getByRole('button', { name: 'Cancel' }).focus()
    expect(document.activeElement).not.toBe(opener)

    const underneath = vi.fn()
    window.addEventListener('keydown', underneath)
    fireEvent.keyDown(dialog, { key: 'Escape' })
    window.removeEventListener('keydown', underneath)

    await waitFor(() => expect(screen.queryByTestId('consequence')).toBeNull())
    expect(underneath).not.toHaveBeenCalled()
    const writes = api.mock.calls.filter((call) => {
      const path = String(call[0])
      const method = String((call[1] as RequestInit | undefined)?.method ?? 'GET').toUpperCase()
      return method !== 'GET' || path.includes('/discard') || path.includes('/accept')
    })
    expect(writes).toEqual([])
    await waitFor(() => expect(document.activeElement).toBe(opener))
  })
})
