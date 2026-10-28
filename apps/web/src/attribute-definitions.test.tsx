// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AttributeDefinitions } from './attribute-definitions'
import { SharedContextMenuHost } from './shared-context-menu'

const api = vi.hoisted(() => vi.fn())

vi.mock('./api', async () => {
  const actual = await vi.importActual<typeof import('./api')>('./api')
  return { ...actual, api }
})

const NUMBER = '018f0000-0000-7000-8000-000000000100'

const definitions = {
  attributes: [],
  types: [
    { id: 't-article', key: 'article', singular: { en: 'Article' }, attributes: ['a-weight', NUMBER] },
    { id: 't-supplier', key: 'supplier', singular: { en: 'Supplier' }, attributes: [] },
  ],
  structures: [
    {
      id: 's-food',
      key: 'food',
      labels: { en: 'Food' },
      groups: [{ id: 'g-cheese', key: 'cheese', labels: { en: 'Cheese' }, attributes: ['a-weight'] }],
      classes: [],
    },
  ],
  unit_families: [],
  value_lists: [],
}

const catalog = {
  attributes: [
    {
      id: '018f0000-0000-7000-8000-00000000a003',
      key: 'created_at',
      label: 'Created',
      kind: 'text',
      standard: true,
      flags: { standard: true },
      used_in: { objects: 3, types: [{ id: 't-article', key: 'article', label: 'Article', objects: 3 }], groups: [] },
    },
    {
      id: NUMBER,
      key: 'number',
      label: 'Number',
      kind: 'text',
      standard: true,
      flags: { required: true, standard: true },
      used_in: { objects: 3, types: [{ id: 't-article', key: 'article', label: 'Article', objects: 3 }], groups: [] },
    },
    {
      id: 'a-weight',
      key: 'gross_weight',
      label: 'Gross weight',
      kind: 'number',
      unit_family: { key: 'mass', label: 'mass' },
      standard: false,
      flags: { sortable: true },
      used_in: {
        objects: 1200,
        types: [{ id: 't-article', key: 'article', label: 'Article', objects: 1200 }],
        groups: [{ id: 'g-cheese', key: 'cheese', label: 'Cheese', structure_key: 'food', structure_label: 'Food', objects: 4 }],
      },
    },
  ],
}

function Host() {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  return (
    <QueryClientProvider client={client}>
      <SharedContextMenuHost />
      <AttributeDefinitions />
    </QueryClientProvider>
  )
}

describe('attribute definitions view', () => {
  beforeEach(() => {
    api.mockReset()
    api.mockImplementation((path: string) => {
      if (path === '/v1/definitions') return Promise.resolve(definitions)
      if (path === '/v1/model/draft') return Promise.resolve({ count: 0, conflict: false, steps: [] })
      if (String(path).includes('model.list_attribute_definitions')) return Promise.resolve(catalog)
      if (String(path).includes('/v1/model/draft/steps')) return Promise.resolve({ count: 1, steps: [] })
      return Promise.resolve({})
    })
  })

  afterEach(() => {
    cleanup()
  })

  it('lists attributes, formats where they are used, and keeps standard attributes locked', async () => {
    render(<Host />)
    const rows = await screen.findAllByTestId('attribute-definition-row')
    expect(rows.map((row) => row.getAttribute('data-key'))).toEqual(['created_at', 'gross_weight', 'number'])
    expect(screen.getByText('Date and time')).toBeTruthy()
    const weight = rows.find((row) => row.getAttribute('data-key') === 'gross_weight')
    expect(weight?.querySelector('[data-testid="used-in"]')?.textContent).toBe('2 places')
    expect(weight?.querySelector('[data-testid="used-in"]')?.getAttribute('title')).toBe('Article (1,200), Food / Cheese (4)')
    expect(weight?.querySelector('[data-testid="used-in"]')?.getAttribute('data-objects')).toBe('1200')
    expect((screen.getByTestId('attr-key-number') as HTMLInputElement).disabled).toBe(true)
    expect((screen.getByTestId('attr-key-gross_weight') as HTMLInputElement).disabled).toBe(false)

    fireEvent.contextMenu(screen.getByTestId('attr-label-number'))
    const menu = await screen.findByTestId('context-menu')
    expect(menu.textContent).toContain('Rename…')
    expect(menu.textContent).not.toContain('Rename key…')
    expect(menu.textContent).not.toContain('Delete…')
    fireEvent.keyDown(menu, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByTestId('context-menu')).toBeNull())
  })

  it('puts add to type in the draft from the keyboard', async () => {
    render(<Host />)
    const search = await screen.findByTestId('attribute-search')
    fireEvent.change(search, { target: { value: 'weight' } })
    fireEvent.keyDown(search, { key: 'ArrowDown' })
    const row = screen.getByTestId('attribute-definition-row')
    expect(row.getAttribute('data-key')).toBe('gross_weight')
    row.focus()
    fireEvent.keyDown(row, { key: 'Enter', ctrlKey: true })
    const menu = await screen.findByTestId('context-menu')
    fireEvent.click(menu.querySelector('button') as HTMLButtonElement)
    const picker = await screen.findByTestId('add-to-type-search')
    fireEvent.change(picker, { target: { value: 'Supplier' } })
    fireEvent.keyDown(picker, { key: 'Enter' })

    await waitFor(() => {
      const posted = api.mock.calls.find((call) => String(call[0]).includes('/v1/model/draft/steps'))
      expect(posted).toBeTruthy()
      const body = JSON.parse(String((posted?.[1] as RequestInit).body))
      expect(body).toEqual({
        operation: 'model.attach_attribute',
        args: { attribute: 'gross_weight', type: 'supplier' },
      })
    })
  })
})
