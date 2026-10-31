import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { useQueries, useQuery } from '@tanstack/react-query'
import { useVirtualizer } from '@tanstack/react-virtual'
import { toast } from 'sonner'
import { api, jsonRecord } from '../api'
import { copy } from '../i18n'
import { handleAppContextMenu } from '../app-context-menu'
import {
  approveImport,
  attachImport,
  detachImport,
  assignImportTarget,
  backToMapping,
  confirmImport,
  dropImportFiles,
  dismissConfirm,
  getImportSnapshot,
  hoverColumn,
  redoImport,
  requestCloseImport,
  discardImport,
  saveImport,
  setImportEmptyClears,
  saveImportMapping,
  selectColumn,
  setImportFilter,
  setImportIdColumn,
  setImportType,
  showImportFixture,
  subscribeImport,
  undoImport,
  type ImportSession,
  type ImportTarget,
} from './controller'
import { TargetPicker } from './target-picker'
import { DraftTable } from '../drafts/draft-table'
import {
  alternativeChoices,
  ariaMap,
  confidenceLabel,
  confidenceTone,
  dedupedTargets,
  targetForStored,
  mappedLines,
  mappingRank,
  matchSummary,
  rowClass,
  cellClass,
  importCellText,
  saveLabel,
  showTypeConfidence,
  stepIndex,
} from './model'

type PreviewRow = {
  row_no: number
  status: string
  object_key?: string | null
  cells: Record<string, { value?: unknown; before?: unknown; state?: string; error?: { message?: string } }>
  errors?: Array<{ message?: string }>
}

function previewFrom(row: { row_no: number; status: string; object_key?: string | null; cells: unknown; errors: unknown }): PreviewRow {
  return {
    row_no: row.row_no,
    status: row.status,
    object_key: row.object_key,
    cells: cellsFrom(row.cells),
    errors: errorsFrom(row.errors),
  }
}

function cellsFrom(value: unknown): PreviewRow['cells'] {
  const record = jsonRecord(value)
  const cells: PreviewRow['cells'] = {}
  if (!record) return cells
  for (const [key, item] of Object.entries(record)) {
    const cell = jsonRecord(item)
    if (!cell) continue
    const error = jsonRecord(cell.error)
    cells[key] = {
      ...(cell.value != null && cell.value !== '' ? { value: cell.value } : {}),
      ...(cell.before != null && cell.before !== '' ? { before: cell.before } : {}),
      ...(typeof cell.state === 'string' ? { state: cell.state } : {}),
      ...(error ? { error: { ...(typeof error.message === 'string' ? { message: error.message } : {}) } } : {}),
    }
  }
  return cells
}

function errorsFrom(value: unknown): PreviewRow['errors'] {
  if (!Array.isArray(value)) return undefined
  return value.flatMap((item) => {
    const row = jsonRecord(item)
    if (!row) return []
    return [{ ...(typeof row.message === 'string' ? { message: row.message } : {}) }]
  })
}

const fixtureRows: PreviewRow[] = [
  { row_no: 2, status: 'new', object_key: 'A-200', cells: { '0': { value: 'A-200', state: 'new' }, '1': { value: 'Lamp', state: 'new' } } },
  {
    row_no: 3,
    status: 'changed',
    object_key: 'A-100',
    cells: { '0': { value: 'A-100', state: 'same' }, '1': { value: 'Cable 5m', state: 'changed', before: 'Cable 3m' } },
  },
  {
    row_no: 4,
    status: 'failed',
    object_key: 'A-300',
    cells: { '0': { value: 'A-300', state: 'same' }, '1': { value: '??', state: 'error', error: { message: 'Not a number' } } },
    errors: [{ message: 'Not a number' }],
  },
]

export function ImportScreen() {
  const snap = useSyncExternalStore(subscribeImport, getImportSnapshot, getImportSnapshot)
  useEffect(() => {
    const params = new URLSearchParams(window.location.search)
    const preview = params.get('importPreview')
    if (import.meta.env.DEV && (preview === 'map' || preview === 'diff')) {
      showImportFixture(preview)
    }
    const onKey = (event: KeyboardEvent) => {
      const current = getImportSnapshot()
      if (!current.open) return
      const typing = event.target instanceof HTMLElement && ['INPUT', 'TEXTAREA', 'SELECT'].includes(event.target.tagName)
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'z' && !typing) {
        event.preventDefault()
        if (event.shiftKey) redoImport()
        else undoImport()
      }
      if (event.key === 'Escape') {
        if (document.querySelector('[data-import-menu-open="true"]')) return
        event.preventDefault()
        if (current.confirm) dismissConfirm()
        else requestCloseImport()
      }
      if ((event.key === 'Delete' || event.key === 'Backspace') && current.selected != null && !typing) {
        event.preventDefault()
        assignImportTarget(current.selected, 'unmapped')
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  if (!snap.open && !snap.drag) return null
  return (
    <>
      {snap.drag ? (
        <div className="import-drop" data-testid="import-drop-overlay" onContextMenu={handleAppContextMenu}>
          {copy.importDropOverlay}
        </div>
      ) : null}
      {snap.open ? (
        <section className="import-screen" data-testid="import-screen" role="dialog" aria-modal="true" aria-label={copy.importFile} onContextMenu={handleAppContextMenu}>
          <header className="import-top">
            <div>
              <strong>{snap.fileName || copy.importFile}</strong>
            </div>
            <Steps phase={snap.phase} />
            <button type="button" className="btn-ghost" onClick={requestCloseImport}>{copy.close}</button>
          </header>
          {snap.notice ? <p className="import-notice">{snap.notice}</p> : null}
          {snap.phase === 'empty' || snap.phase === 'error' ? <Empty snap={snap} /> : null}
          {snap.phase === 'uploading' ? <Uploading name={snap.fileName} ratio={snap.upload} /> : null}
          {snap.phase === 'mapping' && snap.session ? <Mapping /> : null}
          {snap.phase === 'mapping' && !snap.session ? (
            <div className="import-preparing" role="status" data-testid="import-ai-pending">
              <span className="import-spinner" aria-hidden="true" />
              <span>{copy.importPreparing}</span>
            </div>
          ) : null}
          {(snap.phase === 'preview' || snap.phase === 'saving' || snap.phase === 'done') && snap.session ? <Preview /> : null}
          {snap.confirm ? (
            <div className="import-confirm" role="dialog">
              <p>{snap.confirm === 'replace' ? copy.importReplace : copy.importCloseConfirm}</p>
              <button type="button" className="primary" onClick={confirmImport}>{copy.importYes}</button>
              <button type="button" onClick={dismissConfirm}>{copy.importNo}</button>
            </div>
          ) : null}
        </section>
      ) : null}
    </>
  )
}

/**
 * One import draft worked through in a workspace pane: the same Map and Check steps as
 * the import dialog, without the dialog chrome. `onChanged` fires when the draft was saved,
 * `onGone` when it was discarded.
 */
export function ImportWorkspace({ sessionId, onChanged, onGone }: { sessionId: string; onChanged: () => void; onGone: () => void }) {
  const snap = useSyncExternalStore(subscribeImport, getImportSnapshot, getImportSnapshot)
  const seen = useRef(false)
  useEffect(() => {
    seen.current = false
    void attachImport(sessionId)
    return () => detachImport()
  }, [sessionId])
  useEffect(() => {
    if (snap.session?.id === sessionId) seen.current = true
    else if (seen.current && !snap.session && !snap.embedded) {
      seen.current = false
      onGone()
    }
  }, [snap.session, snap.embedded, sessionId, onGone])
  useEffect(() => {
    if (snap.embedded && snap.phase === 'done') onChanged()
  }, [snap.embedded, snap.phase, onChanged])

  if (snap.open && !snap.embedded) {
    return <p className="muted" data-testid="import-workspace-busy">Another import is open. Finish or close it first.</p>
  }
  return (
    <section className="import-screen import-embedded" data-testid="import-workspace" aria-label={copy.importFile}>
      <header className="import-top">
        <div>
          <strong>{snap.fileName || copy.importFile}</strong>
        </div>
        <Steps phase={snap.phase} />
        <span />
      </header>
      {snap.notice ? <p className="import-notice">{snap.notice}</p> : null}
      {snap.phase === 'error' ? <p className="error" role="alert">{snap.error || 'That import could not be opened.'}</p> : null}
      {snap.phase === 'mapping' && snap.session ? <Mapping /> : null}
      {(snap.phase === 'preview' || snap.phase === 'saving' || snap.phase === 'done') && snap.session ? <Preview /> : null}
      {!snap.session && snap.phase !== 'error' ? (
        <div className="import-preparing" role="status">
          <span className="import-spinner" aria-hidden="true" />
          <span>{copy.importPreparing}</span>
        </div>
      ) : null}
    </section>
  )
}

function Empty({ snap }: { snap: ReturnType<typeof getImportSnapshot> }) {
  return (
    <div className="import-empty" data-testid="import-dropzone">
      <p>{snap.error || copy.importDrop}</p>
      <label className="primary import-choose">
        {copy.importChoose}
        <input
          data-testid="import-file"
          type="file"
          accept=".xlsx,.xls,.csv,.tsv,.txt,text/csv"
          hidden
          onChange={(event) => {
            const file = event.target.files?.[0]
            if (file) dropImportFiles([file])
          }}
        />
      </label>
    </div>
  )
}

function Uploading({ name, ratio }: { name: string; ratio: number }) {
  return (
    <div className="import-empty">
      <p>{name}</p>
      <div className="import-progress" aria-valuenow={Math.round(ratio * 100)} aria-valuemin={0} aria-valuemax={100}>
        <span style={{ width: `${Math.round(ratio * 100)}%` }} />
      </div>
    </div>
  )
}

function Steps({ phase }: { phase: ReturnType<typeof getImportSnapshot>['phase'] }) {
  const current = stepIndex(phase)
  const steps = [
    { n: 1 as const, label: copy.importStepMap },
    { n: 2 as const, label: copy.importStepCheck },
    { n: 3 as const, label: copy.importStepSave },
  ]
  return (
    <ol className="import-steps" data-testid="import-steps" data-step={current}>
      {steps.map((step) => (
        <li key={step.n} className={step.n === current ? 'on' : step.n < current ? 'done' : ''}>
          <span>{step.n}</span>
          {step.label}
        </li>
      ))}
    </ol>
  )
}

function Mapping() {
  const snap = useSyncExternalStore(subscribeImport, getImportSnapshot, getImportSnapshot)
  const session = snap.session!
  const targets = session.targets ?? []
  const choices = dedupedTargets(targets.filter((target) => target.id !== 'action'))
  const labelOf = (id: string) => targetForStored(targets, id)?.label ?? id
  const headers = [...(session.headers ?? [])].sort((left, right) => {
    const leftColumn = snap.columns.find((item) => item.col === left.index)
    const rightColumn = snap.columns.find((item) => item.col === right.index)
    return mappingRank(leftColumn) - mappingRank(rightColumn) || left.index - right.index
  })
  const lines = mappedLines(snap.columns)
  const suggested = session.proposal?.type?.value
  const typeConfidence = showTypeConfidence(snap.typeKey, suggested, session.proposal?.type?.confidence)
  const [menu, setMenu] = useState<number | null>(null)
  const waiting = session.status === 'mapping' && (!session.proposal || session.proposal.ai === 'pending')
  if (waiting) {
    return (
      <div className="import-preparing" role="status" data-testid="import-ai-pending">
        <span className="import-spinner" aria-hidden="true" />
        <span>{copy.importPreparing}</span>
      </div>
    )
  }

  return (
    <div className="import-map" data-testid="import-mapping">
      <p className="import-banner" data-testid="import-banner">
        {' '}{matchSummary(snap.columns)}
        {snap.idColumn === 'none' ? ` ${copy.importNoId}` : ''}
      </p>
      <div className="import-map-bar">
        <label>
          {copy.type}
          <select value={(session.types ?? []).find((item) => item.key === snap.typeKey || item.id === snap.typeKey)?.key ?? snap.typeKey} onChange={(event) => setImportType(event.target.value)} data-testid="import-type">
            <option value="">{copy.type}</option>
            {(session.types ?? []).map((item) => (
              <option key={item.key} value={item.key}>{item.label}</option>
            ))}
          </select>
          {typeConfidence != null ? (
            <em className={`import-confidence ${typeConfidence < 0.6 ? 'low' : 'warn'}`}>{confidenceLabel('decisions', typeConfidence)}</em>
          ) : null}
        </label>
        <label>
          {copy.importMatchedBy}
          <select value={snap.idColumn} onChange={(event) => setImportIdColumn(event.target.value)} data-testid="import-id-column">
            <option value="none">none</option>
            {(session.headers ?? []).map((header) => (
              <option key={header.index} value={`col:${header.index}`}>{header.raw || `Column ${header.index + 1}`}</option>
            ))}
          </select>
        </label>
      </div>
      <label className="import-empty-clears">
        <input type="checkbox" data-testid="import-empty-clears" checked={snap.emptyClears} onChange={(event) => setImportEmptyClears(event.target.checked)} />
        {' '}{copy.importEmptyClears}
      </label>
      <div className="import-map-head">
        <span>{copy.importFileColumn}</span>
        <span>{copy.importAttribute}</span>
      </div>
      <div className="import-map-list" data-testid="import-sources">
        {headers.map((header) => {
          const column = snap.columns.find((item) => item.col === header.index) ?? {
            col: header.index,
            target: 'unmapped',
            confidence: 0,
            source: 'deterministic',
          }
          const tone = confidenceTone(column)
          const empty = header.empty_ratio >= 1
          const alts = alternativeChoices(column, targets)
          return (
            <div
              key={header.index}
              className={`import-map-row ${tone} ${empty ? 'empty' : ''} ${snap.selected === header.index ? 'selected' : ''}`}
              data-col={header.index}
              data-testid={`import-col-${header.index}`}
              onMouseEnter={() => hoverColumn(header.index)}
              onMouseLeave={() => hoverColumn(null)}
            >
              <div className="import-map-file">
                <strong>{header.raw || `Column ${header.index + 1}`}</strong>
                <small>{empty ? copy.importEmpty : sampleOf(session, header.index)}</small>
              </div>
              <span className={`import-confidence ${tone}`}>
                {tone === 'unmapped' ? '—' : confidenceLabel(column.source, column.confidence)}
              </span>
              <span className="import-join" aria-hidden="true" />
              <div className="import-map-target">
                <button
                  type="button"
                  className="import-field"
                  data-testid={`import-target-${header.index}`}
                  aria-expanded={menu === header.index}
                  onClick={() => {
                    selectColumn(header.index)
                    setMenu(menu === header.index ? null : header.index)
                  }}
                >
                  {column.target !== 'unmapped' ? labelOf(column.target) : copy.importChooseField}
                </button>
                {menu === header.index ? (
                  <TargetPicker
                    targets={choices}
                    current={column.target}
                    onPick={(target) => {
                      assignImportTarget(header.index, target)
                      setMenu(null)
                    }}
                    onClose={() => setMenu(null)}
                  />
                ) : null}
                {alts.length > 0 ? (
                  <div className="import-alts">
                    {alts.map((alt) => (
                      <button key={alt.id} type="button" onClick={() => assignImportTarget(header.index, alt.id)}>
                        {alt.label}
                      </button>
                    ))}
                  </div>
                ) : null}
              </div>
            </div>
          )
        })}
      </div>
      <ul className="import-aria" aria-label="Column mapping">
        {lines.map((line) => {
          const header = (session.headers ?? []).find((item) => item.index === line.col)
          return <li key={line.col}>{ariaMap(header?.raw || `Column ${line.col + 1}`, labelOf(line.target), line.confidence)}</li>
        })}
      </ul>
      <footer className="import-actions">
        <button
          type="button"
          data-testid="import-save-mapping"
          disabled={mappedLines(snap.columns).length === 0 || !snap.typeKey}
          onClick={() => {
            const name = window.prompt(copy.importSaveMappingAs, session.file_name.replace(/\.[^.]+$/, ''))
            if (!name?.trim()) return
            void saveImportMapping(name.trim()).then(() => toast(copy.importMappingSaved)).catch((error: unknown) => toast(error instanceof Error ? error.message : 'Save failed'))
          }}
        >
          {copy.importSaveMappingAs}
        </button>
        <button
          type="button"
          className="primary"
          data-testid="import-approve"
          disabled={waiting && !session.proposal || mappedLines(snap.columns).length === 0 || !snap.typeKey}
          onClick={() => void approveImport().catch((error: unknown) => toast(error instanceof Error ? error.message : 'Approve failed'))}
        >
          {copy.importNext}
        </button>
      </footer>
    </div>
  )
}

function Preview() {
  const snap = useSyncExternalStore(subscribeImport, getImportSnapshot, getImportSnapshot)
  const session = snap.session!
  const counters = session.counters ?? {}
  const failed = Number(counters.failed ?? 0)
  const changes = Number(counters.new ?? 0) + Number(counters.changed ?? 0)
  const parent = useRef<HTMLDivElement>(null)
  const [excluded, setExcluded] = useState<number[]>([])
  const fixture = session.id === 'fixture'
  const localRows = fixtureRows.filter((row) => snap.filter === 'all' || row.status === snap.filter)
  const probe = useQuery({
    queryKey: ['import-rows', session.id, snap.filter, 'count', session.status, counters.processed ?? 0],
    queryFn: () => api.get('/v1/imports/{id}/rows', { params: { path: { id: session.id }, query: { offset: 0, limit: 1, status: snap.filter } } }),
    enabled: !fixture,
  })
  const total = fixture ? localRows.length : probe.data?.total ?? 0
  const virtualizer = useVirtualizer({
    count: total,
    getScrollElement: () => parent.current,
    estimateSize: () => 28,
    overscan: 16,
  })
  const visible = virtualizer.getVirtualItems()
  const pages = fixture ? [] : [...new Set(visible.map((item) => Math.floor(item.index / 100)))]
  const windows = useQueries({
    queries: pages.map((page) => ({
      queryKey: ['import-rows', session.id, snap.filter, page, session.status, total],
      queryFn: () => api.get('/v1/imports/{id}/rows', { params: { path: { id: session.id }, query: { offset: Number(page * 100), limit: 100, status: snap.filter } } }),
    })),
  })
  const byIndex = new Map<number, PreviewRow>()
  if (fixture) localRows.forEach((row, index) => byIndex.set(index, row))
  windows.forEach((result, index) => {
    const page = pages[index] ?? 0
    ;(result.data?.rows ?? []).forEach((row, offset) => byIndex.set(page * 100 + offset, previewFrom(row)))
  })
  const headers = (session.headers ?? []).filter((header) => {
    const column = snap.columns.find((item) => item.col === header.index)
    return column && column.target !== 'unmapped'
  })
  const percent = session.rows_parsed && !session.parse_done
    ? Math.min(99, Math.round((Number(counters.processed ?? 0) / Math.max(session.rows_parsed, 1)) * 100))
    : session.status === 'diffing'
      ? Math.min(99, Math.round((Number(counters.processed ?? 0) / Math.max(session.rows_parsed, 1)) * 100))
      : 100

  return (
    <div className="import-preview" data-testid="import-preview">
      {session.status === 'diffing' ? <p>{copy.importChecking} {percent}%</p> : null}
      {session.status === 'saving' ? <p>{copy.importChecking}</p> : null}
      {!fixture ? (
        session.draft_id && session.status !== 'diffing' ? (
          <DraftTable
            proposalId={session.draft_id}
            version={`${session.status}:${JSON.stringify(counters)}`}
            excluded={excluded}
            onExcludedChange={setExcluded}
            editable={session.status === 'previewed'}
          />
        ) : (
          <div className="import-table" role="status" data-testid="import-checking" />
        )
      ) : (
      <div className="import-table" ref={parent}>
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map((item) => {
            const row = byIndex.get(item.index)
            if (!row) return null
            return (
              <div
                key={row.row_no}
                className={rowClass(row.status)}
                data-testid={`import-row-${row.status}`}
                style={{ position: 'absolute', top: 0, transform: `translateY(${item.start}px)`, height: 28 }}
              >
                <span>{row.row_no}</span>
                <span>{row.status === 'failed' ? '⛔' : row.status === 'new' ? '＋' : '•'}</span>
                {headers.map((header) => {
                  const cell = row.cells[String(header.index)]
                  const title = cellTitle(cell)
                  return (
                    <span key={header.index} className={cellClass(cell?.state ?? 'same')} title={title}>
                      {cell?.state === 'changed' && cell.before ? (
                        <>
                          <span className="diff-old">{importCellText(cell.before)}</span>
                          <span className="diff-arrow">{' -> '}</span>
                          <span className="diff-new">{importCellText(cell.value)}</span>
                        </>
                      ) : importCellText(cell?.value)}
                    </span>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
      )}
      <footer className="import-actions">
        <div className="import-actions-start">
          <button type="button" className="btn-ghost" onClick={backToMapping}>{copy.importBack}</button>
          {failed > 0 ? (
            <a className="import-link" href={fixture ? undefined : `/v1/imports/${session.id}/failed.xlsx`}>{copy.importFailedDownload}</a>
          ) : null}
        </div>
        {snap.phase === 'done' ? (
          <p className="import-note">
            Imported {counters.new ?? 0} · Updated {counters.changed ?? 0} · Skipped {failed}
          </p>
        ) : (
          <div className="import-actions-end">
            {failed > 0 ? <span className="import-note">{failed} with errors will be skipped</span> : null}
            <button type="button" data-testid="import-discard" onClick={discardImport}>{copy.discard}</button>
            <button
              type="button"
              className="primary"
              data-testid="import-save"
              disabled={fixture || session.status !== 'previewed'}
              onClick={() => {
                void saveImport(excluded).catch((error: unknown) => toast(error instanceof Error ? error.message : 'Save failed'))
              }}
            >
              {saveLabel(failed, changes)}
            </button>
          </div>
        )}
      </footer>
    </div>
  )
}

function sampleOf(session: ImportSession, index: number) {
  return session.sample
    .map((row) => row[index])
    .filter(Boolean)
    .slice(0, 3)
    .join(' · ')
}

function cellTitle(cell: { before?: unknown; state?: string; error?: { message?: string } } | undefined) {
  if (!cell) return undefined
  const parts: string[] = []
  if (cell.before != null && cell.before !== '' && cell.state !== 'same' && cell.state !== 'new') {
    parts.push(`was: ${importCellText(cell.before)}`)
  }
  if (cell.error?.message) parts.push(cell.error.message)
  return parts.join(' · ') || undefined
}
