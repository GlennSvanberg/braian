// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useMemo, useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { FieldPicker } from './field-picker'
import { ensureSystemColumns, orderHiddenToDocument, prefsToOrderHidden, type ColumnPrefsResponse, type SystemViewColumn } from './column-prefs'
import { ObjectTable, type PanesSession } from './panes'
import { dispatchUi } from './ui-bus'

const product = '018f0000-0000-7000-8000-000000000200'

const paths = [
  { text: 'sku', label: 'SKU' },
  { text: 'name', label: 'Name' },
  { text: 'weight', label: 'Weight' },
  { text: 'packaging>name', label: 'Packaging › Name' },
]

const systemColumns: SystemViewColumn[] = [
  { key: 'type', label: 'Type', default_index: 2 },
  { key: 'version', label: 'Version', default_index: 4 },
]

function json(body: unknown) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  })
}

function client() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } })
}

function PrefsTable() {
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
        view={{ id: '018f0000-0000-7000-8000-000000000999', type: 'object_table', title: 'Products', state: { object_type: product, type_key: 'product' } }}
        workspace={{ id: 'ws' }}
      />
    </QueryClientProvider>
  )
}

function headerIds() {
  return Array.from(document.querySelectorAll('.head [data-testid^="col-"]')).map((node) => node.getAttribute('data-testid'))
}

const prefsDocument: ColumnPrefsResponse = {
  document: {
    version: 1,
    columns: [
      { path: 'sku', visible: true },
      { path: 'name', visible: false },
      { path: 'weight', visible: true, stale: true },
      { path: 'packaging>name', visible: true },
    ],
  },
  stale_paths: ['weight'],
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
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.includes('/column-prefs')) {
        if ((init?.method ?? 'GET').toUpperCase() === 'PUT') return json({ document: {} })
        return json(prefsDocument)
      }
      if (url.includes('/attribute-paths')) return json({ paths, system_columns: systemColumns })
      if (url.includes('/v1/search')) return json({ found: 0, hits: [] })
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
          attributes: [],
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



describe('column prefs helpers', () => {
  it('applies order and hidden while skipping stale paths', () => {
    expect(prefsToOrderHidden(prefsDocument)).toEqual({
      order: ['sku', 'name', 'packaging>name'],
      hidden: ['name'],
    })
  })

  it('returns null for an empty document', () => {
    expect(prefsToOrderHidden({ document: null })).toBeNull()
    expect(prefsToOrderHidden({ document: { version: 1, columns: [] } })).toBeNull()
    expect(prefsToOrderHidden(undefined)).toBeNull()
  })

  it('maps built-in ids to attribute paths and keeps system columns from the API allowlist', () => {
    expect(
      orderHiddenToDocument(['key', 'name', 'type', 'status', 'version'], ['status'], null, systemColumns),
    ).toEqual({
      version: 1,
      columns: [
        { path: 'number', visible: true },
        { path: 'name', visible: true },
        { path: 'type', visible: true },
        { path: 'status', visible: false },
        { path: 'version', visible: true },
      ],
    })
  })

  it('injects missing system columns at their default positions', () => {
    expect(ensureSystemColumns(['number', 'name', 'status'], [], systemColumns)).toEqual({
      order: ['number', 'name', 'type', 'status', 'version'],
      hidden: [],
    })
    expect(
      prefsToOrderHidden(
        {
          document: {
            version: 1,
            columns: [
              { path: 'number', visible: true },
              { path: 'name', visible: true },
              { path: 'status', visible: false },
            ],
          },
        },
        systemColumns,
      ),
    ).toEqual({
      order: ['number', 'name', 'type', 'status', 'version'],
      hidden: ['status'],
    })
  })

  it('drops paths that the type does not list', () => {
    expect(orderHiddenToDocument(['sku', 'gone', 'packaging>name'], [], ['sku', 'packaging>name'])).toEqual({
      version: 1,
      columns: [
        { path: 'sku', visible: true },
        { path: 'packaging>name', visible: true },
      ],
    })
  })
})

describe('FieldPicker', () => {
  it('toggles with Space and Enter and reorders with Alt+Arrow', () => {
    const onToggle = vi.fn()
    const onMove = vi.fn()
    const onClose = vi.fn()
    render(
      <FieldPicker
        paths={paths}
        order={['sku', 'name']}
        hidden={['name']}
        onToggle={onToggle}
        onMove={onMove}
        onClose={onClose}
      />,
    )
    const input = screen.getByTestId('field-picker-search')
    expect(screen.getByTestId('field-picker-name').getAttribute('aria-selected')).toBe('false')

    fireEvent.keyDown(input, { key: 'ArrowDown' })
    fireEvent.keyDown(input, { key: ' ' })
    expect(onToggle).toHaveBeenCalledWith('name')

    fireEvent.keyDown(input, { key: 'Enter' })
    expect(onToggle).toHaveBeenCalledTimes(2)

    fireEvent.keyDown(input, { key: 'ArrowUp', altKey: true })
    expect(onMove).toHaveBeenCalledWith('name', -1)

    fireEvent.keyDown(input, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('filters by label', () => {
    render(
      <FieldPicker paths={paths} order={[]} hidden={[]} onToggle={() => undefined} onMove={() => undefined} onClose={() => undefined} />,
    )
    fireEvent.change(screen.getByTestId('field-picker-search'), { target: { value: 'pack' } })
    expect(screen.getByTestId('field-picker-packaging>name')).toBeTruthy()
    expect(screen.queryByTestId('field-picker-sku')).toBeNull()
  })
})

describe('ObjectTable column prefs', () => {
  it('applies the stored order and hidden fields and skips stale paths', async () => {
    render(<PrefsTable />)
    await waitFor(() =>
      expect(headerIds()).toEqual(['col-sku', 'col-type', 'col-version', 'col-packaging>name']),
    )
    expect(screen.queryByTestId('col-weight')).toBeNull()
    expect(screen.queryByTestId('col-name')).toBeNull()
  })

  it('PUTs the document after hiding a field', async () => {
    render(<PrefsTable />)
    await waitFor(() =>
      expect(headerIds()).toEqual(['col-sku', 'col-type', 'col-version', 'col-packaging>name']),
    )

    act(() => {
      dispatchUi('ui.hide_column', { column: 'sku' })
    })
    await waitFor(() => expect(headerIds()).toEqual(['col-type', 'col-version', 'col-packaging>name']))

    await waitFor(() => {
      const put = vi.mocked(fetch).mock.calls.find(([input, init]) =>
        String(input).includes('/column-prefs') && (init?.method ?? 'GET').toUpperCase() === 'PUT',
      )
      expect(put).toBeTruthy()
      const body = JSON.parse(String(put?.[1]?.body))
      expect(body.environment).toBe('default')
      expect(body.document).toEqual({
        version: 1,
        columns: [
          { path: 'sku', visible: false },
          { path: 'name', visible: false },
          { path: 'type', visible: true },
          { path: 'version', visible: true },
          { path: 'packaging>name', visible: true },
        ],
      })
    }, { timeout: 2000 })
  })

  it('keeps type and version after save and reload', async () => {
    let stored: ColumnPrefsResponse = {
      document: {
        version: 1,
        columns: [
          { path: 'sku', visible: true },
          { path: 'type', visible: true },
          { path: 'version', visible: true },
        ],
      },
    }
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input)
        if (url.includes('/column-prefs')) {
          if ((init?.method ?? 'GET').toUpperCase() === 'PUT') {
            stored = { document: JSON.parse(String(init?.body)).document }
            return json(stored)
          }
          return json(stored)
        }
        if (url.includes('/attribute-paths')) return json({ paths, system_columns: systemColumns })
        if (url.includes('/v1/search')) return json({ found: 0, hits: [] })
        if (url.includes('/v1/definitions')) {
          return json({
            languages: ['en'],
            types: [{ id: product, key: 'product', labels: { en: 'Product' }, singular: { en: 'Product' }, plural: { en: 'Products' }, key_attribute: 'sku', attributes: [] }],
            attributes: [],
            structures: [],
          })
        }
        if (url.includes('/v1/assortments')) return json({ assortments: [] })
        if (url.includes('/v1/proposals')) return json({ pending: { total: 0 } })
        if (url.includes('/v1/views')) return json({})
        return json({})
      }),
    )
    const { unmount } = render(<PrefsTable />)
    await waitFor(() => expect(headerIds()).toEqual(['col-sku', 'col-type', 'col-version']))
    act(() => {
      dispatchUi('ui.hide_column', { column: 'sku' })
    })
    await waitFor(() => {
      expect(stored.document?.columns.some((column) => column.path === 'sku' && column.visible === false)).toBe(true)
      expect(stored.document?.columns.some((column) => column.path === 'type' && column.visible === true)).toBe(true)
      expect(stored.document?.columns.some((column) => column.path === 'version' && column.visible === true)).toBe(true)
    }, { timeout: 2000 })
    unmount()
    render(<PrefsTable />)
    await waitFor(() => expect(headerIds()).toEqual(['col-type', 'col-version']))
  })


  it('flushes a pending PUT when unmounted before the debounce', async () => {
    const { unmount } = render(<PrefsTable />)
    await waitFor(() =>
      expect(headerIds()).toEqual(['col-sku', 'col-type', 'col-version', 'col-packaging>name']),
    )
    const putsBefore = vi.mocked(fetch).mock.calls.filter(
      ([input, init]) => String(input).includes('/column-prefs') && (init?.method ?? 'GET').toUpperCase() === 'PUT',
    ).length
    act(() => {
      dispatchUi('ui.hide_column', { column: 'sku' })
    })
    unmount()
    await waitFor(() => {
      const puts = vi.mocked(fetch).mock.calls.filter(
        ([input, init]) => String(input).includes('/column-prefs') && (init?.method ?? 'GET').toUpperCase() === 'PUT',
      )
      expect(puts.length).toBeGreaterThan(putsBefore)
      const body = JSON.parse(String(puts.at(-1)?.[1]?.body))
      expect(body.document).toEqual({
        version: 1,
        columns: [
          { path: 'sku', visible: false },
          { path: 'name', visible: false },
          { path: 'type', visible: true },
          { path: 'version', visible: true },
          { path: 'packaging>name', visible: true },
        ],
      })
      expect(puts.at(-1)?.[1]?.keepalive).not.toBe(true)
    })
  })

  it('flushes with keepalive when the tab becomes hidden', async () => {
    render(<PrefsTable />)
    await waitFor(() =>
      expect(headerIds()).toEqual(['col-sku', 'col-type', 'col-version', 'col-packaging>name']),
    )
    act(() => {
      dispatchUi('ui.hide_column', { column: 'sku' })
    })
    const previous = Object.getOwnPropertyDescriptor(document, 'visibilityState')
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      get: () => 'hidden',
    })
    try {
      act(() => {
        document.dispatchEvent(new Event('visibilitychange'))
      })
      await waitFor(() => {
        const put = vi.mocked(fetch).mock.calls.find(
          ([input, init]) =>
            String(input).includes('/column-prefs') &&
            (init?.method ?? 'GET').toUpperCase() === 'PUT' &&
            init?.keepalive === true,
        )
        expect(put).toBeTruthy()
        const body = JSON.parse(String(put?.[1]?.body))
        expect(body.document.columns.some((column: { path: string; visible: boolean }) => column.path === 'sku' && column.visible === false)).toBe(true)
        expect(body.document.columns.some((column: { path: string }) => column.path === 'type')).toBe(true)
        expect(body.document.columns.some((column: { path: string }) => column.path === 'version')).toBe(true)
      })
    } finally {
      if (previous) Object.defineProperty(document, 'visibilityState', previous)
      else Reflect.deleteProperty(document, 'visibilityState')
    }
  })

  it('shows system columns from an old document that omitted them', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input)
        if (url.includes('/column-prefs')) {
          return json({
            document: {
              version: 1,
              columns: [
                { path: 'number', visible: true },
                { path: 'name', visible: true },
                { path: 'status', visible: true },
              ],
            },
          })
        }
        if (url.includes('/attribute-paths')) {
          return json({
            paths: [
              { text: 'number', label: 'Number' },
              { text: 'name', label: 'Name' },
              { text: 'status', label: 'Status' },
            ],
            system_columns: systemColumns,
          })
        }
        if (url.includes('/v1/search')) return json({ found: 0, hits: [] })
        if (url.includes('/v1/definitions')) {
          return json({
            languages: ['en'],
            types: [{ id: product, key: 'product', labels: { en: 'Product' }, singular: { en: 'Product' }, plural: { en: 'Products' }, key_attribute: 'number', attributes: [] }],
            attributes: [],
            structures: [],
          })
        }
        if (url.includes('/v1/assortments')) return json({ assortments: [] })
        if (url.includes('/v1/proposals')) return json({ pending: { total: 0 } })
        if (url.includes('/v1/views')) return json({})
        return json({})
      }),
    )
    render(<PrefsTable />)
    await waitFor(() =>
      expect(headerIds()).toEqual(['col-number', 'col-name', 'col-type', 'col-status', 'col-version']),
    )
  })
})
