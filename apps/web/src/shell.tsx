import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { barY, defineChart } from '@tanstack/charts'
import { scaleBand } from '@tanstack/charts/scales/band'
import { scaleLinear } from '@tanstack/charts/scales/linear'
import { Chart } from '@tanstack/react-charts'
import { ChevronDown, ChevronRight, Moon, Sun } from 'lucide-react'
import { api, type Json } from './api'
import { humanizeLive, objectIdFromLive } from './activity'
import { SearchRefresh } from './live-refresh'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Kbd } from './components/ui/kbd'
import { Toaster } from './components/ui/sonner'
import { workspaceDisplayName } from './demo-labels'
import { WorkspaceDock } from './dock'
import { flashObject } from './flash'
import { copy } from './i18n'
import { applyLayoutCommand, migrateLayout, zoneFromKey, type LayoutDoc, type Place } from './layout'
import { ConsequenceHost } from './consequence'
import { CommandPalette } from './palette'
import { createWorkspaceFromTemplate, workspaceTemplates } from './templates'
import { onUi } from './ui-bus'
import {
  LAYOUT_ENVIRONMENT,
  canonicalDoc,
  editsArePersonal,
  layoutStorageKey,
  migrateDoc,
  readLayoutStore,
  reconcileLayout,
  writeLayoutStore,
  type ServerLayout,
} from './workspace-doc'
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
type ListedWorkspace = { id: string; name: string; locked?: boolean; share_token?: string; layout?: unknown; owner_id?: string }
type Workspace = { id: string; name: string; locked: boolean; share_token?: string; layout: unknown; owner_id?: string; views: View[] }
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

const layoutCommands = new Set([
  'open_view', 'move_view', 'split', 'resize', 'swap', 'focus',
  'move_view_left', 'move_view_right', 'move_view_up', 'move_view_down',
  'zoom', 'close_view', 'get_layout', 'reset_layout',
  'ui.open_view', 'ui.place', 'ui.link',
])

function layoutCommand(name: string) {
  return layoutCommands.has(name)
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
  const [tabMenu, setTabMenu] = useState<{ x: number; y: number; id: string } | null>(null)
  const [doc, setDoc] = useState<LayoutDoc | null>(null)
  const docRef = useRef<LayoutDoc | null>(null)
  const dirtyRef = useRef(false)
  const saveGen = useRef(0)
  const canonicalRef = useRef<LayoutDoc['layout'] | undefined>(undefined)
  const placeOverride = useRef<Place | undefined>(undefined)
  const loadedFor = useRef('')
  const applyRef = useRef<(name: string, args: Record<string, unknown>) => void>(() => {})
  const paletteModeRef = useRef(paletteMode)
  paletteModeRef.current = paletteMode

  useEffect(() => {
    setBooted(true)
    const stored = readStore<string>('braian.theme', 'dark')
    setTheme(stored === 'light' ? 'light' : 'dark')
  }, [])
  useEffect(() => {
    if (!booted) return
    document.documentElement.dataset.theme = theme
    writeStore('braian.theme', theme)
  }, [theme, booted])

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
    const refresh = new SearchRefresh(
      () => {
        void queryClient.invalidateQueries({ queryKey: ['objects'] })
      },
      (id) => {
        void queryClient.invalidateQueries({ queryKey: ['object', id] })
        void queryClient.invalidateQueries({ queryKey: ['object-ref', id] })
        void queryClient.invalidateQueries({ queryKey: ['history', id] })
      },
    )
    source.onopen = () => setLiveState('open')
    source.onmessage = (event) => {
      setLiveState('message')
      const id = objectIdFromLive(event.data)
      if (id) flashObject(id)
      push(humanizeLive(event.data))
      refresh.onEvent(event.data)
    }
    source.onerror = () => {
      if (source.readyState === EventSource.CLOSED) push(copy.offline)
    }
    return () => {
      refresh.dispose()
      source.close()
    }
  }, [booted, me.isSuccess, queryClient])

  const userId = me.data?.user_id ?? ''
  const workspaceRef = useRef(workspace.data)
  workspaceRef.current = workspace.data

  useEffect(() => {
    if (workspace.data && workspace.data.id === workspaceId) {
      canonicalRef.current = migrateLayout(workspace.data.layout)
    }
  }, [workspace.data, workspaceId])

  useEffect(() => {
    const current = workspace.data
    if (!workspaceId || !userId || !current || current.id !== workspaceId) return
    const stamp = `${userId}:${workspaceId}`
    if (loadedFor.current === stamp) return
    loadedFor.current = stamp
    const key = layoutStorageKey(userId, workspaceId)
    const canonical = canonicalDoc(current)
    canonicalRef.current = canonical.layout
    const local = readLayoutStore(key)
    const painted = local ? migrateDoc(local.document) : canonical
    dirtyRef.current = false
    docRef.current = painted
    setDoc(painted)
    let cancelled = false
    void api<ServerLayout>(`/v1/workspaces/${workspaceId}/state?environment=${LAYOUT_ENVIRONMENT}`)
      .then((server) => {
        if (cancelled || dirtyRef.current || loadedFor.current !== stamp) return
        const fresh = readLayoutStore(key)
        const next = reconcileLayout(fresh, server, canonical)
        const serverAt = server.updated_at ? Date.parse(server.updated_at) : 0
        const serverDoc = server.document ? migrateDoc(server.document) : null
        const diverged = Boolean(
          fresh && fresh.savedAt > serverAt && (!serverDoc || JSON.stringify(next) !== JSON.stringify(serverDoc)),
        )
        docRef.current = next
        dirtyRef.current = diverged
        if (diverged) saveGen.current += 1
        setDoc(next)
        writeLayoutStore(key, next)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [workspaceId, userId, workspace.data?.id])

  useEffect(() => {
    if (!doc || !dirtyRef.current || !workspaceId || !userId) return
    const gen = saveGen.current
    const timer = window.setTimeout(() => {
      const current = docRef.current
      const ws = workspaceRef.current
      if (!current || !ws || ws.id !== workspaceId || saveGen.current !== gen) return
      const body = JSON.stringify({ environment: LAYOUT_ENVIRONMENT, document: current })
      void api(`/v1/workspaces/${workspaceId}/state`, { method: 'PUT', body }).then(() => {
        if (saveGen.current === gen) dirtyRef.current = false
      })
      if (!editsArePersonal(ws)) {
        void api('/v1/workspaces', {
          method: 'POST',
          body: JSON.stringify({
            id: ws.id,
            name: ws.name,
            locked: ws.locked,
            share_token: ws.share_token,
            layout: current.layout,
          }),
        }).then(() => {
          canonicalRef.current = current.layout
        })
      }
    }, 700)
    return () => window.clearTimeout(timer)
  }, [doc, workspaceId, userId])

  useEffect(() => {
    const flush = () => {
      if (sessionStorage.getItem('braian.layout.suppressFlush') === '1') return
      const current = docRef.current
      if (!dirtyRef.current || !current || !workspaceId) return
      void fetch(`/v1/workspaces/${workspaceId}/state`, {
        method: 'PUT',
        credentials: 'include',
        keepalive: true,
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ environment: LAYOUT_ENVIRONMENT, document: current }),
      })
    }
    const onHide = () => {
      if (document.visibilityState === 'hidden') flush()
    }
    window.addEventListener('beforeunload', flush)
    document.addEventListener('visibilitychange', onHide)
    return () => {
      window.removeEventListener('beforeunload', flush)
      document.removeEventListener('visibilitychange', onHide)
    }
  }, [workspaceId])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const typing = event.target instanceof HTMLElement
        && (event.target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(event.target.tagName))
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setPaletteMode('list')
        return
      }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'j') {
        event.preventDefault()
        setPaletteMode('ai')
        return
      }
      if (event.key === 'Escape') {
        setHelp(false)
        setTabMenu(null)
        return
      }
      if (event.key === 'F1') {
        event.preventDefault()
        setHelp(true)
        return
      }
      if (event.ctrlKey && !event.altKey && !event.metaKey && event.key >= '1' && event.key <= '9') {
        const index = Number(event.key) - 1
        const id = workspaces.data?.workspaces[index]?.id
        if (id) {
          event.preventDefault()
          setWorkspaceId(id)
        }
        return
      }
      if (paletteModeRef.current || typing) return
      const arrow = zoneFromKey(event.key)
      if ((event.ctrlKey || event.metaKey) && event.altKey && event.shiftKey && arrow) {
        event.preventDefault()
        const sign = arrow === 'left' || arrow === 'top' ? -1 : 1
        applyRef.current('resize', { direction: arrow, delta: sign * 0.05 })
        return
      }
      if ((event.ctrlKey || event.metaKey) && event.altKey && arrow) {
        event.preventDefault()
        const name = arrow === 'left' ? 'move_view_left' : arrow === 'right' ? 'move_view_right' : arrow === 'top' ? 'move_view_up' : 'move_view_down'
        applyRef.current(name, {})
        return
      }
      if (event.altKey && !event.ctrlKey && !event.metaKey && arrow) {
        event.preventDefault()
        applyRef.current('focus', { direction: arrow })
        return
      }
      if ((event.ctrlKey || event.metaKey) && event.altKey && event.key.toLowerCase() === 'w') {
        event.preventDefault()
        applyRef.current('close_view', {})
        return
      }
      if ((event.ctrlKey || event.metaKey) && event.altKey && event.key.toLowerCase() === 'z') {
        event.preventDefault()
        applyRef.current('zoom', {})
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
      if (layoutCommand(event.name)) applyRef.current(event.name, event.args)
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
  const surface = workspace.data

  const commitDoc = (next: LayoutDoc) => {
    if (!workspaceId || !user.user_id) return
    docRef.current = next
    dirtyRef.current = true
    saveGen.current += 1
    writeLayoutStore(layoutStorageKey(user.user_id, workspaceId), next)
    setDoc(next)
  }
  applyRef.current = (name, args) => {
    const current = docRef.current
    if (!current || !workspace.data) return
    let nextArgs = args
    if ((name === 'open_view' || name === 'ui.open_view') && typeof args.entity === 'string') {
      const entity = args.entity
      const known = workspace.data.views.some((view) => view.id === entity || view.title === entity || view.type === entity)
      if (!known) {
        setObjectId(entity)
        nextArgs = { ...args, title: 'Detail' }
        delete nextArgs.entity
        delete nextArgs.view
        delete nextArgs.id
      }
    }
    const place = name === 'open_view' || name === 'ui.open_view' || name === 'ui.place' ? placeOverride.current : undefined
    const result = applyLayoutCommand(current, name, nextArgs, {
      views: workspace.data.views.map((view) => ({ id: view.id, title: view.title, type: view.type })),
      canonical: canonicalRef.current,
      place,
    })
    if (result.handled && result.doc !== current) commitDoc(result.doc)
    const focus = result.doc.focus
    if (focus) {
      requestAnimationFrame(() => {
        document.querySelector<HTMLElement>(`[data-pane="${CSS.escape(focus)}"]`)?.focus()
      })
    }
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
    <div className="app">
      <header className="app-bar">
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
              {index < 9 ? <span className="tab-index">{index + 1}</span> : null}
            </button>
          ))}
        </nav>
        <span className="spacer" />
        <button className="icon-btn" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}>
          {theme === 'dark' ? <Sun aria-hidden="true" size={14} /> : <Moon aria-hidden="true" size={14} />}
          {theme === 'dark' ? copy.themeLight : copy.themeDark}
        </button>
        <button className="icon-btn" onClick={() => setPaletteMode('ai')}>{copy.agent}</button>
        <span className="who">{userName}</span>
        <button
          className="icon-btn"
          onClick={() => void api('/v1/logout', { method: 'POST' }).then(() => queryClient.invalidateQueries({ queryKey: ['me'] }))}
        >
          {copy.signOut}
        </button>
      </header>
      <main className="workspace" data-testid="workspace">
        {doc && surface ? (
          <WorkspaceDock
            doc={doc}
            views={surface.views}
            onChange={commitDoc}
            renderView={(item) => {
              const full = surface.views.find((view) => view.id === item.id)
              return full ? <ViewBody view={full} session={session} workspace={surface} /> : <p>{copy.empty}</p>
            }}
            onAddView={(target) => {
              placeOverride.current = { target, zone: 'center' }
              setPaletteMode('list')
            }}
          />
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
      <ConsequenceHost session={session} />
      {paletteMode ? (
        <CommandPalette
          key={paletteMode}
          ai={paletteMode === 'ai'}
          session={session}
          workspaces={workspaces.data?.workspaces ?? []}
          views={workspace.data?.views ?? []}
          onClose={() => {
            placeOverride.current = undefined
            setPaletteMode(null)
          }}
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
          <button
            type="button"
            onClick={() => {
              applyRef.current('reset_layout', {})
              setTabMenu(null)
            }}
          >
            {copy.resetLayout}
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
  const counts = useQuery({
    queryKey: ['group-counts'],
    queryFn: () => api<Json>('/v1/search?group=groups&per_page=1'),
  })
  const [openKeys, setOpenKeys] = useState<string[]>(() => (view.state.expanded as string[] | undefined) ?? [])
  const structures = (defs.data?.structures as Json[] | undefined) ?? []
  const countOf = (structure: string, key: string) => {
    const groups = (counts.data?.groups as Json[] | undefined) ?? []
    const hit = groups.find((group) => String(group.key) === `${structure}:${key}`)
    return Number(hit?.found ?? 0)
  }
  const toggle = (key: string) => {
    setOpenKeys((current) => (current.includes(key) ? current.filter((item) => item !== key) : [...current, key]))
  }
  return (
    <div
      className="tree"
      data-testid="structure-tree"
      tabIndex={0}
      onKeyDown={(event) => {
        if (!['ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter'].includes(event.key)) return
        const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('.tree-row')]
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
        const structureKey = String(structure.key)
        const renderGroup = (parent: string | undefined, depth: number) =>
          groups
            .filter((group) => (parent ? String(group.parent ?? '') === parent : !group.parent))
            .map((group) => {
              const key = String(group.key)
              const id = String(group.id)
              const kids = groups.some((item) => String(item.parent ?? '') === id)
              const open = openKeys.includes(key)
              const count = countOf(structureKey, key)
              const selected = session.group === `${structureKey}:${key}`
              return (
                <div key={id} className="tree-branch">
                  <button
                    type="button"
                    className={selected ? 'tree-row on' : 'tree-row'}
                    style={{ paddingLeft: 8 + depth * 16 }}
                    onClick={() => session.setGroup(`${structureKey}:${key}`)}
                  >
                    {Array.from({ length: depth }, (_, level) => (
                      <span key={level} className="tree-guide" style={{ left: 15 + level * 16 }} aria-hidden="true" />
                    ))}
                    {kids ? (
                      <span
                        className="tree-chevron"
                        aria-hidden="true"
                        onClick={(event) => {
                          event.stopPropagation()
                          toggle(key)
                        }}
                      >
                        {open ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
                      </span>
                    ) : (
                      <span className="tree-chevron" aria-hidden="true" />
                    )}
                    <span className="tree-label">{key}</span>
                    {count > 0 ? <span className="tree-count">{count}</span> : null}
                  </button>
                  {kids && open ? renderGroup(id, depth + 1) : null}
                </div>
              )
            })
        return (
          <div key={structureKey}>
            <strong>{structureKey}</strong>
            {renderGroup(undefined, 0)}
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




function Help({ onClose }: { onClose: () => void }) {
  return (
    <div className="help" onClick={onClose}>
      <div onClick={(event) => event.stopPropagation()}>
        <strong>{copy.shortcuts}</strong>
        <p><Kbd>Ctrl</Kbd>+<Kbd>K</Kbd> {copy.commandPlaceholder}</p>
        <p><Kbd>Ctrl</Kbd>+<Kbd>J</Kbd> {copy.agent}</p>
        <p><Kbd>Ctrl</Kbd>+<Kbd>Alt</Kbd>+arrows move the focused view</p>
        <p><Kbd>Alt</Kbd>+arrows focus the neighbour</p>
        <p><Kbd>Ctrl</Kbd>+<Kbd>Alt</Kbd>+<Kbd>Shift</Kbd>+arrows resize</p>
        <p><Kbd>Ctrl</Kbd>+<Kbd>Alt</Kbd>+<Kbd>Z</Kbd> {copy.maximize}</p>
        <p><Kbd>Ctrl</Kbd>+<Kbd>Alt</Kbd>+<Kbd>W</Kbd> {copy.closeView}</p>
        <p><Kbd>Ctrl</Kbd>+<Kbd>1</Kbd>…<Kbd>9</Kbd> workspaces</p>
        <p><Kbd>F1</Kbd> {copy.shortcuts}</p>
        <button onClick={onClose}>{copy.close}</button>
      </div>
    </div>
  )
}

