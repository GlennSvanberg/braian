// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AppContextMenuHost, handleAppContextMenu } from './app-context-menu'

afterEach(cleanup)

function renderHost(feedbackEnabled = false) {
  const onSearch = vi.fn()
  const onFeedback = vi.fn()
  render(
    <div>
      <AppContextMenuHost feedbackEnabled={feedbackEnabled} onSearch={onSearch} onFeedback={onFeedback} />
      <p data-testid="surface">Workspace background</p>
      <div data-testid="handled" onContextMenu={(event) => event.preventDefault()} />
      <div data-testid="local" onContextMenu={handleAppContextMenu} />
      <input data-testid="text" />
    </div>,
  )
  return { onSearch, onFeedback }
}

function contextMenu(target: Element, x = 24, y = 32) {
  const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y })
  act(() => {
    target.dispatchEvent(event)
  })
  return event
}

describe('app context menu fallback', () => {
  it('opens on a non-input surface and prevents the browser menu', () => {
    renderHost()
    const event = contextMenu(screen.getByTestId('surface'))
    expect(event.defaultPrevented).toBe(true)
    expect(screen.getByTestId('app-context-menu')).toBeTruthy()
  })

  it('leaves the browser menu alone on text fields', () => {
    renderHost()
    const event = contextMenu(screen.getByTestId('text'))
    expect(event.defaultPrevented).toBe(false)
    expect(screen.queryByTestId('app-context-menu')).toBeNull()
  })

  it('does not override a surface that already handled the event', () => {
    renderHost()
    const event = contextMenu(screen.getByTestId('handled'))
    expect(event.defaultPrevented).toBe(true)
    expect(screen.queryByTestId('app-context-menu')).toBeNull()
  })

  it('lets a surface explicitly request the shared fallback menu', () => {
    renderHost()
    const event = contextMenu(screen.getByTestId('local'))
    expect(event.defaultPrevented).toBe(true)
    expect(screen.getByTestId('app-context-menu')).toBeTruthy()
  })

  it('opens search and feedback, then closes', () => {
    const { onSearch } = renderHost(true)
    contextMenu(screen.getByTestId('surface'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Search…' }))
    expect(onSearch).toHaveBeenCalledOnce()
    expect(screen.queryByTestId('app-context-menu')).toBeNull()
  })

  it('hides feedback when feedback is disabled', () => {
    renderHost()
    contextMenu(screen.getByTestId('surface'))
    expect(screen.queryByRole('menuitem', { name: 'Send feedback' })).toBeNull()
  })

  it('opens from the keyboard on app chrome but not inside text fields', () => {
    renderHost()
    fireEvent.keyDown(document.body, { key: 'F10', shiftKey: true })
    expect(screen.getByTestId('app-context-menu')).toBeTruthy()

    fireEvent.click(screen.getByRole('menuitem', { name: 'Search…' }))
    fireEvent.keyDown(screen.getByTestId('text'), { key: 'ContextMenu' })
    expect(screen.queryByTestId('app-context-menu')).toBeNull()
  })

  it('closes on Escape', () => {
    renderHost()
    contextMenu(screen.getByTestId('surface'))
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(screen.queryByTestId('app-context-menu')).toBeNull()
  })
})
