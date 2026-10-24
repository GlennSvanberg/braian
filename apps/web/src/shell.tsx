import { useEffect, useMemo, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  columnFilteringFeature,
  columnGroupingFeature,
  columnOrderingFeature,
  columnResizingFeature,
  columnSizingFeature,
  columnVisibilityFeature,
  createFilteredRowModel,
  createGroupedRowModel,
  createSortedRowModel,
  filterFn_includesString,
  rowSelectionFeature,
  rowSortingFeature,
  sortFn_text,
  tableFeatures,
  useTable,
} from '@tanstack/react-table'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useChat, fetchServerSentEvents } from '@tanstack/ai-react'
import { barY, defineChart } from '@tanstack/charts'
import { scaleBand } from '@tanstack/charts/scales/band'
import { scaleLinear } from '@tanstack/charts/scales/linear'
import { Chart } from '@tanstack/react-charts'
import { ApiError, api, textValue, type Json } from './api'
import { sv } from './i18n'
import { dockTab, isSplit, type LayoutNode, type Zone } from './layout'

const features = tableFeatures({
  columnVisibilityFeature,
  columnSizingFeature,
  columnResizingFeature,
  columnFilteringFeature,
  columnOrderingFeature,
  rowSortingFeature,
  rowSelectionFeature,
  columnGroupingFeature,
  sortedRowModel: createSortedRowModel(),
  filteredRowModel: createFilteredRowModel(),
  groupedRowModel: createGroupedRowModel(),
  sortFns: { text: sortFn_text },
  filterFns: { includesString: filterFn_includesString },
})

type View = { id: string; type: string; title: string; state: Json }
type Workspace = { id: string; name: string; locked: boolean; share_token?: string; layout: LayoutNode; views: View[] }
type Me = {
  email?: string
  role: string
  display_name?: string
  handle?: string
  user_id?: string
}
type ObjRow = { id: string; key: string; type: string; version: string; name: string; status: string }

type Session = {
  group: string | null
  objectId: string | null
  proposalId: string | null
  language: string
  activity: string[]
  setGroup: (value: string | null) => void
  setObjectId: (value: string | null) => void
  setProposalId: (value: string | null) => void
  push: (line: string) => void
  openWorkspace: (id: string) => void
}

function readStore<T>(key: string, fallback: T): T {
  if (typeof localStorage === 'undefined') return fallback
  try {
    const raw = localStorage.getItem(key)
    return raw ? (JSON.parse(raw) as T) : fallback
  } catch {
    return fallback
  }
}

function writeStore(key: string, value: unknown) {
  if (typeof localStorage === 'undefined') return
  localStorage.setItem(key, JSON.stringify(value))
}

export function Shell() {
  const queryClient = useQueryClient()
  const me = useQuery({ queryKey: ['me'], queryFn: () => api<Me>('/v1/me'), retry: false })
  const [theme, setTheme] = useState<'dark' | 'light'>('dark')
  const [booted, setBooted] = useState(false)
  const [workspaceId, setWorkspaceId] = useState<string | null>(null)
  const [group, setGroup] = useState<string | null>(null)
  const [objectId, setObjectId] = useState<string | null>(null)
  const [proposalId, setProposalId] = useState<string | null>(null)
  const logRef = useRef<HTMLPreElement>(null)
  const statusRef = useRef<HTMLSpanElement>(null)
  const [palette, setPalette] = useState(false)
  const [help, setHelp] = useState(false)
  const [agentOpen, setAgentOpen] = useState(false)
  const [zoom, setZoom] = useState(1)

  useEffect(() => {
    setBooted(true)
    const stored = readStore<string>('braian.theme', 'dark')
    setTheme(stored === 'light' ? 'light' : 'dark')
  }, [])
  useEffect(() => {
    document.documentElement.dataset.theme = theme
    writeStore('braian.theme', theme)
  }, [theme])

  const push = (line: string) => {
    const stamp = new Date().toLocaleTimeString('sv-SE', { hour12: false })
    const text = `${stamp} ${line}`
    if (statusRef.current) statusRef.current.textContent = text
    const log = logRef.current
    if (!log) return
    const lines = [text, ...(log.textContent ?? '').split('\n').filter(Boolean)].slice(0, 6)
    log.textContent = lines.join('\n')
  }

  const workspaces = useQuery({
    queryKey: ['workspaces'],
    queryFn: () => api<{ workspaces: Array<{ id: string; name: string }> }>('/v1/workspaces'),
    enabled: booted && me.isSuccess,
  })
  useEffect(() => {
    const first = workspaces.data?.workspaces[2]?.id ?? workspaces.data?.workspaces[0]?.id
    if (!workspaceId && first) setWorkspaceId(first)
  }, [workspaces.data, workspaceId])

  const workspace = useQuery({
    queryKey: ['workspace', workspaceId],
    queryFn: () => api<Workspace>(`/v1/workspaces/${workspaceId}`),
    enabled: Boolean(workspaceId),
  })

  useEffect(() => {
    if (!booted || !me.isSuccess) return
    const source = new EventSource('/v1/stream', { withCredentials: true })
    let pending: ReturnType<typeof setTimeout> | undefined
    source.onmessage = (event) => {
      push(`live ${event.data}`)
      clearTimeout(pending)
      pending = setTimeout(() => {
        void queryClient.invalidateQueries({ queryKey: ['objects'] })
        void queryClient.invalidateQueries({ queryKey: ['object'] })
        void queryClient.invalidateQueries({ queryKey: ['history'] })
      }, 200)
    }
    source.onerror = () => {
      if (source.readyState === EventSource.CLOSED) push(sv.offline)
    }
    return () => {
      clearTimeout(pending)
      source.close()
    }
  }, [booted, me.isSuccess, queryClient])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setPalette(true)
      }
      if (event.key === 'Escape') {
        setPalette(false)
        setHelp(false)
      }
      if (event.key === 'F1') {
        event.preventDefault()
        setHelp(true)
      }
      if (event.ctrlKey && event.key >= '1' && event.key <= '9') {
        const index = Number(event.key) - 1
        const id = workspaces.data?.workspaces[index]?.id
        if (id) {
          event.preventDefault()
          setWorkspaceId(id)
        }
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [workspaces.data])

  const session: Session = {
    group,
    objectId,
    proposalId,
    language: 'sv',
    activity: [],
    setGroup,
    setObjectId,
    setProposalId,
    push,
    openWorkspace: setWorkspaceId,
  }

  if (!booted || me.isLoading) return <p className="pane-body">{sv.loading}</p>
  if (me.isError || !me.data) return <Login onDone={() => void queryClient.invalidateQueries({ queryKey: ['me'] })} />
  const user = me.data
  const userName = user.display_name || user.handle || user.email || ''
  const userEmail = user.email || user.handle || ''

  const saveLayout = (layout: LayoutNode) => {
    if (!workspace.data || workspace.data.locked) return
    void api('/v1/workspaces', {
      method: 'POST',
      body: JSON.stringify({
        id: workspace.data.id,
        name: workspace.data.name,
        locked: false,
        share_token: workspace.data.share_token,
        layout,
      }),
    }).then(() => queryClient.invalidateQueries({ queryKey: ['workspace', workspaceId] }))
  }

  return (
    <div className="app" style={{ zoom }}>
      <header className="top">
        <div className="brand">{sv.product}</div>
        <nav className="tabs">
          {workspaces.data?.workspaces.map((item, index) => (
            <button
              key={item.id}
              className={item.id === workspaceId ? 'tab active' : 'tab'}
              onClick={() => setWorkspaceId(item.id)}
            >
              {item.name}
              <span className="icon-btn"> {index < 9 ? index + 1 : ''}</span>
            </button>
          ))}
        </nav>
        <span className="spacer" />
        <button className="icon-btn" onClick={() => setZoom((value) => Math.min(1.4, value + 0.1))}>{sv.zoom}</button>
        <button className="icon-btn" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}>
          {theme === 'dark' ? sv.themeLight : sv.themeDark}
        </button>
        <button className="icon-btn" onClick={() => setAgentOpen(true)}>{sv.agent}</button>
        <span>{userName}</span>
        <button
          className="icon-btn"
          onClick={() => void api('/v1/logout', { method: 'POST' }).then(() => queryClient.invalidateQueries({ queryKey: ['me'] }))}
        >
          {sv.signOut}
        </button>
      </header>
      <main className="workspace">
        {workspace.data ? (
          <LayoutView node={workspace.data.layout} workspace={workspace.data} session={session} onLayout={saveLayout} />
        ) : (
          <p>{sv.loading}</p>
        )}
      </main>
      <pre className="activity" data-testid="activity-log" ref={logRef} />
      <footer className="status">
        <span>{userEmail}</span>
        <span>{user.role}</span>
        <span data-testid="live-flag">{sv.connected}</span>
        {workspace.data?.locked ? <span>{sv.locked}</span> : null}
        <span className="grow" />
        <span ref={statusRef}>{sv.activity}</span>
      </footer>
      {palette ? <Palette session={session} onClose={() => setPalette(false)} onAsk={() => { setPalette(false); setAgentOpen(true) }} /> : null}
      {help ? <Help onClose={() => setHelp(false)} /> : null}
      {agentOpen ? <AgentPanel onClose={() => setAgentOpen(false)} session={session} /> : null}
    </div>
  )
}

function Login({ onDone }: { onDone: () => void }) {
  const [email, setEmail] = useState('admin@braian.local')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  return (
    <div className="login">
      <form
        onSubmit={(event) => {
          event.preventDefault()
          void api('/v1/login', { method: 'POST', body: JSON.stringify({ email, password }) })
            .then(onDone)
            .catch(() => setError(sv.loginFailed))
        }}
      >
        <strong>{sv.product}</strong>
        <h1>{sv.loginTitle}</h1>
        <label>{sv.email}<input data-testid="email" value={email} onChange={(event) => setEmail(event.target.value)} /></label>
        <label>{sv.password}<input data-testid="password" type="password" value={password} onChange={(event) => setPassword(event.target.value)} /></label>
        {error ? <p className="error">{error}</p> : <p>{sv.loginHint}</p>}
        <button className="primary" type="submit">{sv.signIn}</button>
      </form>
    </div>
  )
}

function LayoutView({
  node,
  workspace,
  session,
  onLayout,
}: {
  node: LayoutNode
  workspace: Workspace
  session: Session
  onLayout: (layout: LayoutNode) => void
}) {
  const split = isSplit(node)
  const [ratio, setRatio] = useState(split ? node.ratio : 0.5)
  const ratioRef = useRef(ratio)
  if (!split) {
    return <Pane leaf={node} workspace={workspace} session={session} onLayout={onLayout} root={workspace.layout} />
  }
  return (
    <div className={`split ${node.direction}`} style={{ alignItems: 'stretch' }}>
      <div style={{ flex: `${ratio} 1 0%`, alignSelf: 'stretch', minWidth: 0, minHeight: 0 }}>
        <LayoutView node={node.first} workspace={workspace} session={session} onLayout={onLayout} />
      </div>
      <div
        className="divider"
        onMouseDown={(event) => {
          if (workspace.locked) return
          const start = node.direction === 'row' ? event.clientX : event.clientY
          const parent = event.currentTarget.parentElement
          const size = node.direction === 'row' ? parent?.clientWidth ?? 1 : parent?.clientHeight ?? 1
          const base = ratioRef.current
          const move = (moveEvent: MouseEvent) => {
            const delta = (node.direction === 'row' ? moveEvent.clientX : moveEvent.clientY) - start
            const next = Math.min(0.85, Math.max(0.15, base + delta / size))
            ratioRef.current = next
            setRatio(next)
          }
          const up = () => {
            window.removeEventListener('mousemove', move)
            window.removeEventListener('mouseup', up)
            onLayout(replaceRatio(workspace.layout, node, ratioRef.current))
          }
          window.addEventListener('mousemove', move)
          window.addEventListener('mouseup', up)
        }}
      />
      <div style={{ flex: `${1 - ratio} 1 0%`, alignSelf: 'stretch', minWidth: 0, minHeight: 0 }}>
        <LayoutView node={node.second} workspace={workspace} session={session} onLayout={onLayout} />
      </div>
    </div>
  )
}

function replaceRatio(root: LayoutNode, target: LayoutNode, ratio: number): LayoutNode {
  if (root === target && isSplit(root)) return { ...root, ratio }
  if (!isSplit(root)) return root
  return { ...root, first: replaceRatio(root.first, target, ratio), second: replaceRatio(root.second, target, ratio) }
}

function Pane({
  leaf,
  workspace,
  session,
  onLayout,
  root,
}: {
  leaf: { tabs: string[]; active?: string }
  workspace: Workspace
  session: Session
  onLayout: (layout: LayoutNode) => void
  root: LayoutNode
}) {
  const [active, setActive] = useState(leaf.active ?? leaf.tabs[0])
  const [dragging, setDragging] = useState(false)
  const view = workspace.views.find((item) => item.id === active) ?? workspace.views.find((item) => item.id === leaf.tabs[0])
  return (
    <section className="pane">
      <div className="pane-tabs">
        {leaf.tabs.map((id) => {
          const tab = workspace.views.find((item) => item.id === id)
          return (
            <button
              key={id}
              draggable={!workspace.locked}
              className={id === view?.id ? 'pane-tab active' : 'pane-tab'}
              onClick={() => setActive(id)}
              onDragStart={(event) => {
                event.dataTransfer.setData('text/plain', id)
                event.dataTransfer.effectAllowed = 'move'
              }}
            >
              {tab?.title ?? id}
            </button>
          )
        })}
      </div>
      <div
        className="pane-body"
        onDragOver={(event) => {
          if (workspace.locked) return
          event.preventDefault()
          setDragging(true)
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={(event) => {
          const id = event.dataTransfer.getData('text/plain')
          const zone = (event.target as HTMLElement).dataset.zone as Zone | undefined
          setDragging(false)
          if (!id || !zone || !view) return
          onLayout(dockTab(root, id, zone, view.id))
        }}
      >
        {view ? <ViewBody view={view} session={session} /> : <p>{sv.empty}</p>}
        {dragging ? (
          <div className="drop">
            {(['left', 'right', 'top', 'bottom', 'center'] as Zone[]).map((zone) => (
              <i key={zone} data-zone={zone} />
            ))}
          </div>
        ) : null}
      </div>
    </section>
  )
}

function ViewBody({ view, session }: { view: View; session: Session }) {
  if (view.type === 'structure_tree') return <StructureTree session={session} view={view} />
  if (view.type === 'object_table') return <ObjectTable session={session} view={view} />
  if (view.type === 'object_detail') return <ObjectDetail session={session} />
  if (view.type === 'graph') return <GraphView session={session} />
  if (view.type === 'history') return <HistoryView session={session} />
  if (view.type === 'data_model') return <DataModel view={view} />
  if (view.type === 'dashboard') return <Dashboard />
  if (view.type === 'proposal') return <ProposalView session={session} />
  if (view.type === 'batch_log') return <BatchLog />
  if (view.type === 'job_history') return <JobHistory />
  if (view.type === 'embed') return <EmbedView session={session} view={view} />
  return <p>{view.title}</p>
}

function StructureTree({ session, view }: { session: Session; view: View }) {
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const expanded = (view.state.expanded as string[] | undefined) ?? []
  const structures = (defs.data?.structures as Json[] | undefined) ?? []
  return (
    <div className="tree" data-testid="structure-tree">
      {structures.map((structure) => {
        const groups = (structure.groups as Json[]) ?? []
        const keyById = new Map(groups.map((group) => [String(group.id), String(group.key)]))
        return (
        <div key={String(structure.key)}>
          <strong>{String(structure.key)}</strong>
          {groups.map((group) => {
            const key = String(group.key)
            const parentKey = group.parent ? keyById.get(String(group.parent)) : undefined
            const open = !group.parent || expanded.includes(parentKey ?? '') || expanded.includes(key)
            if (!open) return null
            return (
              <button
                key={key}
                className={session.group === `${structure.key}:${key}` ? 'on' : ''}
                style={{ paddingLeft: group.parent ? 16 : 4 }}
                onClick={() => session.setGroup(`${structure.key}:${key}`)}
              >
                {key}
              </button>
            )
          })}
        </div>
        )
      })}
    </div>
  )
}

function ObjectTable({ session, view }: { session: Session; view: View }) {
  const [filter, setFilter] = useState('')
  const [groupBy, setGroupBy] = useState('')
  const [menu, setMenu] = useState<{ x: number; y: number; id: string } | null>(null)
  const queryClient = useQueryClient()
  const objects = useQuery({
    queryKey: ['objects', filter, session.group],
    queryFn: async () => {
      const params = new URLSearchParams({ q: filter || '*', per_page: '200' })
      if (session.group) params.set('filter', `groups:=${session.group}`)
      const page = await api<Json>(`/v1/search?${params}`)
      const hits = (page.hits as Json[] | undefined) ?? []
      return hits.map((hit) => {
        const doc = (hit.document as Json) ?? hit
        return {
          id: String(doc.id ?? ''),
          key: String(doc.key ?? ''),
          type: String(doc.type_key ?? ''),
          version: String(doc.version ?? ''),
          name: textValue(doc.a_namn__sv),
          status: textValue(doc.a_status),
        } satisfies ObjRow
      })
    },
  })
  const rows = objects.data ?? []
  const columns = useMemo(
    () => [
      { accessorKey: 'key', header: 'Nummer', id: 'key', size: 180 },
      { accessorKey: 'name', header: 'Namn', id: 'name', size: 240 },
      { accessorKey: 'type', header: 'Typ', id: 'type', size: 100 },
      { accessorKey: 'status', header: 'Status', id: 'status', size: 100 },
      { accessorKey: 'version', header: 'Version', id: 'version', size: 80 },
    ],
    [],
  )
  const table = useTable({
    features,
    data: rows,
    columns,
    getRowId: (row) => row.id,
    initialState: groupBy ? { grouping: [groupBy] } : undefined,
  })
  const parent = useRef<HTMLDivElement>(null)
  const model = table.getRowModel().rows
  const virtualizer = useVirtualizer({
    count: model.length,
    getScrollElement: () => parent.current,
    estimateSize: () => 24,
    overscan: 12,
  })
  const selected = table.getSelectedRowModel?.().rows.map((row) => row.id) ?? []
  const mass = useMutation({
    mutationFn: () =>
      api<Json>('/v1/operations/mass_edit', {
        method: 'POST',
        body: JSON.stringify({
          title: 'Status OK',
          ids: selected,
          type: rows.find((row) => selected.includes(row.id))?.type,
          values: { status: 'ok' },
        }),
      }),
    onSuccess: (result) => {
      session.setProposalId(String(result.id))
      session.push(`${sv.proposals} ${result.id}`)
    },
  })
  return (
    <div className="table-wrap" data-testid="object-table">
      <div className="toolbar">
        <input
          data-testid="quick-filter"
          placeholder={sv.searchPlaceholder}
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
        <label>
          {sv.groupBy}
          <select value={groupBy} onChange={(event) => setGroupBy(event.target.value)}>
            <option value="">—</option>
            <option value="type">type</option>
            <option value="status">status</option>
          </select>
        </label>
        <button onClick={() => mass.mutate()} disabled={selected.length === 0}>{sv.massEdit}</button>
        <button
          onClick={() => {
            void api<Blob>('/v1/export', {
              method: 'POST',
              body: JSON.stringify({ q: filter || '*', columns: ['key', 'type_key', 'a_namn__sv', 'a_status'] }),
            }).then((blob) => {
              const url = URL.createObjectURL(blob)
              const link = document.createElement('a')
              link.href = url
              link.download = 'braian.xlsx'
              link.click()
              URL.revokeObjectURL(url)
            })
          }}
        >
          {sv.export}
        </button>
        <NewObject session={session} />
      </div>
      <div className="grid" ref={parent}>
        <div className="head">
          {table.getHeaderGroups().map((group) =>
            group.headers.map((header) => (
              <div key={header.id} className="cell" style={{ width: header.getSize?.() ?? 140 }}>
                {header.column.columnDef.header as string}
              </div>
            )),
          )}
        </div>
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map((item) => {
            const row = model[item.index]
            if (!row) return null
            return (
              <div
                key={row.id}
                data-testid="object-row"
                className={session.objectId === row.id ? 'row selected' : 'row'}
                style={{ position: 'absolute', transform: `translateY(${item.start}px)`, width: '100%' }}
                onClick={() => {
                  session.setObjectId(row.id)
                  row.toggleSelected?.()
                }}
                onContextMenu={(event) => {
                  event.preventDefault()
                  session.setObjectId(row.id)
                  setMenu({ x: event.clientX, y: event.clientY, id: row.id })
                }}
              >
                {row.getAllCells().map((cell) => (
                  <div key={cell.id} className="cell" style={{ width: cell.column.getSize?.() ?? 140 }}>
                    {String(cell.getValue() ?? '')}
                  </div>
                ))}
              </div>
            )
          })}
        </div>
      </div>
      {objects.isError ? <p className="error">{(objects.error as Error).message}</p> : null}
      {!objects.isLoading && rows.length === 0 ? <p>{sv.noRows}</p> : null}
      {menu ? (
        <div className="menu" style={{ left: menu.x, top: menu.y }} data-testid="context-menu">
          <button onClick={() => { session.setObjectId(menu.id); setMenu(null) }}>{sv.open}</button>
          <button onClick={() => { void queryClient.invalidateQueries({ queryKey: ['history', menu.id] }); setMenu(null) }}>{sv.history}</button>
          <button onClick={() => setMenu(null)}>{sv.close}</button>
        </div>
      ) : null}
      <p className="icon-btn">{view.title}</p>
    </div>
  )
}

function NewObject({ session }: { session: Session }) {
  const [open, setOpen] = useState(false)
  const [nummer, setNummer] = useState('')
  const [error, setError] = useState('')
  const queryClient = useQueryClient()
  if (!open) return <button onClick={() => setOpen(true)}>{sv.newObject}</button>
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
            values: { nummer, namn: { sv: nummer }, status: 'utkast' },
          }),
        })
          .then((created) => {
            setError('')
            setOpen(false)
            session.setObjectId(String(created.id))
            session.push(`${sv.saved} ${nummer}`)
            void queryClient.invalidateQueries({ queryKey: ['objects'] })
          })
          .catch((reason: unknown) => setError(reason instanceof ApiError ? reason.message : sv.validation))
      }}
    >
      <input data-testid="new-nummer" value={nummer} onChange={(event) => setNummer(event.target.value)} placeholder="nummer" />
      <button className="primary" type="submit" data-testid="create-object">{sv.save}</button>
      {error ? <span className="error" data-testid="validation-error">{error}</span> : null}
    </form>
  )
}

function ObjectDetail({ session }: { session: Session }) {
  const queryClient = useQueryClient()
  const object = useQuery({
    queryKey: ['object', session.objectId],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}`),
    enabled: Boolean(session.objectId),
  })
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const [draft, setDraft] = useState<Json>({})
  const [error, setError] = useState('')
  useEffect(() => setDraft({}), [session.objectId])
  if (!session.objectId) return <p>{sv.detail}</p>
  if (!object.data) return <p>{sv.loading}</p>
  const values = { ...((object.data.values as Json) ?? {}), ...draft }
  const attributes = (defs.data?.attributes as Json[] | undefined) ?? []
  const lists = (defs.data?.value_lists as Json[] | undefined) ?? []
  const families = (defs.data?.unit_families as Json[] | undefined) ?? []
  const save = () => {
    void api(`/v1/objects/${session.objectId}`, {
      method: 'PATCH',
      headers: { 'if-match': String(object.data?.version ?? '') },
      body: JSON.stringify({ values: draft }),
    })
      .then(() => {
        setDraft({})
        setError('')
        session.push(`${sv.saved} ${object.data?.id}`)
        void queryClient.invalidateQueries({ queryKey: ['object', session.objectId] })
        void queryClient.invalidateQueries({ queryKey: ['objects'] })
      })
      .catch((reason: unknown) => setError(reason instanceof ApiError ? reason.message : sv.validation))
  }
  return (
    <form
      className="form"
      data-testid="object-detail"
      onSubmit={(event) => {
        event.preventDefault()
        save()
      }}
    >
      <strong>{String(object.data.type)} · {textValue((object.data.values as Json)?.nummer)}</strong>
      <label>
        {sv.language}
        <select value={session.language} onChange={() => undefined}>
          <option>sv</option>
          <option>en</option>
          <option>de</option>
        </select>
      </label>
      {Object.keys(values).map((key) => {
        const attribute = attributes.find((item) => item.key === key)
        const kind = (attribute?.kind as Json) ?? {}
        const label = textValue((attribute?.labels as Json) ?? key, 'sv') || key
        const current = values[key]
        const dirty = key in draft
        return (
          <div key={key} className={dirty ? 'field draft-user' : 'field'}>
            <label>{label}</label>
            <Editor
              kind={String(kind.type ?? 'text')}
              multiple={Boolean(attribute?.multiple)}
              languageDependent={Boolean(attribute?.language_dependent)}
              value={current}
              options={optionsFor(lists, String(kind.list ?? ''))}
              units={unitsFor(families, String(kind.unit_family ?? ''))}
              onChange={(next) => setDraft((state) => ({ ...state, [key]: next }))}
            />
          </div>
        )
      })}
      {error ? <p className="error" data-testid="detail-error">{error}</p> : null}
      <div className="toolbar">
        <button className="primary" type="submit" data-testid="save-object">{sv.save}</button>
        <button type="button" onClick={() => setDraft({})}>{sv.discard}</button>
      </div>
    </form>
  )
}

function optionsFor(lists: Json[], key: string) {
  const list = lists.find((item) => item.key === key)
  return ((list?.options as Json[]) ?? []).map((option) => ({
    code: String(option.code),
    label: textValue(option.labels, 'sv') || String(option.code),
  }))
}

function unitsFor(families: Json[], id: string) {
  const family = families.find((item) => item.id === id || item.key === id)
  return ((family?.units as Json[]) ?? []).map((unit) => String(unit.code))
}

function Editor({
  kind,
  value,
  languageDependent,
  options,
  units,
  onChange,
}: {
  kind: string
  multiple: boolean
  languageDependent: boolean
  value: unknown
  options: Array<{ code: string; label: string }>
  units: string[]
  onChange: (value: unknown) => void
}) {
  if (languageDependent && value && typeof value === 'object' && !Array.isArray(value)) {
    const record = value as Json
    return (
      <div>
        {['sv', 'en', 'de'].map((language) => (
          <input
            key={language}
            value={textValue(record[language])}
            onChange={(event) => onChange({ ...record, [language]: event.target.value })}
          />
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
  if (kind === 'number' || kind === 'range') {
    const record = (value && typeof value === 'object' ? value : { value }) as Json
    return (
      <span className="toolbar">
        <input value={textValue(record.value ?? record.min)} onChange={(event) => onChange({ ...record, value: event.target.value })} />
        {units.length > 0 ? (
          <select value={String(record.unit ?? units[0])} onChange={(event) => onChange({ ...record, unit: event.target.value })}>
            {units.map((unit) => <option key={unit}>{unit}</option>)}
          </select>
        ) : null}
      </span>
    )
  }
  if (kind === 'text' && typeof value === 'string' && value.length > 80) {
    return <textarea value={value} onChange={(event) => onChange(event.target.value)} />
  }
  return <input value={textValue(value)} onChange={(event) => onChange(event.target.value)} />
}

function GraphView({ session }: { session: Session }) {
  const graph = useQuery({
    queryKey: ['graph', session.objectId],
    queryFn: () => api<Json>(`/v1/objects/${session.objectId}/graph`),
    enabled: Boolean(session.objectId),
  })
  if (!graph.data) return <p data-testid="graph">{sv.graph}</p>
  const relations = (graph.data.relations as Json[]) ?? []
  const attributes = ((graph.data.attributes as Json[]) ?? []).slice(0, 8)
  return (
    <svg className="graph" data-testid="graph" viewBox="0 0 420 240">
      <rect x="150" y="16" width="120" height="32" fill="var(--select)" />
      <text x="210" y="36" textAnchor="middle" fill="var(--text)">{String(graph.data.key)}</text>
      {relations.map((relation, index) => (
        <g key={index}>
          <line x1="210" y1="48" x2={40 + index * 90} y2="110" stroke="var(--accent)" />
          <text x={40 + index * 90} y="126" fill="var(--text)">{String(relation.kind ?? relation.to)}</text>
        </g>
      ))}
      {attributes.map((attribute, index) => (
        <text key={String(attribute.key)} x="8" y={150 + index * 12} fill="var(--muted)">
          {String(attribute.key)} · {textValue((attribute.sources as Json[])?.[0]?.class ?? (attribute.sources as Json[])?.[0]?.from)}
        </text>
      ))}
    </svg>
  )
}

function HistoryView({ session }: { session: Session }) {
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
  return (
    <div data-testid="history">
      <label>
        {sv.asOf}
        <input
          data-testid="as-of"
          type="range"
          min={Number(rows[0]?.seq ?? 0)}
          max={Number(rows.at(-1)?.seq ?? 0)}
          value={current}
          onChange={(event) => setSeq(Number(event.target.value))}
        />
      </label>
      <button data-testid="rollback" onClick={() => rollback.mutate()}>{sv.rollback}</button>
      <div className="timeline">
        {rows.map((row) => (
          <article key={String(row.seq)}>
            <strong>#{String(row.seq)}</strong> {String(row.op)} · {String(row.actor ?? '')}
            {row.on_behalf_of ? ` för ${String(row.on_behalf_of)}` : ''}
          </article>
        ))}
      </div>
      <pre>{JSON.stringify(asOf.data?.values ?? {}, null, 2).slice(0, 800)}</pre>
    </div>
  )
}

function DataModel({ view }: { view: View }) {
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const [attribute, setAttribute] = useState('status')
  const impact = useMutation({
    mutationFn: () => api<Json>('/v1/impact', { method: 'POST', body: JSON.stringify({ remove_attribute: attribute }) }),
  })
  const mode = String(view.state.mode ?? 'tree')
  return (
    <div data-testid="data-model">
      <p>{sv.dataModel} · {mode}</p>
      {((defs.data?.structures as Json[]) ?? []).map((structure) => (
        <div key={String(structure.key)} className="tree">
          <strong>{String(structure.key)}</strong>
          {((structure.classes as Json[]) ?? []).map((item) => (
            <div key={String(item.key)} draggable>{String(item.key)}</div>
          ))}
        </div>
      ))}
      {mode === 'impact' || mode === 'editor' ? (
        <form
          className="toolbar"
          onSubmit={(event) => {
            event.preventDefault()
            impact.mutate()
          }}
        >
          <input value={attribute} onChange={(event) => setAttribute(event.target.value)} />
          <button type="submit">{sv.impact}</button>
        </form>
      ) : null}
      {impact.data ? <pre>{JSON.stringify(impact.data, null, 2).slice(0, 600)}</pre> : null}
    </div>
  )
}

function Dashboard() {
  const stats = useQuery({ queryKey: ['stats'], queryFn: () => api<Json>('/v1/stats') })
  const rows = ((stats.data?.by_type as Json[]) ?? []).map((row) => ({
    type: String(row.type),
    count: Number(row.count ?? 0),
  }))
  const definition = useMemo(
    () =>
      defineChart({
        marks: [barY(rows, { x: 'type', y: 'count' })],
        scales: {
          x: { scale: () => scaleBand().padding(0.2) },
          y: { scale: scaleLinear, nice: true },
        },
      }),
    [rows],
  )
  return (
    <div data-testid="dashboard">
      <p>{sv.dashboard}: {String(stats.data?.objects ?? 0)}</p>
      {rows.length > 0 ? <Chart definition={definition} height={220} ariaLabel={sv.dashboard} /> : <p>{sv.empty}</p>}
    </div>
  )
}

function ProposalView({ session }: { session: Session }) {
  const proposal = useQuery({
    queryKey: ['proposal', session.proposalId],
    queryFn: () => api<Json>(`/v1/proposals/${session.proposalId}`),
    enabled: Boolean(session.proposalId),
  })
  const [picked, setPicked] = useState<string[]>([])
  const accept = useMutation({
    mutationFn: (rows?: string[]) =>
      api(`/v1/proposals/${session.proposalId}/accept`, {
        method: 'POST',
        body: JSON.stringify(rows && rows.length > 0 ? { rows } : {}),
      }),
    onSuccess: () => session.push(sv.acceptAll),
  })
  if (!session.proposalId) return <p data-testid="proposal">{sv.proposals}</p>
  const rows = (proposal.data?.rows as Json[]) ?? []
  return (
    <div data-testid="proposal">
      <div className="toolbar">
        <strong className={proposal.data?.source === 'agent' ? 'draft-agent' : 'draft-user'}>{String(proposal.data?.title ?? '')}</strong>
        <button data-testid="accept-all" onClick={() => accept.mutate(undefined)}>{sv.acceptAll}</button>
        <button onClick={() => accept.mutate(picked)}>{sv.acceptSelected}</button>
      </div>
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
          {String(row.object_key ?? row.object_id)} · {String(row.status)}
          {row.conflict ? ` · ${sv.conflict}` : ''}
        </label>
      ))}
    </div>
  )
}

function BatchLog() {
  const history = useQuery({ queryKey: ['batches'], queryFn: () => api<Json>('/v1/history?limit=50') })
  const rows = (history.data?.changes as Json[]) ?? []
  return (
    <div className="timeline" data-testid="batch-log">
      {rows.map((row) => (
        <article key={String(row.seq)}>
          #{String(row.seq)} {String(row.op)} {String(row.actor)} {row.on_behalf_of ? `via ${String(row.via)}` : ''}
        </article>
      ))}
    </div>
  )
}

function JobHistory() {
  const jobs = useQuery({ queryKey: ['jobs'], queryFn: () => api<Json>('/v1/job-types/bulk.write/runs') })
  const rows = (jobs.data?.runs as Json[]) ?? (Array.isArray(jobs.data) ? (jobs.data as Json[]) : [])
  return (
    <div data-testid="jobs">
      {rows.length === 0 ? <p>{sv.noRows}</p> : null}
      {rows.map((row) => (
        <article key={String(row.id)}>
          {String(row.status)} {String(row.progress_done ?? '')}/{String(row.progress_total ?? '')}
        </article>
      ))}
    </div>
  )
}

function EmbedView({ session, view }: { session: Session; view: View }) {
  const template = String(view.state.url ?? '')
  const src = template.replace('{object}', session.objectId ?? '').replace('{key}', session.objectId ?? '').replace('{token}', 'session')
  if (!src) return <p>{sv.empty}</p>
  return <iframe title={sv.product} src={src} style={{ width: '100%', height: '100%', border: 0 }} />
}

function Palette({ session, onClose, onAsk }: { session: Session; onClose: () => void; onAsk: () => void }) {
  const [q, setQ] = useState('')
  const found = useQuery({
    queryKey: ['palette', q],
    queryFn: () => api<{ objects: Json[] }>(`/v1/objects?q=${encodeURIComponent(q)}&limit=20`),
    enabled: q.length > 0,
  })
  const ops = useQuery({ queryKey: ['ops'], queryFn: () => api<{ operations: Json[] }>('/v1/operations') })
  return (
    <div className="palette" data-testid="palette" onClick={onClose}>
      <form onClick={(event) => event.stopPropagation()} onSubmit={(event) => event.preventDefault()}>
        <input data-testid="palette-input" autoFocus placeholder={sv.commandPlaceholder} value={q} onChange={(event) => setQ(event.target.value)} />
        {(found.data?.objects ?? []).map((row) => (
          <button key={String(row.id)} className="hit" onClick={() => { session.setObjectId(String(row.id)); onClose() }}>
            {String(row.key)} · {String(row.type)}
          </button>
        ))}
        <button className="hit" onClick={onAsk}>{sv.askAgent}</button>
        {(ops.data?.operations ?? []).slice(0, 6).map((op) => (
          <button key={String(op.name)} className="hit" type="button">{String(op.title)}</button>
        ))}
      </form>
    </div>
  )
}

function Help({ onClose }: { onClose: () => void }) {
  return (
    <div className="help" onClick={onClose}>
      <div onClick={(event) => event.stopPropagation()}>
        <strong>{sv.shortcuts}</strong>
        <p><kbd>Ctrl</kbd>+<kbd>K</kbd> {sv.commandPlaceholder}</p>
        <p><kbd>Ctrl</kbd>+<kbd>1</kbd>…<kbd>9</kbd> arbetsytor</p>
        <p><kbd>F1</kbd> {sv.shortcuts}</p>
        <button onClick={onClose}>{sv.close}</button>
      </div>
    </div>
  )
}

function AgentPanel({ onClose, session }: { onClose: () => void; session: Session }) {
  const status = useQuery({ queryKey: ['agent-status'], queryFn: () => api<{ available: boolean; model: string; reply?: string }>('/api/chat') })
  const chat = useChat({ connection: fetchServerSentEvents('/api/chat'), threadId: 'braian' })
  const [input, setInput] = useState('')
  return (
    <aside className="agent" data-testid="agent">
      <header>
        <span>{sv.agent} · {status.data?.model ?? 'gpt-6-luna'}</span>
        <button onClick={onClose}>{sv.close}</button>
      </header>
      <div className="log">
        {status.data && !status.data.available ? <p className="bubble assistant">{status.data.reply}</p> : null}
        {chat.messages.map((message) => (
          <p key={message.id} className={message.role === 'user' ? 'bubble user' : 'bubble assistant'}>
            {message.parts.map((part) => (part.type === 'text' ? part.content : '')).join('')}
          </p>
        ))}
        {chat.error ? <p className="error">{sv.agentUnavailable}</p> : null}
      </div>
      <form
        className="toolbar"
        onSubmit={(event) => {
          event.preventDefault()
          if (!input.trim()) return
          void chat.sendMessage(input.trim())
          session.push(`${sv.agent}: ${input.trim()}`)
          setInput('')
        }}
      >
        <input value={input} onChange={(event) => setInput(event.target.value)} placeholder={sv.askAgent} />
        <button className="primary" type="submit">{sv.send}</button>
      </form>
    </aside>
  )
}
