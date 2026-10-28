// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useState } from 'react'
import { registerModelCommandRunners } from './model-commands'
import { runCommand } from './command-registry'
import { SharedContextMenuHost } from './shared-context-menu'
import { ObjectTypesTable } from './model-tables'

const stageModel = vi.hoisted(() => vi.fn(() => Promise.resolve({})))
const openModelDialog = vi.hoisted(() => vi.fn())
const api = vi.hoisted(() => vi.fn())

vi.mock('./model-stage', async () => {
  const actual = await vi.importActual<typeof import('./model-stage')>('./model-stage')
  return { ...actual, stageModel, openModelDialog }
})

vi.mock('./api', async () => {
  const actual = await vi.importActual<typeof import('./api')>('./api')
  return { ...actual, api }
})

const definitions = {
  types: [
    { id: 't1', key: 'article', kind: 'object', singular: { en: 'Article' }, plural: { en: 'Articles' }, labels: { en: 'Articles' } },
    { id: 't2', key: 'product', kind: 'object', singular: { en: 'Product' }, plural: { en: 'Products' }, labels: { en: 'Products' } },
  ],
  attributes: [],
  structures: [],
  value_lists: [],
  unit_families: [],
}

describe('model command runners', () => {
  let stop: () => void

  beforeEach(() => {
    stageModel.mockReset()
    openModelDialog.mockReset()
    stop = registerModelCommandRunners()
  })

  afterEach(() => {
    stop()
    cleanup()
  })

  it('runs model:new-type through the registry', () => {
    runCommand('model:new-type')
    expect(openModelDialog).toHaveBeenCalledWith('type')
  })

  it('stages model:delete-type as a proposal, never a direct delete', async () => {
    runCommand('model:delete-type', { typeKey: 'article' })
    await waitFor(() => expect(stageModel).toHaveBeenCalled())
    expect(stageModel).toHaveBeenCalledWith('model.delete_type', { type: 'article' })
    const ops = stageModel.mock.calls.map((call) => String((call as unknown as unknown[])[0] ?? ''))
    expect(ops.every((op) => op.startsWith('model.'))).toBe(true)
  })
})

function Host() {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  return (
    <QueryClientProvider client={client}>
      <SharedContextMenuHost />
      <ObjectTypesTable />
    </QueryClientProvider>
  )
}

describe('model table context menu', () => {
  beforeEach(() => {
    api.mockReset()
    api.mockImplementation((path: string) => {
      if (path === '/v1/definitions') return Promise.resolve(definitions)
      if (path === '/v1/model/draft') return Promise.resolve({ count: 0, conflict: false, steps: [] })
      return Promise.resolve({})
    })
    registerModelCommandRunners()
  })

  afterEach(() => {
    cleanup()
  })

  it('opens the shared menu on right-click with type commands', async () => {
    render(<Host />)
    const row = await screen.findByText('Article')
    fireEvent.contextMenu(row.closest('[role="row"]') as HTMLElement)
    const menu = await screen.findByTestId('context-menu')
    expect(menu.textContent).toMatch(/New type|Delete/i)
  })

  it('opens the shared menu with Shift+F10 on a focused row', async () => {
    render(<Host />)
    const row = (await screen.findByText('Product')).closest('[role="row"]') as HTMLElement
    row.focus()
    fireEvent.keyDown(row, { key: 'F10', shiftKey: true })
    const menu = await screen.findByTestId('context-menu')
    expect(menu.textContent).toMatch(/New type|Delete/i)
  })
})
