// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { useMemo, useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ObjectTable, type PanesSession } from './panes'

const product = '018f0000-0000-7000-8000-000000000200'
const article = '018f0000-0000-7000-8000-000000000201'

function client() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } })
}

function ProductsTable() {
  const [queryClient] = useState(client)
  const session = useMemo<PanesSession>(
    () => ({
      group: null,
      objectId: null,
      proposalId: null,
      language: 'en',
      setGroup: () => undefined,
      setObjectId: () => undefined,
      setProposalId: () => undefined,
      push: () => undefined,
    }),
    [],
  )
  return (
    <QueryClientProvider client={queryClient}>
      <ObjectTable
        session={session}
        view={{
          id: 'table',
          type: 'object_table',
          title: 'Products',
          state: { object_type: product, type_key: 'product' },
        }}
        workspace={{ id: 'ws' }}
      />
    </QueryClientProvider>
  )
}

beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.includes('/v1/definitions')) {
        return Response.json({
          languages: ['en'],
          types: [
            { id: product, key: 'product', labels: { en: 'Product' }, singular: { en: 'Product' }, plural: { en: 'Products' }, key_attribute: 'sku', attributes: [] },
            { id: article, key: 'article', labels: { en: 'Article' }, singular: { en: 'Article' }, plural: { en: 'Articles' }, key_attribute: 'sku', attributes: [] },
          ],
          attributes: [],
          structures: [],
        })
      }
      if (url.includes('/v1/assortments')) {
        return Response.json({
          assortments: [
            { id: 'a3', name: 'Cheese', type: 'product', type_id: product, query: { group: 'food:cheese' } },
            { id: 'a1', name: 'Cheese', type: 'article', type_id: article, query: { group: 'food:cheese' } },
            { id: 'a2', name: 'Organic', type: 'article', type_id: article, query: { labels: 'organic_eu' } },
          ],
        })
      }
      if (url.includes('/v1/search')) return Response.json({ found: 0, hits: [] })
      return Response.json({})
    }),
  )
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('product selections', () => {
  it('lists only selections of the product type', async () => {
    render(<ProductsTable />)
    await waitFor(() => expect(screen.getAllByTestId('assortment').length).toBeGreaterThan(0))
    const chips = screen.getAllByTestId('assortment')
    expect(chips.every((chip) => chip.textContent === 'Cheese')).toBe(true)
    expect(screen.queryByText('Organic')).toBeNull()
    const fetchMock = vi.mocked(fetch)
    expect(fetchMock.mock.calls.some(([input]) => String(input).includes('type=product'))).toBe(true)
  })
})
