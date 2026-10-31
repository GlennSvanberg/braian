/** @vitest-environment jsdom */
import { describe, expect, it } from 'vitest'
import { shouldHandleShortcut } from './shortcut-bindings'
import { shortcutById } from './shortcuts'

function keyEvent(partial: Partial<KeyboardEvent> & { target?: EventTarget | null }): KeyboardEvent {
  return {
    shiftKey: false,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    target: document.body,
    ...partial,
  } as KeyboardEvent
}

describe('shortcut binding guards (#132 + Alt+Shift)', () => {
  it('does not handle shortcuts while a cell editor is the target', () => {
    document.body.innerHTML = '<input class="cell-editor" data-testid="cell-editor-name" />'
    const input = document.querySelector('.cell-editor')!
    const def = shortcutById('palette.search')!
    expect(shouldHandleShortcut(def, keyEvent({ target: input }))).toBe(false)
  })

  it('rejects Alt+arrow focus when Shift is held', () => {
    const def = shortcutById('layout.focus.left')!
    expect(def.rejectShift).toBe(true)
    expect(shouldHandleShortcut(def, keyEvent({ shiftKey: true, altKey: true }))).toBe(false)
    expect(shouldHandleShortcut(def, keyEvent({ shiftKey: false, altKey: true }))).toBe(true)
  })
})
