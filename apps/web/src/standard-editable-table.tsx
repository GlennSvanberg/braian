import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react'
import { StandardTableChrome } from './standard-table'
import { copy } from './i18n'

export type StandardColumnType = 'text' | 'number' | 'boolean' | 'date' | 'datetime' | 'enum' | 'readonly'

export type StandardColumn<T> = {
  id: string
  header: string
  width?: number | string
  type?: StandardColumnType
  editable?: boolean
  /** Options for enum columns. */
  options?: Array<{ value: string; label: string }>
  read: (row: T) => string | number | boolean | null | undefined
  write?: (row: T, value: string) => T
  /** Render display cell; defaults to String(read(row)). */
  display?: (row: T) => ReactNode
  truncate?: boolean
}

export type StandardEditableTableProps<T extends { id: string }> = {
  columns: Array<StandardColumn<T>>
  rows: T[]
  /** When set, cell edits and draft rows collect here until Review and save. */
  draftRows?: Record<string, T>
  onDraftRowsChange?: (next: Record<string, T>) => void
  /** New blank row factory for the plus control. */
  createDraftRow?: () => T
  newRowLabel?: string
  /** Persist all drafts (same as Ctrl+S). */
  onReviewAndSave?: () => void | Promise<void>
  /** Called when a row is activated (click / Enter on non-edit). */
  onActivateRow?: (row: T) => void
  activeRowId?: string | null
  filterValue?: string
  onFilterChange?: (value: string) => void
  testId?: string
  /** Enable inline editing (Enter / F2). Off for product object table. */
  editable?: boolean
  emptyLabel?: string
  getRowTestId?: (row: T) => string
  /** Extra toolbar content under the chrome bar. */
  belowBar?: ReactNode
}

type EditCell = { rowId: string; columnId: string }

/**
 * Generic editable grid on top of StandardTableChrome.
 * Plus adds an inline draft row; dirty drafts show Review and save.
 * Keyboard: Enter/F2 edit, Esc cancel, Tab next cell.
 */
export function StandardEditableTable<T extends { id: string }>({
  columns,
  rows,
  draftRows = {},
  onDraftRowsChange,
  createDraftRow,
  newRowLabel = copy.newRow,
  onReviewAndSave,
  onActivateRow,
  activeRowId,
  filterValue,
  onFilterChange,
  testId = 'standard-editable-table',
  editable = true,
  emptyLabel = copy.noRows,
  getRowTestId,
  belowBar,
}: StandardEditableTableProps<T>) {
  const [editing, setEditing] = useState<EditCell | null>(null)
  const [editValue, setEditValue] = useState('')
  const inputRef = useRef<HTMLInputElement | HTMLSelectElement | null>(null)
  const dirty = Object.keys(draftRows).length > 0

  const merged = useMemo(() => {
    const byId = new Map(rows.map((row) => [row.id, row]))
    for (const [id, draft] of Object.entries(draftRows)) byId.set(id, draft)
    return [...byId.values()]
  }, [rows, draftRows])

  const filtered = useMemo(() => {
    const needle = (filterValue ?? '').trim().toLowerCase()
    if (!needle) return merged
    return merged.filter((row) =>
      columns.some((column) => String(column.read(row) ?? '').toLowerCase().includes(needle)),
    )
  }, [merged, filterValue, columns])

  useEffect(() => {
    if (editing) inputRef.current?.focus()
  }, [editing])

  const patchRow = useCallback(
    (rowId: string, next: T) => {
      if (!onDraftRowsChange) return
      onDraftRowsChange({ ...draftRows, [rowId]: next })
    },
    [draftRows, onDraftRowsChange],
  )

  const beginEdit = (row: T, column: StandardColumn<T>) => {
    if (!editable || column.editable === false || column.type === 'readonly' || !column.write) return
    setEditing({ rowId: row.id, columnId: column.id })
    const raw = column.read(row)
    setEditValue(raw === null || raw === undefined ? '' : String(raw))
  }

  const commitEdit = () => {
    if (!editing) return
    const row = merged.find((item) => item.id === editing.rowId)
    const column = columns.find((item) => item.id === editing.columnId)
    setEditing(null)
    if (!row || !column?.write) return
    const next = column.write(row, editValue)
    patchRow(row.id, next)
  }

  const cancelEdit = () => setEditing(null)

  const editableColumns = columns.filter((column) => column.editable !== false && column.type !== 'readonly' && column.write)

  const moveEdit = (row: T, column: StandardColumn<T>, delta: number) => {
    commitEdit()
    const index = editableColumns.findIndex((item) => item.id === column.id)
    const nextCol = editableColumns[index + delta]
    if (nextCol) {
      queueMicrotask(() => beginEdit(row, nextCol))
      return
    }
    const rowIndex = filtered.findIndex((item) => item.id === row.id)
    const nextRow = filtered[rowIndex + (delta > 0 ? 1 : -1)]
    const wrapCol = delta > 0 ? editableColumns[0] : editableColumns.at(-1)
    if (nextRow && wrapCol) queueMicrotask(() => beginEdit(nextRow, wrapCol))
  }

  const onNewRow = () => {
    if (!createDraftRow || !onDraftRowsChange) return
    const row = createDraftRow()
    onDraftRowsChange({ ...draftRows, [row.id]: row })
    const first = editableColumns[0]
    queueMicrotask(() => {
      onActivateRow?.(row)
      if (first) beginEdit(row, first)
    })
  }

  const onReview = () => {
    void onReviewAndSave?.()
  }

  useEffect(() => {
    if (!editable || !onReviewAndSave) return
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== 's') return
      if (!dirty) return
      event.preventDefault()
      void onReviewAndSave()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [editable, dirty, onReviewAndSave])

  const onRowKey = (event: ReactKeyboardEvent<HTMLElement>, row: T) => {
    if (editing) return
    if (event.key === 'Enter' || event.key === 'F2') {
      const column = editableColumns[0]
      if (!column) return
      event.preventDefault()
      beginEdit(row, column)
      return
    }
    if (event.key === 'Enter') onActivateRow?.(row)
  }

  return (
    <StandardTableChrome
      testId={testId}
      newRowLabel={newRowLabel}
      onNewRow={onNewRow}
      rowCount={filtered.length}
      filterValue={filterValue}
      onFilterChange={onFilterChange}
      reviewVisible={editable && dirty}
      onReview={onReview}
    >
      {belowBar}
      <div className="standard-grid" role="grid" aria-rowcount={filtered.length}>
        <div className="standard-grid-head" role="row">
          {columns.map((column) => (
            <div
              key={column.id}
              className="standard-grid-cell head"
              role="columnheader"
              style={column.width ? { width: column.width, flex: 'none' } : undefined}
            >
              {column.header}
            </div>
          ))}
        </div>
        {filtered.length === 0 ? <p className="standard-grid-empty">{emptyLabel}</p> : null}
        {filtered.map((row) => {
          const isDraft = Boolean(draftRows[row.id]) && !rows.some((item) => item.id === row.id)
          return (
            <div
              key={row.id}
              className={
                activeRowId === row.id ? 'standard-grid-row on' : isDraft ? 'standard-grid-row draft' : 'standard-grid-row'
              }
              role="row"
              tabIndex={0}
              data-testid={getRowTestId?.(row) ?? 'standard-grid-row'}
              data-row-id={row.id}
              data-draft={isDraft ? 'true' : undefined}
              onClick={() => onActivateRow?.(row)}
              onKeyDown={(event) => onRowKey(event, row)}
            >
              {columns.map((column) => {
                const isEditing = editing?.rowId === row.id && editing.columnId === column.id
                const raw = column.read(row)
                const title = raw === null || raw === undefined ? '' : String(raw)
                return (
                  <div
                    key={column.id}
                    className={column.truncate ? 'standard-grid-cell truncate' : 'standard-grid-cell'}
                    role="gridcell"
                    style={column.width ? { width: column.width, flex: 'none' } : undefined}
                    title={column.truncate ? title : undefined}
                    onDoubleClick={() => beginEdit(row, column)}
                  >
                    {isEditing ? (
                      column.type === 'enum' && column.options ? (
                        <select
                          ref={(node) => {
                            inputRef.current = node
                          }}
                          data-testid="standard-cell-editor"
                          value={editValue}
                          onChange={(event) => setEditValue(event.target.value)}
                          onBlur={commitEdit}
                          onKeyDown={(event) => {
                            if (event.key === 'Escape') {
                              event.preventDefault()
                              cancelEdit()
                            } else if (event.key === 'Enter') {
                              event.preventDefault()
                              commitEdit()
                            } else if (event.key === 'Tab') {
                              event.preventDefault()
                              moveEdit(row, column, event.shiftKey ? -1 : 1)
                            }
                          }}
                        >
                          {column.options.map((option) => (
                            <option key={option.value} value={option.value}>
                              {option.label}
                            </option>
                          ))}
                        </select>
                      ) : column.type === 'boolean' ? (
                        <input
                          ref={(node) => {
                            inputRef.current = node
                          }}
                          type="checkbox"
                          data-testid="standard-cell-editor"
                          checked={editValue === 'true'}
                          onChange={(event) => {
                            const next = event.target.checked ? 'true' : 'false'
                            setEditValue(next)
                            if (!column.write) return
                            patchRow(row.id, column.write(row, next))
                            setEditing(null)
                          }}
                          onKeyDown={(event) => {
                            if (event.key === 'Escape') {
                              event.preventDefault()
                              cancelEdit()
                            } else if (event.key === 'Tab') {
                              event.preventDefault()
                              moveEdit(row, column, event.shiftKey ? -1 : 1)
                            }
                          }}
                        />
                      ) : (
                        <input
                          ref={(node) => {
                            inputRef.current = node
                          }}
                          data-testid="standard-cell-editor"
                          type={column.type === 'number' ? 'number' : column.type === 'date' ? 'date' : column.type === 'datetime' ? 'datetime-local' : 'text'}
                          value={editValue}
                          onChange={(event) => setEditValue(event.target.value)}
                          onBlur={commitEdit}
                          onKeyDown={(event) => {
                            if (event.key === 'Escape') {
                              event.preventDefault()
                              cancelEdit()
                            } else if (event.key === 'Enter') {
                              event.preventDefault()
                              commitEdit()
                            } else if (event.key === 'Tab') {
                              event.preventDefault()
                              moveEdit(row, column, event.shiftKey ? -1 : 1)
                            }
                          }}
                        />
                      )
                    ) : column.display ? (
                      column.display(row)
                    ) : column.type === 'boolean' ? (
                      raw === true ? copy.yes : raw === false ? '—' : '—'
                    ) : (
                      title || '—'
                    )}
                  </div>
                )
              })}
            </div>
          )
        })}
      </div>
    </StandardTableChrome>
  )
}
