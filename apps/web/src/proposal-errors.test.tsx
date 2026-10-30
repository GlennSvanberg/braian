// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useMemo, useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Json } from './api'
import { ProposalView, type PanesSession } from './panes'

const mock = vi.hoisted(() => {
  const methods = { get: vi.fn(), post: vi.fn(), put: vi.fn(), patch: vi.fn(), delete: vi.fn() }
  const use = (impl: (path: string, init?: { body?: unknown; params?: { path?: Record<string, string> } }) => unknown) => {
    for (const fn of Object.values(methods)) fn.mockReset().mockImplementation(impl)
  }
  const calls = () => Object.values(methods).flatMap((fn) => fn.mock.calls)
  return { methods, use, calls }
})

vi.mock('./api', async () => {
  const actual = await vi.importActual<typeof import('./api')>('./api')
  return { ...actual, api: mock.methods }
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
  return mock.calls().filter((call) => String(call[0]) === path).length
}

function proposalApi(path: string) {
  if (path === '/v1/proposals') return Promise.resolve({ proposals: [proposal] })
  if (path === '/v1/proposals/{id}') return Promise.resolve(proposal)
  if (path === '/v1/definitions') return Promise.resolve({ languages: ['en'], types: [], attributes: [], structures: [] })
  if (path.endsWith('/accept')) return Promise.reject(new Error('this proposal is no longer pending'))
  if (path.endsWith('/discard')) return Promise.reject(new Error('proposal p1 is already being applied'))
  return Promise.resolve({})
}

describe('proposal mutation errors', () => {
  beforeEach(() => {
    mock.use(proposalApi)
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
    expect(calls('/v1/proposals/{id}')).toBeGreaterThan(1)
  })

  it('shows a discard failure in the toolbar and pushes the message', async () => {
    const push = vi.fn()
    render(<Host push={push} />)
    await screen.findAllByText('Proposal')

    fireEvent.click(screen.getByTestId('discard-all'))

    await waitFor(() => expect(push).toHaveBeenCalledWith('proposal p1 is already being applied'))
    expect(screen.getByTestId('proposal-error').textContent).toBe('proposal p1 is already being applied')
    await waitFor(() => expect(calls('/v1/proposals')).toBeGreaterThan(1))
    expect(calls('/v1/proposals/{id}')).toBeGreaterThan(1)
  })

  it('disables discard while accept is pending, and save while discard is pending', async () => {
    let finishAccept: (error: Error) => void = () => undefined
    mock.use((path: string) => {
      if (path === '/v1/proposals') return Promise.resolve({ proposals: [proposal] })
      if (path === '/v1/proposals/{id}') return Promise.resolve(proposal)
      if (path === '/v1/definitions') return Promise.resolve({ languages: ['en'], types: [], attributes: [], structures: [] })
      if (path.endsWith('/accept')) return new Promise((_, reject) => { finishAccept = reject })
      if (path.endsWith('/discard')) return new Promise(() => undefined)
      return Promise.resolve({})
    })
    const push = vi.fn()
    render(<Host push={push} />)
    await screen.findAllByText('Proposal')
    fireEvent.click(screen.getByTestId('save-all'))
    await waitFor(() => expect((screen.getByTestId('discard-all') as HTMLButtonElement).disabled).toBe(true))
    expect((screen.getByTestId('save-all') as HTMLButtonElement).disabled).toBe(true)
    finishAccept(new Error('this proposal is no longer pending'))
    await waitFor(() => expect((screen.getByTestId('discard-all') as HTMLButtonElement).disabled).toBe(false))
  })

})
