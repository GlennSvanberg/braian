import { ApiError, api } from '../api'
import {
  acceptsFile,
  firstDroppedFile,
  phaseOf,
  setTarget,
  undoColumns,
  redoColumns,
  type ColumnLine,
  type ImportPhase,
} from './model'

export type ImportTarget = {
  id: string
  label: string
  kind: string
  key?: boolean
  source?: string
  language?: string | null
}

export type ImportHeader = {
  index: number
  raw: string
  name: string
  empty_ratio: number
  duplicate_of: number | null
}

export type ImportSession = {
  id: string
  status: string
  types?: Array<{ key: string; label: string }>
  file_name: string
  file_kind: string
  sheets: Array<{ index: number; name: string; empty?: boolean; rows_estimate?: number }>
  sheet_index: number
  headers: ImportHeader[]
  sample: string[][]
  rows_parsed: number
  parse_done: boolean
  targets: ImportTarget[]
  proposal: {
    ai?: string
    type?: { value?: string | null; confidence?: number }
    id_column?: { value?: string | null; confidence?: number }
    columns?: ColumnLine[]
  } | null
  mapping: { type?: string; id_column?: number | null; columns?: Array<{ col: number; target: string }> } | null
  counters: Record<string, number>
  job_id: string | null
  error: string | null
}

export type ImportSnapshot = {
  open: boolean
  drag: boolean
  phase: ImportPhase
  fileName: string
  upload: number
  error: string
  notice: string
  session: ImportSession | null
  columns: ColumnLine[]
  dirty: boolean
  selected: number | null
  hover: number | null
  filter: string
  history: ColumnLine[][]
  future: ColumnLine[][]
  confirm: null | 'close' | 'replace'
  pendingFile: File | null
  typeKey: string
  idColumn: string
}

const initial: ImportSnapshot = {
  open: false,
  drag: false,
  phase: 'empty',
  fileName: '',
  upload: 0,
  error: '',
  notice: '',
  session: null,
  columns: [],
  dirty: false,
  selected: null,
  hover: null,
  filter: 'all',
  history: [],
  future: [],
  confirm: null,
  pendingFile: null,
  typeKey: '',
  idColumn: 'none',
}

let state: ImportSnapshot = initial
const listeners = new Set<() => void>()
let poll: ReturnType<typeof setInterval> | undefined

function emit() {
  for (const listener of listeners) listener()
}

function patch(next: Partial<ImportSnapshot>) {
  state = { ...state, ...next }
  emit()
}

export function getImportSnapshot(): ImportSnapshot {
  return state
}

export function subscribeImport(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export function setImportDrag(drag: boolean) {
  if (state.drag !== drag) patch({ drag })
}

export function openImport() {
  patch({ open: true, phase: state.session ? state.phase : 'empty', confirm: null, error: '' })
}

export function requestCloseImport() {
  if (!state.open) return
  if (state.phase === 'saving') {
    patch({ open: false, drag: false, confirm: null })
    return
  }
  if (state.phase === 'mapping' || state.phase === 'preview' || state.phase === 'uploading') {
    patch({ confirm: 'close' })
    return
  }
  patch({ open: false, drag: false, confirm: null })
}

export function dismissConfirm() {
  patch({ confirm: null, pendingFile: null })
}

export function confirmImport() {
  if (state.confirm === 'replace' && state.pendingFile) {
    const file = state.pendingFile
    patch({ confirm: null, pendingFile: null })
    void beginUpload(file)
    return
  }
  stopPoll()
  patch({ ...initial })
}

export function selectColumn(col: number | null) {
  patch({ selected: col })
}

export function hoverColumn(col: number | null) {
  if (state.hover !== col) patch({ hover: col })
}

export function setImportFilter(filter: string) {
  patch({ filter })
}

export function setImportType(typeKey: string) {
  patch({ typeKey })
  const id = state.session?.id
  if (!id) return
  void api<ImportSession>(`/v1/imports/${id}/remap`, {
    method: 'POST',
    body: JSON.stringify({
      type: typeKey,
      id_column: state.idColumn,
      columns: state.columns.filter((column) => column.source === 'manual').map((column) => ({
        col: column.col,
        target: column.target,
      })),
    }),
  }).then(adopt).catch((error: unknown) => {
    patch({ error: error instanceof Error ? error.message : 'The mapping could not be updated.' })
  })
}

export function setImportIdColumn(idColumn: string) {
  patch({ idColumn })
}

export function assignImportTarget(col: number, target: string) {
  const result = setTarget(state.columns, col, target)
  const notice = result.displaced
    ? `Column ${result.displaced.col + 1} was unmapped so this field stays unique.`
    : state.notice
  patch({
    history: [...state.history, state.columns],
    future: [],
    columns: result.columns,
    dirty: true,
    selected: col,
    notice,
  })
}

export function undoImport() {
  const next = undoColumns(state.history, state.columns, state.future)
  patch({ history: next.history, columns: next.current, future: next.future, dirty: true })
}

export function backToMapping() {
  if (!state.session) return
  patch({ phase: 'mapping' })
}

export function redoImport() {
  const next = redoColumns(state.history, state.columns, state.future)
  patch({ history: next.history, columns: next.current, future: next.future, dirty: true })
}

export function dropImportFiles(files: File[]) {
  const picked = firstDroppedFile(files.map((file) => file.name))
  if ('error' in picked) {
    patch({ open: true, notice: picked.error })
    return
  }
  const file = files[0]
  if (!file || !acceptsFile(file.name)) return
  if (state.open && (state.phase === 'mapping' || state.phase === 'preview')) {
    patch({ open: true, confirm: 'replace', pendingFile: file, notice: picked.notice })
    return
  }
  if (picked.notice) patch({ notice: picked.notice })
  void beginUpload(file)
}

export function onImportLive(raw: string) {
  let event: { op?: string; id?: string; status?: string }
  try {
    event = JSON.parse(raw) as { op?: string; id?: string; status?: string }
  } catch {
    return
  }
  if (event.op !== 'import' || !event.id || event.id !== state.session?.id) return
  void refresh(event.id)
}

export async function approveImport() {
  const session = state.session
  if (!session) return
  const idColumn = state.idColumn.startsWith('col:') ? Number(state.idColumn.slice(4)) : null
  await api(`/v1/imports/${session.id}/mapping`, {
    method: 'PUT',
    body: JSON.stringify({
      type: state.typeKey,
      id_column: idColumn,
      columns: state.columns.map((column) => ({ col: column.col, target: column.target })),
    }),
  })
  await api(`/v1/imports/${session.id}/approve`, { method: 'POST', body: '{}' })
  patch({ phase: 'preview', dirty: false, filter: 'all' })
  await refresh(session.id)
}

export async function saveImport() {
  const session = state.session
  if (!session) return
  const failed = Number(session.counters.failed ?? 0)
  const body = await api<{ job_id: string }>(`/v1/imports/${session.id}/save`, {
    method: 'POST',
    body: JSON.stringify({ skip_failed: failed > 0 }),
  })
  patch({ phase: 'saving', session: { ...session, job_id: body.job_id, status: 'saving' } })
}

export async function resumeImport(id: string) {
  patch({ open: true, phase: 'mapping', error: '' })
  try {
    await refresh(id)
  } catch (error) {
    patch({
      phase: 'error',
      error: error instanceof Error ? error.message : 'That import does not exist.',
    })
  }
}

function adopt(session: ImportSession) {
  const proposal = session.proposal?.columns ?? []
  const columns = state.dirty && state.session?.id === session.id ? state.columns : proposal.map((column) => ({ ...column }))
  const typeKey = state.dirty && state.session?.id === session.id
    ? state.typeKey
    : session.mapping?.type || session.proposal?.type?.value || state.typeKey
  const idColumn = state.dirty && state.session?.id === session.id
    ? state.idColumn
    : session.proposal?.id_column?.value || (session.mapping?.id_column == null ? 'none' : `col:${session.mapping.id_column}`)
  patch({
    open: true,
    session,
    columns,
    typeKey: typeKey || '',
    idColumn: idColumn || 'none',
    phase: phaseOf(session.status),
    fileName: session.file_name,
    error: session.error ?? (session.status === 'failed' ? 'We could not read this file.' : state.error),
  })
  remember(session.id)
  if (session.status === 'mapping' || session.status === 'diffing' || session.status === 'saving' || session.status === 'uploading') {
    startPoll(session.id)
  } else {
    stopPoll()
  }
}

async function refresh(id: string) {
  const session = await api<ImportSession>(`/v1/imports/${id}`)
  adopt(session)
}

function startPoll(id: string) {
  if (poll) return
  poll = setInterval(() => {
    void refresh(id).catch(() => undefined)
  }, 400)
}

function stopPoll() {
  if (poll) clearInterval(poll)
  poll = undefined
}

function remember(id: string) {
  const url = new URL(window.location.href)
  if (url.searchParams.get('import') === id) return
  url.searchParams.set('import', id)
  window.history.replaceState({}, '', url)
}

function beginUpload(file: File) {
  stopPoll()
  patch({
    open: true,
    phase: 'uploading',
    fileName: file.name,
    upload: 0,
    error: '',
    session: null,
    columns: [],
    dirty: false,
    history: [],
    future: [],
    selected: null,
    confirm: null,
    pendingFile: null,
  })
  uploadFile(file, (upload) => patch({ upload }))
    .then(async (created) => {
      patch({ phase: 'mapping' })
      await refresh(created.id)
    })
    .catch((error: unknown) => {
      const message = error instanceof ApiError ? error.message : error instanceof Error ? error.message : 'Upload failed.'
      patch({ phase: 'error', error: message })
    })
}

function uploadFile(file: File, onProgress: (ratio: number) => void): Promise<{ id: string }> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    xhr.open('POST', '/v1/imports')
    xhr.withCredentials = true
    xhr.setRequestHeader('X-File-Name', encodeURIComponent(file.name))
    xhr.setRequestHeader('Content-Type', 'application/octet-stream')
    xhr.upload.onprogress = (event) => {
      if (event.lengthComputable && event.total > 0) onProgress(event.loaded / event.total)
    }
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        resolve(JSON.parse(xhr.responseText) as { id: string })
        return
      }
      let detail = xhr.statusText
      try {
        const body = JSON.parse(xhr.responseText) as { detail?: string }
        if (body.detail) detail = body.detail
      } catch {
        detail = xhr.responseText || detail
      }
      reject(new ApiError(xhr.status, detail, null))
    }
    xhr.onerror = () => reject(new Error('Upload failed.'))
    xhr.send(file)
  })
}

export function showImportFixture(kind: 'map' | 'diff') {
  const columns: ColumnLine[] = [
    { col: 0, target: 'type:sku', confidence: 0.97, source: 'deterministic' },
    { col: 1, target: 'type:name', confidence: 0.42, source: 'decisions', low: true },
    { col: 2, target: 'unmapped', confidence: 0, source: 'deterministic', reason: 'empty' },
  ]
  const session: ImportSession = {
    id: 'fixture',
    status: kind === 'diff' ? 'previewed' : 'ready',
    file_name: 'prices.csv',
    file_kind: 'csv',
    sheets: [{ index: 0, name: 'prices', rows_estimate: 4 }],
    sheet_index: 0,
    headers: [
      { index: 0, raw: 'Artnr', name: 'artnr', empty_ratio: 0, duplicate_of: null },
      { index: 1, raw: 'Name', name: 'name', empty_ratio: 0, duplicate_of: null },
      { index: 2, raw: 'Notes', name: 'notes', empty_ratio: 1, duplicate_of: null },
    ],
    sample: [
      ['A-100', 'Cable 3m', ''],
      ['A-100', 'Cable 5m', ''],
    ],
    rows_parsed: 4,
    parse_done: true,
    types: [{ key: 'product', label: 'Product' }],
    targets: [
      { id: 'type:sku', label: 'SKU', kind: 'text', key: true, source: 'Product' },
      { id: 'type:name', label: 'Name', kind: 'text', source: 'Product' },
      { id: 'type:weight', label: 'Weight', kind: 'number', source: 'Product' },
      { id: 'action', label: 'Action (create/update/delete)', kind: 'action' },
    ],
    proposal: {
      ai: 'skipped',
      type: { value: 'product', confidence: 0.97 },
      id_column: { value: 'col:0', confidence: 0.97 },
      columns,
    },
    mapping: null,
    counters: { new: 2, changed: 1, delete: 1, failed: 1, unchanged: 0 },
    job_id: null,
    error: null,
  }
  patch({
    open: true,
    phase: kind === 'diff' ? 'preview' : 'mapping',
    fileName: session.file_name,
    session,
    columns,
    typeKey: 'product',
    idColumn: 'col:0',
    dirty: false,
    error: '',
    notice: '',
  })
}
