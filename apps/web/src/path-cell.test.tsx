// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { useMemo, useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PATH_SUMMARY_VISIBLE, readPathFromDoc, summarizePathValues } from './path-cell'
import { ObjectTable, type PanesSession } from './panes'

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: (options: { count: number }) => ({
    getVirtualItems: () =>
      Array.from({ length: options.count }, (_, index) => ({
        index,
        start: index * 28,
        size: 28,
        end: (index + 1) * 28,
        key: index,
        lane: 0,
      })),
    getTotalSize: () => options.count * 28,
    scrollToIndex: () => undefined,
  }),
}))

describe('path cell helpers', () => {
  it('summarizes many linked values with a remainder', () => {
    expect(summarizePathValues(['A', 'B'])).toEqual({ text: 'A, B', title: 'A, B' })
    expect(summarizePathValues(['A', 'B', 'C', 'D'])).toEqual({
      text: `A, B +${4 - PATH_SUMMARY_VISIBLE}`,
      title: 'A, B, C, D',
    })
    expect(summarizePathValues([])).toEqual({ text: '', title: '' })
    expect(summarizePathValues(undefined)).toEqual({ text: '', title: '' })
  })

  it('reads part paths from indexed Typesense fields', () => {
    const weightId = '018f0000-0000-7000-8000-000000000102'
    const defs = {
      attributes: [{ id: weightId, key: 'weight' }],
    }
    const field = `a_${weightId.replaceAll('-', '_')}`
    expect(
      readPathFromDoc({ [field]: 500, [`${field}__unit`]: 'g' }, defs, 'weight.unit'),
    ).toBe('g')
    expect(readPathFromDoc({ [field]: 500, [`${field}__unit`]: 'g' }, defs, 'weight')).toBe('500')
    expect(readPathFromDoc({}, defs, 'product>brand')).toBe('')
  })
})

const product = '018f0000-0000-7000-8000-000000000200'

function json(body: unknown) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  })
}

function client() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } })
}

function HopTable() {
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
          id: '018f0000-0000-7000-8000-000000000999',
          type: 'object_table',
          title: 'Products',
          state: { object_type: product, type_key: 'product', table: { order: ['sku', 'product>brand', 'weight.unit'] } },
        }}
        workspace={{ id: 'ws' }}
      />
    </QueryClientProvider>
  )
}

describe('ObjectTable hop and part columns', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    )
    let pathValueCalls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input)
        if (url.includes('/objects:pathValues')) {
          pathValueCalls += 1
          ;(globalThis as { __pathValueCalls?: number }).__pathValueCalls = pathValueCalls
          const body = JSON.parse(String(init?.body ?? '{}'))
          expect(body.paths).toEqual(['product>brand'])
          expect(body.ids).toEqual(['row-1'])
          return json({
            values: {
              'row-1': {
                'product>brand': ['Meadow', 'Dairy Co', 'Extra'],
              },
            },
          })
        }
        if (url.includes('/column-prefs')) return json({ document: null })
        if (url.includes('/attribute-paths')) {
          return json({
            paths: [
              { text: 'sku', label: 'SKU' },
              { text: 'weight.unit', label: 'Weight › Unit' },
              { text: 'product>brand', label: 'Product › Brand' },
            ],
            system_columns: [
              { key: 'type', label: 'Type', default_index: 2 },
              { key: 'version', label: 'Version', default_index: 4 },
            ],
          })
        }
        if (url.includes('/v1/search')) {
          return json({
            found: 1,
            page: 1,
            hits: [
              {
                document: {
                  id: 'row-1',
                  key: 'SKU-1',
                  type_key: 'product',
                  version: 1,
                  a_018f0000_0000_7000_8000_000000000102: 500,
                  a_018f0000_0000_7000_8000_000000000102__unit: 'g',
                },
              },
            ],
          })
        }
        if (url.includes('/v1/definitions')) {
          return json({
            languages: ['en'],
            types: [
              {
                id: product,
                key: 'product',
                labels: { en: 'Product' },
                singular: { en: 'Product' },
                plural: { en: 'Products' },
                key_attribute: 'sku',
                attributes: [],
              },
            ],
            attributes: [{ id: '018f0000-0000-7000-8000-000000000102', key: 'weight' }],
            structures: [],
          })
        }
        if (url.includes('/v1/assortments')) return json({ assortments: [] })
        if (url.includes('/v1/proposals')) return json({ pending: { total: 0 } })
        if (url.includes('/v1/views')) return json({})
        return json({})
      }),
    )
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('renders hop summaries and part paths from one batch request', async () => {
    render(<HopTable />)
    await waitFor(() => expect(screen.getByTestId('hop-cell-product>brand').textContent).toBe('Meadow, Dairy Co +1'))
    expect(screen.getByTestId('hop-cell-product>brand').getAttribute('title')).toBe('Meadow, Dairy Co, Extra')
    await waitFor(() => expect(screen.getByText('g')).toBeTruthy())
    expect((globalThis as { __pathValueCalls?: number }).__pathValueCalls).toBe(1)
    const puts = vi.mocked(fetch).mock.calls.filter(([input]) => String(input).includes('/objects:pathValues'))
    expect(puts).toHaveLength(1)
  })
})
