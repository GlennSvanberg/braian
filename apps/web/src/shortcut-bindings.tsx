import { useCallback } from 'react'
import { useHotkeys } from 'react-hotkeys-hook'
import { isTextEditingTarget } from './cell-edit'
import { shortcutById, type Shortcut } from './shortcuts'

export type ShortcutHandlers = {
  openPalette: () => void
  openAi: () => void
  saveReview: () => void
  openFeedback?: () => void
  openHelp: () => void
  closeHelp: () => void
  openWorkspaceAt: (index: number) => void
  focusPane: (direction: 'left' | 'right' | 'top' | 'bottom') => void
  moveView: (direction: 'left' | 'right' | 'top' | 'bottom') => void
  resize: (direction: 'left' | 'right' | 'top' | 'bottom', delta: number) => void
  zoom: () => void
  closeView: () => void
  moveColumn: (direction: 'left' | 'right') => void
  paletteOpen: boolean
  feedbackEnabled: boolean
}

function typingTarget(event: KeyboardEvent) {
  const target = event.target
  return (
    target instanceof HTMLElement
    && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
  )
}

function useRegisteredHotkey(
  id: string,
  handler: (event: KeyboardEvent) => void,
  options: { enabled?: boolean; paletteOpen?: boolean } = {},
) {
  const def = shortcutById(id)
  if (!def) throw new Error(`unknown shortcut ${id}`)
  useHotkeys(
    def.hotkeys.join(','),
    (event) => {
      if (isTextEditingTarget(event)) return
      if (def.rejectShift && event.shiftKey) return
      if (def.layoutOnly && (options.paletteOpen || typingTarget(event))) return
      handler(event)
    },
    {
      preventDefault: true,
      enableOnFormTags: true,
      enableOnContentEditable: true,
      enabled: options.enabled !== false,
      eventListenerOptions: { capture: true },
    },
    [handler, options.enabled, options.paletteOpen, def],
  )
}

/** Registers every app/layout shortcut from the typed registry via react-hotkeys-hook. */
export function useShortcutBindings(handlers: ShortcutHandlers) {
  const openPalette = useCallback(
    (event: KeyboardEvent) => {
      event.stopPropagation()
      handlers.openPalette()
    },
    [handlers],
  )
  useRegisteredHotkey('palette.search', openPalette)

  useRegisteredHotkey('palette.ai', useCallback(() => handlers.openAi(), [handlers]))
  useRegisteredHotkey('review.save', useCallback(() => handlers.saveReview(), [handlers]))
  useRegisteredHotkey(
    'feedback.send',
    useCallback(() => {
      handlers.openFeedback?.()
    }, [handlers]),
    { enabled: handlers.feedbackEnabled && Boolean(handlers.openFeedback) },
  )
  useRegisteredHotkey('help.open', useCallback(() => handlers.openHelp(), [handlers]))
  useRegisteredHotkey('help.close', useCallback(() => handlers.closeHelp(), [handlers]))

  useRegisteredHotkey('workspace.1', useCallback(() => handlers.openWorkspaceAt(0), [handlers]))
  useRegisteredHotkey('workspace.2', useCallback(() => handlers.openWorkspaceAt(1), [handlers]))
  useRegisteredHotkey('workspace.3', useCallback(() => handlers.openWorkspaceAt(2), [handlers]))
  useRegisteredHotkey('workspace.4', useCallback(() => handlers.openWorkspaceAt(3), [handlers]))
  useRegisteredHotkey('workspace.5', useCallback(() => handlers.openWorkspaceAt(4), [handlers]))
  useRegisteredHotkey('workspace.6', useCallback(() => handlers.openWorkspaceAt(5), [handlers]))
  useRegisteredHotkey('workspace.7', useCallback(() => handlers.openWorkspaceAt(6), [handlers]))
  useRegisteredHotkey('workspace.8', useCallback(() => handlers.openWorkspaceAt(7), [handlers]))
  useRegisteredHotkey('workspace.9', useCallback(() => handlers.openWorkspaceAt(8), [handlers]))

  const layout = { paletteOpen: handlers.paletteOpen }
  useRegisteredHotkey('layout.focus.left', useCallback(() => handlers.focusPane('left'), [handlers]), layout)
  useRegisteredHotkey('layout.focus.right', useCallback(() => handlers.focusPane('right'), [handlers]), layout)
  useRegisteredHotkey('layout.focus.up', useCallback(() => handlers.focusPane('top'), [handlers]), layout)
  useRegisteredHotkey('layout.focus.down', useCallback(() => handlers.focusPane('bottom'), [handlers]), layout)
  useRegisteredHotkey('layout.move.left', useCallback(() => handlers.moveView('left'), [handlers]), layout)
  useRegisteredHotkey('layout.move.right', useCallback(() => handlers.moveView('right'), [handlers]), layout)
  useRegisteredHotkey('layout.move.up', useCallback(() => handlers.moveView('top'), [handlers]), layout)
  useRegisteredHotkey('layout.move.down', useCallback(() => handlers.moveView('bottom'), [handlers]), layout)
  useRegisteredHotkey('layout.resize.left', useCallback(() => handlers.resize('left', -0.05), [handlers]), layout)
  useRegisteredHotkey('layout.resize.right', useCallback(() => handlers.resize('right', 0.05), [handlers]), layout)
  useRegisteredHotkey('layout.resize.up', useCallback(() => handlers.resize('top', -0.05), [handlers]), layout)
  useRegisteredHotkey('layout.resize.down', useCallback(() => handlers.resize('bottom', 0.05), [handlers]), layout)
  useRegisteredHotkey('layout.zoom', useCallback(() => handlers.zoom(), [handlers]), layout)
  useRegisteredHotkey('layout.close', useCallback(() => handlers.closeView(), [handlers]), layout)

  useRegisteredHotkey(
    'table.column.move-left',
    useCallback(() => handlers.moveColumn('left'), [handlers]),
    layout,
  )
  useRegisteredHotkey(
    'table.column.move-right',
    useCallback(() => handlers.moveColumn('right'), [handlers]),
    layout,
  )
}

/** Same guards the bindings use — for unit tests without mounting React. */
export function shouldHandleShortcut(
  def: Shortcut,
  event: KeyboardEvent,
  options: { paletteOpen?: boolean } = {},
) {
  if (isTextEditingTarget(event)) return false
  if (def.rejectShift && event.shiftKey) return false
  if (def.layoutOnly && (options.paletteOpen || typingTarget(event))) return false
  return true
}
