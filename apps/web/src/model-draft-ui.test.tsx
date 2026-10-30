// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ModelDraftHost } from './model-draft-ui'

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

const draft = {
  count: 1,
  conflict: false,
  updated_at: '2026-10-27T10:00:00Z',
  lines: ['Create type recipe'],
  warnings: [],
  consequence: { destructive: false, lines: ['Create type recipe'] },
}

function Host() {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  return (
    <QueryClientProvider client={client}>
      <ModelDraftHost />
    </QueryClientProvider>
  )
}

describe('review dialog', () => {
  beforeEach(() => {
    mock.use((path) => {
      if (path === '/v1/model/draft') return Promise.resolve(draft)
      return Promise.resolve({})
    })
  })

  afterEach(() => {
    cleanup()
  })

  it('closes on Escape without saving and returns focus', async () => {
    render(<Host />)
    const opener = await screen.findByTestId('review-save')
    opener.focus()
    fireEvent.click(opener)
    const dialog = await screen.findByRole('dialog')
    expect(screen.getByTestId('review-dialog')).toBeTruthy()
    screen.getByRole('button', { name: 'Cancel' }).focus()
    expect(document.activeElement).not.toBe(opener)

    const underneath = vi.fn()
    window.addEventListener('keydown', underneath)
    fireEvent.keyDown(dialog, { key: 'Escape' })
    window.removeEventListener('keydown', underneath)

    await waitFor(() => expect(screen.queryByTestId('review-dialog')).toBeNull())
    expect(underneath).not.toHaveBeenCalled()
    const saves = mock.calls().filter((call) => String(call[0]).includes('/v1/model/draft/save'))
    expect(saves).toEqual([])
    await waitFor(() => expect(document.activeElement).toBe(opener))
  })
})
