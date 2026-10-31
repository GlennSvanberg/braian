import { ApiError, api, jsonRecord, type Schemas } from '../api'
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
  attribute_id?: string | null
  structure_id?: string | null
  group_id?: string | null
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
  types?: Array<{ key: string; label: string; id?: string }>
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
  mapping: { type?: string; id_column?: number | null; skip_invalid?: boolean; columns?: Array<{ col: number; target: string }> } | null
  counters: Record<string, number>
  job_id: string | null
  draft_id?: string | null
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
  emptyClears: boolean
  /** Shown inside a workspace pane instead of the full-screen dialog. */
  embedded: boolean
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
  emptyClears: false,
  embedded: false,
}

let state: ImportSnapshot = initial
const listeners = new Set<() => void>()
let poll: ReturnType<typeof setInterval> | undefined
let generation = 0

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
    generation += 1
    stopPoll()
    forgetImport()
    patch({ open: false, drag: false, confirm: null })
    return
  }
  if (state.phase === 'mapping' || state.phase === 'preview' || state.phase === 'uploading') {
    patch({ confirm: 'close' })
    return
  }
  abandonImport()
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
  abandonImport()
}

export function discardImport() {
  abandonImport()
}

function abandonImport() {
  const session = state.session
  generation += 1
  stopPoll()
  forgetImport()
  patch({ ...initial })
  if (session && session.id !== 'fixture') {
    void api.delete('/v1/imports/{id}', { params: { path: { id: session.id } } }).catch(() => undefined)
  }
}

function forgetImport() {
  const url = new URL(window.location.href)
  if (!url.searchParams.has('import')) return
  url.searchParams.delete('import')
  window.history.replaceState({}, '', `${url.pathname}${url.search}${url.hash}`)
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
  void api.post('/v1/imports/{id}/remap', {
    params: { path: { id } },
    body: {
      type: typeKey,
      id_column: state.idColumn,
      columns: state.columns.filter((column) => column.source === 'manual').map((column) => ({
        col: column.col,
        target: column.target,
      })),
    },
  }).then((session) => adopt(sessionFromApi(session))).catch((error: unknown) => {
    patch({ error: error instanceof Error ? error.message : 'The mapping could not be updated.' })
  })
}

export function setImportIdColumn(idColumn: string) {
  patch({ idColumn })
}

export async function saveImportMapping(name: string) {
  const session = await persistMapping()
  if (!session) return
  await api.post('/v1/import-mappings', { body: { session_id: session.id, name } })
}

export function setImportEmptyClears(emptyClears: boolean) {
  patch({ emptyClears })
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

async function persistMapping() {
  const session = state.session
  if (!session) return null
  const idColumn = state.idColumn.startsWith('col:') ? Number(state.idColumn.slice(4)) : null
  await api.put('/v1/imports/{id}/mapping', { params: { path: { id: session.id } }, body: {
      type: state.typeKey,
      id_column: idColumn,
      empty_clears: state.emptyClears,
      skip_invalid: session.mapping?.skip_invalid ?? false,
      columns: state.columns.map((column) => ({ col: column.col, target: column.target })),
    } })
  return session
}

/** Turns "leave out invalid cells" on or off for a draft and checks the rows again. */
export async function setSkipInvalid(sessionId: string, value: boolean) {
  const raw = (await api.get('/v1/imports/{id}', { params: { path: { id: sessionId } } })) as { mapping?: Record<string, unknown> | null }
  if (!raw.mapping) return
  await api.put('/v1/imports/{id}/mapping', {
    params: { path: { id: sessionId } },
    body: { ...raw.mapping, skip_invalid: value } as never,
  })
  await api.post('/v1/imports/{id}/approve', { params: { path: { id: sessionId } } })
  if (state.session?.id === sessionId) await refresh(sessionId)
}

export async function approveImport() {
  const session = await persistMapping()
  if (!session) return
  await api.post('/v1/imports/{id}/approve', { params: { path: { id: session.id } } })
  patch({ phase: 'preview', dirty: false, filter: 'all' })
  await refresh(session.id)
}

export async function saveImport(excluded: number[] = []) {
  const session = state.session
  if (!session) return
  const draftId = session.draft_id
  if (!draftId) throw new Error('The draft is not ready yet.')
  const failed = Number(session.counters.failed ?? 0)
  const body = (await api.post('/v1/proposals/{id}/accept', {
    params: { path: { id: draftId } },
    body: { skip_failed: failed > 0, exclude_rows: excluded },
  })) as { job_id?: string }
  patch({ phase: 'saving', session: { ...session, job_id: body.job_id ?? null, status: 'saving' } })
}
/** Works through an import draft inside a workspace pane (no dialog). */
export async function attachImport(id: string) {
  if (state.open && !state.embedded) return
  if (state.embedded && state.session?.id === id) return
  generation += 1
  stopPoll()
  patch({ ...initial, embedded: true, phase: 'preview' })
  try {
    await refresh(id)
  } catch (error) {
    patch({ phase: 'error', error: error instanceof Error ? error.message : 'That import does not exist.' })
  }
}

export function detachImport() {
  if (!state.embedded) return
  generation += 1
  stopPoll()
  patch({ ...initial })
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

function sessionFromApi(raw: Schemas['ImportSessionView']): ImportSession {
  return {
    id: raw.id,
    status: raw.status,
    types: raw.types,
    file_name: raw.file_name,
    file_kind: raw.file_kind,
    sheets: sheetsFrom(raw.sheets),
    sheet_index: raw.sheet_index,
    headers: headersFrom(raw.headers),
    sample: sampleFrom(raw.sample),
    rows_parsed: raw.rows_parsed,
    parse_done: raw.parse_done,
    targets: raw.targets,
    proposal: proposalFrom(raw.proposal),
    mapping: mappingFrom(raw.mapping),
    counters: countersFrom(raw.counters),
    job_id: raw.job_id ?? null,
    draft_id: raw.draft_id ?? null,
    error: raw.error ?? null,
  }
}

function sheetsFrom(value: unknown): ImportSession['sheets'] {
  if (!Array.isArray(value)) return []
  const sheets: ImportSession['sheets'] = []
  for (const item of value) {
    const row = jsonRecord(item)
    if (!row || typeof row.name !== 'string') continue
    sheets.push({
      index: Number(row.index ?? 0),
      name: row.name,
      ...(typeof row.empty === 'boolean' ? { empty: row.empty } : {}),
      ...(typeof row.rows_estimate === 'number' ? { rows_estimate: row.rows_estimate } : {}),
    })
  }
  return sheets
}

function headersFrom(value: unknown): ImportHeader[] {
  if (!Array.isArray(value)) return []
  const headers: ImportHeader[] = []
  for (const item of value) {
    const row = jsonRecord(item)
    if (!row) continue
    headers.push({
      index: Number(row.index ?? 0),
      raw: String(row.raw ?? ''),
      name: String(row.name ?? ''),
      empty_ratio: Number(row.empty_ratio ?? 0),
      duplicate_of: typeof row.duplicate_of === 'number' ? row.duplicate_of : null,
    })
  }
  return headers
}

function sampleFrom(value: unknown): string[][] {
  if (!Array.isArray(value)) return []
  return value.map((row) => (Array.isArray(row) ? row.map((cell) => String(cell ?? '')) : []))
}

function countersFrom(value: unknown): Record<string, number> {
  const record = jsonRecord(value)
  const counters: Record<string, number> = {}
  if (!record) return counters
  for (const [key, item] of Object.entries(record)) {
    if (typeof item === 'number') counters[key] = item
  }
  return counters
}

function proposalFrom(value: unknown): ImportSession['proposal'] {
  const record = jsonRecord(value)
  if (!record) return null
  const type = jsonRecord(record.type)
  const idColumn = jsonRecord(record.id_column)
  return {
    ...(typeof record.ai === 'string' ? { ai: record.ai } : {}),
    ...(type
      ? {
          type: {
            value: type.value == null ? null : String(type.value),
            ...(typeof type.confidence === 'number' ? { confidence: type.confidence } : {}),
          },
        }
      : {}),
    ...(idColumn
      ? {
          id_column: {
            value: idColumn.value == null ? null : String(idColumn.value),
            ...(typeof idColumn.confidence === 'number' ? { confidence: idColumn.confidence } : {}),
          },
        }
      : {}),
    columns: columnLinesFrom(record.columns),
  }
}

function columnLinesFrom(value: unknown): ColumnLine[] {
  if (!Array.isArray(value)) return []
  const columns: ColumnLine[] = []
  for (const item of value) {
    const row = jsonRecord(item)
    if (!row || typeof row.target !== 'string') continue
    columns.push({
      col: Number(row.col ?? 0),
      target: row.target,
      confidence: typeof row.confidence === 'number' ? row.confidence : 0,
      source: typeof row.source === 'string' ? row.source : '',
      ...(typeof row.low === 'boolean' ? { low: row.low } : {}),
      ...(row.reason == null || typeof row.reason === 'string' ? { reason: row.reason ?? null } : {}),
    })
  }
  return columns
}

function mappingFrom(value: unknown): ImportSession['mapping'] {
  const record = jsonRecord(value)
  if (!record) return null
  const columns = Array.isArray(record.columns)
    ? record.columns.flatMap((item) => {
        const column = jsonRecord(item)
        if (!column || typeof column.col !== 'number' || typeof column.target !== 'string') return []
        return [{ col: column.col, target: column.target }]
      })
    : undefined
  return {
    ...(typeof record.type === 'string' ? { type: record.type } : {}),
    id_column: typeof record.id_column === 'number' ? record.id_column : null,
    ...(columns ? { columns } : {}),
    ...(record.skip_invalid === true ? { skip_invalid: true } : {}),
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
    open: !state.embedded,
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
  const ticket = generation
  const session = await api.get('/v1/imports/{id}', { params: { path: { id: id } } })
  if (ticket !== generation) return
  adopt(sessionFromApi(session))
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
  if (state.embedded || !state.open) return
  const url = new URL(window.location.href)
  if (url.searchParams.get('import') === id) return
  url.searchParams.set('import', id)
  window.history.replaceState({}, '', url)
}

function beginUpload(file: File) {
  const ticket = ++generation
  stopPoll()
  patch({
    open: true,
    embedded: false,
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
      if (ticket !== generation) {
        void api.delete('/v1/imports/{id}', { params: { path: { id: created.id } } }).catch(() => undefined)
        return
      }
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
    {
      col: 1,
      target: 'type:name',
      confidence: 0.42,
      source: 'decisions',
      low: true,
      alternatives: [{ value: 'type:title', p: 0.38 }],
    },
    { col: 2, target: 'unmapped', confidence: 0, source: 'deterministic', reason: 'empty' },
    {
      col: 3,
      target: 'type:description',
      confidence: 0.72,
      source: 'decisions',
      alternatives: [{ value: 'type:description@en', p: 0.61 }],
    },
    { col: 4, target: 'type:status', confidence: 0.96, source: 'deterministic' },
    { col: 5, target: 'type:color', confidence: 0.93, source: 'deterministic' },
    { col: 6, target: 'type:active', confidence: 0.88, source: 'decisions' },
    { col: 7, target: 'type:launched', confidence: 0.91, source: 'deterministic' },
  ]
  const session: ImportSession = {
    id: 'fixture',
    status: kind === 'diff' ? 'previewed' : 'ready',
    file_name: 'articles.csv',
    file_kind: 'csv',
    sheets: [{ index: 0, name: 'articles', rows_estimate: 24 }],
    sheet_index: 0,
    headers: [
      { index: 0, raw: 'Number', name: 'number', empty_ratio: 0, duplicate_of: null },
      { index: 1, raw: 'Name', name: 'name', empty_ratio: 0, duplicate_of: null },
      { index: 2, raw: 'Notes', name: 'notes', empty_ratio: 1, duplicate_of: null },
      { index: 3, raw: 'Description', name: 'description', empty_ratio: 0, duplicate_of: null },
      { index: 4, raw: 'Status', name: 'status', empty_ratio: 0, duplicate_of: null },
      { index: 5, raw: 'Colour', name: 'colour', empty_ratio: 0, duplicate_of: null },
      { index: 6, raw: 'Active', name: 'active', empty_ratio: 0, duplicate_of: null },
      { index: 7, raw: 'Launched', name: 'launched', empty_ratio: 0, duplicate_of: null },
    ],
    sample: [
      ['A-100', 'Cable 3 m', '', 'Created by import', 'Discontinued', 'Grey', 'yes', '2026-11-06'],
      ['A-101', 'Cable 5 m', '', 'New description', 'OK', 'Green', 'no', '2026-10-10'],
    ],
    rows_parsed: 24,
    parse_done: true,
    types: [
      { key: 'article', label: 'Article' },
      { key: 'product', label: 'Product' },
    ],
    targets: [
      { id: 'type:sku', label: 'Article number', kind: 'text', key: true, source: 'Article' },
      { id: 'type:name', label: 'Name', kind: 'text', source: 'Article' },
      { id: 'type:title', label: 'Title', kind: 'text', source: 'Article' },
      { id: 'type:description', label: 'Description', kind: 'text', source: 'Article' },
      { id: 'type:description@en', label: 'Description (en)', kind: 'text', source: 'Article' },
      { id: 'type:status', label: 'Status', kind: 'text', source: 'Article' },
      { id: 'type:color', label: 'Colour', kind: 'text', source: 'Article' },
      { id: 'type:active', label: 'Active', kind: 'boolean', source: 'Article' },
      { id: 'type:launched', label: 'Launched', kind: 'date', source: 'Article' },
      { id: 'type:weight', label: 'Weight', kind: 'number', source: 'Article' },
    ],
    proposal: {
      ai: 'skipped',
      type: { value: 'article', confidence: 0.84 },
      id_column: { value: 'col:0', confidence: 0.97 },
      columns,
    },
    mapping: null,
    counters: { new: 2, changed: 1, failed: 1, unchanged: 0 },
    job_id: null,
    error: null,
  }
  patch({
    open: true,
    phase: kind === 'diff' ? 'preview' : 'mapping',
    fileName: session.file_name,
    session,
    columns,
    typeKey: 'article',
    idColumn: 'col:0',
    dirty: false,
    error: '',
    notice: '',
  })
}
