// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useMemo, useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Json } from './api'
import { ProposalView, type PanesSession } from './panes'

const api = vi.hoisted(() => vi.fn())

vi.mock('./api', async () => {
  const actual = await vi.importActual<typeof import('./api')>('./api')
  return { ...actual, api }
})

const proposal: Json = {
  id: 'p1',
  title: 'Proposal',
  status: 'open',
  rows: [],
}

function Host({ push }: { push: (line: string) => void }) {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  const session = useMemo<PanesSession>(
    () => ({
      group: null,
      objectId: null,
      proposalId: 'p1',
      language: 'en',
      setGroup: () => undefined,
      setObjectId: () => undefined,
      setProposalId: () => undefined,
      push,
    }),
    [push],
  )
  return (
    <QueryClientProvider client={client}>
      <ProposalView session={session} />
    </QueryClientProvider>
  )
}

function calls(path: string) {
  return api.mock.calls.filter((call) => String(call[0]) === path).length
}

describe('proposal mutation errors', () => {
  beforeEach(() => {
    api.mockReset()
    api.mockImplementation((path: string) => {
      const url = String(path)
      if (url === '/v1/proposals') return Promise.resolve({ proposals: [proposal] })
      if (url === '/v1/proposals/p1') return Promise.resolve(proposal)
      if (url === '/v1/definitions') return Promise.resolve({ languages: ['en'], types: [], attributes: [], structures: [] })
      if (url.endsWith('/accept')) return Promise.reject(new Error('this proposal is no longer pending'))
      if (url.endsWith('/discard')) return Promise.reject(new Error('proposal p1 is already being applied'))
      return Promise.resolve({})
    })
  })

  afterEach(() => {
    cleanup()
  })

  it('shows an accept failure in the toolbar and pushes the message', async () => {
    const push = vi.fn()
    render(<Host push={push} />)
    await screen.findAllByText('Proposal')

    fireEvent.click(screen.getByTestId('save-all'))

    await waitFor(() => expect(push).toHaveBeenCalledWith('this proposal is no longer pending'))
    expect(screen.getByTestId('proposal-error').textContent).toBe('this proposal is no longer pending')
    await waitFor(() => expect(calls('/v1/proposals')).toBeGreaterThan(1))
    expect(calls('/v1/proposals/p1')).toBeGreaterThan(1)
  })

  it('shows a discard failure in the toolbar and pushes the message', async () => {
    const push = vi.fn()
    render(<Host push={push} />)
    await screen.findAllByText('Proposal')

    fireEvent.click(screen.getByTestId('discard-all'))

    await waitFor(() => expect(push).toHaveBeenCalledWith('proposal p1 is already being applied'))
    expect(screen.getByTestId('proposal-error').textContent).toBe('proposal p1 is already being applied')
    await waitFor(() => expect(calls('/v1/proposals')).toBeGreaterThan(1))
    expect(calls('/v1/proposals/p1')).toBeGreaterThan(1)
  })
})
