// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { commandsForContext, registerCommandRunner } from './command-registry'
import { SharedContextMenuHost, openSharedContextMenu } from './shared-context-menu'

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('shared context menu registry', () => {
  it('lists Search and feedback on the empty fallback surface', () => {
    const items = commandsForContext('empty', { feedbackEnabled: true })
    expect(items.some((item) => item.id === 'palette:search')).toBe(true)
    expect(items.some((item) => item.id === 'feedback:send')).toBe(true)
  })

  it('opens the empty fallback menu and runs Search… into the palette runner', async () => {
    const search = vi.fn()
    const stop = registerCommandRunner('palette:search', search)
    render(<SharedContextMenuHost feedbackEnabled />)
    openSharedContextMenu({ context: 'empty', x: 40, y: 40 })
    const menu = await screen.findByTestId('context-menu')
    expect(menu).toBeTruthy()
    fireEvent.click(screen.getByRole('menuitem', { name: /search/i }))
    await waitFor(() => expect(search).toHaveBeenCalled())
    stop()
  })

  it('moves focus with arrow keys like the table menus', async () => {
    registerCommandRunner('palette:search', () => undefined)
    registerCommandRunner('feedback:send', () => undefined)
    render(<SharedContextMenuHost feedbackEnabled />)
    openSharedContextMenu({ context: 'empty', x: 12, y: 12 })
    const menu = await screen.findByTestId('context-menu')
    const items = screen.getAllByRole('menuitem')
    expect(items.length).toBeGreaterThan(1)
    items[0].focus()
    fireEvent.keyDown(menu, { key: 'ArrowDown' })
    expect(document.activeElement).toBe(items[1])
    fireEvent.keyDown(menu, { key: 'ArrowUp' })
    expect(document.activeElement).toBe(items[0])
  })

  it('uses context-menu test id for table row menus', async () => {
    registerCommandRunner('object:open', () => undefined)
    render(<SharedContextMenuHost />)
    openSharedContextMenu({
      context: 'row',
      x: 20,
      y: 20,
      target: { objectId: 'o1' },
    })
    expect(await screen.findByTestId('context-menu')).toBeTruthy()
  })
})
