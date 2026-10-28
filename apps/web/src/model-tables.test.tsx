// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { copy } from './i18n'
import { ValueListsTable, valueListOptions } from './model-tables'

const api = vi.hoisted(() => vi.fn())
const stageModel = vi.hoisted(() => vi.fn(async () => undefined))
const reviewDraft = vi.hoisted(() => vi.fn(async () => undefined))

vi.mock('./api', async () => {
  const actual = await vi.importActual<typeof import('./api')>('./api')
  return { ...actual, api }
})

vi.mock('./model-stage', async () => {
  const actual = await vi.importActual<typeof import('./model-stage')>('./model-stage')
  return { ...actual, stageModel, reviewDraft }
})

const definitions = {
  types: [],
  attributes: [],
  value_lists: [
    {
      key: 'colour',
      labels: { en: 'Colour' },
      options: [
        { code: 'red', labels: { en: 'Red' } },
        { code: 'blue', labels: { en: 'Blue' } },
      ],
    },
  ],
  unit_families: [],
  structures: [],
}

function Host({ onOpenList }: { onOpenList?: (key: string) => void }) {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  return (
    <QueryClientProvider client={client}>
      <ValueListsTable onOpenList={onOpenList} />
    </QueryClientProvider>
  )
}

describe('value lists table', () => {
  beforeEach(() => {
    api.mockReset()
    stageModel.mockClear()
    reviewDraft.mockClear()
    api.mockImplementation((path: string) => {
      if (path === '/v1/definitions') return Promise.resolve(definitions)
      if (path === '/v1/model/draft') return Promise.resolve({ count: 0, conflict: false, steps: [] })
      return Promise.resolve({})
    })
  })

  afterEach(() => cleanup())

  it('lists value lists and opens master-detail on activate', async () => {
    const opened: string[] = []
    const events: string[] = []
    const onEvent = (event: Event) => {
      events.push(String((event as CustomEvent<{ key?: string }>).detail?.key ?? ''))
    }
    window.addEventListener('braian-open-value-list', onEvent)
    render(<Host onOpenList={(key) => opened.push(key)} />)
    await screen.findByTestId('value-lists-table')
    expect(screen.getByText('Colour')).toBeTruthy()
    fireEvent.click(screen.getByText('Colour'))
    await waitFor(() => {
      expect(opened).toEqual(['colour'])
      expect(events).toEqual(['colour'])
    })
    window.removeEventListener('braian-open-value-list', onEvent)
  })

  it('adds a draft row from plus for Review and save', async () => {
    render(<Host />)
    await screen.findByTestId('value-lists-table')
    fireEvent.click(screen.getByTestId('table-add-row'))
    await waitFor(() => expect(screen.getByTestId('review-and-save')).toBeTruthy())
  })

  it('reads options for a list key', () => {
    const rows = valueListOptions(definitions, 'colour')
    expect(rows.map((row) => row.code)).toEqual(['red', 'blue'])
    expect(copy.valueListValues).toBe('List values')
  })
})
