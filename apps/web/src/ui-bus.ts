import type { Json } from './api'

export type UiEvent = { name: string; args: Json }

const listeners = new Set<(event: UiEvent) => void>()

export function onUi(listener: (event: UiEvent) => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function dispatchUi(name: string, args: Json = {}) {
  const event = { name, args }
  for (const listener of listeners) listener(event)
  return event
}

export function applyUiResult(result: unknown) {
  if (!result || typeof result !== 'object') return false
  const record = result as Json
  if (record.ui !== true || typeof record.name !== 'string') return false
  const args = record.args && typeof record.args === 'object' ? (record.args as Json) : {}
  dispatchUi(record.name, args)
  return true
}

export const uiOperations = [
  'ui.open_view',
  'ui.open_template',
  'ui.open_workspace',
  'ui.open_changes',
  'ui.place',
  'ui.link',
  'ui.filter',
  'ui.group',
  'ui.sort',
  'ui.select',
  'ui.select_rows',
  'ui.show_column',
  'ui.hide_column',
  'ui.move_column',
  'ui.set_theme',
  'ui.export',
  'ui.get_context',
  'ui.get_selection',
  'open_view',
  'move_view',
  'split',
  'resize',
  'swap',
  'focus',
  'move_view_left',
  'move_view_right',
  'move_view_up',
  'move_view_down',
  'zoom',
  'close_view',
  'get_layout',
  'reset_layout',
] as const
