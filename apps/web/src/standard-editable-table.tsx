import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from 'react'
import type { CommandTarget, MenuContext } from './command-registry'
import { openSharedContextMenu } from './shared-context-menu'
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
  /** Optional test id for the cell content, used by attribute definitions. */
  cellTestId?: (row: T) => string | undefined
}

export type EditRequest = { rowId: string; columnId: string }

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
  getRowKeyAttr?: (row: T) => string | undefined
  /** Extra toolbar content under the chrome bar. */
  belowBar?: ReactNode
  /** Opens the field picker (Columns control in chrome). */
  onSelectFields?: () => void
  selectFieldsLabel?: string
  /** Per-row edit gate (e.g. lock standard attribute keys). */
  canEdit?: (row: T, column: StandardColumn<T>) => boolean
  /** Shared context menu: context for a row, or empty surface when row is null. */
  menuContext?: MenuContext | ((row: T | null) => MenuContext)
  getMenuTarget?: (row: T | null) => CommandTarget
  /** Imperative edit request (rename commands). */
  editRequest?: EditRequest | null
  onEditRequestHandled?: () => void
}

type EditCell = { rowId: string; columnId: string }

/**
 * Generic editable grid on top of StandardTableChrome.
 * Plus adds an inline draft row; dirty drafts show Review and save.
 * Keyboard: Enter/F2 edit, Esc cancel, Tab next cell.
 * Right-click / Shift+F10 / ContextMenu open the shared command menu when configured.
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
  getRowKeyAttr,
  belowBar,
  onSelectFields,
  selectFieldsLabel,
  canEdit,
  menuContext,
  getMenuTarget,
  editRequest,
  onEditRequestHandled,
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

  const columnEditable = (row: T, column: StandardColumn<T>) => {
    if (!editable || column.editable === false || column.type === 'readonly' || !column.write) return false
    if (canEdit && !canEdit(row, column)) return false
    return true
  }

  const beginEdit = (row: T, column: StandardColumn<T>) => {
    if (!columnEditable(row, column)) return
    setEditing({ rowId: row.id, columnId: column.id })
    const raw = column.read(row)
    setEditValue(raw === null || raw === undefined ? '' : String(raw))
  }

  useEffect(() => {
    if (!editRequest) return
    const row = merged.find((item) => item.id === editRequest.rowId)
    const column = columns.find((item) => item.id === editRequest.columnId)
    if (row && column) beginEdit(row, column)
    onEditRequestHandled?.()
    // intentionally only when editRequest identity changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editRequest])

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

  const editableColumnsFor = (row: T) => columns.filter((column) => columnEditable(row, column))

  const moveEdit = (row: T, column: StandardColumn<T>, delta: number) => {
    commitEdit()
    const editableColumns = editableColumnsFor(row)
    const index = editableColumns.findIndex((item) => item.id === column.id)
    const nextCol = editableColumns[index + delta]
    if (nextCol) {
      queueMicrotask(() => beginEdit(row, nextCol))
      return
    }
    const rowIndex = filtered.findIndex((item) => item.id === row.id)
    const nextRow = filtered[rowIndex + (delta > 0 ? 1 : -1)]
    if (!nextRow) return
    const wrapCols = editableColumnsFor(nextRow)
    const wrapCol = delta > 0 ? wrapCols[0] : wrapCols.at(-1)
    if (wrapCol) queueMicrotask(() => beginEdit(nextRow, wrapCol))
  }

  const onNewRow = () => {
    if (!createDraftRow || !onDraftRowsChange) return
    const row = createDraftRow()
    onDraftRowsChange({ ...draftRows, [row.id]: row })
    const first = editableColumnsFor(row)[0]
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

  const resolveContext = (row: T | null): MenuContext | null => {
    if (!menuContext) return null
    return typeof menuContext === 'function' ? menuContext(row) : menuContext
  }

  const openMenu = (row: T | null, x: number, y: number) => {
    const context = resolveContext(row)
    if (!context) return
    if (row) onActivateRow?.(row)
    openSharedContextMenu({
      context,
      x,
      y,
      target: getMenuTarget?.(row) ?? {},
    })
  }

  const onRowContextMenu = (event: ReactMouseEvent<HTMLElement>, row: T) => {
    if (!menuContext) return
    event.preventDefault()
    event.stopPropagation()
    openMenu(row, event.clientX, event.clientY)
  }

  const onSurfaceContextMenu = (event: ReactMouseEvent<HTMLElement>) => {
    if (!menuContext) return
    if ((event.target as HTMLElement).closest('[role="row"]')) return
    event.preventDefault()
    openMenu(null, event.clientX, event.clientY)
  }

  const onRowKey = (event: ReactKeyboardEvent<HTMLElement>, row: T) => {
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      if (!menuContext) return
      event.preventDefault()
      const box = event.currentTarget.getBoundingClientRect()
      openMenu(row, box.left, box.bottom)
      return
    }
    if (editing) return
    if (event.key === 'Enter' || event.key === 'F2') {
      const column = editableColumnsFor(row)[0]
      if (!column) {
        if (event.key === 'Enter') onActivateRow?.(row)
        return
      }
      event.preventDefault()
      beginEdit(row, column)
    }
  }

  return (
    <StandardTableChrome
      testId={testId}
      newRowLabel={newRowLabel}
      onNewRow={createDraftRow && onDraftRowsChange ? onNewRow : undefined}
      onSelectFields={onSelectFields}
      selectFieldsLabel={selectFieldsLabel}
      rowCount={filtered.length}
      filterValue={filterValue}
      onFilterChange={onFilterChange}
      reviewVisible={editable && dirty}
      onReview={onReview}
    >
      {belowBar}
      <div
        className="standard-grid"
        role="grid"
        aria-rowcount={filtered.length}
        onContextMenu={onSurfaceContextMenu}
      >
        <div className="standard-grid-head" role="row">
          {columns.map((column) => (
            <div
              key={column.id}
              className="standard-grid-cell head"
              role="columnheader"
              data-column={column.id}
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
              data-key={getRowKeyAttr?.(row)}
              data-draft={isDraft ? 'true' : undefined}
              onClick={() => onActivateRow?.(row)}
              onContextMenu={(event) => onRowContextMenu(event, row)}
              onKeyDown={(event) => onRowKey(event, row)}
            >
              {columns.map((column) => {
                const isEditing = editing?.rowId === row.id && editing.columnId === column.id
                const raw = column.read(row)
                const title = raw === null || raw === undefined ? '' : String(raw)
                const cellTestId = column.cellTestId?.(row)
                return (
                  <div
                    key={column.id}
                    className={column.truncate ? 'standard-grid-cell truncate' : 'standard-grid-cell'}
                    role="gridcell"
                    data-column={column.id}
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
                          data-testid={cellTestId ?? 'standard-cell-editor'}
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
                          data-testid={cellTestId ?? 'standard-cell-editor'}
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
                          data-testid={cellTestId ?? 'standard-cell-editor'}
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
                      <span data-testid={cellTestId}>{column.display(row)}</span>
                    ) : column.type === 'boolean' ? (
                      <span data-testid={cellTestId}>{raw === true ? copy.yes : '—'}</span>
                    ) : (
                      <span data-testid={cellTestId}>{title || '—'}</span>
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
