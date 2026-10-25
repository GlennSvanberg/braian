import { describe, expect, it, beforeEach } from 'vitest'
import {
  enrichSelection,
  panesFromCatalog,
  publishFrame,
  publishMarked,
  publishSelection,
  publishTable,
  readUiContext,
  resetUiContext,
} from './ui-context'

const id = '018f1000-0000-7000-8000-000000000011'

describe('ui context', () => {
  beforeEach(() => resetUiContext())

  it('resolves a marked row to the object open in detail', () => {
    publishFrame({
      workspace: { id: 'ws', name: 'Enrich objects' },
      panes: panesFromCatalog(['v-table', 'v-detail', 'v-graph'], 'v-table', [
        { id: 'v-table', type: 'object_table', title: 'Objects' },
        { id: 'v-detail', type: 'object_detail', title: 'Detail' },
        { id: 'v-graph', type: 'graph', title: 'Graph' },
      ]),
      focused: 'v-table',
      language: 'sv',
    })
    publishTable('v-table', '', 'a_nummer:asc')
    publishMarked(id)
    publishSelection('v-table', [{ id, key: 'SYN-000001', name: '', type: 'artikel' }])
    enrichSelection({ id, key: 'SYN-000001', name: 'Insulation 2', type: 'artikel' })
    const snap = readUiContext()
    expect(snap.selection).toEqual([{ id, key: 'SYN-000001', name: 'Insulation 2', type: 'artikel' }])
    expect(snap.panes.map((pane) => pane.type)).toEqual(['object_table', 'object_detail', 'graph'])
    expect(snap.panes.find((pane) => pane.focused)?.id).toBe('v-table')
    expect(snap.filter).toBe('')
    expect(snap.sort).toBe('a_nummer:asc')
    expect(snap.language).toBe('sv')
    expect(snap.workspace?.name).toBe('Enrich objects')
  })
})
