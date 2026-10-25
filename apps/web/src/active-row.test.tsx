// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useMemo, useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CommandPalette } from './palette'
import { ObjectTable, type PanesSession } from './panes'
import { StructureTree } from './shell'
import { readUiContext, resetUiContext } from './ui-context'

const { scrollToIndex } = vi.hoisted(() => ({ scrollToIndex: vi.fn() }))

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
    scrollToIndex,
  }),
}))

const tableRows = ['Zero', 'One', 'Two', 'Three', 'Four', 'Five']

function json(body: unknown) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  })
}

function client() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } })
}

function TableHarness() {
  const [queryClient] = useState(client)
  const [objectId, setObjectId] = useState<string | null>(null)
  const session = useMemo<PanesSession>(
    () => ({
      group: null,
      objectId,
      proposalId: null,
      language: 'en',
      setGroup: () => undefined,
      setObjectId,
      setProposalId: () => undefined,
      push: () => undefined,
    }),
    [objectId],
  )
  return (
    <QueryClientProvider client={queryClient}>
      <ObjectTable
        session={session}
        view={{ id: 'table', type: 'object_table', title: 'Objects', state: {} }}
        workspace={{ id: 'ws' }}
      />
      <div data-testid="detail">{objectId ?? ''}</div>
    </QueryClientProvider>
  )
}

function TreeHarness() {
  const [queryClient] = useState(client)
  const [group, setGroup] = useState<string | null>(null)
  const session = useMemo(
    () => ({
      group,
      objectId: null,
      proposalId: null,
      language: 'en',
      activity: [],
      setGroup,
      setObjectId: () => undefined,
      setProposalId: () => undefined,
      listType: null,
      setListType: () => undefined,
      push: () => undefined,
      openWorkspace: () => undefined,
      openTypeView: () => undefined,
    }),
    [group],
  )
  return (
    <QueryClientProvider client={queryClient}>
      <StructureTree session={session} view={{ id: 'tree', type: 'structure_tree', title: 'Assortment', state: {} }} />
      <div data-testid="detail">{group ?? ''}</div>
    </QueryClientProvider>
  )
}

function PaletteHarness() {
  const [queryClient] = useState(client)
  const [objectId, setObjectId] = useState<string | null>('obj-9')
  const session = useMemo(
    () => ({
      objectId,
      setObjectId,
      setProposalId: () => undefined,
      push: () => undefined,
      openWorkspace: () => undefined,
    }),
    [objectId],
  )
  return (
    <QueryClientProvider client={queryClient}>
      <CommandPalette session={session} workspaces={[]} views={[]} onClose={() => undefined} />
      <div data-testid="detail">{objectId ?? ''}</div>
    </QueryClientProvider>
  )
}

beforeEach(() => {
  resetUiContext()
  scrollToIndex.mockClear()
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  Element.prototype.scrollIntoView = () => undefined
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.includes('/v1/search')) {
        if (url.includes('group=groups')) return json({ groups: [] })
        return json({
          found: tableRows.length,
          hits: tableRows.map((name, index) => ({
            document: {
              id: `id-${index}`,
              key: `ART-${index}`,
              type_key: 'artikel',
              version: '1',
              a_namn__sv: name,
              a_status: 'ok',
            },
          })),
        })
      }
      if (url.includes('/v1/definitions')) {
        return json({
          types: [],
          attributes: [],
          unit_families: [],
          structures: [
            {
              key: 'assortment',
              groups: [
                { id: '1', key: 'alpha' },
                { id: '2', key: 'beta', parent: '1' },
                { id: '3', key: 'gamma' },
                { id: '4', key: 'delta' },
              ],
            },
          ],
        })
      }
      if (url.includes('/v1/assortments')) return json({ assortments: [] })
      if (url.includes('/v1/frecency')) return json({ entries: {} })
      return json({})
    }),
  )
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

function grid() {
  const node = document.querySelector('.grid')
  if (!(node instanceof HTMLElement)) throw new Error('missing grid')
  return node
}

describe('active row', () => {
  it('continues arrows from the clicked table row and ignores hover', async () => {
    render(<TableHarness />)
    await waitFor(() => expect(screen.getAllByTestId('object-row')).toHaveLength(tableRows.length))
    const rows = () => screen.getAllByTestId('object-row')

    fireEvent.keyDown(grid(), { key: 'ArrowDown' })
    fireEvent.keyDown(grid(), { key: 'ArrowDown' })
    await waitFor(() => expect(screen.getByTestId('detail').textContent).toBe('id-1'))
    expect(rows()[1]?.getAttribute('aria-selected')).toBe('true')

    fireEvent.click(rows()[3]!)
    await waitFor(() => expect(screen.getByTestId('detail').textContent).toBe('id-3'))
    expect(grid().getAttribute('aria-activedescendant')).toBe('object-row-id-3')

    fireEvent.keyDown(grid(), { key: 'ArrowDown' })
    await waitFor(() => expect(screen.getByTestId('detail').textContent).toBe('id-4'))
    expect(rows()[4]?.getAttribute('aria-selected')).toBe('true')
    expect(rows()[1]?.getAttribute('aria-selected')).toBe('false')
    expect(rows()[2]?.getAttribute('aria-selected')).toBe('false')

    fireEvent.mouseEnter(rows()[0]!)
    fireEvent.mouseOver(rows()[0]!)
    fireEvent.pointerMove(rows()[0]!)
    expect(screen.getByTestId('detail').textContent).toBe('id-4')
    expect(rows()[0]?.getAttribute('aria-selected')).toBe('false')
    expect(scrollToIndex).toHaveBeenLastCalledWith(4)
    expect(readUiContext().selection).toEqual([{ id: 'id-4', key: 'ART-4', name: 'Four', type: 'artikel' }])
  })

  it('moves the tree selection with the same active row arrows start from', async () => {
    render(<TreeHarness />)
    await waitFor(() => expect(screen.getAllByRole('treeitem').length).toBeGreaterThan(0))
    const tree = screen.getByTestId('structure-tree')
    const item = (name: string) => screen.getByRole('treeitem', { name })

    fireEvent.keyDown(tree, { key: 'ArrowDown' })
    expect(screen.getByTestId('detail').textContent).toBe('assortment:alpha')
    fireEvent.keyDown(tree, { key: 'ArrowDown' })
    expect(screen.getByTestId('detail').textContent).toBe('assortment:gamma')

    fireEvent.click(item('alpha'))
    expect(screen.getByTestId('detail').textContent).toBe('assortment:alpha')
    expect(item('alpha').getAttribute('aria-selected')).toBe('true')

    fireEvent.keyDown(tree, { key: 'ArrowDown' })
    expect(screen.getByTestId('detail').textContent).toBe('assortment:gamma')
    expect(item('delta').getAttribute('aria-selected')).toBe('false')

    fireEvent.mouseEnter(item('delta'))
    fireEvent.pointerMove(item('delta'))
    expect(screen.getByTestId('detail').textContent).toBe('assortment:gamma')
    expect(tree.getAttribute('aria-activedescendant')).toBe('tree-row-assortment-gamma')
  })

  it('does not move the palette row when the pointer hovers', async () => {
    render(<PaletteHarness />)
    const input = await screen.findByTestId('palette-input')
    await waitFor(() => expect(screen.getAllByRole('option').length).toBeGreaterThan(3))

    fireEvent.keyDown(input, { key: 'ArrowDown' })
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    expect(screen.getByTestId('palette-active').textContent).toBe('Split beside the table')

    fireEvent.mouseEnter(screen.getAllByRole('option')[0]!)
    fireEvent.pointerMove(screen.getAllByRole('option')[0]!)
    expect(screen.getByTestId('palette-active').textContent).toBe('Split beside the table')

    fireEvent.pointerDown(screen.getByRole('option', { name: /Graph/ }))
    expect(screen.getByTestId('palette-active').textContent).toBe('Relations and where values come from')
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    expect(screen.getByTestId('palette-active').textContent).toBe('obj-9')
    expect(screen.getByTestId('detail').textContent).toBe('obj-9')
  })
})
