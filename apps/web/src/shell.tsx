import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { barY, defineChart } from '@tanstack/charts'
import { scaleBand } from '@tanstack/charts/scales/band'
import { scaleLinear } from '@tanstack/charts/scales/linear'
import { Chart } from '@tanstack/react-charts'
import { Moon, Sun } from 'lucide-react'
import { ApiError, api, type Json } from './api'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Toaster } from './components/ui/sonner'
import { viewDisplayTitle, workspaceDisplayName } from './demo-labels'
import { copy } from './i18n'
import { dockTab, isSplit, type LayoutNode, type Zone } from './layout'
import {
  DataModel,
  GraphView,
  HistoryView,
  JobHistory,
  ObjectDetail,
  ObjectTable,
  ProposalView,
} from './panes'

type View = { id: string; type: string; title: string; pane?: string; position?: number; state: Json }
type Workspace = { id: string; name: string; locked: boolean; share_token?: string; layout: LayoutNode; views: View[] }
type Me = {
  email?: string
  role: string
  display_name?: string
  handle?: string
  user_id?: string
}
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
    const stamp = new Date().toLocaleTimeString('en-GB', { hour12: false })
    const text = `${stamp} ${line}`
    if (statusRef.current) statusRef.current.textContent = text
    const log = logRef.current
    if (!log) return
    const lines = [text, ...(log.textContent ?? '').split('\n').filter(Boolean)].slice(0, 6)
    log.textContent = lines.join('\n')
  }

  const workspaces = useQuery({
    queryKey: ['workspaces'],
    queryFn: () => api<{ workspaces: Array<{ id: string; name: string; share_token?: string }> }>('/v1/workspaces'),
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
    source.onopen = () => {
      if (logRef.current) logRef.current.dataset.live = 'open'
    }
    source.onmessage = (event) => {
      if (logRef.current) logRef.current.dataset.live = 'message'
      push(`live ${event.data}`)
      clearTimeout(pending)
      pending = setTimeout(() => {
        void queryClient.invalidateQueries({ queryKey: ['objects'] })
        void queryClient.invalidateQueries({ queryKey: ['object'] })
        void queryClient.invalidateQueries({ queryKey: ['history'] })
      }, 200)
    }
    source.onerror = () => {
      if (source.readyState === EventSource.CLOSED) push(copy.offline)
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

  if (!booted || me.isLoading) return <p className="pane-body">{copy.loading}</p>
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
        <div className="brand">
          <img src="/brand/braian-mark.svg" alt="" width={16} height={16} />
          <span>{copy.product}</span>
        </div>
        <nav className="tabs">
          {workspaces.data?.workspaces.map((item, index) => (
            <button
              key={item.id}
              className={item.id === workspaceId ? 'tab active' : 'tab'}
              onClick={() => setWorkspaceId(item.id)}
            >
              {workspaceDisplayName(item.name, item.share_token)}
              <span className="icon-btn"> {index < 9 ? index + 1 : ''}</span>
            </button>
          ))}
        </nav>
        <span className="spacer" />
        <button className="icon-btn" onClick={() => setZoom((value) => Math.min(1.4, value + 0.1))}>{copy.zoom}</button>
        <button className="icon-btn" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}>
          {theme === 'dark' ? <Sun aria-hidden="true" size={14} /> : <Moon aria-hidden="true" size={14} />}
          {theme === 'dark' ? copy.themeLight : copy.themeDark}
        </button>
        <button className="icon-btn" onClick={() => setAgentOpen(true)}>{copy.agent}</button>
        <span>{userName}</span>
        <button
          className="icon-btn"
          onClick={() => void api('/v1/logout', { method: 'POST' }).then(() => queryClient.invalidateQueries({ queryKey: ['me'] }))}
        >
          {copy.signOut}
        </button>
      </header>
      <main className="workspace">
        {workspace.data ? (
          <LayoutView node={workspace.data.layout} workspace={workspace.data} session={session} onLayout={saveLayout} />
        ) : (
          <p>{copy.loading}</p>
        )}
      </main>
      <pre className="activity" data-testid="activity-log" ref={logRef} />
      <footer className="status">
        <span>{userEmail}</span>
        <span>{user.role}</span>
        <span data-testid="live-flag">{copy.connected}</span>
        {workspace.data?.locked ? <span>{copy.locked}</span> : null}
        <span className="grow" />
        <span ref={statusRef}>{copy.activity}</span>
      </footer>
      {palette ? <Palette session={session} onClose={() => setPalette(false)} onAsk={() => { setPalette(false); setAgentOpen(true) }} /> : null}
      {help ? <Help onClose={() => setHelp(false)} /> : null}
      {agentOpen ? <AgentPanel onClose={() => setAgentOpen(false)} session={session} /> : null}
      <Toaster theme={theme} />
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
            .catch(() => setError(copy.loginFailed))
        }}
      >
        <div className="brand">
          <img src="/brand/braian-mark.svg" alt="" width={16} height={16} />
          <span>{copy.product}</span>
        </div>
        <h1>{copy.loginTitle}</h1>
        <label>
          {copy.email}
          <Input data-testid="email" value={email} onChange={(event) => setEmail(event.target.value)} />
        </label>
        <label>
          {copy.password}
          <Input data-testid="password" type="password" value={password} onChange={(event) => setPassword(event.target.value)} />
        </label>
        {error ? <p className="error">{error}</p> : <p className="hint">{copy.loginHint}</p>}
        <Button type="submit">{copy.signIn}</Button>
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
        data-testid="split-divider"
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

let dockDrag: string | null = null
const dockListeners = new Set<(id: string | null) => void>()

function setDockDrag(id: string | null) {
  dockDrag = id
  dockListeners.forEach((listen) => listen(id))
}

function useDockDrag() {
  const [id, setId] = useState(dockDrag)
  useEffect(() => {
    dockListeners.add(setId)
    return () => {
      dockListeners.delete(setId)
    }
  }, [])
  return id
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
  const dockId = useDockDrag()
  const view = workspace.views.find((item) => item.id === active) ?? workspace.views.find((item) => item.id === leaf.tabs[0])
  const acceptDock = (zone: Zone) => {
    const id = dockDrag
    setDockDrag(null)
    if (!id || !view || workspace.locked) return
    onLayout(dockTab(root, id, zone, view.id))
  }
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
                setDockDrag(id)
              }}
              onDragEnd={() => setDockDrag(null)}
            >
              {viewDisplayTitle(tab?.title ?? id)}
            </button>
          )
        })}
      </div>
      <div
        className="pane-body"
        onDragOver={(event) => {
          if (workspace.locked || !dockId) return
          event.preventDefault()
        }}
      >
        {view ? <ViewBody view={view} session={session} workspace={workspace} /> : <p>{copy.empty}</p>}
        {dockId && !workspace.locked ? (
          <div className="drop" data-testid="drop-overlay">
            {(['left', 'right', 'top', 'bottom', 'center'] as Zone[]).map((zone) => (
              <i
                key={zone}
                data-zone={zone}
                data-testid={`drop-${zone}`}
                onDragOver={(event) => event.preventDefault()}
                onDrop={(event) => {
                  event.preventDefault()
                  event.stopPropagation()
                  const carried = event.dataTransfer.getData('text/plain')
                  if (carried) dockDrag = carried
                  acceptDock(zone)
                }}
              />
            ))}
          </div>
        ) : null}
      </div>
    </section>
  )
}

function ViewBody({ view, session, workspace }: { view: View; session: Session; workspace: Workspace }) {
  if (view.type === 'structure_tree') return <StructureTree session={session} view={view} />
  if (view.type === 'object_table') return <ObjectTable session={session} view={view} workspace={workspace} />
  if (view.type === 'object_detail') return <ObjectDetail session={session} />
  if (view.type === 'graph') return <GraphView session={session} />
  if (view.type === 'history') return <HistoryView session={session} />
  if (view.type === 'data_model') return <DataModel view={view} />
  if (view.type === 'dashboard') return <Dashboard />
  if (view.type === 'proposal') return <ProposalView session={session} />
  if (view.type === 'batch_log') return <BatchLog />
  if (view.type === 'job_history') return <JobHistory view={view} />
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
      <p>{copy.dashboard}: {String(stats.data?.objects ?? 0)}</p>
      {rows.length > 0 ? <Chart definition={definition} height={220} ariaLabel={copy.dashboard} /> : <p>{copy.empty}</p>}
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

function EmbedView({ session, view }: { session: Session; view: View }) {
  const template = String(view.state.url ?? '')
  const src = template.replace('{object}', session.objectId ?? '').replace('{key}', session.objectId ?? '').replace('{token}', 'session')
  if (!src) return <p>{copy.empty}</p>
  return <iframe title={copy.product} src={src} style={{ width: '100%', height: '100%', border: 0 }} />
}

function Palette({ session, onClose, onAsk }: { session: Session; onClose: () => void; onAsk: () => void }) {
  const [q, setQ] = useState('')
  const [activeId, setActiveId] = useState<string | null>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const armed = useRef(false)
  const found = useQuery({
    queryKey: ['palette', q],
    queryFn: () => api<{ objects: Json[] }>(`/v1/objects?q=${encodeURIComponent(q)}&limit=20`),
    enabled: q.length > 0,
  })
  const ops = useQuery({ queryKey: ['ops'], queryFn: () => api<{ operations: Json[] }>('/v1/operations') })
  const hits = [
    ...(found.data?.objects ?? []).map((row) => ({
      id: String(row.id),
      kind: 'object' as const,
      label: `${String(row.key)} · ${String(row.type)}`,
      run: () => {
        session.setObjectId(String(row.id))
        onClose()
      },
    })),
    { id: 'ask', kind: 'command' as const, label: copy.askAgent, run: onAsk },
    ...(ops.data?.operations ?? []).slice(0, 6).map((op) => ({
      id: String(op.name),
      kind: 'command' as const,
      label: String(op.title),
      run: () => {},
    })),
  ]
  const hitsRef = useRef(hits)
  hitsRef.current = hits
  const resultKey = hits.filter((hit) => hit.kind === 'object').map((hit) => hit.id).join('\n')
  const activeIndex = hits.findIndex((hit) => hit.id === activeId)
  useEffect(() => {
    if (!armed.current) return
    const list = hitsRef.current
    const selected = list.find((hit) => hit.id === activeId)
    if (selected?.kind === 'object') return
    const first = list.find((hit) => hit.kind === 'object')
    if (first) setActiveId(first.id)
  }, [resultKey, activeId])
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>('.hit.on')?.scrollIntoView({ block: 'nearest' })
  }, [activeId])

  const onKeyDown = (event: ReactKeyboardEvent<HTMLFormElement>) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp' && event.key !== 'Enter') return
    event.preventDefault()
    if (hits.length === 0) return
    if (event.key === 'Enter') {
      const chosen = !armed.current || activeIndex < 0 ? 0 : activeIndex
      hits[chosen]?.run()
      return
    }
    const next = event.key === 'ArrowDown'
      ? (!armed.current || activeIndex < 0 ? 0 : Math.min(activeIndex + 1, hits.length - 1))
      : (!armed.current || activeIndex < 0 ? hits.length - 1 : Math.max(activeIndex - 1, 0))
    armed.current = true
    setActiveId(hits[next].id)
  }

  return (
    <div className="palette" data-testid="palette" onClick={onClose}>
      <form onClick={(event) => event.stopPropagation()} onSubmit={(event) => event.preventDefault()} onKeyDown={onKeyDown}>
        <input
          data-testid="palette-input"
          autoFocus
          autoComplete="off"
          role="combobox"
          aria-expanded={hits.length > 0}
          aria-controls="palette-list"
          aria-activedescendant={activeIndex >= 0 ? `palette-hit-${activeIndex}` : undefined}
          placeholder={copy.commandPlaceholder}
          value={q}
          onChange={(event) => {
            setQ(event.target.value)
            setActiveId(null)
            armed.current = false
          }}
        />
        <div className="palette-list" id="palette-list" role="listbox" ref={listRef}>
          {hits.map((hit, index) => (
            <button
              key={hit.id}
              id={`palette-hit-${index}`}
              type="button"
              role="option"
              aria-selected={index === activeIndex}
              className={index === activeIndex ? 'hit on' : 'hit'}
              onMouseEnter={() => setActiveId(hit.id)}
              onClick={hit.run}
            >
              {hit.label}
            </button>
          ))}
        </div>
      </form>
    </div>
  )
}

function Help({ onClose }: { onClose: () => void }) {
  return (
    <div className="help" onClick={onClose}>
      <div onClick={(event) => event.stopPropagation()}>
        <strong>{copy.shortcuts}</strong>
        <p><kbd>Ctrl</kbd>+<kbd>K</kbd> {copy.commandPlaceholder}</p>
        <p><kbd>Ctrl</kbd>+<kbd>1</kbd>…<kbd>9</kbd> workspaces</p>
        <p><kbd>F1</kbd> {copy.shortcuts}</p>
        <button onClick={onClose}>{copy.close}</button>
      </div>
    </div>
  )
}

function AgentPanel({ onClose, session }: { onClose: () => void; session: Session }) {
  const status = useQuery({ queryKey: ['agent-status'], queryFn: () => api<{ available: boolean; model: string; reply?: string }>('/api/chat') })
  const [lines, setLines] = useState<Array<{ role: 'user' | 'assistant'; text: string }>>([])
  const [input, setInput] = useState('')
  const [pending, setPending] = useState(false)
  return (
    <aside className="agent" data-testid="agent">
      <header>
        <span>{copy.agent} · {status.data?.model ?? 'gpt-6-luna'}</span>
        <button onClick={onClose}>{copy.close}</button>
      </header>
      <div className="log">
        {status.data && !status.data.available ? <p className="bubble assistant">{status.data.reply}</p> : null}
        {lines.map((line, index) => (
          <p key={`${line.role}-${index}`} className={line.role === 'user' ? 'bubble user' : 'bubble assistant'}>
            {line.text}
          </p>
        ))}
      </div>
      <form
        className="toolbar"
        onSubmit={(event) => {
          event.preventDefault()
          const text = input.trim()
          if (!text || pending) return
          const history = [...lines, { role: 'user' as const, text }]
          setLines(history)
          setInput('')
          setPending(true)
          session.push(`${copy.agent}: ${text}`)
          void api<{ reply?: string; tools?: Json[] }>('/v1/agent', {
            method: 'POST',
            body: JSON.stringify({
              messages: history.map((line) => ({ role: line.role, content: line.text })),
            }),
          })
            .then((result) => {
              const tools = (result.tools ?? [])
                .map((tool) => `${String(tool.name)}: ${JSON.stringify(tool.result)}`)
                .join('\n')
              const reply = [result.reply, tools].filter(Boolean).join('\n')
              setLines([...history, { role: 'assistant', text: reply || copy.agentEmpty }])
            })
            .catch((error: unknown) => {
              const message = error instanceof ApiError ? error.message : copy.agentEmpty
              setLines([...history, { role: 'assistant', text: message }])
            })
            .finally(() => setPending(false))
        }}
      >
        <input value={input} onChange={(event) => setInput(event.target.value)} placeholder={copy.askAgent} />
        <button className="primary" type="submit">{copy.send}</button>
      </form>
    </aside>
  )
}
