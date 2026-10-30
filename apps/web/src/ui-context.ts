export type SelectedObject = { id: string; key: string; name: string; type: string; version?: string }

export type UiPane = { id: string; type: string; title: string; focused: boolean }

/** A table column in display order. `id` is the key `ui.hide_column` and `ui.show_column` take. */
export type UiColumn = { id: string; header: string; hidden: boolean }

export type UiContextSnapshot = {
  workspace: { id: string; name: string; type?: string } | null
  panes: UiPane[]
  focused: string | null
  selection: SelectedObject[]
  filter: string
  sort: string
  columns: UiColumn[]
  /** Header the user last clicked in the focused table (for Move column left/right). */
  activeColumn: string | null
  language: string
}

type Frame = {
  workspace: UiContextSnapshot['workspace']
  panes: UiPane[]
  focused: string | null
  language: string
}

const blankFrame = (): Frame => ({ workspace: null, panes: [], focused: null, language: 'en' })

let frame: Frame = blankFrame()
let markedId: string | null = null
const tables = new Map<string, { filter: string; sort: string }>()
const tableColumns = new Map<string, UiColumn[]>()
const activeColumns = new Map<string, string>()
const selections = new Map<string, SelectedObject[]>()
const enriched = new Map<string, SelectedObject>()

export function resetUiContext() {
  frame = blankFrame()
  markedId = null
  tables.clear()
  tableColumns.clear()
  activeColumns.clear()
  selections.clear()
  enriched.clear()
}

export function panesFromCatalog(
  openIds: string[],
  focused: string | null,
  catalog: Array<{ id: string; type: string; title: string }>,
): UiPane[] {
  const byId = new Map(catalog.map((view) => [view.id, view]))
  return openIds.map((id) => {
    const view = byId.get(id)
    return { id, type: view?.type ?? '', title: view?.title ?? id, focused: id === focused }
  })
}

export function publishFrame(next: Frame) {
  frame = next
}

export function publishMarked(id: string | null) {
  markedId = id
}

export function publishTable(viewId: string, filter: string, sort: string) {
  tables.set(viewId, { filter, sort })
}

export function publishColumns(viewId: string, columns: UiColumn[]) {
  tableColumns.set(viewId, columns)
}

export function publishActiveColumn(viewId: string, columnId: string | null) {
  if (!columnId) activeColumns.delete(viewId)
  else activeColumns.set(viewId, columnId)
}

export function publishSelection(viewId: string, rows: SelectedObject[]) {
  selections.set(viewId, rows)
}

export function enrichSelection(object: SelectedObject) {
  const previous = enriched.get(object.id)
  const next: SelectedObject = {
    id: object.id,
    key: object.key || previous?.key || '',
    name: object.name || previous?.name || '',
    type: object.type || previous?.type || '',
  }
  const version = object.version || previous?.version || ''
  if (version) next.version = version
  enriched.set(object.id, next)
}

function mergeObject(object: SelectedObject): SelectedObject {
  const extra = enriched.get(object.id)
  if (!extra) return object
  const merged: SelectedObject = {
    id: object.id,
    key: object.key || extra.key,
    name: object.name || extra.name,
    type: object.type || extra.type,
  }
  const version = object.version || extra.version || ''
  if (version) merged.version = version
  return merged
}

export function readUiContext(): UiContextSnapshot {
  const focused = frame.focused
  const table = (focused && tables.get(focused)) || [...tables.values()][0]
  const columns = (focused && tableColumns.get(focused)) || [...tableColumns.values()][0] || []
  const activeColumn =
    (focused && activeColumns.get(focused)) || [...activeColumns.values()][0] || null
  const chosen = (focused && selections.get(focused)) || [...selections.values()].find((rows) => rows.length > 0) || []
  let selection = chosen.map(mergeObject)
  if (selection.length === 0 && markedId) {
    selection = [mergeObject({ id: markedId, key: '', name: '', type: '' })]
  } else if (markedId && !selection.some((item) => item.id === markedId)) {
    selection = [mergeObject({ id: markedId, key: '', name: '', type: '' }), ...selection]
  }
  return {
    workspace: frame.workspace,
    panes: frame.panes,
    focused,
    selection,
    filter: table?.filter ?? '',
    sort: table?.sort ?? '',
    columns: columns.map((column) => ({ ...column })),
    activeColumn,
    language: frame.language || 'en',
  }
}
