import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { useQueries, useQuery } from '@tanstack/react-query'
import { useVirtualizer } from '@tanstack/react-virtual'
import { toast } from 'sonner'
import { api } from '../api'
import { copy } from '../i18n'
import {
  approveImport,
  assignImportTarget,
  backToMapping,
  confirmImport,
  dropImportFiles,
  dismissConfirm,
  getImportSnapshot,
  hoverColumn,
  redoImport,
  requestCloseImport,
  saveImport,
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
import {
  ariaMap,
  confidenceLabel,
  curve,
  lineTone,
  mappedLines,
  matchBanner,
  rowClass,
  cellClass,
  saveLabel,
  stepIndex,
} from './model'

type PreviewRow = {
  row_no: number
  status: string
  object_key?: string | null
  cells: Record<string, { value?: string; before?: string; state?: string; error?: { message?: string } }>
  errors?: Array<{ message?: string }>
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
  { row_no: 5, status: 'delete', object_key: 'A-400', cells: { '0': { value: 'A-400', state: 'same' }, '1': { value: 'Old', state: 'same' } } },
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
        <div className="import-drop" data-testid="import-drop-overlay">
          {copy.importDropOverlay}
        </div>
      ) : null}
      {snap.open ? (
        <section className="import-screen" data-testid="import-screen" role="dialog" aria-modal="true" aria-label={copy.importFile}>
          <header className="import-top">
            <div>
              <strong>{snap.fileName || copy.importFile}</strong>
              <p className="import-steps" data-testid="import-steps">{copy.importSteps}</p>
            </div>
            <span className={`import-step on-${stepIndex(snap.phase)}`} data-step={stepIndex(snap.phase)} />
            <button type="button" className="btn-ghost" onClick={requestCloseImport}>{copy.close}</button>
          </header>
          {snap.notice ? <p className="import-notice">{snap.notice}</p> : null}
          {snap.phase === 'empty' || snap.phase === 'error' ? <Empty snap={snap} /> : null}
          {snap.phase === 'uploading' ? <Uploading name={snap.fileName} ratio={snap.upload} /> : null}
          {snap.phase === 'mapping' && snap.session ? <Mapping /> : null}
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

function Mapping() {
  const snap = useSyncExternalStore(subscribeImport, getImportSnapshot, getImportSnapshot)
  const session = snap.session!
  const banner = matchBanner(snap.columns)
  const [query, setQuery] = useState('')
  const [popover, setPopover] = useState<number | null>(null)
  const stage = useRef<HTMLDivElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const targets = session.targets ?? []
  const labelOf = (id: string) => targets.find((target) => target.id === id)?.label ?? id
  const headers = [...(session.headers ?? [])].sort((left, right) => left.empty_ratio - right.empty_ratio)
  const lines = mappedLines(snap.columns)

  useEffect(() => {
    const node = canvas.current
    const root = stage.current
    if (!node || !root) return
    const draw = () => {
      const rect = root.getBoundingClientRect()
      const dpr = window.devicePixelRatio || 1
      node.width = Math.max(1, Math.floor(rect.width * dpr))
      node.height = Math.max(1, Math.floor(rect.height * dpr))
      node.style.width = `${rect.width}px`
      node.style.height = `${rect.height}px`
      const context = node.getContext('2d')
      if (!context) return
      context.setTransform(dpr, 0, 0, dpr, 0, 0)
      context.clearRect(0, 0, rect.width, rect.height)
      const styles = getComputedStyle(document.documentElement)
      const text = styles.getPropertyValue('--text-2').trim() || '#8b93a7'
      const warn = styles.getPropertyValue('--warn').trim() || '#f6c445'
      const raised = styles.getPropertyValue('--raised').trim() || '#1c2030'
      const hover = snap.hover ?? snap.selected
      for (const line of lines) {
        const from = anchor(root, `[data-col="${line.col}"]`)
        const to = anchor(root, `[data-target="${cssEscape(line.target)}"]`)
        if (!from || !to) continue
        const path = curve(from, to)
        const tone = lineTone(line)
        const emphasized = hover == null || hover === line.col
        context.save()
        context.globalAlpha = emphasized ? 0.95 : 0.12
        context.strokeStyle = tone === 'warn' ? warn : text
        context.lineWidth = emphasized && hover === line.col ? path.widthHover : path.width
        context.setLineDash(tone === 'warn' || tone === 'suggest' ? [4, 4] : [])
        context.beginPath()
        context.moveTo(from.x, from.y)
        context.quadraticCurveTo(path.control.x, path.control.y, to.x, to.y)
        context.stroke()
        const label = confidenceLabel(line.source, line.confidence)
        const mid = { x: (from.x + to.x) / 2, y: (from.y + to.y) / 2 }
        context.font = '11px Inter, sans-serif'
        const boxW = context.measureText(label).width + 12
        context.globalAlpha = emphasized ? 1 : 0.16
        context.fillStyle = raised
        roundRect(context, mid.x - boxW / 2, mid.y - 8, boxW, 16, 8)
        context.fill()
        context.fillStyle = tone === 'warn' ? warn : text
        context.textAlign = 'center'
        context.textBaseline = 'middle'
        context.fillText(label, mid.x, mid.y)
        context.restore()
      }
    }
    draw()
    const observer = new ResizeObserver(draw)
    observer.observe(root)
    return () => observer.disconnect()
  }, [lines, snap.hover, snap.selected, session.id, query])

  const filtered = targets.filter((target) => {
    const hay = `${target.label} ${target.id}`.toLowerCase()
    return hay.includes(query.trim().toLowerCase())
  })

  return (
    <div className="import-map" data-testid="import-mapping">
      <div className="import-banner">
        We matched {banner.matched} of {banner.total} columns. Check the {banner.marked} marked ones.
        {snap.idColumn === 'none' ? ` ${copy.importNoId}` : ''}
      </div>
      <div className="import-map-bar">
        <label>
          {copy.type}
          <select value={snap.typeKey} onChange={(event) => setImportType(event.target.value)} data-testid="import-type">
            <option value="">Type</option>
            {(session.types ?? []).map((item) => (
              <option key={item.key} value={item.key}>
                {item.label} · {Math.round((session.proposal?.type?.confidence ?? 1) * 100)} %
              </option>
            ))}
          </select>
        </label>
        <label>
          {copy.importMatchedBy}
          <select value={snap.idColumn} onChange={(event) => setImportIdColumn(event.target.value)} data-testid="import-id-column">
            <option value="none">none</option>
            {headers.map((header) => (
              <option key={header.index} value={`col:${header.index}`}>{header.raw || `Column ${header.index + 1}`}</option>
            ))}
          </select>
        </label>
      </div>
      <div className="import-columns" ref={stage}>
        <div className="import-col-list" data-testid="import-sources">
          <h2>{copy.importColumns}</h2>
          {headers.map((header) => {
            const column = snap.columns.find((item) => item.col === header.index)
            const empty = header.empty_ratio >= 1
            return (
              <button
                key={header.index}
                type="button"
                className={`import-source ${empty ? 'empty' : ''} ${lineTone(column ?? { col: header.index, target: 'unmapped', confidence: 0, source: 'deterministic' })}`}
                data-col={header.index}
                data-testid={`import-col-${header.index}`}
                onClick={() => {
                  selectColumn(header.index)
                  setPopover(header.index)
                }}
                onMouseEnter={() => hoverColumn(header.index)}
                onMouseLeave={() => hoverColumn(null)}
                draggable={!empty}
                onDragStart={(event) => event.dataTransfer.setData('text/import-col', String(header.index))}
              >
                <span>{header.raw || `Column ${header.index + 1}`}</span>
                <small>{empty ? copy.importEmpty : sampleOf(session, header.index)}</small>
                <em className="import-chip">{column && column.target !== 'unmapped' ? labelOf(column.target) : ''}</em>
              </button>
            )
          })}
        </div>
        <canvas ref={canvas} className="import-canvas" data-testid="import-canvas" aria-hidden="true" />
        <div className="import-col-list" data-testid="import-targets">
          <h2>{copy.importModel}</h2>
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={copy.importSearchFields} />
          {filtered.filter((target) => target.id !== 'action').map((target) => (
            <div
              key={target.id}
              className="import-target"
              data-target={target.id}
              onDragOver={(event) => event.preventDefault()}
              onDrop={(event) => {
                const col = Number(event.dataTransfer.getData('text/import-col'))
                if (Number.isFinite(col)) assignImportTarget(col, target.id)
              }}
            >
              <i className={lines.some((line) => line.target === target.id) ? 'dot on' : 'dot'} />
              <span>{target.key ? '🔑 ' : ''}{target.label}</span>
            </div>
          ))}
          <div
            className="import-target"
            data-target="action"
            onDragOver={(event) => event.preventDefault()}
            onDrop={(event) => {
              const col = Number(event.dataTransfer.getData('text/import-col'))
              if (Number.isFinite(col)) assignImportTarget(col, 'action')
            }}
          >
            <i className={lines.some((line) => line.target === 'action') ? 'dot on' : 'dot'} />
            <span>{copy.importAction}</span>
          </div>
        </div>
      </div>
      <ul className="import-aria" aria-label="Column mapping">
        {lines.map((line) => {
          const header = headers.find((item) => item.index === line.col)
          return <li key={line.col}>{ariaMap(header?.raw || `Column ${line.col + 1}`, labelOf(line.target), line.confidence)}</li>
        })}
      </ul>
      {popover != null ? (
        <TargetPopover
          targets={filtered}
          onPick={(target) => {
            assignImportTarget(popover, target)
            setPopover(null)
          }}
          onClose={() => setPopover(null)}
        />
      ) : null}
      <footer className="import-actions">
        <button
          type="button"
          className="primary"
          data-testid="import-approve"
          disabled={mappedLines(snap.columns).length === 0 || !snap.typeKey}
          onClick={() => void approveImport().catch((error: unknown) => toast(error instanceof Error ? error.message : 'Approve failed'))}
        >
          {copy.importApprove}
        </button>
      </footer>
    </div>
  )
}

function TargetPopover({
  targets,
  onPick,
  onClose,
}: {
  targets: ImportTarget[]
  onPick: (id: string) => void
  onClose: () => void
}) {
  return (
    <div className="import-popover" data-testid="import-popover">
      <button type="button" onClick={() => onPick('unmapped')}>Remove line</button>
      {targets.map((target) => (
        <button key={target.id} type="button" onClick={() => onPick(target.id)}>{target.label}</button>
      ))}
      <button type="button" onClick={onClose}>{copy.close}</button>
    </div>
  )
}

function Preview() {
  const snap = useSyncExternalStore(subscribeImport, getImportSnapshot, getImportSnapshot)
  const session = snap.session!
  const counters = session.counters ?? {}
  const failed = Number(counters.failed ?? 0)
  const changes = Number(counters.new ?? 0) + Number(counters.changed ?? 0) + Number(counters.delete ?? 0)
  const parent = useRef<HTMLDivElement>(null)
  const fixture = session.id === 'fixture'
  const localRows = fixtureRows.filter((row) => snap.filter === 'all' || row.status === snap.filter)
  const probe = useQuery({
    queryKey: ['import-rows', session.id, snap.filter, 'count'],
    queryFn: () => api<{ total: number; rows: PreviewRow[] }>(
      `/v1/imports/${session.id}/rows?offset=0&limit=1&status=${encodeURIComponent(snap.filter)}`,
    ),
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
      queryKey: ['import-rows', session.id, snap.filter, page],
      queryFn: () => api<{ total: number; rows: PreviewRow[] }>(
        `/v1/imports/${session.id}/rows?offset=${page * 100}&limit=100&status=${encodeURIComponent(snap.filter)}`,
      ),
    })),
  })
  const byIndex = new Map<number, PreviewRow>()
  if (fixture) localRows.forEach((row, index) => byIndex.set(index, row))
  windows.forEach((result, index) => {
    const page = pages[index] ?? 0
    ;(result.data?.rows ?? []).forEach((row, offset) => byIndex.set(page * 100 + offset, row))
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
      <div className="import-counters">
        {chip('new', copy.importNew, counters.new, 'ok')}
        {chip('changed', copy.importChanged, counters.changed, 'warn')}
        {chip('delete', copy.importDelete, counters.delete, 'danger')}
        {chip('failed', copy.importErrors, counters.failed, 'danger')}
        {chip('all', copy.importUnchanged, counters.unchanged, 'neutral')}
      </div>
      {session.status === 'diffing' ? <p>{copy.importChecking} {percent}%</p> : null}
      {session.status === 'saving' ? <p>{copy.importChecking}</p> : null}
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
                <span>{row.status === 'delete' ? '🗑' : row.status === 'failed' ? '⛔' : row.status === 'new' ? '＋' : '•'}</span>
                {headers.map((header) => {
                  const cell = row.cells[String(header.index)]
                  const title = cell?.state === 'changed' ? `was: ${cell.before ?? ''}` : cell?.error?.message
                  return (
                    <span key={header.index} className={cellClass(cell?.state ?? 'same')} title={title}>
                      {cell?.value ?? ''}
                    </span>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
      <footer className="import-actions">
        <button type="button" onClick={backToMapping}>{copy.importBack}</button>
        {failed > 0 ? (
          <a href={fixture ? undefined : `/v1/imports/${session.id}/failed.xlsx`}>{copy.importFailedDownload}</a>
        ) : null}
        {snap.phase === 'done' ? (
          <p>
            Imported {counters.new ?? 0} · Updated {counters.changed ?? 0} · Deleted {counters.delete ?? 0} · Skipped {failed}
          </p>
        ) : (
          <button
            type="button"
            className="primary"
            data-testid="import-save"
            disabled={fixture || session.status === 'diffing' || session.status === 'saving'}
            onClick={() => {
              if (Number(counters.delete ?? 0) > 0) {
                const ok = window.confirm(`${copy.importDeleteObjects} ${counters.delete} ${copy.importObjectsWord}`)
                if (!ok) return
              }
              void saveImport().catch((error: unknown) => toast(error instanceof Error ? error.message : 'Save failed'))
            }}
          >
            {saveLabel(failed, changes)}
          </button>
        )}
      </footer>
    </div>
  )
}

function chip(filter: string, label: string, value: number | undefined, tone: string) {
  return (
    <button type="button" className={`meta-chip ${tone}`} onClick={() => setImportFilter(filter === 'all' ? 'all' : filter)}>
      {label} {value ?? 0}
    </button>
  )
}

function sampleOf(session: ImportSession, index: number) {
  return session.sample
    .map((row) => row[index])
    .filter(Boolean)
    .slice(0, 3)
    .join(' · ')
}

function anchor(root: HTMLElement, selector: string) {
  const node = root.querySelector(selector)
  if (!(node instanceof HTMLElement)) return null
  const rect = node.getBoundingClientRect()
  const host = root.getBoundingClientRect()
  const onLeft = node.hasAttribute('data-col')
  return {
    x: (onLeft ? rect.right : rect.left) - host.left,
    y: rect.top + rect.height / 2 - host.top,
  }
}

function cssEscape(value: string) {
  return typeof CSS !== 'undefined' && CSS.escape ? CSS.escape(value) : value.replace(/"/g, '\\"')
}

function roundRect(context: CanvasRenderingContext2D, x: number, y: number, width: number, height: number, radius: number) {
  context.beginPath()
  context.roundRect(x, y, width, height, radius)
}
