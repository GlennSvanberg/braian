import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { StandardEditableTable, type StandardColumn } from '../../standard-editable-table'
import { FieldsPalette, type FieldsPaletteOpen } from './fields-palette'
import { MOCK_PRODUCTS, type ProductRow } from './mock-catalog'
import { columnTitle, columnWidthPx, defaultColumns, readCellDetail, type ProtoColumn } from './columns'

type HeaderMenu = { columnId: string; x: number; y: number }

/**
 * Clickable field-picker prototype: mock products table + Fields › palette mode.
 * No backend calls. Route: /prototype/field-picker.
 */
export function FieldPickerPrototypePage() {
  const [columns, setColumns] = useState<ProtoColumn[]>(() => defaultColumns())
  const [palette, setPalette] = useState<FieldsPaletteOpen | null>(null)
  const [headerMenu, setHeaderMenu] = useState<HeaderMenu | null>(null)
  const rootRef = useRef<HTMLDivElement | null>(null)

  const openBrowse = useCallback(() => setPalette({ kind: 'browse' }), [])

  const tableColumns: Array<StandardColumn<ProductRow>> = useMemo(
    () =>
      columns.map((col) => {
        const header = columnTitle(col)
        return {
          id: col.id,
          header,
          type: 'readonly' as const,
          editable: false,
          truncate: true,
          width: columnWidthPx(col),
          read: (row) => readCellDetail(row, col).text,
          title: (row) => readCellDetail(row, col).title,
          display: (row) => {
            const cell = readCellDetail(row, col)
            return <span title={cell.title || undefined}>{cell.text}</span>
          },
        }
      }),
    [columns],
  )

  const onToggleColumn = useCallback((column: ProtoColumn) => {
    setColumns((prev) => {
      const exists = prev.some((item) => item.id === column.id)
      if (exists) return prev.filter((item) => item.id !== column.id)
      return [...prev, column]
    })
  }, [])

  const onReplaceColumn = useCallback((previousId: string, next: ProtoColumn) => {
    setColumns((prev) => prev.map((item) => (item.id === previousId ? next : item)))
  }, [])

  useEffect(() => {
    document.title = 'Field picker prototype · Braian'
    rootRef.current?.focus()
  }, [])

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setPalette({ kind: 'browse' })
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    function onContextMenu(event: MouseEvent) {
      const target = event.target as HTMLElement | null
      const header = target?.closest('[role="columnheader"]') as HTMLElement | null
      if (!header) return
      const columnId = header.getAttribute('data-column')
      if (!columnId) return
      event.preventDefault()
      event.stopPropagation()
      setHeaderMenu({ columnId, x: event.clientX, y: event.clientY })
    }
    root.addEventListener('contextmenu', onContextMenu)
    return () => root.removeEventListener('contextmenu', onContextMenu)
  }, [])

  useEffect(() => {
    if (!headerMenu) return
    function onClose() {
      setHeaderMenu(null)
    }
    window.addEventListener('click', onClose)
    return () => window.removeEventListener('click', onClose)
  }, [headerMenu])

  return (
    <div
      className="app"
      data-testid="field-picker-prototype"
      ref={rootRef}
      tabIndex={-1}
      style={{ outline: 'none' }}
    >
      <header className="topbar" style={{ padding: '12px 16px', display: 'flex', gap: 12, alignItems: 'center' }}>
        <strong>Field picker prototype</strong>
        <span style={{ opacity: 0.7 }}>Prototype with mock data. Press Ctrl+K to pick fields.</span>
        <button type="button" data-testid="open-fields-command" onClick={openBrowse}>
          Select fields…
        </button>
      </header>
      <main style={{ padding: 16 }}>
        <StandardEditableTable
          testId="prototype-product-table"
          columns={tableColumns}
          rows={MOCK_PRODUCTS}
          editable={false}
          onSelectFields={openBrowse}
          selectFieldsLabel="Select fields"
          emptyLabel="No products"
        />
      </main>
      {palette ? (
        <FieldsPalette
          key={palette.kind === 'edit' ? `edit:${palette.column.id}` : 'browse'}
          open={palette}
          columns={columns}
          onClose={() => setPalette(null)}
          onToggleColumn={onToggleColumn}
          onReplaceColumn={onReplaceColumn}
        />
      ) : null}
      {headerMenu ? (
        <div
          role="menu"
          data-testid="column-header-menu"
          style={{
            position: 'fixed',
            left: headerMenu.x,
            top: headerMenu.y,
            zIndex: 80,
            background: 'var(--panel, #1e1e1e)',
            border: '1px solid var(--border, #333)',
            borderRadius: 8,
            padding: 4,
            minWidth: 160,
          }}
          onClick={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            role="menuitem"
            data-testid="edit-field"
            style={{ display: 'block', width: '100%', textAlign: 'left', padding: '6px 10px' }}
            onClick={() => {
              const col = columns.find((item) => item.id === headerMenu.columnId)
              setHeaderMenu(null)
              if (col) setPalette({ kind: 'edit', column: col })
            }}
          >
            Edit field…
          </button>
          <button
            type="button"
            role="menuitem"
            data-testid="remove-column"
            style={{ display: 'block', width: '100%', textAlign: 'left', padding: '6px 10px' }}
            onClick={() => {
              setColumns((prev) => prev.filter((item) => item.id !== headerMenu.columnId))
              setHeaderMenu(null)
            }}
          >
            Remove column
          </button>
          <button
            type="button"
            role="menuitem"
            data-testid="select-fields-menu"
            style={{ display: 'block', width: '100%', textAlign: 'left', padding: '6px 10px' }}
            onClick={() => {
              setHeaderMenu(null)
              openBrowse()
            }}
          >
            Select fields…
          </button>
        </div>
      ) : null}
    </div>
  )
}
