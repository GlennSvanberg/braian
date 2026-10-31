import { useEffect, useMemo, useRef, useState } from 'react'
import { ImportMappingsView } from './import/mappings-view'
import { flushSync } from 'react-dom'
import { useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { ChevronDown, ChevronRight, File, Folder, FolderOpen } from 'lucide-react'
import { StatusBar, useOnline } from './status-bar'
import { isConnected, type LiveState } from './status-message'
import { UserMenu } from './user-menu'
import { api, freeObject, jsonRecord, keepaliveJsonPut, operationId, recordTitle, textValue, type Json } from './api'
import { AssortmentList, NewSelectionDialog } from './assortments'
import { humanizeLive, objectIdFromLive } from './activity'
import { ImportScreen } from './import/view'
import { dropImportFiles, onImportLive, openImport, resumeImport, setImportDrag } from './import/controller'
import { importRefreshesObjects, isFileDrag } from './import/model'
import { SearchRefresh } from './live-refresh'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Kbd } from './components/ui/kbd'
import { Toaster } from './components/ui/sonner'
import { workspaceDisplayName } from './demo-labels'
import { WorkspaceDock } from './dock'
import { flashObject } from './flash'
import { structureCaption } from './graph-scene'
import { dropReparents, groupKeyFromLabel } from './group-tree'
import { publishProposal } from './proposal-bus'
import { formatCount, formatTimestamp } from './format'
import { toast } from 'sonner'
import {
  clearQueuedOpenTypeView,
  peekQueuedOpenTypeView,
  queueOpenTypeView,
  takeQueuedOpenTypeView,
} from './open-type-view-queue'
import { copy } from './i18n'
import { applyLayoutCommand, migrateLayout, views as layoutViews, type LayoutDoc, type Place } from './layout'
import { ConsequenceHost } from './consequence'
import { ModelDraftHost } from './model-draft-ui'
import { stageModel } from './model-stage'
import { AttributeDefinitions } from './attribute-definitions'
import { ModelTableView } from './model-tables'
import { clearQueuedViewType, peekQueuedViewType } from './attribute-actions'
import { FeedbackDialog } from './feedback-dialog'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from './components/ui/dialog'
import { installClientLog } from './feedback-log'
import { prepareFeedbackCapture, type FeedbackCapture } from './feedback'
import { CommandPalette } from './palette'
import { SharedContextMenuHost } from './shared-context-menu'
import { StateNote } from './states'
import { requestNewSelection } from './assortment-filter'
import { dispatchUi } from './ui-bus'
import { readUiContext } from './ui-context'
import { registerCommandRunner, runCommand } from './command-registry'
import { useShortcutBindings } from './shortcut-bindings'
import { shortcuts } from './shortcuts'
import { registerModelCommandRunners } from './model-commands'
import { createWorkspaceFromTemplate, typeWorkspaceTemplate, workspaceTemplates } from './templates'
import { missingTypeWorkspaces, orderWorkspaces, typeKeyFromShare, typeShareToken } from './type-workspaces'
import {
  readActiveWorkspaceId,
  resolveActiveWorkspaceId,
  writeActiveWorkspaceId,
} from './active-workspace'
import { healDataModelWorkspace, isKnownViewType, layoutHasMissingViews, migrateLegacyModelViews } from './model-workspace-migrate'
import { persistWorkspaceMigration } from './workspace-migration-persist'
import { commandPlaceholder, displayViewTitle, migrateSavedWorkspace, singularOf, typeNames } from './type-labels'
import { onUi } from './ui-bus'
import { panesFromCatalog, publishFrame, publishMarked } from './ui-context'
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
  ClassAttributes,
  DataModel,
  GraphView,
  HistoryView,
  JobHistory,
  ObjectDetail,
  ObjectTable,
  ProposalView,
  ProposalListView,
} from './panes'

type View = { id: string; type: string; title: string; pane?: string; position?: number; state: Json }
type ListedWorkspace = { id: string; name: string; locked?: boolean; share_token?: string | null; layout?: Json | null; owner_id?: string }
type Workspace = { id: string; name: string; locked: boolean; share_token?: string | null; layout: Json; owner_id?: string; views?: View[] | null }

function viewRows<T>(views: readonly T[] | null | undefined): T[] {
  return views ? [...views] : []
}
type Me = {
  email?: string
  role: string
  display_name?: string
  handle?: string
  user_id?: string
  feedback?: boolean
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
  listType: string | null
  workspaceType?: string
  setListType: (value: string | null) => void
  push: (line: string) => void
  openWorkspace: (id: string) => void
  openTypeView: (typeId: string) => void
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
  const me = useQuery({ queryKey: ['me'], queryFn: () => api.get('/v1/me'), retry: false })
  const [theme, setTheme] = useState<'dark' | 'light'>('dark')
  const themeRef = useRef(theme)
  themeRef.current = theme
  const signOutRef = useRef<() => void>(() => {})
  const [booted, setBooted] = useState(false)
  const [signedOut, setSignedOut] = useState(false)
  const [workspaceId, setWorkspaceId] = useState<string | null>(null)
  const [group, setGroup] = useState<string | null>(null)
  const [objectId, setObjectId] = useState<string | null>(null)
  const [listType, setListType] = useState<string | null>(null)
  const [proposalId, setProposalId] = useState<string | null>(null)
  const [logText, setLogText] = useState('')
  const [liveState, setLiveState] = useState<LiveState>('connecting')
  const liveStateRef = useRef<LiveState>('connecting')
  liveStateRef.current = liveState
  const [streamEpoch, setStreamEpoch] = useState(0)
  const online = useOnline()
  const [paletteMode, setPaletteMode] = useState<null | 'list' | 'ai' | 'fields'>(null)
  /** Bumped on every Ctrl+K / palette:search so list opens at root even if mode was already list. */
  const [paletteEpoch, setPaletteEpoch] = useState(0)
  const [fieldsEditColumnId, setFieldsEditColumnId] = useState<string | null>(null)
  useEffect(() => {
    const close = () => setPaletteMode(null)
    window.addEventListener('braian-close-palette', close)
    return () => window.removeEventListener('braian-close-palette', close)
  }, [])
  useEffect(() => {
    const onFields = (event: Event) => {
      const detail = (event as CustomEvent<{ editColumnId?: string | null }>).detail
      setFieldsEditColumnId(detail?.editColumnId ?? null)
      setPaletteMode('fields')
      window.setTimeout(() => {
        document.querySelector<HTMLInputElement>('[data-testid="fields-mode-input"]')?.focus()
      }, 0)
    }
    window.addEventListener('braian-fields-palette', onFields)
    return () => window.removeEventListener('braian-fields-palette', onFields)
  }, [])

  const [paletteStartAddView, setPaletteStartAddView] = useState(false)
  const [paletteStartNewFromTemplate, setPaletteStartNewFromTemplate] = useState(false)
  const [feedbackOpen, setFeedbackOpen] = useState(false)
  const [feedbackCapture, setFeedbackCapture] = useState<FeedbackCapture | null>(null)
  const [help, setHelp] = useState(false)
  const [tabMenu, setTabMenu] = useState<{ x: number; y: number; id: string } | null>(null)
  const [deleteConfirm, setDeleteConfirm] = useState<{ id: string; name: string } | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [doc, setDoc] = useState<LayoutDoc | null>(null)
  // Gate dock paint while a dangling/incomplete Data model layout is healed in one pass.
  const [layoutHealPending, setLayoutHealPending] = useState(false)
  const docRef = useRef<LayoutDoc | null>(null)
  const healPendingFor = useRef<string | null>(null)
  const dirtyRef = useRef(false)
  const saveGen = useRef(0)
  const canonicalRef = useRef<LayoutDoc['layout'] | undefined>(undefined)
  const placeOverride = useRef<Place | undefined>(undefined)
  const loadedFor = useRef('')
  const applyRef = useRef<(name: string, args: Record<string, unknown>) => void>(() => {})
  const openTypeViewRef = useRef<(typeId: string) => void>(() => {})
  const migratedRef = useRef(new Set<string>())
  const ensuredTypes = useRef(new Set<string>())
  const workspaceIdRef = useRef<string | null>(null)
  const workspacesRef = useRef<ListedWorkspace[]>([])
  const requestDeleteActiveRef = useRef<() => void>(() => {})
  const paletteModeRef = useRef(paletteMode)
  paletteModeRef.current = paletteMode
  const openFeedback = () => {
    void prepareFeedbackCapture().then((capture) => {
      setFeedbackCapture(capture)
      setFeedbackOpen(true)
    })
  }

  useEffect(() => {
    installClientLog()
  }, [])

  useEffect(() => {
    const files = (event: DragEvent) => isFileDrag(event.dataTransfer?.types ?? [])
    let depth = 0
    const enter = (event: DragEvent) => {
      if (!files(event)) return
      event.preventDefault()
      depth += 1
      setImportDrag(true)
    }
    const over = (event: DragEvent) => {
      if (!files(event)) return
      event.preventDefault()
    }
    const leave = (event: DragEvent) => {
      if (!files(event)) return
      depth = Math.max(0, depth - 1)
      if (depth === 0) setImportDrag(false)
    }
    const drop = (event: DragEvent) => {
      if (!files(event)) return
      event.preventDefault()
      depth = 0
      setImportDrag(false)
      const list = [...(event.dataTransfer?.files ?? [])]
      if (list.length > 0) dropImportFiles(list)
    }
    const open = () => openImport()
    window.addEventListener('dragenter', enter)
    window.addEventListener('dragover', over)
    window.addEventListener('dragleave', leave)
    window.addEventListener('drop', drop)
    window.addEventListener('braian-import-open', open)
    const params = new URLSearchParams(window.location.search)
    const sessionId = params.get('import')
    if (sessionId && !params.get('importPreview')) void resumeImport(sessionId)
    return () => {
      window.removeEventListener('dragenter', enter)
      window.removeEventListener('dragover', over)
      window.removeEventListener('dragleave', leave)
      window.removeEventListener('drop', drop)
      window.removeEventListener('braian-import-open', open)
    }
  }, [])

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
    const stamp = formatTimestamp(new Date())
    const text = `${stamp} ${line}`
    setLogText((current) => [text, ...current.split('\n').filter(Boolean)].slice(0, 6).join('\n'))
  }

  const workspaces = useQuery({
    queryKey: ['workspaces'],
    queryFn: () => api.get('/v1/workspaces'),
    enabled: booted && me.isSuccess,
  })
  const defs = useQuery({
    queryKey: ['definitions'],
    queryFn: () => api.get('/v1/definitions'),
    enabled: booted && me.isSuccess,
    staleTime: 60_000,
  })

  workspaceIdRef.current = workspaceId
  workspacesRef.current = workspaces.data?.workspaces ?? []

  const orderedWorkspaces = useMemo(
    () => orderWorkspaces(workspaces.data?.workspaces ?? [], typeNames(defs.data)),
    [workspaces.data, defs.data],
  )
  useEffect(() => {
    const user = me.data?.user_id ?? ''
    if (!user || orderedWorkspaces.length === 0) return
    const stored = readActiveWorkspaceId(user)
    const next = resolveActiveWorkspaceId(orderedWorkspaces, workspaceId, stored)
    if (next && next !== workspaceId) setWorkspaceId(next)
  }, [orderedWorkspaces, workspaceId, me.data?.user_id])

  useEffect(() => {
    const user = me.data?.user_id ?? ''
    if (!user || !workspaceId) return
    writeActiveWorkspaceId(user, workspaceId)
  }, [workspaceId, me.data?.user_id])

  useEffect(() => {
    const rows = workspaces.data?.workspaces
    const types = typeNames(defs.data)
    if (!rows || types.length === 0) return
    const missing = missingTypeWorkspaces(rows, types).filter((type) => !ensuredTypes.current.has(type.key))
    if (missing.length === 0) return
    for (const type of missing) ensuredTypes.current.add(type.key)
    void (async () => {
      for (const type of missing) {
        try {
          await createWorkspaceFromTemplate(typeWorkspaceTemplate(type), { shareToken: typeShareToken(type.key) })
        } catch {
          ensuredTypes.current.delete(type.key)
        }
      }
      void queryClient.invalidateQueries({ queryKey: ['workspaces'] })
    })()
  }, [workspaces.data, defs.data, queryClient])

  const workspace = useQuery({
    queryKey: ['workspace', workspaceId],
    queryFn: () => api.get('/v1/workspaces/{id}', { params: { path: { id: workspaceId ?? '' } } }),
    enabled: Boolean(workspaceId),
  })
  const opened = useQuery({
    queryKey: ['object', objectId],
    queryFn: () => api.get('/v1/objects/{id}', { params: { path: { id: objectId ?? '' } } }),
    enabled: Boolean(objectId),
  })
  const feedbackEnabled = me.data?.feedback === true

  useEffect(() => {
    const stops = [
      registerCommandRunner('palette:search', () => {
        setFieldsEditColumnId(null)
        setPaletteStartAddView(false)
        setPaletteStartNewFromTemplate(false)
        setPaletteMode('list')
        setPaletteEpoch((value) => value + 1)
      }),

      registerCommandRunner('feedback:send', () => openFeedback()),
      registerCommandRunner('theme:toggle', () => {
        setTheme(themeRef.current === 'dark' ? 'light' : 'dark')
      }),
      registerCommandRunner('agent:open', () => {
        setFieldsEditColumnId(null)
        setPaletteStartAddView(false)
        setPaletteStartNewFromTemplate(false)
        setPaletteMode('ai')
        setPaletteEpoch((value) => value + 1)
      }),
      registerCommandRunner('session:sign-out', () => {
        signOutRef.current()
      }),
      registerCommandRunner('layout:reset_layout', () => applyRef.current('reset_layout', {})),
      registerCommandRunner('workspace:delete', () => { requestDeleteActiveRef.current() }),

      registerCommandRunner('layout:close_view', (target) =>
        applyRef.current('close_view', target.viewId ? { view: target.viewId } : {}),
      ),
      registerCommandRunner('layout:zoom', (target) =>
        applyRef.current('zoom', target.viewId ? { view: target.viewId } : {}),
      ),
      registerCommandRunner('layout:move_view_left', (target) =>
        applyRef.current('move_view_left', target.viewId ? { view: target.viewId } : {}),
      ),
      registerCommandRunner('layout:move_view_right', (target) =>
        applyRef.current('move_view_right', target.viewId ? { view: target.viewId } : {}),
      ),
      registerCommandRunner('layout:move_view_up', (target) =>
        applyRef.current('move_view_up', target.viewId ? { view: target.viewId } : {}),
      ),
      registerCommandRunner('layout:move_view_down', (target) =>
        applyRef.current('move_view_down', target.viewId ? { view: target.viewId } : {}),
      ),
      registerCommandRunner('layout:add-view', () => {
        const focus = docRef.current?.focus
        placeOverride.current = focus ? { target: focus, zone: 'center' } : { zone: 'center' }
        setPaletteStartNewFromTemplate(false)
        setPaletteStartAddView(true)
        setPaletteMode('list')
      }),
      registerCommandRunner('workspace:new', () => {
        setFieldsEditColumnId(null)
        setPaletteStartAddView(false)
        setPaletteStartNewFromTemplate(true)
        setPaletteMode('list')
        setPaletteEpoch((value) => value + 1)
      }),
      registerCommandRunner('selection:new', () => requestNewSelection()),
      registerCommandRunner('layout:split-left', (target) => applyRef.current('split', { view: target.viewId, zone: 'left' })),
      registerCommandRunner('layout:split-right', (target) => applyRef.current('split', { view: target.viewId, zone: 'right' })),
      registerCommandRunner('layout:split-top', (target) => applyRef.current('split', { view: target.viewId, zone: 'top' })),
      registerCommandRunner('layout:split-bottom', (target) => applyRef.current('split', { view: target.viewId, zone: 'bottom' })),
      registerCommandRunner('columns:hide', (target) => {
        if (target.column) dispatchUi('ui.hide_column', { column: String(target.column) })
      }),
      registerModelCommandRunners(),
    ]
    const onCommand = (event: Event) => {
      const detail = (event as CustomEvent<{ id: string; target: Record<string, unknown> }>).detail
      if (!detail?.id?.startsWith('columns:show:')) return
      const column = detail.id.slice('columns:show:'.length)
      dispatchUi('ui.show_column', { column })
    }
    const onWorkspaceDelete = () => { requestDeleteActiveRef.current() }
    window.addEventListener('braian:command', onCommand)
    window.addEventListener('braian-workspace-delete', onWorkspaceDelete)
    return () => {
      stops.forEach((stop) => stop())
      window.removeEventListener('braian:command', onCommand)
      window.removeEventListener('braian-workspace-delete', onWorkspaceDelete)
    }
  }, [])


  useEffect(() => {
    if (!booted || !me.isSuccess) return
    const source = new EventSource('/v1/stream', { withCredentials: true })
    const refresh = new SearchRefresh(
      () => {
        // One sweep: Typesense hits (Version) and pathValues (Description / hops)
        // must invalidate together or the row paints in two steps.
        void queryClient.invalidateQueries({ queryKey: ['objects'] })
        void queryClient.invalidateQueries({ queryKey: ['path-values'] })
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
      onImportLive(event.data)
      const sentence = humanizeLive(event.data, singularFromLive(event.data, queryClient.getQueryData<Json>(['definitions'])))
      if (sentence) push(sentence)
      if (importRefreshesObjects(event.data)) {
        void queryClient.invalidateQueries({ queryKey: ['objects'] })
      }
      refresh.onEvent(event.data)
    }
    source.onerror = () => {
      // CONNECTING: the browser retries on its own. CLOSED: only a new stream recovers.
      setLiveState(source.readyState === EventSource.CLOSED ? 'closed' : 'reconnecting')
    }
    return () => {
      refresh.dispose()
      source.close()
    }
  }, [booted, me.isSuccess, queryClient, streamEpoch])

  // Back online with a broken stream: reconnect now instead of waiting for the retry timer.
  useEffect(() => {
    if (online && liveStateRef.current !== 'open' && liveStateRef.current !== 'message' && liveStateRef.current !== 'connecting') {
      setStreamEpoch((value) => value + 1)
    }
  }, [online])

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
    const listed = viewRows(current.views)
    // Heal against the layout we would paint (local/personal), not only the server
    // workspace layout — otherwise a healed server + stale personal doc keeps
    // Unavailable tabs across reloads without re-entering heal.
    const pendingHeal = healDataModelWorkspace(current, listed, painted.layout).changed
      || healDataModelWorkspace(current, listed, canonical.layout).changed
    dirtyRef.current = false
    if (pendingHeal) {
      // Force the heal effect to run even if a prior no-op marked this id.
      migratedRef.current.delete(current.id)
      healPendingFor.current = current.id
      setLayoutHealPending(true)
      docRef.current = null
      setDoc(null)
      return
    }
    healPendingFor.current = null
    setLayoutHealPending(false)
    docRef.current = painted
    setDoc(painted)
    let cancelled = false
    void api.get('/v1/workspaces/{id}/state', { params: { path: { id: workspaceId }, query: { environment: LAYOUT_ENVIRONMENT } } })
      .then((server) => {
        if (cancelled || dirtyRef.current || loadedFor.current !== stamp) return
        if (healPendingFor.current === current.id) return
        const fresh = readLayoutStore(key)
        const next = reconcileLayout(fresh, server, canonical)
        if (
          layoutHasMissingViews(next.layout, listed)
          || healDataModelWorkspace(current, listed, next.layout).changed
        ) {
          // Stale personal state still references missing views — let heal rebuild once.
          healPendingFor.current = current.id
          setLayoutHealPending(true)
          docRef.current = null
          setDoc(null)
          migratedRef.current.delete(current.id)
          return
        }
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
      void api.put('/v1/workspaces/{id}/state', {
        params: { path: { id: workspaceId } },
        body: { environment: LAYOUT_ENVIRONMENT, document: freeObject(current) },
      }).then(() => {
        if (saveGen.current === gen) dirtyRef.current = false
      })
      if (!editsArePersonal(ws)) {
        void api.post('/v1/workspaces', { body: {
            id: ws.id,
            name: ws.name,
            locked: ws.locked,
            share_token: ws.share_token,
            layout: current.layout,
          } }).then(() => {
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
      keepaliveJsonPut(`/v1/workspaces/${workspaceId}/state`, {
        environment: LAYOUT_ENVIRONMENT,
        document: current,
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

  useShortcutBindings({
    openPalette: () => {
      // Remount at list root (clears Fields › / stack left from Select fields ›) and
      // focus synchronously so the first typed character is not dropped.
      flushSync(() => {
        setFieldsEditColumnId(null)
        setPaletteStartAddView(false)
        setPaletteStartNewFromTemplate(false)
        setPaletteMode('list')
        setPaletteEpoch((value) => value + 1)
      })
      document.querySelector<HTMLInputElement>('[data-testid="palette-input"]')?.focus()
    },
    openAi: () => { void runCommand('agent:open') },
    saveReview: () => window.dispatchEvent(new Event('braian-review-save')),
    openFeedback: feedbackEnabled ? openFeedback : undefined,
    openHelp: () => setHelp(true),
    closeHelp: () => {
      setHelp(false)
      setTabMenu(null)
    },
    openWorkspaceAt: (index) => {
      const id = orderWorkspaces(workspaces.data?.workspaces ?? [], typeNames(defs.data))[index]?.id
      if (id) setWorkspaceId(id)
    },
    focusPane: (direction) => applyRef.current('focus', { direction }),
    moveView: (direction) => {
      const name =
        direction === 'left'
          ? 'move_view_left'
          : direction === 'right'
            ? 'move_view_right'
            : direction === 'top'
              ? 'move_view_up'
              : 'move_view_down'
      applyRef.current(name, {})
    },
    resize: (direction, delta) => applyRef.current('resize', { direction, delta }),
    zoom: () => applyRef.current('zoom', {}),
    closeView: () => applyRef.current('close_view', {}),
    moveColumn: (direction) => {
      const column = readUiContext().activeColumn
      if (!column) return
      dispatchUi('ui.move_column', { column, direction })
    },
    paletteOpen: Boolean(paletteMode),
    helpOpen: help,
    feedbackEnabled,
  })


  useEffect(() => {
    publishMarked(objectId)
  }, [objectId])

  useEffect(() => {
    const surface = workspace.data
    if (!doc || !surface || surface.id !== workspaceId) return
    const openIds = doc.zoom ? [doc.zoom] : layoutViews(doc.layout)
    const focused = doc.focus && openIds.includes(doc.focus) ? doc.focus : openIds[0] ?? null
    const detail = viewRows(surface.views).find((view) => view.type === 'object_detail')
    const language = typeof detail?.state.language === 'string' ? detail.state.language : 'en'
    const workspaceType = typeKeyFromShare(surface.share_token)
    publishFrame({
      workspace: {
        id: surface.id,
        name: workspaceDisplayName(surface.name, surface.share_token),
        ...(workspaceType ? { type: workspaceType } : {}),
      },
      panes: panesFromCatalog(openIds, focused, viewRows(surface.views)),
      focused,
      language,
    })
  }, [doc, workspace.data, workspaceId])


  const openTypeQueueWorkspaceRef = useRef<string | null>(null)
  useEffect(() => {
    if (openTypeQueueWorkspaceRef.current === null) {
      openTypeQueueWorkspaceRef.current = workspaceId
      return
    }
    if (openTypeQueueWorkspaceRef.current !== workspaceId) {
      openTypeQueueWorkspaceRef.current = workspaceId
      clearQueuedOpenTypeView()
    }
  }, [workspaceId])

  useEffect(() => {
    const typeId = peekQueuedOpenTypeView()
    if (!typeId) return
    if (!workspace.data || workspace.data.id !== workspaceId || !docRef.current) return
    takeQueuedOpenTypeView()
    openTypeViewRef.current(typeId)
  }, [workspace.data, workspaceId, doc])

  useEffect(() => {
    const surface = workspace.data
    const wanted = peekQueuedViewType()
    if (!surface || !wanted) return
    const found = viewRows(surface.views).find((view) => view.type === wanted)
    if (!found) return
    clearQueuedViewType()
    applyRef.current('ui.open_view', { id: found.id, zone: 'center' })
  }, [workspace.data])

  useEffect(() => {
    const openList = (event: Event) => {
      const key = String((event as CustomEvent<{ key?: string }>).detail?.key ?? '')
      const surfaceNow = workspaceRef.current
      const docNow = docRef.current
      if (!key || !surfaceNow || !docNow) return
      void (async () => {
        let views: View[] = viewRows(surfaceNow.views)
        let found = views.find((view) => view.type === 'value_list_values')
        const title = copy.valueListValues
        if (!found) {
          const id = crypto.randomUUID()
          const created: View = {
            id,
            type: 'value_list_values',
            title,
            pane: 'right',
            position: views.length,
            state: freeObject({ list_key: key }),
          }
          await api.post('/v1/views', {
            body: {
              id,
              workspace_id: surfaceNow.id,
              type: created.type,
              title,
              pane: 'right',
              position: views.length,
              state: freeObject({ list_key: key }),
            },
          })
          views = [...views, created]
          found = created
        } else {
          const updated: View = { ...found, state: freeObject({ ...found.state, list_key: key }), title }
          await api.post('/v1/views', {
            body: {
              id: updated.id,
              workspace_id: surfaceNow.id,
              type: updated.type,
              title,
              pane: updated.pane ?? 'right',
              position: updated.position ?? 0,
              state: freeObject(updated.state),
            },
          })
          views = views.map((view) => (view.id === found!.id ? updated : view))
          found = updated
        }
        const nextSurface = { ...surfaceNow, views: views.map((view) => ({ ...view, pane: view.pane ?? 'center', position: view.position ?? 0, state: freeObject(view.state) })) }
        workspaceRef.current = nextSurface
        queryClient.setQueryData(['workspace', surfaceNow.id], nextSurface)
        const current = docRef.current
        if (!current || !found) return
        const lists = views.find((view) => view.type === 'value_lists')
        placeOverride.current = lists
          ? { target: lists.id, zone: 'right' }
          : { zone: 'right' }
        const result = applyLayoutCommand(current, 'ui.open_view', { id: found.id, zone: 'right' }, {
          views: views.map((view) => ({ id: view.id, title: view.title, type: view.type })),
          canonical: canonicalRef.current,
          place: placeOverride.current,
        })
        placeOverride.current = undefined
        if (result.handled && result.doc !== current) commitDoc(result.doc)
      })()
    }
    window.addEventListener('braian-open-value-list', openList)
    return () => window.removeEventListener('braian-open-value-list', openList)
  }, [queryClient])

  useEffect(() => {
    return onUi((event) => {
      if (event.name === 'ui.set_theme') setTheme(event.args.theme === 'light' ? 'light' : 'dark')
      if (event.name === 'ui.open_workspace' && typeof event.args.id === 'string') setWorkspaceId(event.args.id)
      if (event.name === 'ui.open_changes') applyRef.current('ui.open_view', { title: 'Changes', zone: 'center' })
      if (event.name === 'ui.open_template' && typeof event.args.id === 'string') {
        setWorkspaceId(event.args.id)
        void queryClient.invalidateQueries({ queryKey: ['workspaces'] })
      }
      if (layoutCommand(event.name)) applyRef.current(event.name, event.args)
    })
  }, [queryClient])

  useEffect(() => {
    const current = workspace.data
    if (!current || !defs.data || migratedRef.current.has(current.id)) return
    const types = typeNames(defs.data)
    const objectPatch =
      types.length > 0
        ? migrateSavedWorkspace(current, viewRows(current.views), types)
        : { changed: false as const, name: undefined, views: [] as Array<{ id: string; title: string; state: Json; type?: string }> }
    const listed = viewRows(current.views)
    const key = userId ? layoutStorageKey(userId, current.id) : ''
    const local = key ? readLayoutStore(key) : null
    // Prefer the layout the user would see (local/personal), then server canonical.
    const paintedLayout = local ? migrateDoc(local.document).layout : migrateLayout(current.layout)
    const modelFromPainted = healDataModelWorkspace(current, listed, paintedLayout)
    const modelFromServer = healDataModelWorkspace(current, listed, migrateLayout(current.layout))
    const modelPatch = modelFromPainted.changed ? modelFromPainted : modelFromServer
    const patch = {
      changed: objectPatch.changed || modelPatch.changed,
      name: objectPatch.name,
      views: [...objectPatch.views, ...modelPatch.views],
      create: modelPatch.create,
      layout: modelPatch.layout,
    }
    migratedRef.current.add(current.id)
    if (!patch.changed) {
      if (healPendingFor.current === current.id) {
        healPendingFor.current = null
        setLayoutHealPending(false)
      }
      return
    }
    healPendingFor.current = current.id
    setLayoutHealPending(true)
    void persistWorkspaceMigration(current, patch, {
      postView: (body) => api.post('/v1/views', { body }),
      postWorkspace: (body) => api.post('/v1/workspaces', { body }),
      freeObject,
    })
      .then(async () => {
        // Merge creates into the workspace cache before painting so dock never
        // briefly resolves new template ids as Unavailable / Nothing to show.
        const created = (patch.create ?? []).map((view) => ({
          id: view.id,
          workspace_id: current.id,
          type: view.type,
          title: view.title,
          pane: view.pane ?? 'main',
          position: view.position ?? 0,
          state: view.state,
        }))
        const byId = new Map(listed.map((view) => [view.id, view]))
        for (const view of patch.views) {
          const prev = byId.get(view.id)
          if (!prev) continue
          byId.set(view.id, {
            ...prev,
            title: view.title,
            state: view.state,
            ...(view.type ? { type: view.type } : {}),
          })
        }
        for (const view of created) {
          byId.set(view.id, {
            id: view.id,
            type: view.type,
            title: view.title,
            pane: view.pane,
            position: view.position,
            state: freeObject(view.state as object),
          } as typeof listed[number])
        }
        const nextViews = [...byId.values()]
        const nextLayout = freeObject(
          (patch.layout && typeof patch.layout === 'object' ? patch.layout : current.layout) as object,
        )
        const nextWorkspace = {
          ...current,
          name: patch.name ?? current.name,
          layout: nextLayout,
          views: nextViews,
        }
        queryClient.setQueryData(['workspace', current.id], nextWorkspace)
        workspaceRef.current = nextWorkspace
        if (patch.layout) {
          const next = canonicalDoc({
            id: current.id,
            name: patch.name ?? current.name,
            layout: patch.layout,
            owner_id: current.owner_id,
          })
          if (userId) writeLayoutStore(layoutStorageKey(userId, current.id), next)
          docRef.current = next
          dirtyRef.current = false
          setDoc(next)
          canonicalRef.current = next.layout
          if (userId) {
            loadedFor.current = `${userId}:${current.id}`
            void api.put('/v1/workspaces/{id}/state', {
              params: { path: { id: current.id } },
              body: { environment: LAYOUT_ENVIRONMENT, document: freeObject(next) },
            })
          }
        }
        healPendingFor.current = null
        setLayoutHealPending(false)
        await queryClient.invalidateQueries({ queryKey: ['workspace', current.id] })
        await queryClient.invalidateQueries({ queryKey: ['workspaces'] })
      })
      .catch(() => {
        // Allow a later open/reload to retry; never surface as an unhandled page error.
        migratedRef.current.delete(current.id)
        healPendingFor.current = null
        setLayoutHealPending(false)
        // Fall back to canonical so the shell is not stuck on a blank gate.
        const fallback = canonicalDoc(current)
        docRef.current = fallback
        setDoc(fallback)
      })
  }, [workspace.data, defs.data, queryClient, userId])

  const session: Session = {
    group,
    objectId,
    proposalId,
    language: 'en',
    activity: [],
    listType,
    workspaceType: typeKeyFromShare(workspace.data?.share_token),
    setGroup,
    setObjectId,
    setListType,
    setProposalId,
    push,
    openWorkspace: setWorkspaceId,
    openTypeView: (typeId) => openTypeViewRef.current(typeId),
  }

  const requestDeleteActive = () => {
    const id = workspaceIdRef.current
    if (!id) return
    const rows = workspacesRef.current
    const item = rows.find((row) => row.id === id)
    if (!item) return
    if (item.locked || rows.length <= 1) {
      push(item.locked ? copy.deleteWorkspaceLocked : copy.deleteWorkspaceLast)
      return
    }
    setDeleteConfirm({ id: item.id, name: item.name })
  }
  requestDeleteActiveRef.current = requestDeleteActive

  const deleteWorkspace = async (id: string) => {
    const ordered = orderedWorkspaces
    const index = ordered.findIndex((item) => item.id === id)
    const neighbor =
      index > 0 ? ordered[index - 1]?.id
      : ordered[index + 1]?.id
        ?? ordered.find((item) => item.id !== id)?.id
        ?? null
    setDeleteBusy(true)
    try {
      await api.delete('/v1/workspaces/{id}', { params: { path: { id } } })
      await queryClient.invalidateQueries({ queryKey: ['workspaces'] })
      queryClient.removeQueries({ queryKey: ['workspace', id] })
      if (workspaceId === id) {
        if (neighbor) setWorkspaceId(neighbor)
      }
      push(copy.deleteWorkspace)
    } finally {
      setDeleteBusy(false)
      setDeleteConfirm(null)
      setTabMenu(null)
    }
  }


  if (!booted || (me.isPending && !me.data && !signedOut)) return <p className="pane-body">{copy.loading}</p>
  if (signedOut || !me.data) {
    return (
      <>
        <Login
          onDone={() => {
            setSignedOut(false)
            void queryClient.invalidateQueries({ queryKey: ['me'] })
          }}
        />
        <ImportScreen />
      </>
    )
  }
  const user = me.data
  const signOut = () => {
    setSignedOut(true)
    void queryClient.cancelQueries({ queryKey: ['me'] })
    void api.post('/v1/logout').then(() => {
      queryClient.setQueryData(['me'], undefined)
    })
  }
  signOutRef.current = signOut
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
      const known = viewRows(workspace.data.views).some((view) => view.id === entity || view.title === entity || view.type === entity)
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
      views: viewRows(workspace.data.views).map((view) => ({ id: view.id, title: view.title, type: view.type })),
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
  const typeCatalog = typeNames(defs.data)
  const openTypeKey = String(opened.data?.type ?? '')
  openTypeViewRef.current = (typeId: string) => {
    const surfaceNow = workspaceRef.current
    const docNow = docRef.current
    if (!surfaceNow || !docNow) {
      // Notion 112: palette already closes on select — queue until doc+workspace paint.
      queueOpenTypeView(typeId)
      toast.message(copy.workspaceOpeningWhenReady)
      return
    }
    clearQueuedOpenTypeView()
    const type = typeNames(queryClient.getQueryData<Json>(['definitions'])).find((item) => item.id === typeId)
    if (!type) return
    void (async () => {
      let views: View[] = viewRows(surfaceNow.views)
      let found = views.find((view) => view.type === 'object_table' && (String(view.state.object_type ?? '') === type.id || String(view.state.type_key ?? '') === type.key))
      if (!found) {
        const id = crypto.randomUUID()
        const created: View = {
          id,
          type: 'object_table',
          title: type.plural,
          pane: 'center',
          position: views.length,
          state: freeObject({
            object_type: type.id,
            type_key: type.key,
            columns: ['number', 'name', 'type', 'status'],
            sort: 'number',
          }),
        }
        await api.post('/v1/views', {
          body: {
            id,
            workspace_id: surfaceNow.id,
            type: created.type,
            title: created.title,
            pane: 'center',
            position: views.length,
            state: freeObject(created.state),
          },
        })
        views = [...views, created]
        const nextSurface = { ...surfaceNow, views: views.map((view) => ({ ...view, pane: view.pane ?? 'center', position: view.position ?? 0, state: freeObject(view.state) })) }
        workspaceRef.current = nextSurface
        queryClient.setQueryData(['workspace', surfaceNow.id], nextSurface)
        found = created
      }
      const current = docRef.current
      if (!current) return
      const result = applyLayoutCommand(current, 'ui.open_view', { id: found.id, zone: 'center' }, {
        views: views.map((view) => ({ id: view.id, title: view.title, type: view.type })),
        canonical: canonicalRef.current,
        place: placeOverride.current,
      })
      if (result.handled && result.doc !== current) commitDoc(result.doc)
    })()
  }


  const toggleLock = (item: ListedWorkspace) => {
    void api.post('/v1/workspaces', { body: {
        id: item.id,
        name: item.name,
        locked: !item.locked,
        share_token: item.share_token,
        layout: freeObject(item.layout ?? { tabs: [] }),
      } }).then(() => {
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
          {orderedWorkspaces.map((item, index) => (
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
        <UserMenu
          name={user.display_name}
          email={user.email}
          handle={user.handle}
          role={user.role}
          connection={isConnected(online, liveState) ? 'online' : 'offline'}
          theme={theme}
          onToggleTheme={() => { void runCommand('theme:toggle') }}
          onSignOut={() => { void runCommand('session:sign-out') }}
        />
      </header>
      <main className="workspace" data-testid="workspace">
        {doc && surface && !layoutHealPending ? (
          <WorkspaceDock
            doc={doc}
            views={viewRows(surface.views).map((view) => ({ ...view, title: displayViewTitle(view, typeCatalog, openTypeKey) }))}
            onChange={commitDoc}
            renderView={(item) => {
              const full = viewRows(surface.views).find((view) => view.id === item.id)
              return full ? <ViewBody view={full} session={session} workspace={surface} /> : <p>{copy.empty}</p>
            }}
            onAddView={(target) => {
              placeOverride.current = { target, zone: 'center' }
              setPaletteStartNewFromTemplate(false)
              setPaletteStartAddView(true)
              setPaletteMode('list')
            }}
          />
        ) : (
          <p data-testid="layout-heal-loading">{copy.loading}</p>
        )}
      </main>
      <div className="bottom-bar">
        <pre className="activity" data-testid="activity-log" data-live={liveState}>{logText}</pre>
        <StatusBar online={online} live={liveState} locked={Boolean(workspace.data?.locked)} />
      </div>
      <ModelDraftHost />
      <ConsequenceHost session={session} />
      <SharedContextMenuHost feedbackEnabled={feedbackEnabled} />
      {feedbackEnabled ? (
        <FeedbackDialog
          open={feedbackOpen}
          onClose={() => {
            setFeedbackOpen(false)
            setFeedbackCapture(null)
          }}
          capture={feedbackCapture}
          user={{ handle: user.handle || '', role: user.role }}
          object={objectId ? {
            id: objectId,
            type: String(opened.data?.type ?? ''),
            key: String(jsonRecord(opened.data)?.key ?? ''),
            title: recordTitle(opened.data?.values as Json | undefined),
          } : null}
        />
      ) : null}
      <NewSelectionDialog session={session} />
      {paletteMode ? (
        <CommandPalette
              fieldsMode={paletteMode === 'fields'}
              fieldsEditColumnId={fieldsEditColumnId}
              onFieldsConsumed={() => setFieldsEditColumnId(null)}
          key={`${paletteMode}:${paletteEpoch}:${paletteStartAddView ? 'add-view' : paletteStartNewFromTemplate ? 'new-template' : 'root'}`}
          ai={paletteMode === 'ai'}
          session={session}
          workspaces={orderedWorkspaces}
          views={workspace.data?.views ?? []}
          feedbackEnabled={feedbackEnabled}
          onSendFeedback={openFeedback}
          theme={theme}
          onSignOut={() => { void runCommand('session:sign-out') }}
          focusViewId={doc?.focus ?? null}
          startAddView={paletteStartAddView}
          startNewFromTemplate={paletteStartNewFromTemplate}
          onPrepareAddView={() => {
            const focus = docRef.current?.focus
            placeOverride.current = focus ? { target: focus, zone: 'center' } : { zone: 'center' }
          }}
          onClose={() => {
            placeOverride.current = undefined
            setPaletteStartAddView(false)
            setPaletteStartNewFromTemplate(false)
            setPaletteMode(null)
          }}
        />
      ) : null}
      {tabMenu ? (
        <div className="menu" style={{ left: tabMenu.x, top: tabMenu.y }} data-testid="workspace-menu">
          {workspaces.data?.workspaces.filter((item) => item.id === tabMenu.id).map((item) => {
            const last = orderedWorkspaces.length <= 1
            const locked = Boolean(item.locked)
            const deleteTitle = locked ? copy.deleteWorkspaceLocked : last ? copy.deleteWorkspaceLast : copy.deleteWorkspace
            return (
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
              <button
                type="button"
                data-testid="workspace-delete"
                disabled={locked || last}
                title={deleteTitle}
                onClick={() => {
                  setTabMenu(null)
                  setDeleteConfirm({ id: item.id, name: item.name })
                }}
              >
                {copy.deleteWorkspace}
              </button>
            </div>
            )
          })}
          <button
            type="button"
            data-testid="workspace-new-from-template"
            onClick={() => {
              setTabMenu(null)
              setPaletteStartAddView(false)
              setPaletteStartNewFromTemplate(true)
              setPaletteMode('list')
              setPaletteEpoch((n) => n + 1)
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
        </div>
      ) : null}
      {deleteConfirm ? (
        <Dialog open onOpenChange={(open) => { if (!open && !deleteBusy) setDeleteConfirm(null) }}>
          <DialogContent data-testid="workspace-delete-dialog">
            <DialogTitle>{copy.deleteWorkspace}</DialogTitle>
            <DialogDescription>{copy.deleteWorkspaceConfirm}</DialogDescription>
            <p>{workspaceDisplayName(deleteConfirm.name, workspaces.data?.workspaces.find((w) => w.id === deleteConfirm.id)?.share_token)}</p>
            <div className="dialog-actions">
              <button type="button" disabled={deleteBusy} onClick={() => setDeleteConfirm(null)}>{copy.cancel}</button>
              <button
                type="button"
                className="primary"
                data-testid="workspace-delete-confirm"
                disabled={deleteBusy}
                autoFocus
                onClick={() => { void deleteWorkspace(deleteConfirm.id) }}
              >
                {copy.deleteWorkspace}
              </button>
            </div>
          </DialogContent>
        </Dialog>
      ) : null}
      <ImportScreen />
      {help ? <Help onClose={() => setHelp(false)} placeholder={commandPlaceholder(typeCatalog)} /> : null}
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
          void api.post('/v1/login', { body: { email, password } })
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
        {error ? <p className="error">{error}</p> : null}
        <Button type="submit">{copy.signIn}</Button>
      </form>
    </div>
  )
}

function ViewBody({ view, session, workspace }: { view: View; session: Session; workspace: Workspace }) {
  if (view.type === 'structure_tree') return <StructureTree session={session} view={view} />
  if (view.type === 'assortment_list') return <AssortmentList session={session} />
  if (view.type === 'object_table') return <ObjectTable session={session} view={view} workspace={workspace} />
  if (view.type === 'object_detail') return <ObjectDetail session={session} />
  if (view.type === 'graph') return <GraphView session={session} />
  if (view.type === 'history') return <HistoryView session={session} />
  if (view.type === 'class_attributes') return <ClassAttributes session={session} />
  if (view.type === 'object_types' || view.type === 'link_types' || view.type === 'value_lists' || view.type === 'value_list_values' || view.type === 'units') {
    return <ModelTableView view={view} />
  }
  if (view.type === 'data_model') return <DataModel view={view} />
  if (view.type === 'attribute_definitions') return <AttributeDefinitions />
  if (view.type === 'proposal') return <ProposalView session={session} />
  if (view.type === 'proposal_list') return <ProposalListView session={session} />
  if (view.type === 'import_mappings') return <ImportMappingsView />
  if (view.type === 'batch_log') return <BatchLog />
  if (view.type === 'job_history') return <JobHistory view={view} />
  if (view.type === 'embed') return <EmbedView session={session} view={view} />
  // Dropped / unknown types must never paint a removed pane (e.g. old ModelNav/Preview).
  if (!isKnownViewType(view.type)) {
    return <StateNote tone="empty" title={copy.unknownView} detail={copy.unknownViewDetail} />
  }
  return <StateNote tone="empty" title={copy.unknownView} detail={copy.unknownViewDetail} />
}

function treeRowDomId(structureKey: string, key: string) {
  return `tree-row-${structureKey}-${key}`
}

type TreeEntry = {
  structureKey: string
  key: string
  id: string
  parentKey: string | null
  depth: number
  kids: boolean
  open: boolean
  count: number
}

export function StructureTree({ session, view }: { session: Session; view: View }) {
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api.get('/v1/definitions') })
  const counts = useQuery({
    queryKey: ['group-counts'],
    queryFn: () => api.get('/v1/search', { params: { query: { group: "groups", per_page: 1 } } }),
  })
  const [openKeys, setOpenKeys] = useState<string[]>(() => (view.state.expanded as string[] | undefined) ?? [])
  const [menu, setMenu] = useState<{ x: number; y: number; row: TreeEntry } | null>(null)
  const [editing, setEditing] = useState<{ row: TreeEntry; mode: 'rename' | 'child' | 'sibling' | 'key'; value: string } | null>(null)
  const [dropId, setDropId] = useState<string | null>(null)
  const structures = (defs.data?.structures as Json[] | undefined) ?? []
  const typeKey = session.workspaceType || session.listType || ''
  const typeRow = ((defs.data?.types as Json[]) ?? []).find((item) => String(item.key) === typeKey)
  const allowedTrees = Array.isArray(typeRow?.structures) ? (typeRow.structures as unknown[]).map(String) : null
  const unplacedCounts = useQueries({
    queries: structures.map((structure) => {
      const token = String(structure.id ?? structure.key)
      const shown = allowedTrees == null
        ? true
        : allowedTrees.length > 0 && (allowedTrees.includes(token) || allowedTrees.includes(String(structure.key)))
      return {
        queryKey: ['unplaced', String(typeRow?.id ?? ''), token],
        enabled: Boolean(typeRow?.id) && shown,
        queryFn: () => api.get('/v1/types/{id}/unplaced', { params: { path: { id: String(typeRow?.id) }, query: { structure: token } } }),
      }
    }),
  })
  const countOf = (structure: string, key: string) => {
    const groups = (counts.data?.groups as Json[] | undefined) ?? []
    const hit = groups.find((group) => String(group.key) === `${structure}:${key}`)
    return Number(hit?.found ?? 0)
  }
  const toggle = (key: string) => {
    setOpenKeys((current) => (current.includes(key) ? current.filter((item) => item !== key) : [...current, key]))
  }
  const entries: TreeEntry[] = []
  for (const structure of structures) {
    const groups = (structure.groups as Json[]) ?? []
    const structureKey = String(structure.key)
    const walk = (parentId: string | undefined, parentKey: string | null, depth: number) => {
      for (const group of groups) {
        const child = parentId ? String(group.parent ?? '') === parentId : !group.parent
        if (!child) continue
        const key = String(group.key)
        const id = String(group.id)
        const kids = groups.some((item) => String(item.parent ?? '') === id)
        const open = openKeys.includes(key) || openKeys.includes(id) || openKeys.includes(`${structureKey}:${key}`) || openKeys.includes(`${String(structure.id ?? '')}:${id}`)
        entries.push({ structureKey, key, id, parentKey, depth, kids, open, count: countOf(structureKey, key) })
        if (kids && open) walk(id, key, depth + 1)
      }
    }
    walk(undefined, null, 0)
    const token = String(structure.id ?? structure.key)
    const shown = allowedTrees == null
      ? Boolean(typeRow)
      : allowedTrees.includes(token) || allowedTrees.includes(structureKey)
    if (shown) {
      const found = Number(unplacedCounts[structures.indexOf(structure)]?.data?.count ?? 0)
      entries.push({
        structureKey,
        key: 'unplaced',
        id: `${token}:unplaced`,
        parentKey: null,
        depth: 0,
        kids: false,
        open: false,
        count: found,
      })
    }
  }
  const activeIndex = entries.findIndex((row) => session.group === `${row.structureKey}:${row.key}`)
  const activeRow = activeIndex >= 0 ? entries[activeIndex] : undefined
  useEffect(() => {
    if (!activeRow) return
    document.getElementById(treeRowDomId(activeRow.structureKey, activeRow.key))?.scrollIntoView({ block: 'nearest' })
  }, [session.group, entries.length])
  useEffect(() => {
    if (!menu) return
    const close = (event: PointerEvent) => {
      const target = event.target
      if (target instanceof Element && target.closest('[data-testid="context-menu"], [data-testid="delete-group"]')) return
      setMenu(null)
    }
    window.addEventListener('pointerdown', close)
    return () => window.removeEventListener('pointerdown', close)
  }, [menu])
  useEffect(() => {
    if (!editing) return
    const close = (event: PointerEvent) => {
      const target = event.target
      if (target instanceof Element && target.closest('[data-testid="tree-edit"]')) return
      setEditing(null)
    }
    window.addEventListener('pointerdown', close)
    return () => window.removeEventListener('pointerdown', close)
  }, [editing])
  const choose = (index: number) => {
    const row = entries[index]
    if (!row) return
    session.setGroup(`${row.structureKey}:${row.key}`)
  }
  const propose = async (name: string, args: Json) => {
    setMenu(null)
    setEditing(null)
    try {
      const result = await stageModel(name, args)
      const id = operationId(result)
      if (id && name.includes('delete')) {
        publishProposal(id)
        session.setProposalId(id)
        session.push(copy.proposed)
      } else {
        session.push(`${copy.unsavedChanges}`)
      }
    } catch (reason) {
      session.push(reason instanceof Error ? reason.message : copy.validation)
    }
  }
  const startEdit = (row: TreeEntry, mode: 'rename' | 'child' | 'sibling' | 'key') => {
    if (row.key === 'unplaced') return
    setMenu(null)
    if (mode === 'child' && !openKeys.includes(row.key)) setOpenKeys((current) => [...current, row.key])
    const value = mode === 'rename'
      ? structureCaption(row.key, defs.data, row.structureKey)
      : mode === 'key'
        ? row.key
        : ''
    setEditing({ row, mode, value })
  }
  const commitEdit = (typedRaw?: string) => {
    if (!editing) return
    const { row, mode } = editing
    const typed = (typedRaw ?? editing.value).trim()
    setEditing(null)
    if (!typed) return
    if (mode === 'rename') {
      if (typed === structureCaption(row.key, defs.data, row.structureKey) || typed === row.key) return
      void propose('model.rename_group', { structure: row.structureKey, group: row.key, label: typed })
      return
    }
    if (mode === 'key') {
      if (typed === row.key) return
      void propose('model.rename_group_key', { structure: row.structureKey, group: row.key, new_key: typed })
      return
    }
    const name = groupKeyFromLabel(typed)
    if (!name) return
    const parent = mode === 'child' ? row.key : row.parentKey
    void propose('model.create_group', {
      structure: row.structureKey,
      group: name,
      label: typed,
      ...(parent ? { parent } : {}),
    })
  }
  const openMenu = (row: TreeEntry, x: number, y: number) => {
    session.setGroup(`${row.structureKey}:${row.key}`)
    if (row.key === 'unplaced') {
      setMenu(null)
      return
    }
    setMenu({
      row,
      x: Math.max(8, Math.min(x, window.innerWidth - 220)),
      y: Math.max(8, Math.min(y, window.innerHeight - 240)),
    })
  }
  const groupsOf = (structureKey: string) => {
    const structure = structures.find((item) => String(item.key) === structureKey)
    return ((structure?.groups as Json[]) ?? []).map((group) => ({
      id: String(group.id),
      parent: group.parent ? String(group.parent) : null,
    }))
  }
  return (
    <div
      className="tree"
      data-testid="structure-tree"
      role="tree"
      tabIndex={0}
      aria-activedescendant={activeRow ? treeRowDomId(activeRow.structureKey, activeRow.key) : undefined}
      onKeyDown={(event) => {
        if (event.target instanceof HTMLInputElement) return
        if (event.key === 'Escape') {
          setMenu(null)
          setEditing(null)
          return
        }
        if (event.key === 'F2' && activeRow) {
          event.preventDefault()
          startEdit(activeRow, 'rename')
          return
        }
        if (event.key === 'Insert' && activeRow) {
          event.preventDefault()
          startEdit(activeRow, 'child')
          return
        }
        if ((event.key === 'Delete' || event.key === 'Backspace') && activeRow && activeRow.key !== 'unplaced') {
          event.preventDefault()
          setMenu(null)
          void propose('model.delete_group', { structure: activeRow.structureKey, group: activeRow.key })
          return
        }
        if (!['ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter'].includes(event.key)) return
        event.preventDefault()
        event.stopPropagation()
        if (entries.length === 0) return
        const next = event.key === 'ArrowDown'
          ? Math.min(entries.length - 1, activeIndex + 1)
          : event.key === 'ArrowUp'
            ? Math.max(0, activeIndex < 0 ? entries.length - 1 : activeIndex - 1)
            : event.key === 'Home'
              ? 0
              : event.key === 'End'
                ? entries.length - 1
                : Math.max(0, activeIndex)
        choose(next)
      }}
    >
      {structures.map((structure) => {
        const structureKey = String(structure.key)
        const rows = entries.filter((row) => row.structureKey === structureKey)
        return (
          <div key={structureKey}>
            <strong>{structureCaption(structureKey, defs.data, structureKey)}</strong>
            {rows.map((row) => {
              const selected = session.group === `${structureKey}:${row.key}`
              const Icon = row.kids ? (row.open ? FolderOpen : Folder) : File
              return (
                <div key={row.id} className="tree-branch">
                  <button
                    type="button"
                    id={treeRowDomId(structureKey, row.key)}
                    role="treeitem"
                    aria-selected={selected}
                    aria-expanded={row.kids ? row.open : undefined}
                    tabIndex={-1}
                    draggable
                    className={dropId === row.id ? 'tree-row drop' : selected ? 'tree-row on' : 'tree-row'}
                    style={{ paddingLeft: 8 + row.depth * 16 }}
                    onClick={() => session.setGroup(`${structureKey}:${row.key}`)}
                    onContextMenu={(event) => {
                      event.preventDefault()
                      openMenu(row, event.clientX, event.clientY)
                    }}
                    onDragStart={(event) => {
                      event.dataTransfer.setData('text/plain', JSON.stringify({ structureKey, id: row.id, key: row.key }))
                      event.dataTransfer.effectAllowed = 'move'
                    }}
                    onDragOver={(event) => {
                      event.preventDefault()
                      setDropId(row.id)
                    }}
                    onDragLeave={() => setDropId((current) => (current === row.id ? null : current))}
                    onDrop={(event) => {
                      event.preventDefault()
                      setDropId(null)
                      const raw = event.dataTransfer.getData('text/plain')
                      if (!raw) return
                      let dragged: { structureKey?: string; id?: string; key?: string }
                      try {
                        dragged = JSON.parse(raw) as { structureKey?: string; id?: string; key?: string }
                      } catch {
                        return
                      }
                      if (dragged.structureKey !== structureKey || !dragged.id || !dragged.key) return
                      if (!dropReparents(groupsOf(structureKey), dragged.id, row.id)) return
                      void propose('model.move_group', { structure: structureKey, group: dragged.key, parent: row.key })
                    }}
                  >
                    {Array.from({ length: row.depth }, (_, level) => (
                      <span key={level} className="tree-guide" style={{ left: 15 + level * 16 }} aria-hidden="true" />
                    ))}
                    {row.kids ? (
                      <span
                        className="tree-chevron"
                        aria-hidden="true"
                        onClick={(event) => {
                          event.preventDefault()
                          event.stopPropagation()
                          toggle(row.key)
                        }}
                      >
                        {row.open ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
                      </span>
                    ) : (
                      <span className="tree-chevron" aria-hidden="true" />
                    )}
                    <Icon size={14} className="tree-icon" aria-hidden="true" />
                    <span className="tree-label">{row.key === 'unplaced' ? copy.unplaced : structureCaption(row.key, defs.data, structureKey)}</span>
                    {row.count > 0 ? <span className="tree-count">{formatCount(row.count)}</span> : null}
                  </button>
                  {editing?.row.id === row.id ? (
                    <input
                      className="tree-edit"
                      data-testid="tree-edit"
                      autoFocus
                      placeholder={editing.mode === 'key' ? copy.key : editing.mode === 'rename' ? '' : copy.newGroup}
                      aria-label={editing.mode === 'key' ? copy.renameKey : copy.rename}
                      value={editing.value}
                      style={{ marginLeft: 28 + row.depth * 16 }}
                      onChange={(event) => setEditing({ ...editing, value: event.target.value })}
                      onKeyDown={(event) => {
                        event.stopPropagation()
                        if (event.key === 'Enter') {
                          event.preventDefault()
                          commitEdit(event.currentTarget.value)
                        }
                        if (event.key === 'Escape') {
                          event.preventDefault()
                          setEditing(null)
                        }
                      }}
                    />
                  ) : null}
                  {editing?.row.id === row.id && editing.mode === 'key' && editing.value.trim() && editing.value.trim() !== row.key ? (
                    <p className="warn" data-testid="key-rename-warning">{copy.keyRenameWarning.replace('{key}', row.key)}</p>
                  ) : null}
                </div>
              )
            })}
          </div>
        )
      })}
      {menu ? (
        <div
          className="menu"
          data-testid="context-menu"
          role="menu"
          style={{ left: menu.x, top: menu.y }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button type="button" onClick={() => { session.setGroup(`${menu.row.structureKey}:${menu.row.key}`); setMenu(null) }}>{copy.open}</button>
          <button type="button" onClick={() => startEdit(menu.row, 'rename')}>{copy.rename}<kbd>F2</kbd></button>
          <button type="button" onClick={() => startEdit(menu.row, 'key')}>{copy.renameKey}</button>
          <button type="button" onClick={() => startEdit(menu.row, 'child')}>{copy.addChild}<kbd>Ins</kbd></button>
          <button type="button" onClick={() => startEdit(menu.row, 'sibling')}>{copy.addSibling}</button>
          <button
            type="button"
            onClick={() => void propose('model.duplicate_group', {
              structure: menu.row.structureKey,
              group: menu.row.key,
              name: `${menu.row.key}_copy`,
            })}
          >
            {copy.duplicate}
          </button>
          <button type="button" className="danger" onClick={() => void propose('model.delete_group', { structure: menu.row.structureKey, group: menu.row.key })}>{copy.deleteMenu}<kbd>Del</kbd></button>
        </div>
      ) : null}
    </div>
  )
}


function BatchLog() {
  const history = useQuery({ queryKey: ['batches'], queryFn: () => api.get('/v1/history', { params: { query: { limit: 50 } } }) })
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




function singularFromLive(raw: string, defs: Json | undefined) {
  try {
    const event = JSON.parse(raw) as Record<string, unknown>
    const key = typeof event.type === 'string' ? event.type : ''
    return key ? singularOf(defs, key) || undefined : undefined
  } catch {
    return undefined
  }
}

function Help({ onClose, placeholder }: { onClose: () => void; placeholder: string }) {
  // F1 overlay only — theme / Agent / Sign out stay in the user menu; Ctrl+K uses the registry.
  const rows = shortcuts.filter((item) => item.id !== 'help.close' && item.id !== 'palette.search')
  return (
    <div className="help" data-testid="help-shortcuts" onClick={onClose}>
      <div onClick={(event) => event.stopPropagation()}>
        <strong>{copy.shortcuts}</strong>
        <p><Kbd>Ctrl</Kbd>+<Kbd>K</Kbd> {placeholder}</p>
        {rows.map((item) => (
          <p key={item.id} data-shortcut-id={item.id}>
            <Kbd>{item.chord}</Kbd> {item.description}
          </p>
        ))}
        <button onClick={onClose}>{copy.close}</button>
      </div>
    </div>
  )
}

