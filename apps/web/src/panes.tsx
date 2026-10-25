import { useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { useVirtualizer } from '@tanstack/react-virtual'
import { ApiError, api, textValue, type Json } from './api'
import { Kbd } from './components/ui/kbd'
import { assortmentDisplayName, jobTypeDisplayTitle } from './demo-labels'
import { onFlash } from './flash'
import { GraphMap } from './graph-map'
import { copy, uiLanguage } from './i18n'
import { publishProposal } from './proposal-bus'
import { StateNote } from './states'
import { dispatchUi, onUi } from './ui-bus'

export type PanesSession = {
  group: string | null
  objectId: string | null
  proposalId: string | null
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
  { id: 'key', header: 'Number', size: 160, sort: 'a_nummer' },
  { id: 'name', header: 'Name', size: 280, sort: 'a_namn__sv' },
  { id: 'type', header: 'Type', size: 90, sort: 'type_key' },
  { id: 'status', header: 'Status', size: 96, sort: 'a_status' },
  { id: 'version', header: 'Version', size: 72, sort: 'version' },
]

const pageSize = 100

function savedTable(view: PanesView) {
  const table = (view.state.table as Json | undefined) ?? {}
  return {
    filter: String(table.filter ?? ''),
    sort: String(table.sort ?? 'a_nummer:asc'),
    groupBy: String(table.groupBy ?? ''),
    order: Array.isArray(table.order) ? table.order.map(String) : columns.map((column) => column.id),
    sizes: (table.sizes as Record<string, number> | undefined) ?? {},
  }
}

function toRow(doc: Json): ObjRow {
  return {
    id: String(doc.id ?? ''),
    key: String(doc.key ?? ''),
    type: String(doc.type_key ?? ''),
    version: String(doc.version ?? ''),
    name: textValue(doc.a_namn__sv),
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
  const [cursor, setCursor] = useState(-1)
  const [menu, setMenu] = useState<{ x: number; y: number; id: string } | null>(null)
  const [flashId, setFlashId] = useState('')
  const queryClient = useQueryClient()
  const parent = useRef<HTMLDivElement>(null)
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

  const filterBy = session.group ? `groups:=${session.group}` : ''
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

  const ordered = order.filter((id) => columns.some((column) => column.id === id))
  const shown = (ordered.length > 0 ? ordered : columns.map((column) => column.id)).filter((id) => !hidden.includes(id))
  const exportVisible = () => {
    void api<Blob>('/v1/export', {
      method: 'POST',
      body: JSON.stringify({ q: queryText || '*', filter: filterBy, columns: ['key', 'type_key', 'a_namn__sv', 'a_status'] }),
    }).then((blob) => {
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      link.download = 'braian.xlsx'
      link.click()
      URL.revokeObjectURL(url)
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
      session.setObjectId(String(args.id))
      setSelected([String(args.id)])
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
  }), [filterBy, queryText, session])

  const moveCursor = (next: number, extend: boolean) => {
    const clamped = Math.max(0, Math.min(Math.max(found - 1, 0), next))
    setCursor(clamped)
    virtualizer.scrollToIndex(clamped)
    const row = byIndex.get(clamped)
    if (!row) return
    session.setObjectId(row.id)
    if (!extend) {
      setSelected([row.id])
      return
    }
    const [start, end] = cursor < clamped ? [cursor, clamped] : [clamped, Math.max(cursor, 0)]
    const ids: string[] = []
    for (let index = start; index <= end; index += 1) {
      const item = byIndex.get(index)
      if (item) ids.push(item.id)
    }
    setSelected(ids.length > 0 ? ids : [row.id])
  }

  const onTableKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.ctrlKey || event.altKey || event.metaKey) return
    if (!['ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', 'Home', 'End', 'Enter', 'Escape'].includes(event.key)) return
    if (event.key === 'Escape') {
      setMenu(null)
      return
    }
    event.preventDefault()
    if (event.key === 'Enter') {
      const row = byIndex.get(cursor)
      if (row) session.setObjectId(row.id)
      return
    }
    const next = event.key === 'ArrowDown'
      ? cursor + 1
      : event.key === 'ArrowUp'
        ? cursor - 1
        : event.key === 'PageDown'
          ? cursor + 10
          : event.key === 'PageUp'
            ? cursor - 10
            : event.key === 'Home'
              ? 0
              : found - 1
    moveCursor(next, event.shiftKey)
  }

  const cell = (row: ObjRow, id: string) => {
    if (id === 'key') return row.key
    if (id === 'name') return row.name
    if (id === 'type') return row.type
    if (id === 'status') return row.status
    return row.version
  }

  return (
    <div className="table-wrap" data-testid="object-table">
      <div className="toolbar">
        <input
          data-testid="quick-filter"
          placeholder={copy.searchPlaceholder}
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
        <label>
          {copy.groupBy}
          <select value={groupBy} onChange={(event) => setGroupBy(event.target.value)}>
            <option value="">—</option>
            <option value="type_key">type</option>
            <option value="a_status">status</option>
          </select>
        </label>
        <button
          type="button"
          onClick={() => {
            setFilter('')
            session.setGroup(null)
          }}
        >
          {copy.all}
        </button>
        {(assortments.data?.assortments ?? []).map((item) => (
          <button
            key={String(item.id)}
            data-testid="assortment"
            onClick={() => {
              const query = (item.query as Json) ?? {}
              setFilter(String(query.text ?? ''))
              session.setGroup(query.group ? String(query.group) : null)
            }}
          >
            {assortmentDisplayName(String(item.name))}
          </button>
        ))}
        <button onClick={() => mass.mutate()} disabled={selected.length === 0}>{copy.massEdit}</button>
        <button onClick={exportVisible}>{copy.export}</button>
        <NewObject session={session} />
        <span className="icon-btn" data-testid="row-count">{found} {copy.rows}</span>
      </div>
      <div className="grid" ref={parent} tabIndex={0} onKeyDown={onTableKey}>
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
          <div>
            {groups.map((group) => (
              <div key={group.key}>
                <strong>{group.key || '—'} ({group.found})</strong>
                {group.hits.map((row) => (
                  <Row key={row.id} row={row} index={0} shown={shown} sizes={sizes} session={session} selected={selected} setSelected={setSelected} setMenu={setMenu} cell={cell} flash={flashId === row.id} cursor={cursor} />
                ))}
              </div>
            ))}
          </div>
        ) : (
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
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
                  session={session}
                  selected={selected}
                  setSelected={setSelected}
                  setMenu={setMenu}
                  cell={cell}
                  flash={flashId === row.id}
                  cursor={cursor}
                  style={{ position: 'absolute', transform: `translateY(${item.start}px)`, width: '100%' }}
                />
              )
            })}
          </div>
        )}
      </div>
      {probe.isError ? <p className="error">{(probe.error as Error).message}</p> : null}
      {!probe.isLoading && found === 0 ? <p>{copy.noRows}</p> : null}
      {menu ? (
        <div className="menu" style={{ left: menu.x, top: menu.y }} data-testid="context-menu" role="menu">
          <button type="button" onClick={() => { session.setObjectId(menu.id); setMenu(null) }}>{copy.open} <Kbd>Enter</Kbd></button>
          <button type="button" onClick={() => { dispatchUi('ui.place', { title: 'Detail', zone: 'right' }); setMenu(null) }}>{copy.placeRight}</button>
          <button type="button" onClick={() => { dispatchUi('ui.open_view', { title: 'History' }); setMenu(null) }}>{copy.history}</button>
          <button type="button" onClick={() => { setSelected((current) => current.includes(menu.id) ? current : [...current, menu.id]); setMenu(null) }}>{copy.selectAll}</button>
          <button type="button" onClick={() => { setHidden((current) => [...new Set([...current, 'status'])]); setMenu(null) }}>{copy.hideColumn}</button>
          <button type="button" onClick={() => { mass.mutate(); setMenu(null) }}>{copy.massEdit}</button>
          <button type="button" onClick={() => { exportVisible(); setMenu(null) }}>{copy.export}</button>
          <button type="button" onClick={() => setMenu(null)}>{copy.close} <Kbd>Esc</Kbd></button>
        </div>
      ) : null}
    </div>
  )
}

function Row({
  row,
  index,
  shown,
  sizes,
  session,
  selected,
  setSelected,
  setMenu,
  cell,
  flash,
  cursor,
  style,
}: {
  row: ObjRow
  index: number
  shown: string[]
  sizes: Record<string, number>
  session: PanesSession
  selected: string[]
  setSelected: (value: string[] | ((current: string[]) => string[])) => void
  setMenu: (value: { x: number; y: number; id: string } | null) => void
  cell: (row: ObjRow, id: string) => string
  flash: boolean
  cursor: number
  style?: CSSProperties
}) {
  const marked = session.objectId === row.id || selected.includes(row.id) || cursor === index
  return (
    <div
      data-testid="object-row"
      data-index={index}
      tabIndex={-1}
      className={marked ? `row selected${flash ? ' flash' : ''}` : flash ? 'row flash' : 'row'}
      style={style}
      onClick={(event) => {
        session.setObjectId(row.id)
        if (event.shiftKey) setSelected((current) => current.includes(row.id) ? current : [...current, row.id])
        else if (event.metaKey || event.ctrlKey) setSelected((current) => current.includes(row.id) ? current.filter((id) => id !== row.id) : [...current, row.id])
        else setSelected([row.id])
      }}
      onContextMenu={(event) => {
        event.preventDefault()
        session.setObjectId(row.id)
        setMenu({ x: event.clientX, y: event.clientY, id: row.id })
      }}
    >
      {shown.map((id) => {
        const column = columns.find((item) => item.id === id)
        return (
          <div key={id} className="cell" style={{ width: sizes[id] ?? column?.size ?? 120 }}>
            {cell(row, id)}
          </div>
        )
      })}
    </div>
  )
}

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

function NewObject({ session }: { session: PanesSession }) {
  const [open, setOpen] = useState(false)
  const [nummer, setNummer] = useState('')
  const [error, setError] = useState('')
  const queryClient = useQueryClient()
  if (!open) return <button onClick={() => setOpen(true)}>{copy.newObject}</button>
  return (
    <form
      className="toolbar"
      onSubmit={(event) => {
        event.preventDefault()
        const group = session.group?.split(':')[1]
        void api<Json>('/v1/objects', {
          method: 'POST',
          body: JSON.stringify({
            type: 'artikel',
            groups: group ? { sortiment: group } : { sortiment: 'friluft' },
            values: { nummer, namn: { sv: nummer, en: nummer, de: nummer }, status: 'utkast' },
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

export function ObjectDetail({ session }: { session: PanesSession }) {
  const queryClient = useQueryClient()
  const object = useQuery({
    queryKey: ['object', session.objectId],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}`),
    enabled: Boolean(session.objectId),
  })
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const [draft, setDraft] = useState<Json>({})
  const [error, setError] = useState('')
  const [pendingId, setPendingId] = useState<string | null>(null)
  const [flashId, setFlashId] = useState('')
  useEffect(() => onFlash(setFlashId), [])
  useEffect(() => {
    setDraft({})
    setPendingId(null)
    setError('')
  }, [session.objectId])
  if (!session.objectId) {
    return (
      <div className="detail-empty" data-testid="detail-empty">
        <p>{copy.detailEmpty}</p>
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
  const label = textValue((object.data.values as Json)?.nummer) || String(object.data.id ?? '')
  const save = () => {
    if (Object.keys(draft).length === 0) return
    void api<Json>('/v1/proposals', {
      method: 'POST',
      body: JSON.stringify({
        title: `${copy.save} ${label}`,
        source: 'user',
        rows: [
          {
            object_id: session.objectId,
            object_key: label,
            type_key: String(object.data?.type ?? ''),
            base_version: Number(object.data?.version ?? 0),
            patch: { values: draft },
          },
        ],
      }),
    })
      .then((proposal) => {
        setPendingId(String(proposal.id))
        setError('')
      })
      .catch((reason: unknown) => setError(reason instanceof ApiError ? reason.message : copy.validation))
  }
  const confirm = () => {
    if (!pendingId) return
    void api<Json>(`/v1/proposals/${pendingId}/accept`, { method: 'POST', body: '{}' })
      .then((result) => {
        const failed = (result.failed as Json[] | undefined) ?? []
        if (failed.length > 0) {
          setError(String(failed[0]?.detail ?? copy.validation))
          return
        }
        setDraft({})
        setPendingId(null)
        setError('')
        session.setProposalId(pendingId)
        session.push(`${copy.saved} ${label}`)
        void queryClient.invalidateQueries({ queryKey: ['object', session.objectId] })
        void queryClient.invalidateQueries({ queryKey: ['objects'] })
      })
      .catch((reason: unknown) => setError(reason instanceof ApiError ? reason.message : copy.validation))
  }
  return (
    <form
      className={flashId === session.objectId ? 'form flash' : 'form'}
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
        const fields = [...event.currentTarget.querySelectorAll<HTMLElement>('input, textarea, select')]
        const index = fields.indexOf(target)
        if (event.key === 'Escape') {
          setDraft({})
          return
        }
        if (index < 0) return
        event.preventDefault()
        const next = event.key === 'ArrowDown' ? index + 1 : event.key === 'ArrowUp' ? index - 1 : event.key === 'Home' ? 0 : fields.length - 1
        fields[Math.max(0, Math.min(fields.length - 1, next))]?.focus()
      }}
    >
      <div className="toolbar field-chips">
        <span className="chip">{String(object.data.type)}</span>
        {textValue(values.status) ? <span className="chip">{textValue(values.status)}</span> : null}
        <strong>{textValue((object.data.values as Json)?.nummer)}</strong>
      </div>
      {Object.keys(values).map((key) => {
        const attribute = attributes.find((item) => item.key === key)
        const kind = (attribute?.kind as Json) ?? {}
        const label = textValue((attribute?.labels as Json) ?? key, uiLanguage) || key
        return (
          <div key={key} className="field">
            <label>{label}</label>
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
      })}
      {error ? <p className="error" data-testid="detail-error">{error}</p> : null}
      {pendingId ? (
        <div className="toolbar">
          <span className="hint">{copy.reviewChange}</span>
          <button type="button" onClick={() => { setPendingId(null); setDraft({}) }}>{copy.cancel}</button>
          <button type="button" className="primary" data-testid="confirm-proposal" onClick={confirm}>{copy.confirm}</button>
        </div>
      ) : (
        <div className="toolbar">
          <button className="primary" type="submit" data-testid="save-object">{copy.save}</button>
          <button type="button" onClick={() => setDraft({})}>{copy.discard}</button>
        </div>
      )}
    </form>
  )
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
          <label key={language} className="lang">
            <b>{language}</b>
            {form === 'long' ? (
              <textarea value={textValue(record[language])} onChange={(event) => onChange({ ...record, [language]: event.target.value })} />
            ) : (
              <input value={textValue(record[language])} onChange={(event) => onChange({ ...record, [language]: event.target.value })} />
            )}
          </label>
        ))}
      </div>
    )
  }
  if (multiple && Array.isArray(value) && kind !== 'value_list') {
    const items = value.map((item) => textValue(item))
    return (
      <div className="choices">
        {items.map((item) => <span key={item} className="chip">{item}</span>)}
        <input
          value=""
          onChange={(event) => {
            const next = event.target.value.trim()
            if (next) onChange([...items, next])
          }}
        />
      </div>
    )
  }
  if (kind === 'object_ref') return <ObjectRefField value={value} onChange={onChange} />
  if (kind === 'value_list' && options.length > 0 && multiple) {
    const picked = Array.isArray(value) ? value.map(String) : []
    return (
      <div className="choices">
        {options.map((option) => (
          <label key={option.code}>
            <input
              type="checkbox"
              checked={picked.includes(option.code)}
              onChange={(event) =>
                onChange(event.target.checked ? [...picked, option.code] : picked.filter((code) => code !== option.code))
              }
            />
            {option.label}
          </label>
        ))}
      </div>
    )
  }
  if (kind === 'value_list' && options.length > 0) {
    return (
      <select value={textValue(value)} onChange={(event) => onChange(event.target.value)}>
        <option value="" />
        {options.map((option) => (
          <option key={option.code} value={option.code}>{option.label}</option>
        ))}
      </select>
    )
  }
  if (kind === 'boolean') {
    return <input type="checkbox" checked={value === true} onChange={(event) => onChange(event.target.checked)} />
  }
  if (kind === 'range') {
    const record = (value && typeof value === 'object' ? value : {}) as Json
    return (
      <span className="measure">
        <input value={textValue(record.min)} onChange={(event) => onChange({ ...record, min: event.target.value })} />
        <input value={textValue(record.max)} onChange={(event) => onChange({ ...record, max: event.target.value })} />
        {units.length > 0 ? (
          <select value={String(record.unit ?? units[0])} onChange={(event) => onChange({ ...record, unit: event.target.value })}>
            {units.map((unit) => <option key={unit}>{unit}</option>)}
          </select>
        ) : null}
      </span>
    )
  }
  if (kind === 'number') {
    const record = (value && typeof value === 'object' ? value : { value }) as Json
    return (
      <span className="measure">
        <input value={textValue(record.value)} onChange={(event) => onChange({ ...record, value: event.target.value })} />
        {units.length > 0 ? (
          <select value={String(record.unit ?? units[0])} onChange={(event) => onChange({ ...record, unit: event.target.value })}>
            {units.map((unit) => <option key={unit}>{unit}</option>)}
          </select>
        ) : null}
      </span>
    )
  }
  if (form === 'long' || (typeof value === 'string' && value.length > 80)) {
    return <textarea value={textValue(value)} onChange={(event) => onChange(event.target.value)} />
  }
  return <input value={textValue(value)} onChange={(event) => onChange(event.target.value)} />
}

function ObjectRefField({ value, onChange }: { value: unknown; onChange: (value: unknown) => void }) {
  const id = typeof value === 'string' ? value : ''
  const current = useQuery({
    queryKey: ['object', id],
    queryFn: () => api<Json>(`/v1/objects/${id}`),
    enabled: /^[0-9a-f-]{36}$/i.test(id),
  })
  const label = current.data
    ? `${textValue((current.data.values as Json)?.nummer)} · ${textValue((current.data.values as Json)?.namn)}`
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
                {String(doc.key)} · {textValue(doc.a_namn__sv)}
              </button>
            )
          })}
        </div>
      ) : null}
    </div>
  )
}

export function GraphView({ session }: { session: PanesSession }) {
  const graph = useQuery({
    queryKey: ['graph', session.objectId],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}/graph`),
    enabled: Boolean(session.objectId),
  })
  if (!session.objectId) return <StateNote tone="empty" title={copy.graphEmpty} testId="graph" />
  if (graph.isError) return <StateNote tone="error" title={copy.graphError} testId="graph" />
  if (!graph.data) return <StateNote tone="loading" title={copy.graphLoading} testId="graph" />
  return <GraphMap data={graph.data} onOpen={(id) => { if (id) session.setObjectId(id) }} />
}

export function HistoryView({ session }: { session: PanesSession }) {
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const changes = useQuery({
    queryKey: ['history', session.objectId],
    queryFn: () => api<Json[]>(`/v1/objects/${session.objectId}/changes`),
    enabled: Boolean(session.objectId),
  })
  const [seq, setSeq] = useState(0)
  const rows = changes.data ?? []
  const current = seq || Number(rows.at(-1)?.seq ?? 0)
  const asOf = useQuery({
    queryKey: ['asof', session.objectId, current],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}/as-of?seq=${current}`),
    enabled: Boolean(session.objectId && current),
  })
  const rollback = useMutation({
    mutationFn: () =>
      api<Json>(`/v1/objects/${session.objectId}/rollback`, {
        method: 'POST',
        body: JSON.stringify({ seq: current }),
      }),
    onSuccess: (proposal) => session.setProposalId(String(proposal.id)),
  })
  const names = useMemo(() => {
    const map = new Map<string, string>()
    for (const attribute of (defs.data?.attributes as Json[] | undefined) ?? []) {
      map.set(String(attribute.id), String(attribute.key))
    }
    return map
  }, [defs.data])
  const selected = rows.find((row) => Number(row.seq) === current) ?? rows.at(-1)
  const diff = (selected?.diff as Json | undefined) ?? {}
  return (
    <div data-testid="history">
      <p className="hint">{copy.asOf} at #{current}</p>
      <label>
        {copy.asOf}
        <input
          data-testid="as-of"
          type="range"
          min={Number(rows[0]?.seq ?? 0)}
          max={Number(rows.at(-1)?.seq ?? 0)}
          value={current}
          onChange={(event) => setSeq(Number(event.target.value))}
        />
      </label>
      <button data-testid="rollback" onClick={() => rollback.mutate()}>{copy.rollback}</button>
      <div className="timeline">
        {rows.map((row) => (
          <article key={String(row.seq)} className={Number(row.seq) === current ? 'selected' : ''}>
            <strong>#{String(row.seq)}</strong> {String(row.op)} · {String(row.actor ?? '')}
            {row.on_behalf_of ? ` for ${String(row.on_behalf_of)}` : ''}
          </article>
        ))}
      </div>
      <table className="diff" data-testid="field-diff">
        <thead>
          <tr>
            <th />
            <th>{copy.before}</th>
            <th>{copy.after}</th>
          </tr>
        </thead>
        <tbody>
          {Object.entries(diff).map(([key, change]) => {
            const pair = (change as Json) ?? {}
            if (key === '$groups') {
              const before = groupText(pair, 'before', defs.data)
              const after = groupText(pair, 'after', defs.data)
              return (
                <tr key={key}>
                  <th>grupp</th>
                  <td>{before}</td>
                  <td>{after}</td>
                </tr>
              )
            }
            const label = names.get(key) ?? key
            return (
              <tr key={key}>
                <th>{label}</th>
                <td>{textValue(pair.before)}</td>
                <td>{textValue(pair.after)}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
      <dl className="asof">
        {Object.entries((asOf.data?.values as Json | undefined) ?? {}).map(([key, value]) => (
          <div key={key}>
            <dt>{key}</dt>
            <dd>{textValue(value)}</dd>
          </div>
        ))}
      </dl>
    </div>
  )
}

export { DataModel } from './data-model'

function groupText(groups: Json, side: 'before' | 'after', defs: Json | undefined) {
  const names = new Map<string, string>()
  for (const structure of (defs?.structures as Json[] | undefined) ?? []) {
    for (const group of (structure.groups as Json[] | undefined) ?? []) names.set(String(group.id), String(group.key))
  }
  return Object.values(groups)
    .map((entry) => names.get(String((entry as Json)[side] ?? '')) ?? textValue((entry as Json)[side]))
    .filter(Boolean)
    .join(', ')
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
          {String(row.status)} {String(row.progress_done ?? '')}/{String(row.progress_total ?? '')} {String(row.log ?? '')}
        </article>
      ))}
    </div>
  )
}

export function ProposalView({ session }: { session: PanesSession }) {
  const list = useQuery({
    queryKey: ['proposals'],
    queryFn: () => api<{ proposals: Json[] }>('/v1/proposals'),
  })
  const proposal = useQuery({
    queryKey: ['proposal', session.proposalId],
    queryFn: () => api<Json>(`/v1/proposals/${session.proposalId}`),
    enabled: Boolean(session.proposalId),
  })
  const [picked, setPicked] = useState<string[]>([])
  useEffect(() => {
    if (session.proposalId) return
    const rows = list.data?.proposals ?? []
    const first = rows.find((row) => row.status === 'open') ?? rows[0]
    if (first) session.setProposalId(String(first.id))
  }, [list.data, session])
  const accept = useMutation({
    mutationFn: (rows?: string[]) =>
      api(`/v1/proposals/${session.proposalId}/accept`, {
        method: 'POST',
        body: JSON.stringify(rows && rows.length > 0 ? { rows } : {}),
      }),
    onSuccess: () => session.push(copy.acceptAll),
  })
  if (!session.proposalId) {
    return (
      <div data-testid="proposal">
        {(list.data?.proposals ?? []).map((row) => (
          <button key={String(row.id)} onClick={() => session.setProposalId(String(row.id))}>
            {String(row.title)} · {String(row.status)} · {String(row.source)}
          </button>
        ))}
        {(list.data?.proposals ?? []).length === 0 ? <p>{copy.proposals}</p> : null}
      </div>
    )
  }
  const rows = (proposal.data?.rows as Json[]) ?? []
  const lines = ((proposal.data?.consequence as Json | undefined)?.lines as unknown[] | undefined)?.map(String) ?? []
  return (
    <div data-testid="proposal">
      <div className="toolbar">
        {(list.data?.proposals ?? []).map((row) => (
          <button key={String(row.id)} type="button" className={String(row.id) === session.proposalId ? 'primary' : ''} onClick={() => session.setProposalId(String(row.id))}>
            {String(row.title)}
          </button>
        ))}
      </div>
      <div className="toolbar">
        <strong className={proposal.data?.source === 'agent' ? 'draft-agent' : 'draft-user'}>{String(proposal.data?.title ?? '')}</strong>
        {proposal.data?.source ? <span className="chip">{String(proposal.data.source)}</span> : null}
        <button data-testid="accept-all" onClick={() => accept.mutate(undefined)}>{copy.acceptAll}</button>
        <button onClick={() => accept.mutate(picked)}>{copy.acceptSelected}</button>
        {lines.length > 0 && session.proposalId ? (
          <button type="button" onClick={() => publishProposal(session.proposalId ?? '')}>{copy.reviewChange}</button>
        ) : null}
      </div>
      {lines.length > 0 ? (
        <ul className="proposal-tree" data-testid="proposal-tree">
          {lines.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      ) : null}
      {rows.map((row) => (
        <label key={String(row.id)} className={row.conflict ? 'error' : row.status === 'pending' ? 'draft-agent' : ''}>
          <input
            type="checkbox"
            checked={picked.includes(String(row.id))}
            onChange={(event) =>
              setPicked((current) =>
                event.target.checked ? [...current, String(row.id)] : current.filter((id) => id !== String(row.id)),
              )
            }
          />
          {String(row.type_key) === 'model' ? String((row.patch as Json | undefined)?.op ?? 'model') : String(row.object_key ?? row.object_id)} · {String(row.status)}
          {row.conflict ? ` · ${copy.conflict}` : ''}
        </label>
      ))}
    </div>
  )
}
