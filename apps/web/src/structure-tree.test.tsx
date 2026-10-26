// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Json } from './api'
import { StructureSection } from './structure-tree'

const api = vi.hoisted(() => vi.fn())

vi.mock('./api', async () => {
  const actual = await vi.importActual<typeof import('./api')>('./api')
  return { ...actual, api }
})

const defs: Json = {
  languages: ['en'],
  structures: [
    {
      key: 'sortiment',
      labels: { en: 'eCom' },
      groups: [
        { id: 'outdoor', key: 'friluft', labels: { en: 'Outdoor' } },
        { id: 'tents', key: 'talt', parent: 'outdoor', labels: { en: 'Tents' } },
        { id: 'bags', key: 'sovsack', parent: 'outdoor', labels: { en: 'Sleeping bags' } },
        { id: 'building', key: 'bygg', labels: { en: 'Building' } },
        { id: 'timber', key: 'virke', parent: 'building', labels: { en: 'Timber' } },
      ],
    },
    {
      key: 'environment',
      labels: { en: 'Environment' },
      groups: [
        { id: 'climate', key: 'climate', labels: { en: 'Climate' } },
      ],
    },
  ],
}

function Tree() {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  return (
    <QueryClientProvider client={client}>
      <StructureSection defs={defs} selection={null} onPick={() => undefined} />
    </QueryClientProvider>
  )
}

function installDom() {
  Element.prototype.scrollIntoView = () => undefined
  if (!Element.prototype.hasPointerCapture) {
    Element.prototype.hasPointerCapture = () => false
    Element.prototype.setPointerCapture = () => undefined
    Element.prototype.releasePointerCapture = () => undefined
  }
}

async function chooseMenu(testId: string, name: string) {
  fireEvent.pointerDown(screen.getByTestId(testId), { button: 0, ctrlKey: false, pointerType: 'mouse' })
  const item = await screen.findByRole('menuitem', { name })
  fireEvent.click(item)
}

describe('model structure tree', () => {
  beforeEach(() => {
    installDom()
    api.mockReset()
    api.mockResolvedValue({})
  })

  afterEach(() => {
    cleanup()
  })

  it('starts collapsed and expands only the nodes the user opens', () => {
    render(<Tree />)
    expect(screen.getByTestId('group-friluft')).toBeTruthy()
    expect(screen.getByTestId('group-bygg')).toBeTruthy()
    expect(screen.getByTestId('group-climate')).toBeTruthy()
    expect(screen.queryByTestId('group-talt')).toBeNull()
    expect(screen.queryByTestId('group-sovsack')).toBeNull()
    expect(screen.queryByTestId('group-virke')).toBeNull()
    expect(screen.getByRole('button', { name: 'Outdoor' }).getAttribute('aria-expanded')).toBe('false')
    expect(screen.getByRole('button', { name: 'Climate' }).getAttribute('aria-expanded')).toBeNull()

    fireEvent.click(within(screen.getByTestId('group-friluft')).getByRole('button', { name: 'Outdoor' }).querySelector('.twist')!)
    expect(screen.getByTestId('group-talt')).toBeTruthy()
    expect(screen.getByTestId('group-sovsack')).toBeTruthy()
    expect(screen.queryByTestId('group-virke')).toBeNull()
    expect(screen.getByRole('button', { name: 'Outdoor' }).getAttribute('aria-expanded')).toBe('true')

    fireEvent.click(within(screen.getByTestId('group-friluft')).getByRole('button', { name: 'Outdoor' }).querySelector('.twist')!)
    expect(screen.queryByTestId('group-talt')).toBeNull()
    expect(screen.getByRole('button', { name: 'Outdoor' }).getAttribute('aria-expanded')).toBe('false')
  })

  it('places add child below the clicked node, expands it, and saves from the keyboard', async () => {
    render(<Tree />)
    await chooseMenu('group-menu-sortiment-friluft', 'Add child')

    const outdoor = screen.getByTestId('group-friluft')
    expect(within(outdoor).getByRole('button', { name: 'Outdoor' })).toBeTruthy()
    const draft = within(outdoor).getByTestId('group-draft')
    expect(draft.style.paddingLeft).toBe('14px')
    const button = within(outdoor).getByRole('button', { name: 'Outdoor' })
    expect(button.compareDocumentPosition(draft) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(screen.getByTestId('group-talt')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Outdoor' }).getAttribute('aria-expanded')).toBe('true')

    const input = within(draft).getByRole('textbox', { name: 'Add child' })
    await waitFor(() => expect(document.activeElement).toBe(input))

    fireEvent.keyDown(input, { key: 'Escape' })
    expect(screen.queryByTestId('group-draft')).toBeNull()
    expect(within(outdoor).getByRole('button', { name: 'Outdoor' })).toBeTruthy()
    expect(api).not.toHaveBeenCalled()

    await chooseMenu('group-menu-sortiment-talt', 'Add child')
    const tents = screen.getByTestId('group-talt')
    expect(within(tents).getByRole('button', { name: 'Tents' })).toBeTruthy()
    const child = within(tents).getByTestId('group-draft')
    expect(child.style.paddingLeft).toBe('28px')
    const tentsButton = within(tents).getByRole('button', { name: 'Tents' })
    expect(tentsButton.compareDocumentPosition(child) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    const childInput = within(child).getByRole('textbox', { name: 'Add child' })
    await waitFor(() => expect(document.activeElement).toBe(childInput))
    fireEvent.change(childInput, { target: { value: 'Canopy' } })
    fireEvent.keyDown(childInput, { key: 'Enter' })
    await waitFor(() => expect(api).toHaveBeenCalledWith('/v1/structures/sortiment/groups', {
      method: 'POST',
      body: JSON.stringify({ key: 'canopy', label: 'Canopy', parent: 'talt' }),
    }))
  })

  it('places a new root group under the structure title', async () => {
    render(<Tree />)
    const structure = screen.getByTestId('structure-sortiment')
    expect(structure.querySelector('strong')?.textContent).toBe('eCom')
    await chooseMenu('structure-menu-sortiment', 'Add child')
    expect(structure.querySelector('strong')?.textContent).toBe('eCom')
    const draft = within(structure).getByTestId('group-draft')
    expect(draft.style.paddingLeft).toBe('0px')
    const title = structure.querySelector('strong')
    if (!title) throw new Error('missing structure title')
    expect(title.compareDocumentPosition(draft) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    const input = within(draft).getByRole('textbox', { name: 'Add child' })
    await waitFor(() => expect(document.activeElement).toBe(input))
    fireEvent.change(input, { target: { value: 'Campaigns' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => expect(api).toHaveBeenCalledWith('/v1/structures/sortiment/groups', {
      method: 'POST',
      body: JSON.stringify({ key: 'campaigns', label: 'Campaigns' }),
    }))
  })
})
