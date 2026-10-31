import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { useVirtualizer } from '@tanstack/react-virtual'
import * as api from '../api/client'
import { StandardTableChrome } from '../standard-table'
import { cellClass, importCellText, rowClass } from '../import/model'
import { setSkipInvalid } from '../import/controller'

const PAGE = 100
const ROW = 30

type Cell = { value?: unknown; before?: unknown; state?: string; error?: { message?: string }; warning?: { message?: string } }
type DraftRow = {
  row_no: number
  status: string
  object_key?: string | null
  cells: Record<string, Cell>
  errors?: unknown
}
type DraftColumn = { col: number; header: string; label?: string; options?: Array<{ value: string; label: string }> | null }
type Page = {
  total: number
  kind?: string
  session_id?: string
  columns?: DraftColumn[]
  counters?: Record<string, number>
  skip_invalid?: boolean
  rows?: DraftRow[]
}

const FILTERS: Array<{ id: string; label: string; tone: string }> = [
  { id: 'all', label: 'All', tone: 'neutral' },
  { id: 'new', label: 'New', tone: 'ok' },
  { id: 'changed', label: 'Changed', tone: 'warn' },
  { id: 'failed', label: 'Errors', tone: 'danger' },
  { id: 'unchanged', label: 'Unchanged', tone: 'neutral' },
  { id: 'excluded', label: 'Excluded', tone: 'neutral' },
]

export type DraftTableProps = {
  /** The proposal that holds the draft. */
  proposalId: string
  /** Changes whenever the draft changes on the server (status, counters). */
  version?: string
  /** Rows the reviewer left out; they are sent as exclude_rows on approve. */
  excluded: number[]
  onExcludedChange: (next: number[]) => void
  editable?: boolean
  testId?: string
}

/**
 * The review table for a draft: paged from the server, virtualized, editable cell by cell.
 * Import dialog (Check step) and the Review pane render the same component.
 */
export function DraftTable({ proposalId, version = '', excluded, onExcludedChange, editable = true, testId = 'draft-table' }: DraftTableProps) {
  const queryClient = useQueryClient()
  const [filter, setFilter] = useState('all')
  const [text, setText] = useState('')
  const [editing, setEditing] = useState<{ row: number; col: number; text: string } | null>(null)
  const [error, setError] = useState('')
  const parent = useRef<HTMLDivElement>(null)
  const status = filter === 'all' ? undefined : filter
  const head = useQuery({
    queryKey: ['draft-rows', proposalId, filter, 'head', version],
    queryFn: () => api.get('/v1/proposals/{id}/rows', { params: { path: { id: proposalId }, query: { offset: 0, limit: 1, status } } }) as Promise<Page>,
  })
  const total = head.data?.total ?? 0
  const columns = head.data?.columns ?? []
  const counters = head.data?.counters ?? {}
  const sessionId = head.data?.session_id
  const skipInvalid = head.data?.skip_invalid === true
  const [busy, setBusy] = useState(false)
  const changeSkipInvalid = async (value: boolean) => {
    if (!sessionId) return
    setBusy(true)
    try {
      await setSkipInvalid(sessionId, value)
      setError('')
      await queryClient.invalidateQueries({ queryKey: ['draft-rows', proposalId] })
      await queryClient.invalidateQueries({ queryKey: ['proposal', proposalId] })
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'The change failed.')
    } finally {
      setBusy(false)
    }
  }
  const virtualizer = useVirtualizer({ count: total, getScrollElement: () => parent.current, estimateSize: () => ROW, overscan: 12 })
  const visible = virtualizer.getVirtualItems()
  const pages = [...new Set(visible.map((item) => Math.floor(item.index / PAGE)))]
  const windows = useQueries({
    queries: pages.map((page) => ({
      queryKey: ['draft-rows', proposalId, filter, 'page', page, version, total],
      queryFn: () => api.get('/v1/proposals/{id}/rows', { params: { path: { id: proposalId }, query: { offset: page * PAGE, limit: PAGE, status } } }) as Promise<Page>,
    })),
  })
  const byIndex = new Map<number, DraftRow>()
  windows.forEach((result, index) => {
    const page = pages[index] ?? 0
    ;(result.data?.rows ?? []).forEach((row, offset) => byIndex.set(page * PAGE + offset, row))
  })
  const needle = text.trim().toLowerCase()
  const matches = (row: DraftRow) =>
    !needle || Object.values(row.cells).some((cell) => importCellText(cell?.value).toLowerCase().includes(needle))

  useEffect(() => {
    setEditing(null)
  }, [filter, proposalId])

  const commit = async () => {
    const current = editing
    if (!current || !sessionId) return
    setEditing(null)
    try {
      await api.patch('/v1/imports/{id}/rows/{row_no}', {
        params: { path: { id: sessionId, row_no: current.row } },
        body: { col: current.col, value: current.text },
      })
      setError('')
      await queryClient.invalidateQueries({ queryKey: ['draft-rows', proposalId] })
      await queryClient.invalidateQueries({ queryKey: ['proposal', proposalId] })
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'The edit failed.')
    }
  }
  const onEditKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault()
      void commit()
    } else if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      setEditing(null)
    }
  }
  const toggle = (rowNo: number) =>
    onExcludedChange(excluded.includes(rowNo) ? excluded.filter((item) => item !== rowNo) : [...excluded, rowNo])
  const template = `${editable ? '28px ' : ''}56px 28px repeat(${Math.max(columns.length, 1)}, minmax(110px, 1fr))`
  const count = (id: string) =>
    id === 'all'
      ? ['new', 'changed', 'failed', 'unchanged', 'excluded'].reduce((sum, key) => sum + Number(counters[key] ?? 0), 0)
      : Number(counters[id] ?? 0)

  return (
    <StandardTableChrome
      testId={testId}
      rowCount={total}
      filterValue={text}
      onFilterChange={setText}
    >
      <div className="import-counters" role="group" aria-label="Filter rows">
        {FILTERS.map((item) => (
          <button
            key={item.id}
            type="button"
            className={`meta-chip ${item.tone}${filter === item.id ? ' on' : ''}`}
            aria-pressed={filter === item.id}
            data-testid={`draft-filter-${item.id}`}
            onClick={() => setFilter(item.id)}
          >
            {item.label} {count(item.id)}
          </button>
        ))}
        {excluded.length > 0 ? <span className="meta-chip neutral">{excluded.length} left out</span> : null}
        {editable && sessionId ? (
          <label className="import-empty-clears" title="Rows with an invalid cell are imported without that cell instead of being skipped.">
            <input
              type="checkbox"
              data-testid="draft-skip-invalid"
              checked={skipInvalid}
              disabled={busy}
              onChange={(event) => void changeSkipInvalid(event.target.checked)}
            />
            {' '}Leave out invalid cells instead of skipping the row
          </label>
        ) : null}
        {error ? <span className="error" role="alert" data-testid="draft-error">{error}</span> : null}
      </div>
      <div className="import-table" ref={parent} style={{ maxHeight: '70vh' }}>
        <div className="import-row head" style={{ display: 'grid', gridTemplateColumns: template, position: 'sticky', top: 0, zIndex: 1, background: 'var(--panel)', height: ROW }}>
          {editable ? <span /> : null}
          <span>#</span>
          <span />
          {columns.map((column) => (
            <strong key={column.col} title={column.header}>{column.label ?? column.header}</strong>
          ))}
        </div>
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {visible.map((item) => {
            const row = byIndex.get(item.index)
            if (!row || !matches(row)) return null
            const skipped = excluded.includes(row.row_no) || row.status === 'excluded'
            return (
              <div
                key={row.row_no}
                className={rowClass(row.status)}
                data-testid={`import-row-${row.status}`}
                style={{
                  position: 'absolute',
                  top: 0,
                  transform: `translateY(${item.start}px)`,
                  height: ROW,
                  display: 'grid',
                  gridTemplateColumns: template,
                  opacity: skipped ? 0.45 : 1,
                }}
              >
                {editable ? (
                  <span>
                    {row.status === 'new' || row.status === 'changed' || row.status === 'excluded' ? (
                      <input
                        type="checkbox"
                        aria-label={`Include row ${row.row_no}`}
                        checked={!skipped}
                        onChange={() => toggle(row.row_no)}
                      />
                    ) : null}
                  </span>
                ) : null}
                <span>{row.row_no}</span>
                <span title={row.status}>{row.status === 'failed' ? '⛔' : row.status === 'new' ? '＋' : row.status === 'changed' ? '•' : ''}</span>
                {columns.map((column) => {
                  const cell = row.cells[String(column.col)]
                  const active = editing?.row === row.row_no && editing.col === column.col
                  return (
                    <span
                      key={column.col}
                      className={cellClass(cell?.state ?? 'same')}
                      title={cell?.error?.message ?? cell?.warning?.message}
                      data-testid={`draft-cell-${row.row_no}-${column.col}`}
                      tabIndex={editable ? 0 : undefined}
                      onDoubleClick={() => editable && setEditing({ row: row.row_no, col: column.col, text: importCellText(cell?.value) })}
                      onKeyDown={(event) => {
                        if (editable && (event.key === 'Enter' || event.key === 'F2') && !active) {
                          event.preventDefault()
                          setEditing({ row: row.row_no, col: column.col, text: importCellText(cell?.value) })
                        }
                      }}
                    >
                      {active && column.options ? (
                        <select
                          autoFocus
                          aria-label={`Edit ${column.label ?? column.header}`}
                          data-testid={`draft-select-${row.row_no}-${column.col}`}
                          value={editing.text}
                          onChange={(event) => setEditing({ ...editing, text: event.target.value })}
                          onKeyDown={onEditKey as unknown as (event: KeyboardEvent<HTMLSelectElement>) => void}
                          onBlur={() => void commit()}
                          style={{ width: '100%' }}
                        >
                          {column.options.some((option) => option.value === editing.text) ? null : <option value={editing.text}>{editing.text}</option>}
                          {column.options.map((option) => (
                            <option key={option.value} value={option.value}>{option.label}</option>
                          ))}
                        </select>
                      ) : active ? (
                        <input
                          autoFocus
                          aria-label={`Edit ${column.label ?? column.header}`}
                          value={editing.text}
                          onChange={(event) => setEditing({ ...editing, text: event.target.value })}
                          onKeyDown={onEditKey}
                          onBlur={() => void commit()}
                          style={{ width: '100%' }}
                        />
                      ) : cell?.state === 'changed' && cell.before ? (
                        <>
                          <span className="diff-old">{importCellText(cell.before)}</span>
                          <span className="diff-arrow">{' -> '}</span>
                          <span className="diff-new">{importCellText(cell.value)}</span>
                        </>
                      ) : (
                        importCellText(cell?.value)
                      )}
                    </span>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
    </StandardTableChrome>
  )
}
