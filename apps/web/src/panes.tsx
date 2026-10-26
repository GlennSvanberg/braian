import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from 'react'
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { useVirtualizer } from '@tanstack/react-virtual'
import { MoreHorizontal, Search } from 'lucide-react'
import { ApiError, api, catalogName, recordNumber, recordTitle, textValue, type Json } from './api'
import { batchGetObjects } from './batch'
import { MetaChip, optionLabel, statusTone, typeLabel } from './chips'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from './components/ui/dropdown-menu'
import { Slider } from './components/ui/slider'
import { Kbd } from './components/ui/kbd'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import { Switch } from './components/ui/switch'
import { assortmentDisplayName, jobTypeDisplayTitle } from './demo-labels'
import { onFlash } from './flash'
import { GraphMap } from './graph-map'
import { exportPayload, runExport, saveWorkbook } from './export'
import { diffLines, historyTitle, isCreate, proposedValues, relativeTime, sourceLabel, suggestionLines, valuesFromKept } from './field-diff'
import { FieldDiff } from './field-diff-view'
import { copy, uiLanguage } from './i18n'
import {
  emptyDetail,
  emptyGraph,
  isGenericObjectTitle,
  newObjectLabel,
  singularOf,
  typeNames,
  typeOfView,
} from './type-labels'
import { publishProposal } from './proposal-bus'
import { StateNote } from './states'
import { classSections } from './class-sections'
import { objectTableFilter } from './table-filter'
import { dispatchUi, onUi } from './ui-bus'
import { enrichSelection, publishColumns, publishSelection, publishTable } from './ui-context'

export type PanesSession = {
  group: string | null
  objectId: string | null
  proposalId: string | null
  language: string
  listType?: string | null
  setListType?: (value: string | null) => void
  setGroup: (value: string | null) => void
  setObjectId: (value: string | null) => void
  setProposalId: (value: string | null) => void
  push: (line: string) => void
}

export type PanesView = {
  id: string
  type: string
  title: string
  pane?: string
  position?: number
  state: Json
}

export type PanesWorkspace = { id: string; locked?: boolean }

type ObjRow = { id: string; key: string; type: string; version: string; name: string; status: string }

const columns = [
  { id: 'key', header: 'Number', size: 160, sort: 'a_number' },
  { id: 'name', header: 'Name', size: 280, sort: 'a_name__en' },
  { id: 'type', header: 'Type', size: 108, sort: 'type_key' },
  { id: 'status', header: 'Status', size: 128, sort: 'a_status' },
  { id: 'version', header: 'Version', size: 72, sort: 'version' },
]

const pageSize = 100

function savedTable(view: PanesView) {
  const table = (view.state.table as Json | undefined) ?? {}
  return {
    filter: String(table.filter ?? ''),
    sort: String(table.sort ?? 'a_number:asc'),
    groupBy: String(table.groupBy ?? ''),
    order: Array.isArray(table.order) ? table.order.map(String) : columns.map((column) => column.id),
    sizes: (table.sizes as Record<string, number> | undefined) ?? {},
  }
}

function objectRowId(id: string) {
  return `object-row-${id}`
}

function toRow(doc: Json): ObjRow {
  return {
    id: String(doc.id ?? ''),
    key: String(doc.key ?? ''),
    type: String(doc.type_key ?? ''),
    version: String(doc.version ?? ''),
    name: catalogName(doc),
    status: textValue(doc.a_status),
  }
}

export function ObjectTable({
  session,
  view,
  workspace,
}: {
  session: PanesSession
  view: PanesView
  workspace: PanesWorkspace
}) {
  const initial = savedTable(view)
  const [filter, setFilter] = useState(initial.filter)
  const [queryText, setQueryText] = useState(initial.filter)
  const [sort, setSort] = useState(initial.sort)
  const [groupBy, setGroupBy] = useState(initial.groupBy)
  const [order, setOrder] = useState(initial.order)
  const [sizes, setSizes] = useState<Record<string, number>>(initial.sizes)
  const [selected, setSelected] = useState<string[]>([])
  const [hidden, setHidden] = useState<string[]>([])
  // Single current row. Click and arrows both write this; the detail object follows it.
  const [active, setActive] = useState(-1)
  // The active cell's column, by key. Left and Right move it; the keyboard menu acts on it.
  const [activeColumn, setActiveColumn] = useState('')
  const [menu, setMenu] = useState<{ x: number; y: number; id: string; column: string } | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const menuOpen = useRef(false)
  const [flashId, setFlashId] = useState('')
  const queryClient = useQueryClient()
  const parent = useRef<HTMLDivElement>(null)
  const virtualizerRef = useRef<{ scrollToIndex: (index: number) => void }>({ scrollToIndex: () => {} })
  const locateRef = useRef<(id: string) => number>(() => -1)
  const selectMode = useRef<{ extend: boolean; anchor: number } | null>(null)
  useEffect(() => onFlash(setFlashId), [])

  useEffect(() => {
    const timer = setTimeout(() => setQueryText(filter), 120)
    return () => clearTimeout(timer)
  }, [filter])

  useEffect(() => {
    if (workspace.locked) return
    const timer = setTimeout(() => {
      const state = { ...view.state, table: { filter, sort, groupBy, order, sizes } }
      void api('/v1/views', {
        method: 'POST',
        body: JSON.stringify({
          id: view.id,
          workspace_id: workspace.id,
          type: view.type,
          title: view.title,
          pane: view.pane ?? 'center',
          position: view.position ?? 0,
          state,
        }),
      })
    }, 400)
    return () => clearTimeout(timer)
  }, [filter, sort, groupBy, order, sizes, view, workspace])

  const assortments = useQuery({
    queryKey: ['assortments'],
    queryFn: () => api<{ assortments: Json[] }>('/v1/assortments'),
  })
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const barRef = useRef<HTMLDivElement>(null)
  const filtersRef = useRef<HTMLDivElement>(null)
  const [overflow, setOverflow] = useState(false)
  useLayoutEffect(() => {
    const bar = barRef.current
    const filters = filtersRef.current
    if (!bar || !filters) return
    const update = () => {
      const reserved = 140 + 124 + 88 + 36 + 32
      setOverflow(filters.scrollWidth > bar.clientWidth - reserved)
    }
    const observer = new ResizeObserver(update)
    observer.observe(bar)
    update()
    return () => observer.disconnect()
  }, [assortments.data, groupBy, selected.length])
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== '/' || event.ctrlKey || event.metaKey || event.altKey) return
      const target = event.target
      if (target instanceof HTMLElement && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))) return
      const field = document.querySelector<HTMLInputElement>('.pane.focused [data-testid="quick-filter"]')
        ?? document.querySelector<HTMLInputElement>('[data-testid="quick-filter"]')
      if (!field) return
      event.preventDefault()
      field.focus()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const names = typeNames(defs.data, uiLanguage)
  const bound = typeOfView(view, names) ?? (isGenericObjectTitle(view.title) ? names[0] : undefined)
  const typeKey = bound?.key ?? ''
  useEffect(() => {
    session.setListType?.(typeKey || null)
  }, [session, typeKey])
  const filterBy = objectTableFilter(typeKey, session.group, assortments.data?.assortments ?? [])
  const probe = useQuery({
    queryKey: ['objects', queryText, filterBy, sort, groupBy, 1],
    queryFn: () => loadPage(queryText, filterBy, sort, groupBy, 1),
  })
  const found = Number(probe.data?.found ?? 0)
  const virtualizer = useVirtualizer({
    count: groupBy ? 0 : found,
    getScrollElement: () => parent.current,
    estimateSize: () => 28,
    overscan: 16,
  })
  const visible = virtualizer.getVirtualItems()
  const start = visible[0]?.index ?? 0
  const end = visible.at(-1)?.index ?? start
  const pages = groupBy ? [] : [...new Set([Math.floor(start / pageSize) + 1, Math.floor(end / pageSize) + 1])]
  const windows = useQueries({
    queries: pages.map((page) => ({
      queryKey: ['objects', queryText, filterBy, sort, groupBy, page],
      queryFn: () => loadPage(queryText, filterBy, sort, groupBy, page),
    })),
  })
  const byIndex = new Map<number, ObjRow>()
  const rowsRef = useRef(byIndex)
  for (const result of windows) {
    const page = Number(result.data?.page ?? 0)
    const hits = (result.data?.hits as Json[] | undefined) ?? []
    hits.forEach((hit, offset) => {
      const doc = (hit.document as Json) ?? hit
      byIndex.set((page - 1) * pageSize + offset, toRow(doc))
    })
  }
  const groups = ((probe.data?.groups as Json[] | undefined) ?? []).map((group) => ({
    key: String(group.key ?? ''),
    found: Number(group.found ?? 0),
    hits: ((group.hits as Json[]) ?? []).map((hit) => toRow((hit.document as Json) ?? hit)),
  }))
  const groupedRows = groups.flatMap((group) => group.hits)
  const listCount = groupBy ? groupedRows.length : found
  const rowAt = (index: number) => (groupBy ? groupedRows[index] : byIndex.get(index))
  const activeRow = active >= 0 ? rowAt(active) : undefined
  const activeRowId = activeRow?.id
  virtualizerRef.current = virtualizer
  locateRef.current = (id: string) => {
    if (groupBy) return groupedRows.findIndex((row) => row.id === id)
    for (const [index, row] of byIndex) if (row.id === id) return index
    return -1
  }

  rowsRef.current = byIndex
  const knownStamp = [...byIndex.values()].map((row) => `${row.id}:${row.key}:${row.name}:${row.type}`).join('|')
  const activeStamp = activeRow ? `${activeRow.id}:${activeRow.key}:${activeRow.name}:${activeRow.type}` : ''
  useEffect(() => {
    const known = [...rowsRef.current.values()]
    const described = (id: string) => {
      const row = known.find((item) => item.id === id)
      return row
        ? { id: row.id, key: row.key, name: row.name, type: row.type }
        : { id, key: '', name: '', type: '' }
    }
    // One active row (arrow or click). Checkbox ranges stay local; the agent sees the active row.
    const marked = activeRow
      ? [{ id: activeRow.id, key: activeRow.key, name: activeRow.name, type: activeRow.type }]
      : selected.length > 0
        ? selected.map(described)
        : session.objectId
          ? [described(session.objectId)]
          : []
    publishTable(view.id, [queryText, filterBy].filter(Boolean).join(' '), sort)
    publishSelection(view.id, marked)
  }, [activeStamp, selected, session.objectId, queryText, filterBy, sort, view.id, knownStamp])

  const ordered = order.filter((id) => columns.some((column) => column.id === id))
  const all = ordered.length > 0 ? ordered : columns.map((column) => column.id)
  const shown = all.filter((id) => !hidden.includes(id))
  const hiddenShown = all.filter((id) => hidden.includes(id))
  const cellColumn = shown.includes(activeColumn) ? activeColumn : shown[0] ?? ''
  const columnHeader = (id: string) => columns.find((column) => column.id === id)?.header ?? id
  const columnStamp = all.map((id) => `${id}:${hidden.includes(id) ? 1 : 0}`).join('|')
  useEffect(() => {
    publishColumns(view.id, all.map((id) => ({ id, header: columnHeader(id), hidden: hidden.includes(id) })))
  }, [columnStamp, view.id])
  const exportVisible = () => {
    const body = exportPayload({
      columns,
      hidden,
      order,
      q: queryText,
      filter: filterBy,
      sort,
      ids: selected,
      lang: session.language,
    })
    session.push(copy.exportStarted)
    void runExport(body)
      .then((blob) => {
        saveWorkbook(blob)
        session.push(copy.exportReady)
      })
      .catch((error: unknown) => {
        session.push(error instanceof Error ? error.message : copy.exportFailed)
      })
  }
  const mass = useMutation({
    mutationFn: () =>
      api<Json>('/v1/operations/mass_edit', {
        method: 'POST',
        body: JSON.stringify({
          title: 'Status OK',
          ids: selected,
          type: [...byIndex.values()].find((row) => selected.includes(row.id))?.type ?? 'artikel',
          values: { status: 'ok' },
        }),
      }),
    onSuccess: (result) => {
      session.setProposalId(String(result.id))
      session.push(`${copy.proposals} ${result.id}`)
    },
  })

  useEffect(() => onUi((event) => {
    const args = event.args
    if (event.name === 'ui.filter') setFilter(String(args.q ?? args.text ?? args.filter ?? ''))
    if (event.name === 'ui.group') setGroupBy(String(args.group ?? args.by ?? ''))
    if (event.name === 'ui.sort' && args.sort) setSort(String(args.sort))
    if (event.name === 'ui.select' && args.id) {
      const id = String(args.id)
      selectMode.current = null
      session.setObjectId(id)
      setSelected([id])
      const index = locateRef.current(id)
      if (index >= 0) setActive(index)
    }
    if (event.name === 'ui.select_rows' && Array.isArray(args.ids)) setSelected(args.ids.map(String))
    if (event.name === 'ui.show_column') {
      const column = String(args.column ?? args.id ?? '')
      setHidden((current) => current.filter((id) => id !== column))
    }
    if (event.name === 'ui.hide_column') setHidden((current) => [...new Set([...current, String(args.column ?? args.id ?? '')])])
    if (event.name === 'ui.move_column') {
      const from = String(args.from ?? args.column ?? '')
      const to = String(args.to ?? '')
      if (!from) return
      setOrder((current) => {
        const next = current.filter((item) => item !== from)
        const at = to ? next.indexOf(to) : next.length
        next.splice(at < 0 ? next.length : at, 0, from)
        return next
      })
    }
    if (event.name === 'ui.export') exportVisible()
  }), [filterBy, hidden, order, queryText, selected, session, sort])

  const applySelection = (row: ObjRow, index: number, anchor: number, extend: boolean) => {
    session.setObjectId(row.id)
    if (!extend) {
      setSelected([row.id])
      return
    }
    const [start, end] = anchor < index ? [anchor, index] : [index, Math.max(anchor, 0)]
    const ids: string[] = []
    for (let cursor = start; cursor <= end; cursor += 1) {
      const item = rowAt(cursor)
      if (item) ids.push(item.id)
    }
    setSelected(ids.length > 0 ? ids : [row.id])
  }

  // Arrows and clicks share `active`. A virtualized row may mount a frame later; the detail follows once it is known.
  useEffect(() => {
    if (active < 0) return
    if (groupBy) {
      const row = rowAt(active)
      if (row) document.getElementById(objectRowId(row.id))?.scrollIntoView({ block: 'nearest' })
    } else {
      virtualizerRef.current.scrollToIndex(active)
    }
  }, [active, groupBy])

  useEffect(() => {
    if (!activeRow || activeRow.id !== activeRowId) return
    const mode = selectMode.current
    if (mode) {
      selectMode.current = null
      applySelection(activeRow, active, mode.anchor, mode.extend)
      return
    }
    if (session.objectId !== activeRow.id) session.setObjectId(activeRow.id)
  }, [active, activeRowId])

  const moveActive = (next: number, extend: boolean) => {
    if (listCount <= 0) return
    const clamped = Math.max(0, Math.min(listCount - 1, next))
    selectMode.current = { extend, anchor: active }
    setActive(clamped)
    const row = rowAt(clamped)
    if (!row) return
    selectMode.current = null
    applySelection(row, clamped, active, extend)
  }

  const focusGrid = () => {
    const node = parent.current
    if (!node || document.activeElement === node) return
    node.focus({ preventScroll: true })
  }

  const onRowClick = (index: number, event: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean }) => {
    focusGrid()
    selectMode.current = null
    setActive(index)
    const row = rowAt(index)
    if (!row) return
    session.setObjectId(row.id)
    if (event.shiftKey) setSelected((current) => (current.includes(row.id) ? current : [...current, row.id]))
    else if (event.metaKey || event.ctrlKey) {
      setSelected((current) => (current.includes(row.id) ? current.filter((id) => id !== row.id) : [...current, row.id]))
    } else setSelected([row.id])
  }

  const onRowMenu = (index: number, x: number, y: number, column: string) => {
    focusGrid()
    selectMode.current = null
    setActive(index)
    if (column) setActiveColumn(column)
    const row = rowAt(index)
    if (!row) return
    session.setObjectId(row.id)
    setMenu({ x, y, id: row.id, column })
  }

  // Shift+F10 or the ContextMenu key: the same menu, placed under the active cell.
  const openMenuFromKeyboard = () => {
    const index = active < 0 ? 0 : active
    const row = rowAt(index)
    if (!row) return
    const cellNode = document.getElementById(objectRowId(row.id))?.querySelector(`[data-column="${cellColumn}"]`)
    const box = cellNode?.getBoundingClientRect()
    onRowMenu(index, box ? box.left : 0, box ? box.bottom : 0, cellColumn)
  }

  const closeMenu = () => {
    setMenu(null)
    focusGrid()
  }

  useEffect(() => {
    if (menu && !menuOpen.current) menuRef.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus()
    // An item that closed the menu took focus with it; give it back to the grid.
    if (!menu && menuOpen.current && (document.activeElement === document.body || !document.activeElement)) focusGrid()
    menuOpen.current = Boolean(menu)
  }, [menu])

  const onMenuKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])
    const at = items.findIndex((item) => item === document.activeElement)
    if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault()
      event.stopPropagation()
      closeMenu()
      return
    }
    if (event.key === 'Enter') {
      // Space stays native: a focused button clicks itself on keyup.
      event.preventDefault()
      items[at]?.click()
      return
    }
    const next = event.key === 'ArrowDown'
      ? (at + 1) % items.length
      : event.key === 'ArrowUp'
        ? (at <= 0 ? items.length - 1 : at - 1)
        : event.key === 'Home'
          ? 0
          : event.key === 'End'
            ? items.length - 1
            : -1
    if (next < 0 || items.length === 0) return
    event.preventDefault()
    items[next]?.focus()
  }

  const onTableKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.ctrlKey || event.altKey || event.metaKey) return
    if ((event.key === 'F10' && event.shiftKey) || event.key === 'ContextMenu') {
      event.preventDefault()
      openMenuFromKeyboard()
      return
    }
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      event.preventDefault()
      const at = shown.indexOf(cellColumn)
      const next = shown[Math.max(0, Math.min(shown.length - 1, at + (event.key === 'ArrowRight' ? 1 : -1)))]
      if (next) setActiveColumn(next)
      return
    }
    if (!['ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', 'Home', 'End', 'Enter', 'Escape'].includes(event.key)) return
    if (event.key === 'Escape') {
      setMenu(null)
      return
    }
    event.preventDefault()
    if (event.key === 'Enter') {
      const row = rowAt(active)
      if (row) session.setObjectId(row.id)
      return
    }
    const next = event.key === 'ArrowDown'
      ? active + 1
      : event.key === 'ArrowUp'
        ? active - 1
        : event.key === 'PageDown'
          ? active + 10
          : event.key === 'PageUp'
            ? active - 10
            : event.key === 'Home'
              ? 0
              : listCount - 1
    moveActive(next, event.shiftKey)
  }

  const suggestions = useQuery({
    queryKey: ['proposals'],
    queryFn: () => api<Json>('/v1/proposals'),
  })
  const suggestionCounts = suggestionIndex(suggestions.data)
  const cell = (row: ObjRow, id: string) => {
    if (id === 'key') return row.key
    if (id === 'name') {
      const count = suggestionCounts.get(row.id) ?? 0
      return count > 0 ? (
        <span>
          {row.name}
          <span className="suggestion-count" data-testid="suggestion-count">{count} {copy.suggested}</span>
        </span>
      ) : row.name
    }
    if (id === 'type') return <MetaChip>{typeLabel(defs.data, row.type, uiLanguage)}</MetaChip>
    if (id === 'status') return <MetaChip tone={statusTone(row.status)}>{optionLabel(defs.data, 'status', row.status, uiLanguage)}</MetaChip>
    if (id === 'version') return <MetaChip>{row.version}</MetaChip>
    return ''
  }

  return (
    <div className="table-wrap" data-testid="object-table">
      <div className="toolbar object-bar" ref={barRef}>
        <label className="search-field">
          <Search size={14} aria-hidden="true" />
          <input
            data-testid="quick-filter"
            placeholder={copy.search}
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          />
          <Kbd>/</Kbd>
        </label>
        <div className="toolbar-measure" ref={filtersRef} aria-hidden="true" inert>
          <TableFilters
            groupBy={groupBy}
            setGroupBy={setGroupBy}
            setFilter={setFilter}
            setGroup={session.setGroup}
            assortments={assortments.data?.assortments ?? []}
            massDisabled={selected.length === 0}
            onMass={() => mass.mutate()}
            onExport={exportVisible}
          />
        </div>
        {overflow ? (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button type="button" className="icon-btn" aria-label={copy.moreActions} data-testid="toolbar-more">
                <MoreHorizontal size={14} aria-hidden="true" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              <TableFilters
                groupBy={groupBy}
                setGroupBy={setGroupBy}
                setFilter={setFilter}
                setGroup={session.setGroup}
                assortments={assortments.data?.assortments ?? []}
                massDisabled={selected.length === 0}
                onMass={() => mass.mutate()}
                onExport={exportVisible}
                menu
              />
            </DropdownMenuContent>
          </DropdownMenu>
        ) : (
          <div className="toolbar-actions">
            <TableFilters
              groupBy={groupBy}
              setGroupBy={setGroupBy}
              setFilter={setFilter}
              setGroup={session.setGroup}
              assortments={assortments.data?.assortments ?? []}
              massDisabled={selected.length === 0}
              onMass={() => mass.mutate()}
              onExport={exportVisible}
            />
          </div>
        )}
        <NewObject session={session} typeKey={typeKey} singular={bound?.singular} />
        <span className="row-count" data-testid="row-count">{found} {copy.rows}</span>
      </div>
      <div
        className="grid"
        ref={parent}
        tabIndex={0}
        role="grid"
        aria-activedescendant={activeRow ? objectRowId(activeRow.id) : undefined}
        onKeyDown={onTableKey}
        onContextMenu={(event) => {
          // The ContextMenu key fires this on the grid itself; a row handles a pointer right-click.
          if (event.target !== event.currentTarget) return
          event.preventDefault()
          if (!menu) openMenuFromKeyboard()
        }}
      >
        <div className="head">
          {shown.map((id) => {
            const column = columns.find((item) => item.id === id)
            if (!column) return null
            const width = sizes[id] ?? column.size
            return (
              <div key={id} className="cell" style={{ width }} data-testid={`col-${id}`}>
                <button
                  className="icon-btn"
                  draggable
                  onDragStart={(event) => event.dataTransfer.setData('text/column', id)}
                  onDragOver={(event) => event.preventDefault()}
                  onDrop={(event) => {
                    const from = event.dataTransfer.getData('text/column')
                    if (!from || from === id) return
                    setOrder((current) => {
                      const next = current.filter((item) => item !== from)
                      const at = next.indexOf(id)
                      next.splice(at < 0 ? next.length : at, 0, from)
                      return next
                    })
                  }}
                  onClick={() => {
                    const desc = sort === `${column.sort}:asc` ? 'desc' : 'asc'
                    setSort(`${column.sort}:${desc}`)
                  }}
                >
                  {column.header}
                </button>
                <span
                  className="icon-btn"
                  data-testid={`resize-${id}`}
                  onMouseDown={(event) => {
                    event.preventDefault()
                    const startX = event.clientX
                    const base = width
                    const move = (moveEvent: MouseEvent) => {
                      setSizes((current) => ({ ...current, [id]: Math.max(56, base + moveEvent.clientX - startX) }))
                    }
                    const up = () => {
                      window.removeEventListener('mousemove', move)
                      window.removeEventListener('mouseup', up)
                    }
                    window.addEventListener('mousemove', move)
                    window.addEventListener('mouseup', up)
                  }}
                >
                  |
                </span>
              </div>
            )
          })}
        </div>
        {groupBy ? (
          <div style={{ flex: 'none' }}>
            {(() => {
              let index = 0
              return groups.map((group) => (
                <div key={group.key}>
                  <strong>{group.key || '—'} ({group.found})</strong>
                  {group.hits.map((row) => {
                    const rowIndex = index
                    index += 1
                    return (
                      <Row
                        key={row.id}
                        row={row}
                        index={rowIndex}
                        shown={shown}
                        sizes={sizes}
                        selected={selected}
                        cell={cell}
                        flash={flashId === row.id}
                        active={active === rowIndex}
                        activeColumn={active === rowIndex ? cellColumn : ''}
                        onColumn={setActiveColumn}
                        onActivate={onRowClick}
                        onMenu={onRowMenu}
                        onFocusGrid={focusGrid}
                      />
                    )
                  })}
                </div>
              ))
            })()}
          </div>
        ) : (
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative', flex: 'none' }}>
            {visible.map((item) => {
              const row = byIndex.get(item.index)
              if (!row) {
                return <div key={item.index} className="row" style={{ position: 'absolute', transform: `translateY(${item.start}px)`, height: 28, width: '100%' }} />
              }
              return (
                <Row
                  key={row.id}
                  row={row}
                  index={item.index}
                  shown={shown}
                  sizes={sizes}
                  selected={selected}
                  cell={cell}
                  flash={flashId === row.id}
                  active={active === item.index}
                  activeColumn={active === item.index ? cellColumn : ''}
                  onColumn={setActiveColumn}
                  onActivate={onRowClick}
                  onMenu={onRowMenu}
                  onFocusGrid={focusGrid}
                  style={{ position: 'absolute', transform: `translateY(${item.start}px)`, width: '100%' }}
                />
              )
            })}
          </div>
        )}
        {probe.isError || (!probe.isLoading && found === 0) ? (
          <div className="table-status">
            {probe.isError ? <p className="error">{(probe.error as Error).message}</p> : null}
            {!probe.isLoading && found === 0 ? <p>{copy.noRows}</p> : null}
          </div>
        ) : null}
      </div>
      {menu ? (
        <div
          className="menu"
          ref={menuRef}
          style={{ left: menu.x, top: menu.y }}
          data-testid="context-menu"
          role="menu"
          aria-label={menu.column ? columnHeader(menu.column) : undefined}
          onKeyDown={onMenuKey}
        >
          <button type="button" role="menuitem" tabIndex={-1} onClick={() => { const index = locateRef.current(menu.id); if (index >= 0) setActive(index); session.setObjectId(menu.id); setMenu(null) }}>{copy.open} <Kbd>Enter</Kbd></button>
          <button type="button" role="menuitem" tabIndex={-1} onClick={() => { dispatchUi('ui.place', { title: 'Detail', zone: 'right' }); setMenu(null) }}>{copy.placeRight}</button>
          <button type="button" role="menuitem" tabIndex={-1} onClick={() => { dispatchUi('ui.open_view', { title: 'History' }); setMenu(null) }}>{copy.history}</button>
          <button type="button" role="menuitem" tabIndex={-1} onClick={() => { setSelected((current) => current.includes(menu.id) ? current : [...current, menu.id]); setMenu(null) }}>{copy.selectAll}</button>
          {menu.column ? (
            <button type="button" role="menuitem" tabIndex={-1} onClick={() => { dispatchUi('ui.hide_column', { column: menu.column }); setMenu(null) }}>{copy.hideColumn}</button>
          ) : null}
          {hiddenShown.map((id) => (
            <button key={id} type="button" role="menuitem" tabIndex={-1} onClick={() => { dispatchUi('ui.show_column', { column: id }); setMenu(null) }}>{copy.show} {columnHeader(id)}</button>
          ))}
          <button type="button" role="menuitem" tabIndex={-1} onClick={() => { mass.mutate(); setMenu(null) }}>{copy.massEdit}</button>
          <button type="button" role="menuitem" tabIndex={-1} onClick={() => { exportVisible(); setMenu(null) }}>{copy.export}</button>
          <button type="button" role="menuitem" tabIndex={-1} onClick={closeMenu}>{copy.close} <Kbd>Esc</Kbd></button>
        </div>
      ) : null}
    </div>
  )
}

function columnOf(target: EventTarget) {
  const cell = target instanceof Element ? target.closest('[data-column]') : null
  return cell?.getAttribute('data-column') ?? ''
}

function Row({
  row,
  index,
  shown,
  sizes,
  selected,
  cell,
  flash,
  active,
  activeColumn,
  onActivate,
  onColumn,
  onMenu,
  onFocusGrid,
  style,
}: {
  row: ObjRow
  index: number
  shown: string[]
  sizes: Record<string, number>
  selected: string[]
  cell: (row: ObjRow, id: string) => ReactNode
  flash: boolean
  active: boolean
  activeColumn: string
  onActivate: (index: number, event: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean }) => void
  onColumn: (column: string) => void
  onMenu: (index: number, x: number, y: number, column: string) => void
  onFocusGrid: () => void
  style?: CSSProperties
}) {
  // Background and border share this class. Range picks stay quieter so only the active row looks selected.
  const tone = active ? 'selected' : selected.includes(row.id) ? 'picked' : ''
  const className = ['row', tone, flash ? 'flash' : ''].filter(Boolean).join(' ')
  return (
    <div
      id={objectRowId(row.id)}
      role="row"
      aria-selected={active}
      data-testid="object-row"
      data-index={index}
      className={className}
      style={style}
      onMouseDown={(event) => {
        // tabindex used to land on the row, so its focus ring stayed put while arrows moved the fill.
        if (event.button !== 0) return
        event.preventDefault()
        onFocusGrid()
      }}
      onClick={(event) => {
        const column = columnOf(event.target)
        if (column) onColumn(column)
        onActivate(index, event)
      }}
      onContextMenu={(event) => {
        event.preventDefault()
        // The menu acts on the cell under the pointer, so it needs that cell's column key.
        onMenu(index, event.clientX, event.clientY, columnOf(event.target))
      }}
    >
      {shown.map((id) => {
        const column = columns.find((item) => item.id === id)
        return (
          <div key={id} className={id === activeColumn ? 'cell active-cell' : 'cell'} data-column={id} style={{ width: sizes[id] ?? column?.size ?? 120 }}>
            {cell(row, id)}
          </div>
        )
      })}
    </div>
  )
}

// The table reads search documents. Those already carry the columns on screen,
// so a list does not load full objects from Postgres.
async function loadPage(q: string, filter: string, sort: string, groupBy: string, page: number) {
  const params = new URLSearchParams({ page: String(page), per_page: String(pageSize) })
  if (q) params.set('q', q)
  if (filter) params.set('filter', filter)
  if (sort) params.set('sort', sort)
  if (groupBy) params.set('group', groupBy)
  const body = await api<Json>(`/v1/search?${params}`)
  body.page = page
  return body
}

function TableFilters({
  groupBy,
  setGroupBy,
  setFilter,
  setGroup,
  assortments,
  massDisabled,
  onMass,
  onExport,
  menu = false,
}: {
  groupBy: string
  setGroupBy: (value: string) => void
  setFilter: (value: string) => void
  setGroup: (value: string | null) => void
  assortments: Json[]
  massDisabled: boolean
  onMass: () => void
  onExport: () => void
  menu?: boolean
}) {
  const clear = () => {
    setGroupBy('')
    setFilter('')
    setGroup(null)
  }
  const applyAssortment = (item: Json) => {
    const query = (item.query as Json) ?? {}
    setFilter(String(query.text ?? ''))
    setGroup(query.group ? String(query.group) : null)
  }
  if (menu) {
    return (
      <>
        <DropdownMenuItem onSelect={clear}>{copy.all}</DropdownMenuItem>
        <DropdownMenuItem onSelect={() => setGroupBy(groupBy === 'type_key' ? '' : 'type_key')}>{copy.type}</DropdownMenuItem>
        <DropdownMenuItem onSelect={() => setGroupBy(groupBy === 'a_status' ? '' : 'a_status')}>{copy.status}</DropdownMenuItem>
        {assortments.map((item) => (
          <DropdownMenuItem key={String(item.id)} onSelect={() => applyAssortment(item)}>
            {assortmentDisplayName(String(item.name))}
          </DropdownMenuItem>
        ))}
        <DropdownMenuItem disabled={massDisabled} onSelect={onMass}>{copy.massEdit}</DropdownMenuItem>
        <DropdownMenuItem onSelect={onExport}>{copy.export}</DropdownMenuItem>
      </>
    )
  }
  return (
    <>
      <button type="button" className={groupBy === '' ? 'filter-chip on' : 'filter-chip'} onClick={clear}>{copy.all}</button>
      <button type="button" className={groupBy === 'type_key' ? 'filter-chip on' : 'filter-chip'} onClick={() => setGroupBy(groupBy === 'type_key' ? '' : 'type_key')}>{copy.type}</button>
      <button type="button" className={groupBy === 'a_status' ? 'filter-chip on' : 'filter-chip'} onClick={() => setGroupBy(groupBy === 'a_status' ? '' : 'a_status')}>{copy.status}</button>
      {assortments.map((item) => (
        <button key={String(item.id)} type="button" className="filter-chip" data-testid="assortment" onClick={() => applyAssortment(item)}>
          {assortmentDisplayName(String(item.name))}
        </button>
      ))}
      <button type="button" onClick={onMass} disabled={massDisabled}>{copy.massEdit}</button>
      <button type="button" onClick={onExport}>{copy.export}</button>
    </>
  )
}

function createGroups(typeKey: string, group: string | null) {
  if (typeKey === 'supplier' || !group) {
    return typeKey === 'supplier' ? {} : { food: 'bread' }
  }
  const splitAt = group.indexOf(':')
  if (splitAt > 0) return { [group.slice(0, splitAt)]: group.slice(splitAt + 1) }
  return { food: group }
}

function NewObject({ session, typeKey, singular }: { session: PanesSession; typeKey?: string; singular?: string }) {
  const [open, setOpen] = useState(false)
  const [nummer, setNummer] = useState('')
  const [error, setError] = useState('')
  const queryClient = useQueryClient()
  if (!open) return <button onClick={() => setOpen(true)}>{newObjectLabel(singular)}</button>
  return (
    <form
      className="create-row"
      onSubmit={(event) => {
        event.preventDefault()
        void api<Json>('/v1/objects', {
          method: 'POST',
          body: JSON.stringify({
            type: typeKey || 'product',
            groups: createGroups(typeKey || 'product', session.group),
            values: { number: nummer, name: { en: nummer }, status: 'draft' },
          }),
        })
          .then((created) => {
            setError('')
            setOpen(false)
            session.setObjectId(String(created.id))
            session.push(`${copy.saved} ${nummer}`)
            void queryClient.invalidateQueries({ queryKey: ['objects'] })
          })
          .catch((reason: unknown) => setError(reason instanceof ApiError ? reason.message : copy.validation))
      }}
    >
      <input data-testid="new-nummer" value={nummer} onChange={(event) => setNummer(event.target.value)} placeholder={copy.numberPlaceholder} />
      <button className="primary" type="submit" data-testid="create-object">{copy.save}</button>
      {error ? <span className="error" data-testid="validation-error">{error}</span> : null}
    </form>
  )
}

export function ClassAttributes({ session }: { session: PanesSession }) {
  const object = useQuery({
    queryKey: ['object', session.objectId],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}`),
    enabled: Boolean(session.objectId),
  })
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const types = typeNames(defs.data, uiLanguage)
  const listed = session.listType ? types.find((item) => item.key === session.listType) : undefined
  if (!session.objectId || !object.data) {
    const empty = listed
      ? `Select a ${listed.singular.toLowerCase()} to see the attributes on its classes.`
      : copy.classAttributesEmpty
    return (
      <div className="detail-empty" data-testid="class-empty">
        <p>{empty}</p>
      </div>
    )
  }
  const sections = classSections(defs.data, String(object.data.type ?? ''), object.data.groups, uiLanguage)
  if (sections.length === 0) {
    return (
      <div className="detail-empty" data-testid="class-empty">
        <p>{copy.noClasses}</p>
      </div>
    )
  }
  return (
    <div className="class-attributes" data-testid="class-attributes">
      {sections.map((section) => (
        <section key={section.id} className="class-section" data-testid="class-section">
          <h3>{section.title}</h3>
          {section.attributes.length === 0 ? <p className="hint">{copy.noClassAttributes}</p> : null}
          {section.attributes.map((attribute) => (
            <div key={attribute.id} className="class-attr" data-testid="class-attribute">
              <span>{attribute.label}</span>
              <span>{attribute.kind}</span>
            </div>
          ))}
        </section>
      ))}
    </div>
  )
}

export function ObjectDetail({ session }: { session: PanesSession }) {
  const queryClient = useQueryClient()
  const object = useQuery({
    queryKey: ['object', session.objectId],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}`),
    enabled: Boolean(session.objectId),
  })
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const suggestions = useQuery({
    queryKey: ['proposals'],
    queryFn: () => api<Json>('/v1/proposals'),
  })
  const [draft, setDraft] = useState<Json>({})
  const [error, setError] = useState('')
  const [flashId, setFlashId] = useState('')
  useEffect(() => onFlash(setFlashId), [])
  useEffect(() => {
    setDraft({})
    setError('')
  }, [session.objectId])
  useEffect(() => {
    if (!session.objectId || !object.data) return
    const stored = (object.data.values as Json) ?? {}
    const language = session.language || 'sv'
    enrichSelection({
      id: session.objectId,
      key: recordNumber(stored) || String(object.data.key ?? ''),
      name: recordTitle(stored, language),
      type: String(object.data.type ?? ''),
    })
  }, [session.objectId, session.language, object.data])
  if (!session.objectId) {
    return (
      <div className="detail-empty" data-testid="detail-empty">
        <p>{emptyDetail(typeNames(defs.data, uiLanguage), session.listType)}</p>
        <ul>
          <li><kbd>↑</kbd><kbd>↓</kbd> {copy.detailKeysMove}</li>
          <li><kbd>Enter</kbd> {copy.detailKeysOpen}</li>
          <li><kbd>Ctrl</kbd><kbd>K</kbd> {copy.detailKeysPalette}</li>
        </ul>
      </div>
    )
  }
  if (object.isError) return <StateNote tone="error" title={copy.graphError} detail={copy.validation} testId="detail-error" />
  if (!object.data) return <StateNote tone="loading" title={copy.loading} />
  const values = { ...((object.data.values as Json) ?? {}), ...draft }
  const attributes = (defs.data?.attributes as Json[] | undefined) ?? []
  const lists = (defs.data?.value_lists as Json[] | undefined) ?? []
  const families = (defs.data?.unit_families as Json[] | undefined) ?? []
  const label = recordNumber(object.data.values as Json) || String(object.data.id ?? '')
  const suggestionCount = suggestionIndex(suggestions.data).get(session.objectId) ?? 0
  const save = () => {
    if (Object.keys(draft).length === 0) return
    void api<Json>(`/v1/objects/${session.objectId}`, {
      method: 'PATCH',
      headers: { 'If-Match': String(object.data?.version ?? 0) },
      body: JSON.stringify({ values: draft }),
    })
      .then(() => {
        setDraft({})
        setError('')
        session.push(`${copy.saved} ${label}`)
        void queryClient.invalidateQueries({ queryKey: ['object', session.objectId] })
        void queryClient.invalidateQueries({ queryKey: ['objects'] })
      })
      .catch((reason: unknown) => setError(reason instanceof ApiError ? reason.message : copy.validation))
  }
  const status = textValue(values.status)
  const number = recordNumber(values)
  const name = recordTitle(values, uiLanguage)
  const dirty = Object.keys(draft).length > 0
  const typeDef = ((defs.data?.types as Json[]) ?? []).find((item) => item.key === object.data?.type)
  const typeIds = new Set(((typeDef?.attributes as unknown[]) ?? []).map(String))
  const fileKeys = attributes
    .filter((item) => typeIds.has(String(item.id)) && String((item.kind as Json)?.type) === 'file_ref')
    .map((item) => String(item.key))
  const valueKeys = [...new Set([...Object.keys(values), ...fileKeys])]
  const renderField = (key: string) => {
    const attribute = attributes.find((item) => item.key === key)
    const kind = (attribute?.kind as Json) ?? {}
    const fieldLabel = textValue((attribute?.labels as Json) ?? key, uiLanguage) || key
    return (
      <div key={key} className="field">
        <label>{fieldLabel}</label>
        <div className={key in draft ? 'control draft-user' : 'control'}>
          <Editor
            kind={String(kind.type ?? 'text')}
            form={String(kind.form ?? '')}
            multiple={Boolean(attribute?.multiple)}
            languageDependent={Boolean(attribute?.language_dependent)}
            value={values[key]}
            options={optionsFor(lists, String(kind.list ?? ''))}
            units={unitsFor(families, String(kind.unit_family ?? ''))}
            onChange={(next) => setDraft((state) => ({ ...state, [key]: next }))}
          />
        </div>
      </div>
    )
  }
  return (
    <form
      className={flashId === session.objectId ? 'detail-form form flash' : 'detail-form form'}
      data-testid="object-detail"
      onSubmit={(event) => {
        event.preventDefault()
        save()
      }}
      onKeyDown={(event) => {
        if (event.ctrlKey || event.altKey || event.metaKey) return
        if (!['ArrowDown', 'ArrowUp', 'Home', 'End', 'Escape'].includes(event.key)) return
        const target = event.target as HTMLElement
        if (target.tagName === 'TEXTAREA' && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) return
        const fields = [...event.currentTarget.querySelectorAll<HTMLElement>('[data-field]')]
        const index = fields.indexOf(target)
        if (event.key === 'Escape') {
          if (target.closest('[role="listbox"]')) return
          setDraft({})
          return
        }
        if (index < 0) return
        event.preventDefault()
        const next = event.key === 'ArrowDown' ? index + 1 : event.key === 'ArrowUp' ? index - 1 : event.key === 'Home' ? 0 : fields.length - 1
        fields[Math.max(0, Math.min(fields.length - 1, next))]?.focus()
      }}
    >
      <div className="detail-scroll">
        <header className="detail-head">
          <p className="detail-kicker">{singularOf(defs.data, String(object.data.type), uiLanguage)}</p>
          <div className="detail-title">
            <strong>{name || number}</strong>
            {number ? <span className="detail-id">{number}</span> : null}
          </div>
          <div className="field-chips">
            <MetaChip>{typeLabel(defs.data, String(object.data.type), uiLanguage)}</MetaChip>
            {status ? <MetaChip tone={statusTone(status)}>{optionLabel(defs.data, 'status', status, uiLanguage)}</MetaChip> : null}
            <MetaChip>{String(object.data.version ?? '')}</MetaChip>
            {suggestionCount > 0 ? <span className="suggestion-count" data-testid="suggestion-count">{suggestionCount} {copy.suggested}</span> : null}
          </div>
        </header>
        {fieldSections(valueKeys).map((section) => (
          <section key={section.id} className="field-card">
            <h3>{section.title}</h3>
            {section.keys.map((key) => renderField(key))}
          </section>
        ))}
        {error ? <p className="error" data-testid="detail-error">{error}</p> : null}
      </div>
      {dirty ? (
        <div className="save-bar">
          <button type="button" onClick={() => setDraft({})}>{copy.discard}</button>
          <button className="primary" type="submit" data-testid="save-object">{copy.save}</button>
        </div>
      ) : null}
    </form>
  )
}

const fieldSectionDefs: Array<{ id: string; title: string; keys: string[] }> = [
  { id: 'identity', title: copy.identity, keys: ['number', 'name', 'company_name', 'gtin', 'brand', 'org_number', 'gln', 'nummer', 'namn'] },
  { id: 'description', title: copy.description, keys: ['description', 'ingredients', 'beskrivning'] },
  { id: 'classification', title: copy.classification, keys: ['status', 'allergens', 'traces', 'labels', 'nutriscore', 'nova', 'country', 'country_of_origin', 'certifications', 'storage', 'packaging', 'packaging_material', 'deposit', 'active', 'farg', 'aktiv'] },
  { id: 'measures', title: copy.measures, keys: ['net_weight', 'net_volume', 'units_per_pack', 'height', 'width', 'depth', 'gross_weight', 'energy', 'fat', 'saturated_fat', 'carbohydrates', 'sugars', 'fiber', 'protein', 'salt', 'fat_content', 'cocoa_content', 'caffeine', 'shelf_life_days', 'lead_time_days', 'co2_footprint', 'vikt', 'langd', 'spann'] },
  { id: 'dates', title: copy.dates, keys: ['launched', 'lanserad'] },
  { id: 'references', title: copy.references, keys: ['product', 'supplier', 'replaces', 'image', 'website', 'bild', 'ersatter'] },
]

function fieldSections(keys: string[]) {
  const known = new Set<string>()
  const sections = fieldSectionDefs.flatMap((section) => {
    const present = section.keys.filter((key) => keys.includes(key))
    present.forEach((key) => known.add(key))
    return present.length > 0 ? [{ ...section, keys: present }] : []
  })
  const rest = keys.filter((key) => !known.has(key))
  if (rest.length > 0) sections.push({ id: 'other', title: copy.other, keys: rest })
  return sections
}

function optionsFor(lists: Json[], key: string) {
  const list = lists.find((item) => item.key === key)
  return ((list?.options as Json[]) ?? []).map((option) => ({
    code: String(option.code),
    label: textValue(option.labels, uiLanguage) || String(option.code),
  }))
}

function unitsFor(families: Json[], id: string) {
  const family = families.find((item) => item.id === id || item.key === id)
  return ((family?.units as Json[]) ?? []).map((unit) => String(unit.code))
}

function Editor({
  kind,
  form,
  value,
  languageDependent,
  multiple,
  options,
  units,
  onChange,
}: {
  kind: string
  form: string
  multiple: boolean
  languageDependent: boolean
  value: unknown
  options: Array<{ code: string; label: string }>
  units: string[]
  onChange: (value: unknown) => void
}) {
  if (languageDependent) {
    const record = value && typeof value === 'object' && !Array.isArray(value) ? (value as Json) : { sv: textValue(value) }
    return (
      <div className="langs">
        {['sv', 'en', 'de'].map((language) => (
          <div key={language} className="lang-card">
            <MetaChip>{language}</MetaChip>
            {form === 'long' ? (
              <textarea data-field="true" value={textValue(record[language])} onChange={(event) => onChange({ ...record, [language]: event.target.value })} />
            ) : (
              <input data-field="true" value={textValue(record[language])} onChange={(event) => onChange({ ...record, [language]: event.target.value })} />
            )}
          </div>
        ))}
      </div>
    )
  }
  if (multiple && Array.isArray(value) && kind !== 'value_list') {
    const items = value.map((item) => textValue(item))
    return (
      <div className="choices">
        {items.map((item) => <span key={item} className="meta-chip">{item}</span>)}
        <input
          data-field="true"
          value=""
          placeholder="+"
          onChange={(event) => {
            const next = event.target.value.trim()
            if (next) onChange([...items, next])
          }}
        />
      </div>
    )
  }
  if (kind === 'object_ref') return <ObjectRefField value={value} onChange={onChange} />
  if (kind === 'file_ref') return <FileField value={value} onChange={onChange} />
  if (kind === 'value_list' && options.length > 0 && multiple) {
    const picked = Array.isArray(value) ? value.map(String) : []
    return (
      <div className="choices">
        {options.map((option) => {
          const on = picked.includes(option.code)
          return (
            <button
              key={option.code}
              type="button"
              className={on ? 'choice-chip on' : 'choice-chip'}
              aria-pressed={on}
              onClick={() => onChange(on ? picked.filter((code) => code !== option.code) : [...picked, option.code])}
            >
              {option.label}
            </button>
          )
        })}
      </div>
    )
  }
  if (kind === 'value_list' && options.length > 0) {
    const current = textValue(value)
    return (
      <Select value={current || undefined} onValueChange={onChange}>
        <SelectTrigger data-field="true">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map((option) => (
            <SelectItem key={option.code} value={option.code}>{option.label}</SelectItem>
          ))}
        </SelectContent>
      </Select>
    )
  }
  if (kind === 'boolean') {
    return <Switch data-field="true" checked={value === true} onCheckedChange={(checked) => onChange(checked)} />
  }
  if (kind === 'range') {
    const record = (value && typeof value === 'object' ? value : {}) as Json
    return (
      <span className="measure">
        <input data-field="true" value={textValue(record.min)} onChange={(event) => onChange({ ...record, min: event.target.value })} />
        <input data-field="true" value={textValue(record.max)} onChange={(event) => onChange({ ...record, max: event.target.value })} />
        <UnitSuffix units={units} value={String(record.unit ?? units[0] ?? '')} onChange={(unit) => onChange({ ...record, unit })} />
      </span>
    )
  }
  if (kind === 'number') {
    const record = (value && typeof value === 'object' ? value : { value }) as Json
    return (
      <span className="measure">
        <input data-field="true" value={textValue(record.value)} onChange={(event) => onChange({ ...record, value: event.target.value })} />
        <UnitSuffix units={units} value={String(record.unit ?? units[0] ?? '')} onChange={(unit) => onChange({ ...record, unit })} />
      </span>
    )
  }
  if (form === 'long' || (typeof value === 'string' && value.length > 80)) {
    return <textarea data-field="true" value={textValue(value)} onChange={(event) => onChange(event.target.value)} />
  }
  return <input data-field="true" value={textValue(value)} onChange={(event) => onChange(event.target.value)} />
}

function UnitSuffix({ units, value, onChange }: { units: string[]; value: string; onChange: (unit: string) => void }) {
  if (units.length === 0) return null
  if (units.length === 1) return <span className="meta-chip">{units[0]}</span>
  return (
    <Select value={value || units[0]} onValueChange={onChange}>
      <SelectTrigger className="unit-suffix" data-field="true" aria-label={copy.unit}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {units.map((unit) => (
          <SelectItem key={unit} value={unit}>{unit}</SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

function FileField({ value, onChange }: { value: unknown; onChange: (value: unknown) => void }) {
  const input = useRef<HTMLInputElement>(null)
  const [name, setName] = useState('')
  const id = typeof value === 'string' ? value : ''
  const label = name || (id ? `${copy.file} ${id.slice(0, 8)}` : '')
  return (
    <div className="file-field">
      <input
        ref={input}
        className="file-input"
        type="file"
        data-testid="file-input"
        onChange={(event) => {
          const file = event.target.files?.[0]
          if (!file) return
          setName(file.name)
          onChange(crypto.randomUUID())
          event.target.value = ''
        }}
      />
      {label ? (
        <span data-testid="file-chip"><MetaChip>{label}</MetaChip></span>
      ) : (
        <span className="hint" data-testid="file-chip">{copy.noFile}</span>
      )}
      <button type="button" data-field="true" onClick={() => input.current?.click()}>
        {id ? copy.replaceFile : copy.chooseFile}
      </button>
      {id ? (
        <button
          type="button"
          onClick={() => {
            setName('')
            onChange(null)
          }}
        >
          {copy.clearFile}
        </button>
      ) : null}
    </div>
  )
}

function ObjectRefField({ value, onChange }: { value: unknown; onChange: (value: unknown) => void }) {
  const id = typeof value === 'string' ? value : ''
  const current = useQuery({
    queryKey: ['object-ref', id],
    queryFn: () =>
      batchGetObjects([id], { groups: true, fields: ['number', 'name', 'company_name', 'nummer', 'namn'], languages: [uiLanguage] }).then(
        (rows) => rows[0] ?? null,
      ),
    enabled: /^[0-9a-f-]{36}$/i.test(id),
  })
  const label = current.data
    ? `${recordNumber(current.data.values as Json)} · ${recordTitle(current.data.values as Json, uiLanguage)}`
    : ''
  const [q, setQ] = useState('')
  const [open, setOpen] = useState(false)
  const found = useQuery({
    queryKey: ['ref-pick', q],
    queryFn: () => api<Json>(`/v1/search?q=${encodeURIComponent(q)}&per_page=8`),
    enabled: open && q.trim().length > 1,
  })
  return (
    <div className="ref">
      <input
        data-testid="object-ref"
        data-field="true"
        value={open ? q : label}
        placeholder={copy.pickObject}
        onFocus={() => {
          setOpen(true)
          setQ('')
        }}
        onBlur={() => setTimeout(() => setOpen(false), 150)}
        onChange={(event) => setQ(event.target.value)}
      />
      {open && q.trim().length > 1 ? (
        <div className="ref-menu">
          {((found.data?.hits as Json[]) ?? []).map((hit) => {
            const doc = (hit.document as Json) ?? hit
            return (
              <button
                type="button"
                key={String(doc.id)}
                onMouseDown={(event) => {
                  event.preventDefault()
                  onChange(String(doc.id))
                  setOpen(false)
                }}
              >
                {String(doc.key)} · {catalogName(doc)}
              </button>
            )
          })}
        </div>
      ) : null}
    </div>
  )
}

export function GraphView({ session }: { session: PanesSession }) {
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const graph = useQuery({
    queryKey: ['graph', session.objectId],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}/graph`),
    enabled: Boolean(session.objectId),
  })
  const object = useQuery({
    queryKey: ['object', session.objectId],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}`),
    enabled: Boolean(session.objectId),
  })
  if (!session.objectId) return <StateNote tone="empty" title={emptyGraph(typeNames(defs.data, uiLanguage), session.listType)} testId="graph" />
  if (graph.isError || object.isError) return <StateNote tone="error" title={copy.graphError} testId="graph" />
  if (!graph.data || !object.data) return <StateNote tone="loading" title={copy.graphLoading} testId="graph" />
  return (
    <GraphMap
      data={graph.data}
      values={(object.data.values as Json) ?? {}}
      defs={defs.data}
      onOpen={(id) => {
        if (id) session.setObjectId(id)
      }}
    />
  )
}

export function HistoryView({ session }: { session: PanesSession }) {
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const changes = useQuery({
    queryKey: ['history', session.objectId],
    queryFn: () => api<Json[]>(`/v1/objects/${session.objectId}/changes`),
    enabled: Boolean(session.objectId),
  })
  const [open, setOpen] = useState<number | null>(null)
  const [travel, setTravel] = useState(false)
  const [seq, setSeq] = useState(0)
  const rows = [...(changes.data ?? [])].sort((left, right) => Number(right.seq) - Number(left.seq))
  const current = seq || Number(rows[0]?.seq ?? 0)
  const asOf = useQuery({
    queryKey: ['asof', session.objectId, current],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}/as-of?seq=${current}`),
    enabled: Boolean(session.objectId && current && travel),
  })
  const rollback = useMutation({
    mutationFn: (at: number) =>
      api<Json>(`/v1/objects/${session.objectId}/rollback`, {
        method: 'POST',
        body: JSON.stringify({ seq: at }),
      }),
    onSuccess: (proposal) => session.setProposalId(String(proposal.id)),
  })
  const seqs = rows.map((row) => Number(row.seq)).sort((left, right) => left - right)
  const minSeq = seqs[0] ?? 0
  const maxSeq = seqs.at(-1) ?? 0
  if (!session.objectId) return <StateNote tone="empty" title={copy.history} testId="history" />
  return (
    <div data-testid="history">
      <div className="history-head">
        <button type="button" className="btn-ghost" data-testid="time-travel" aria-pressed={travel} onClick={() => setTravel((value) => !value)}>
          {copy.timeTravel}
        </button>
      </div>
      {travel ? (
        <div className="history-asof">
          <span className="hint">{copy.asOf}</span>
          {rows.length > 0 ? (
            <Slider
              data-testid="as-of"
              min={minSeq}
              max={maxSeq === minSeq ? minSeq + 1 : maxSeq}
              step={1}
              disabled={maxSeq === minSeq}
              value={[Math.min(maxSeq === minSeq ? minSeq : maxSeq, Math.max(minSeq, current || maxSeq))]}
              onValueChange={([next]) => setSeq(nearestSeq(seqs, next ?? maxSeq))}
              aria-label={copy.asOf}
            />
          ) : null}
          <span data-testid="as-of-when"><MetaChip>{formatWhen(rows.find((row) => Number(row.seq) === current)?.at)}</MetaChip></span>
          <dl className="asof">
            {Object.entries((asOf.data?.values as Json | undefined) ?? {}).map(([key, value]) => (
              <div key={key}>
                <dt>{fieldLabel(defs.data, key)}</dt>
                <dd>{renderValue(value, key, defs.data)}</dd>
              </div>
            ))}
          </dl>
        </div>
      ) : null}
      <div className="timeline">
        {rows.map((row) => {
          const at = Number(row.seq)
          const expanded = open === at
          const created = isCreate(row)
          return (
            <article
              key={String(row.seq)}
              className={expanded ? 'selected' : ''}
              data-active={expanded ? 'true' : undefined}
              onClick={() => setOpen(expanded ? null : at)}
            >
              <header className="history-line">
                <strong>{historyTitle(row)}</strong>
                <time dateTime={String(row.at ?? '')}>{relativeTime(row.at)}</time>
              </header>
              {created ? null : <FieldDiff lines={diffLines(row.diff, defs.data)} />}
              {expanded ? (
                <div className="history-actions" onClick={(event) => event.stopPropagation()}>
                  <button
                    type="button"
                    className="btn-ghost"
                    data-testid="view-as-of"
                    onClick={() => {
                      setSeq(at)
                      setTravel(true)
                    }}
                  >
                    {copy.viewAsItWas}
                  </button>
                  <button type="button" className="btn-ghost" data-testid="rollback" onClick={() => rollback.mutate(at)}>{copy.rollback}</button>
                </div>
              ) : null}
            </article>
          )
        })}
        {rows.length === 0 ? <p>{copy.noRows}</p> : null}
      </div>
    </div>
  )
}

export { DataModel } from './data-model'

const LANGS = ['sv', 'en', 'de']

function isLanguageMap(value: unknown): value is Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const keys = Object.keys(value)
  return keys.length > 0 && keys.every((key) => LANGS.includes(key))
}

function attributeBy(defs: Json | undefined, key: string) {
  return ((defs?.attributes as Json[]) ?? []).find((item) => String(item.id) === key || String(item.key) === key)
}

function fieldLabel(defs: Json | undefined, key: string) {
  if (key === '$groups') return copy.group
  const attribute = attributeBy(defs, key)
  return textValue(attribute?.labels, uiLanguage) || String(attribute?.key ?? key)
}

function nearestSeq(seqs: number[], value: number) {
  if (seqs.length === 0) return value
  return seqs.reduce((best, seq) => (Math.abs(seq - value) < Math.abs(best - value) ? seq : best))
}

function formatWhen(value: unknown) {
  if (typeof value !== 'string' || !value) return ''
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat('en-GB', { dateStyle: 'medium', timeStyle: 'short' }).format(date)
}

function renderValue(value: unknown, field: string, defs: Json | undefined): ReactNode {
  if (value == null || value === '') return ''
  if (isLanguageMap(value)) {
    const present = LANGS.filter((language) => language in value && value[language] != null)
    return (
      <span className="lang-values">
        {present.map((language) => (
          <span key={language} className="lang-inline">
            <MetaChip>{language}</MetaChip>
            <span>{textValue(value[language], language)}</span>
          </span>
        ))}
      </span>
    )
  }
  const attribute = attributeBy(defs, field)
  const kind = (attribute?.kind as Json) ?? {}
  const kindType = String(kind.type ?? '')
  const attrKey = String(attribute?.key ?? field)
  if (Array.isArray(value)) {
    return (
      <span className="lang-inline">
        {value.map((item, index) => {
          const code = textValue(item)
          if (!code) return null
          if (kindType === 'value_list') {
            const list = String(kind.list ?? attrKey)
            return (
              <MetaChip key={`${code}-${index}`} tone={list === 'status' ? statusTone(code) : 'neutral'}>
                {optionLabel(defs, list, code, uiLanguage)}
              </MetaChip>
            )
          }
          return <MetaChip key={`${code}-${index}`}>{code}</MetaChip>
        })}
      </span>
    )
  }
  if (kindType === 'value_list') {
    const code = textValue(value)
    const list = String(kind.list ?? attrKey)
    return <MetaChip tone={list === 'status' ? statusTone(code) : 'neutral'}>{optionLabel(defs, list, code, uiLanguage)}</MetaChip>
  }
  if (field === 'type' || field === 'type_key' || attrKey === 'type' || attrKey === 'type_key') {
    return <MetaChip>{typeLabel(defs, textValue(value), uiLanguage)}</MetaChip>
  }
  if (kindType === 'file_ref' || kindType === 'object_ref') {
    const id = textValue(value)
    if (!id) return ''
    return <MetaChip>{kindType === 'file_ref' ? `${copy.file} ${id.slice(0, 8)}` : id.slice(0, 8)}</MetaChip>
  }
  if (kindType === 'date') {
    const text = textValue(value)
    return text ? <MetaChip>{text}</MetaChip> : ''
  }
  if (kindType === 'boolean') return <MetaChip>{value === true ? copy.yes : copy.no}</MetaChip>
  return textValue(value, uiLanguage)
}

export function JobHistory({ view }: { view: PanesView }) {
  const types = useQuery({
    queryKey: ['job-types'],
    queryFn: () => api<{ job_types: Json[] }>('/v1/job-types'),
  })
  const [key, setKey] = useState(String(view.state.type ?? ''))
  useEffect(() => {
    const first = types.data?.job_types[0]?.key
    if (!key && first) setKey(String(first))
  }, [types.data, key])
  const runs = useQuery({
    queryKey: ['jobs', key],
    queryFn: () => api<{ runs: Json[] }>(`/v1/job-types/${encodeURIComponent(key)}/runs`),
    enabled: Boolean(key),
  })
  return (
    <div data-testid="jobs">
      <div className="toolbar">
        {(types.data?.job_types ?? []).map((item) => (
          <button key={String(item.key)} className={item.key === key ? 'primary' : ''} onClick={() => setKey(String(item.key))}>
            {jobTypeDisplayTitle(String(item.title))}
          </button>
        ))}
      </div>
      {(runs.data?.runs ?? []).length === 0 ? <p>{copy.noRows}</p> : null}
      {(runs.data?.runs ?? []).map((row) => (
        <article key={String(row.id)}>
          <MetaChip tone={statusTone(String(row.status))}>{String(row.status)}</MetaChip> {String(row.progress_done ?? '')}/{String(row.progress_total ?? '')} {String(row.log ?? '')}
        </article>
      ))}
    </div>
  )
}

export function ProposalView({ session }: { session: PanesSession }) {
  const queryClient = useQueryClient()
  const list = useQuery({
    queryKey: ['proposals'],
    queryFn: () => api<Json>('/v1/proposals'),
  })
  const proposal = useQuery({
    queryKey: ['proposal', session.proposalId],
    queryFn: () => api<Json>(`/v1/proposals/${session.proposalId}`),
    enabled: Boolean(session.proposalId),
  })
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const [dropped, setDropped] = useState<string[]>([])
  useEffect(() => {
    setDropped([])
  }, [session.proposalId])
  useEffect(() => {
    if (session.proposalId) return
    const rows = (list.data?.proposals as Json[] | undefined) ?? []
    const first = rows.find((row) => row.status === 'open') ?? rows[0]
    if (first) session.setProposalId(String(first.id))
  }, [list.data, session])
  const rows = (proposal.data?.rows as Json[]) ?? []
  const objectRows = rows.filter((row) => row.object_id && row.type_key !== 'model' && row.type_key !== 'assortment')
  const loaded = useQueries({
    queries: objectRows.map((row) => ({
      queryKey: ['object', String(row.object_id)],
      queryFn: () => api<Json>(`/v1/objects/${String(row.object_id)}`),
    })),
  })
  const objects = new Map(objectRows.map((row, index) => [String(row.object_id), loaded[index]?.data]))
  const loadingObjects = loaded.some((query) => query.isLoading)
  const accept = useMutation({
    mutationFn: (body: { rows: string[]; patches: Record<string, Json> }) =>
      api<Json>(`/v1/proposals/${session.proposalId}/accept`, {
        method: 'POST',
        body: JSON.stringify(body),
      }),
    onSuccess: (result) => {
      const failed = (result.failed as Json[] | undefined) ?? []
      session.push(failed.length > 0 ? String(failed[0]?.detail ?? copy.validation) : copy.saved)
      setDropped([])
      void queryClient.invalidateQueries({ queryKey: ['proposals'] })
      void queryClient.invalidateQueries({ queryKey: ['proposal'] })
      void queryClient.invalidateQueries({ queryKey: ['object'] })
      void queryClient.invalidateQueries({ queryKey: ['objects'] })
    },
  })
  const discard = useMutation({
    mutationFn: () => api(`/v1/proposals/${session.proposalId}/discard`, { method: 'POST' }),
    onSuccess: () => {
      session.push(copy.discard)
      session.setProposalId(null)
      void queryClient.invalidateQueries({ queryKey: ['proposals'] })
    },
  })
  const proposals = (list.data?.proposals as Json[] | undefined) ?? []
  if (!session.proposalId) {
    return (
      <div data-testid="proposal">
        {proposals.map((row) => (
          <button key={String(row.id)} onClick={() => session.setProposalId(String(row.id))}>
            {String(row.title)} · <MetaChip tone={statusTone(String(row.status))}>{String(row.status)}</MetaChip> · {sourceLabel(row)}
          </button>
        ))}
        {proposals.length === 0 ? <p>{copy.proposals}</p> : null}
      </div>
    )
  }
  const consequence = ((proposal.data?.consequence as Json | undefined)?.lines as unknown[] | undefined)?.map(String) ?? []
  const droppedIds = new Set(dropped)
  const saveAll = () => {
    accept.mutate(acceptSelection(rows, objects, defs.data, droppedIds))
  }
  return (
    <div data-testid="proposal">
      <div className="proposal-switch">
        {proposals.map((row) => (
          <button key={String(row.id)} type="button" className={String(row.id) === session.proposalId ? 'primary' : ''} onClick={() => session.setProposalId(String(row.id))}>
            {String(row.title)}
          </button>
        ))}
      </div>
      <div className="toolbar">
        <strong>{String(proposal.data?.title ?? '')}</strong>
        {proposal.data ? <span className="chip" data-testid="source-badge">{sourceLabel(proposal.data)}</span> : null}
        {proposal.data?.status ? <MetaChip tone={statusTone(String(proposal.data.status))}>{String(proposal.data.status)}</MetaChip> : null}
        <button type="button" data-testid="save-all" disabled={loadingObjects || accept.isPending} onClick={saveAll}>{copy.saveAll}</button>
        <button type="button" data-testid="discard-all" onClick={() => discard.mutate()}>{copy.discardAll}</button>
        {consequence.length > 0 && session.proposalId ? (
          <button type="button" onClick={() => publishProposal(session.proposalId ?? '')}>{copy.reviewChange}</button>
        ) : null}
      </div>
      {consequence.length > 0 ? (
        <ul className="proposal-tree" data-testid="proposal-tree">
          {consequence.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      ) : null}
      {rows.map((row) => (
        <SuggestionCard
          key={String(row.id)}
          row={row}
          current={(objects.get(String(row.object_id))?.values as Json | undefined) ?? {}}
          defs={defs.data}
          dropped={droppedIds}
          onToggle={(lineId) =>
            setDropped((current) =>
              current.includes(lineId) ? current.filter((id) => id !== lineId) : [...current, lineId],
            )
          }
        />
      ))}
    </div>
  )
}

function SuggestionCard({
  row,
  current,
  defs,
  dropped,
  onToggle,
}: {
  row: Json
  current: Json
  defs: Json | undefined
  dropped: Set<string>
  onToggle: (lineId: string) => void
}) {
  const model = row.type_key === 'model' || row.type_key === 'assortment'
  const lines = model ? [] : suggestionLines(current, row.patch, defs)
  const pending = row.status === 'pending'
  return (
    <article className="suggestion-card" data-testid="suggestion-card">
      <h3>
        {model ? <MetaChip>{String((row.patch as Json | undefined)?.op ?? row.type_key ?? 'model')}</MetaChip> : String(row.object_key ?? row.object_id)}
        {' '}
        <MetaChip tone={statusTone(String(row.status))}>{String(row.status)}</MetaChip>
        {row.conflict ? ` · ${copy.conflict}` : ''}
      </h3>
      {lines.map((line) => {
        const id = `${String(row.id)}:${line.id}`
        const rejected = dropped.has(id)
        return (
          <div key={id} className="suggestion-field" data-testid="suggestion-field" data-line={line.id}>
            <FieldDiff lines={[line]} />
            {pending ? (
              <>
                <button type="button" data-testid="accept-field" aria-pressed={!rejected} onClick={() => (rejected ? onToggle(id) : undefined)}>{copy.acceptField}</button>
                <button type="button" data-testid="reject-field" aria-pressed={rejected} onClick={() => (rejected ? undefined : onToggle(id))}>{copy.rejectField}</button>
              </>
            ) : null}
          </div>
        )
      })}
    </article>
  )
}

function acceptSelection(
  rows: Json[],
  objects: Map<string, Json | undefined>,
  defs: Json | undefined,
  dropped: Set<string>,
): { rows: string[]; patches: Record<string, Json> } {
  const ids: string[] = []
  const patches: Record<string, Json> = {}
  for (const row of rows) {
    if (row.status !== 'pending') continue
    const id = String(row.id)
    ids.push(id)
    if (row.type_key === 'model' || row.type_key === 'assortment') continue
    const current = (objects.get(String(row.object_id))?.values as Json | undefined) ?? {}
    const lines = suggestionLines(current, row.patch, defs)
    if (lines.length === 0) continue
    const kept = lines.filter((line) => !dropped.has(`${id}:${line.id}`))
    if (kept.length === lines.length) continue
    patches[id] = kept.length === 0 ? { values: {} } : { values: valuesFromKept(current, proposedValues(row.patch), lines, kept) }
  }
  return { rows: ids, patches }
}

function suggestionIndex(body: Json | undefined): Map<string, number> {
  const pending = (body?.pending as Json | undefined) ?? {}
  const objects = (pending.objects as Json[] | undefined) ?? []
  return new Map(objects.map((row) => [String(row.object_id), Number(row.count ?? 0)]))
}
