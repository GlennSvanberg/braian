// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ModelDraftHost } from './model-draft-ui'

const api = vi.hoisted(() => vi.fn())

vi.mock('./api', async () => {
  const actual = await vi.importActual<typeof import('./api')>('./api')
  return { ...actual, api }
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
    api.mockReset()
    api.mockImplementation((path: string) => {
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
    const saves = api.mock.calls.filter((call) => String(call[0]).includes('/v1/model/draft/save'))
    expect(saves).toEqual([])
    await waitFor(() => expect(document.activeElement).toBe(opener))
  })
})
