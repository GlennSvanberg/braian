// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useMemo, useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { copy } from './i18n'
import { CommandPalette } from './palette'
import { addViewRootItem, addViewSubmenuRows, parseAddViewChoice } from './palette-add-view'
import { visibleRows } from './palette-model'
import { onUi } from './ui-bus'

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

beforeEach(() => {
  Element.prototype.scrollIntoView = () => undefined
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
        return new Response(JSON.stringify({ types: [], attributes: [] }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        })
      }
      if (url.includes('/v1/search')) {
        return new Response(JSON.stringify({ found: 0, hits: [] }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        })
      }
      if (url.includes('/v1/frecency')) {
        return new Response(JSON.stringify({ entries: {} }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        })
      }
      if (url.includes('/v1/proposals')) {
        return new Response(JSON.stringify({ pending: { total: 0 } }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        })
      }
      return new Response(JSON.stringify({}), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      })
    }),
  )
})

describe('add view palette helpers', () => {
  it('shows Add view › when typing add view', () => {
    const root = addViewRootItem()
    const rows = visibleRows('add view', [root], [], {})
    expect(rows.some((row) => (row.sourceId ?? row.id) === 'layout:add-view')).toBe(true)
    expect(rows.some((row) => row.title === `${copy.addView} ›`)).toBe(true)
  })

  it('lists workspace views in the submenu', () => {
    const rows = addViewSubmenuRows(
      [
        { id: 'v-detail', title: 'Detail', type: 'object_detail' },
        { id: 'v-history', title: 'History', type: 'history' },
      ],
      [],
    )
    expect(rows.map((row) => row.id)).toEqual(['add-view:view:v-detail', 'add-view:view:v-history'])
    expect(rows.map((row) => row.title)).toEqual(['Detail', 'History'])
  })

  it('parses submenu choices', () => {
    expect(parseAddViewChoice('add-view:view:abc')).toEqual({ kind: 'view', value: 'abc' })
    expect(parseAddViewChoice('add-view:type:t1')).toEqual({ kind: 'type', value: 't1' })
    expect(parseAddViewChoice('layout:add-view')).toBeNull()
  })
})

function PaletteHost({
  focusViewId = 'v-table',
  onPrepareAddView,
}: {
  focusViewId?: string
  onPrepareAddView?: () => void
}) {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  const session = useMemo(
    () => ({
      objectId: null as string | null,
      setObjectId: () => undefined,
      setProposalId: () => undefined,
      push: () => undefined,
      openWorkspace: () => undefined,
      openTypeView: () => undefined,
    }),
    [],
  )
  return (
    <QueryClientProvider client={client}>
      <CommandPalette
        session={session}
        workspaces={[]}
        views={[
          { id: 'v-table', title: 'Products', type: 'object_table' },
          { id: 'v-detail', title: 'Detail', type: 'object_detail' },
          { id: 'v-graph', title: 'Graph', type: 'graph' },
        ]}
        focusViewId={focusViewId}
        onPrepareAddView={onPrepareAddView}
        onClose={() => undefined}
      />
    </QueryClientProvider>
  )
}

describe('Add view › in Ctrl+K', () => {
  it('opens the submenu and adds the chosen view to the focused pane', async () => {
    const prepared: string[] = []
    const placed: Array<Record<string, unknown>> = []
    const stop = onUi((event) => {
      if (event.name === 'ui.open_view') placed.push(event.args)
    })
    render(
      <PaletteHost
        focusViewId="v-table"
        onPrepareAddView={() => prepared.push('ready')}
      />,
    )
    const input = await screen.findByTestId('palette-input')
    fireEvent.change(input, { target: { value: 'add view' } })
    await waitFor(() => {
      expect(screen.getByRole('option', { name: new RegExp(`${copy.addView}`) })).toBeTruthy()
    })
    // Activate the Add view › row, then Enter to open the submenu.
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => {
      expect(screen.getByRole('option', { name: /Detail/ })).toBeTruthy()
      expect(screen.getByRole('option', { name: /Graph/ })).toBeTruthy()
    })
    expect(prepared.length).toBeGreaterThan(0)
    // First submenu row is Products (v-table); move to Detail and confirm.
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => expect(placed.length).toBe(1))
    expect(placed[0]).toMatchObject({ id: 'v-detail', zone: 'center', target: 'v-table' })
    stop()
  })
})
