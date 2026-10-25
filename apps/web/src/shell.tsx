import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { barY, defineChart } from '@tanstack/charts'
import { scaleBand } from '@tanstack/charts/scales/band'
import { scaleLinear } from '@tanstack/charts/scales/linear'
import { Chart } from '@tanstack/react-charts'
import { Group, Panel, Separator } from 'react-resizable-panels'
import { Moon, Sun } from 'lucide-react'
import { api, type Json } from './api'
import { humanizeLive, objectIdFromLive } from './activity'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Kbd } from './components/ui/kbd'
import { Toaster } from './components/ui/sonner'
import { viewDisplayTitle, workspaceDisplayName } from './demo-labels'
import { flashObject } from './flash'
import { copy } from './i18n'
import { dockTab, isSplit, type LayoutNode, type Zone } from './layout'
import { CommandPalette } from './palette'
import { createWorkspaceFromTemplate, workspaceTemplates } from './templates'
import { onUi } from './ui-bus'
import { onViewRequest, requestView } from './view-focus'
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
type ListedWorkspace = { id: string; name: string; locked?: boolean; share_token?: string; layout?: LayoutNode }
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
  const [logText, setLogText] = useState('')
  const [liveState, setLiveState] = useState('closed')
  const [statusLine, setStatusLine] = useState<string>(copy.activity)
  const [paletteMode, setPaletteMode] = useState<null | 'list' | 'ai'>(null)
  const [help, setHelp] = useState(false)
  const [zoom, setZoom] = useState(1)
  const [tabMenu, setTabMenu] = useState<{ x: number; y: number; id: string } | null>(null)
  const placeRef = useRef<(name: string, args: Json) => void>(() => {})

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
    setStatusLine(text)
    setLogText((current) => [text, ...current.split('\n').filter(Boolean)].slice(0, 6).join('\n'))
  }

  const workspaces = useQuery({
    queryKey: ['workspaces'],
    queryFn: () => api<{ workspaces: ListedWorkspace[] }>('/v1/workspaces'),
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
    source.onopen = () => setLiveState('open')
    source.onmessage = (event) => {
      setLiveState('message')
      const id = objectIdFromLive(event.data)
      if (id) flashObject(id)
      push(humanizeLive(event.data))
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
    const protectDrag = (event: PointerEvent) => {
      const target = event.target
      if (target instanceof Element && target.closest('.pane-tab, .pane-tabs')) event.stopPropagation()
    }
    window.addEventListener('pointerdown', protectDrag, true)
    return () => window.removeEventListener('pointerdown', protectDrag, true)
  }, [])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.altKey && ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) {
        event.preventDefault()
        movePaneFocus(event.key)
        return
      }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setPaletteMode('list')
      }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'j') {
        event.preventDefault()
        setPaletteMode('ai')
      }
      if (event.key === 'Escape') {
        setHelp(false)
        setTabMenu(null)
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

  useEffect(() => {
    return onUi((event) => {
      if (event.name === 'ui.set_theme') setTheme(event.args.theme === 'light' ? 'light' : 'dark')
      if (event.name === 'ui.open_workspace' && typeof event.args.id === 'string') setWorkspaceId(event.args.id)
      if (event.name === 'ui.open_template' && typeof event.args.id === 'string') {
        setWorkspaceId(event.args.id)
        void queryClient.invalidateQueries({ queryKey: ['workspaces'] })
      }
      if (event.name === 'ui.open_view' || event.name === 'ui.place' || event.name === 'ui.link') {
        const token = String(event.args.title ?? event.args.id ?? '')
        if (token) requestView(token)
      }
      if (event.name === 'ui.place' || event.name === 'ui.link') placeRef.current(event.name, event.args)
    })
  }, [queryClient])

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
  placeRef.current = (_name, args) => {
    if (!workspace.data) return
    const token = String(args.id ?? args.title ?? '')
    const view = workspace.data.views.find((item) => item.id === token || item.title === token || item.type === token)
    const zone = (String(args.zone ?? 'right') as Zone) || 'right'
    const active = workspace.data.views.find((item) => item.type === 'object_table') ?? workspace.data.views[0]
    if (view && active) saveLayout(dockTab(workspace.data.layout, view.id, zone === 'center' ? 'right' : zone, active.id))
  }

  const toggleLock = (item: ListedWorkspace) => {
    void api('/v1/workspaces', {
      method: 'POST',
      body: JSON.stringify({
        id: item.id,
        name: item.name,
        locked: !item.locked,
        share_token: item.share_token,
        layout: item.layout ?? { tabs: [] },
      }),
    }).then(() => {
      void queryClient.invalidateQueries({ queryKey: ['workspaces'] })
      void queryClient.invalidateQueries({ queryKey: ['workspace', item.id] })
      push(item.locked ? copy.unlockWorkspace : copy.lockWorkspace)
    })
    setTabMenu(null)
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
              onContextMenu={(event) => {
                event.preventDefault()
                setTabMenu({ x: event.clientX, y: event.clientY, id: item.id })
              }}
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
        <button className="icon-btn" onClick={() => setPaletteMode('ai')}>{copy.agent}</button>
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
      <pre className="activity" data-testid="activity-log" data-live={liveState}>{logText}</pre>
      <footer className="status">
        <span>{userEmail}</span>
        <span>{user.role}</span>
        <span data-testid="live-flag">{copy.connected}</span>
        {workspace.data?.locked ? <span>{copy.locked}</span> : null}
        <span className="grow" />
        <span>{statusLine}</span>
      </footer>
      {paletteMode ? (
        <CommandPalette
          key={paletteMode}
          ai={paletteMode === 'ai'}
          session={session}
          workspaces={workspaces.data?.workspaces ?? []}
          views={workspace.data?.views ?? []}
          onClose={() => setPaletteMode(null)}
        />
      ) : null}
      {tabMenu ? (
        <div className="menu" style={{ left: tabMenu.x, top: tabMenu.y }} data-testid="workspace-menu">
          {workspaces.data?.workspaces.filter((item) => item.id === tabMenu.id).map((item) => (
            <div key={item.id}>
              <button type="button" onClick={() => toggleLock(item)}>{item.locked ? copy.unlockWorkspace : copy.lockWorkspace}</button>
              <button
                type="button"
                onClick={() => {
                  const link = item.share_token ? `${window.location.origin}/share/${item.share_token}` : item.id
                  void navigator.clipboard?.writeText(link)
                  push(copy.copyShare)
                  setTabMenu(null)
                }}
              >
                {copy.copyShare}
              </button>
            </div>
          ))}
          <button
            type="button"
            onClick={() => {
              const template = workspaceTemplates[0]
              if (!template) return
              void createWorkspaceFromTemplate(template).then((id) => {
                void queryClient.invalidateQueries({ queryKey: ['workspaces'] })
                setWorkspaceId(id)
                push(copy.newWorkspace)
              })
              setTabMenu(null)
            }}
          >
            {copy.newWorkspace}
          </button>
          <button type="button" onClick={() => setTabMenu(null)}>{copy.close}</button>
        </div>
      ) : null}
      {help ? <Help onClose={() => setHelp(false)} /> : null}
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
  path = 'p',
}: {
  node: LayoutNode
  workspace: Workspace
  session: Session
  onLayout: (layout: LayoutNode) => void
  path?: string
}) {
  if (!isSplit(node)) {
    return <Pane leaf={node} workspace={workspace} session={session} onLayout={onLayout} root={workspace.layout} />
  }
  const left = `${path}a`
  const right = `${path}b`
  return (
    <Group
      className={`split ${node.direction}`}
      orientation={node.direction === 'row' ? 'horizontal' : 'vertical'}
      defaultLayout={{ [left]: node.ratio * 100, [right]: (1 - node.ratio) * 100 }}
      onLayoutChanged={(layout, meta) => {
        if (!meta.isUserInteraction || workspace.locked) return
        const next = (layout[left] ?? node.ratio * 100) / 100
        onLayout(replaceRatio(workspace.layout, node, Math.min(0.85, Math.max(0.15, next))))
      }}
    >
      <Panel className="pane-slot" id={left} minSize="15" defaultSize={`${node.ratio * 100}`}>
        <LayoutView path={left} node={node.first} workspace={workspace} session={session} onLayout={onLayout} />
      </Panel>
      <Separator className="divider" data-testid="split-divider" disabled={workspace.locked} />
      <Panel className="pane-slot" id={right} minSize="15" defaultSize={`${(1 - node.ratio) * 100}`}>
        <LayoutView path={right} node={node.second} workspace={workspace} session={session} onLayout={onLayout} />
      </Panel>
    </Group>
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
  useEffect(() => {
    return onViewRequest((token) => {
      const match = leaf.tabs.find((id) => {
        const tab = workspace.views.find((item) => item.id === id)
        return id === token || tab?.title === token || tab?.type === token
      })
      if (match) setActive(match)
    })
  }, [leaf.tabs, workspace.views])
  const acceptDock = (zone: Zone) => {
    const id = dockDrag
    setDockDrag(null)
    if (!id || !view || workspace.locked) return
    onLayout(dockTab(root, id, zone, view.id))
  }
  return (
    <section className="pane" data-pane tabIndex={-1}>
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
    <div
      className="tree"
      data-testid="structure-tree"
      tabIndex={0}
      onKeyDown={(event) => {
        if (!['ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter'].includes(event.key)) return
        const buttons = [...event.currentTarget.querySelectorAll('button')]
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
        if (event.key === 'Enter') {
          if (index >= 0) buttons[index]?.click()
          return
        }
        event.preventDefault()
        const next = event.key === 'ArrowDown'
          ? Math.min(buttons.length - 1, index + 1)
          : event.key === 'ArrowUp'
            ? Math.max(0, index < 0 ? buttons.length - 1 : index - 1)
            : event.key === 'Home'
              ? 0
              : buttons.length - 1
        buttons[next]?.focus()
      }}
    >
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



function movePaneFocus(key: string) {
  const panes = [...document.querySelectorAll<HTMLElement>('[data-pane]')]
  if (panes.length === 0) return
  const active = document.activeElement as HTMLElement | null
  const current = panes.findIndex((pane) => pane === active || pane.contains(active))
  const origin = (panes[Math.max(0, current)] ?? panes[0]).getBoundingClientRect()
  const score = (pane: HTMLElement) => {
    const box = pane.getBoundingClientRect()
    const dx = box.left + box.width / 2 - (origin.left + origin.width / 2)
    const dy = box.top + box.height / 2 - (origin.top + origin.height / 2)
    if (key === 'ArrowRight') return dx > 8 ? dx * dx + dy * dy : Number.POSITIVE_INFINITY
    if (key === 'ArrowLeft') return dx < -8 ? dx * dx + dy * dy : Number.POSITIVE_INFINITY
    if (key === 'ArrowDown') return dy > 8 ? dx * dx + dy * dy : Number.POSITIVE_INFINITY
    return dy < -8 ? dx * dx + dy * dy : Number.POSITIVE_INFINITY
  }
  const next = panes
    .map((pane, index) => ({ pane, index, score: score(pane) }))
    .filter((item) => item.index !== current && Number.isFinite(item.score))
    .sort((left, right) => left.score - right.score)[0]
  const target = next?.pane ?? panes[(Math.max(0, current) + 1) % panes.length]
  target.focus()
}

function Help({ onClose }: { onClose: () => void }) {
  return (
    <div className="help" onClick={onClose}>
      <div onClick={(event) => event.stopPropagation()}>
        <strong>{copy.shortcuts}</strong>
        <p><Kbd>Ctrl</Kbd>+<Kbd>K</Kbd> {copy.commandPlaceholder}</p>
        <p><Kbd>Ctrl</Kbd>+<Kbd>J</Kbd> {copy.agent}</p>
        <p><Kbd>Ctrl</Kbd>+<Kbd>Alt</Kbd>+arrows focus the next pane</p>
        <p><Kbd>Ctrl</Kbd>+<Kbd>1</Kbd>…<Kbd>9</Kbd> workspaces</p>
        <p><Kbd>F1</Kbd> {copy.shortcuts}</p>
        <button onClick={onClose}>{copy.close}</button>
      </div>
    </div>
  )
}

