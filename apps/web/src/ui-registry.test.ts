import { describe, expect, it } from 'vitest'
import { uiOperations } from './ui-bus'

/**
 * UI operations the server registers for the agent and MCP.
 * Keep this identical to `CLIENT_UI_OPERATIONS` in `crates/api/src/platform.rs`.
 * `AGENT_UI_EXCLUDED` there is empty: every name is exposed.
 */
const registeredUiOperations = [
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

const agentExcluded = new Set<string>([])

describe('ui operation registry', () => {
  it('exposes every UI operation to the agent unless it is explicitly excluded', () => {
    expect([...uiOperations].sort()).toEqual([...registeredUiOperations].sort())
    for (const name of uiOperations) {
      expect(agentExcluded.has(name)).toBe(false)
    }
    expect(uiOperations).toContain('ui.get_context')
    expect(uiOperations).toContain('ui.get_selection')
    expect(uiOperations).toContain('get_layout')
  })
})
