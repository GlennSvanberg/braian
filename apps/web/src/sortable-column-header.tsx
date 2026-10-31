/**
 * Object-table header cell with dnd-kit sortable drag.
 * PointerSensor distance keeps click-to-sort and resize handles working.
 */
import { useSortable } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import type { CSSProperties, MouseEvent as ReactMouseEvent } from 'react'
import { copy } from './i18n'

export type HeaderSortState = 'asc' | 'desc' | false

type Props = {
  id: string
  width: number
  header: string
  /** Typesense can sort this column. */
  sortable: boolean
  sortState: HeaderSortState
  /** Last header/cell the user activated (Move column left/right target). */
  active?: boolean
  onActivate: () => void
  onSort: () => void
  onContextMenu: (event: ReactMouseEvent<HTMLElement>) => void
  onResizeStart: (event: ReactMouseEvent<HTMLElement>) => void
}

export function SortableColumnHeader({
  id,
  width,
  header,
  sortable,
  sortState,
  active = false,
  onActivate,
  onSort,
  onContextMenu,
  onResizeStart,
}: Props) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id })
  const style: CSSProperties = {
    width,
    minWidth: width,
    maxWidth: width,
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.85 : undefined,
    zIndex: isDragging ? 2 : undefined,
  }
  const title = sortable
    ? header
    : `${header} — ${copy.columnNotSortable}`
  const indicator =
    sortState === 'asc' ? ' ↑' : sortState === 'desc' ? ' ↓' : ''
  return (
    <div
      ref={setNodeRef}
      className={`cell column-header-cell${sortable ? '' : ' column-not-sortable'}${active ? ' column-active' : ''}`}
      style={style}
      data-testid={`col-${id}`}
      data-column={id}
      data-active-column={active ? 'true' : undefined}
      data-sortable={sortable ? 'true' : 'false'}
      aria-sort={
        sortState === 'asc' ? 'ascending' : sortState === 'desc' ? 'descending' : 'none'
      }
      onClick={onActivate}
      onContextMenu={onContextMenu}
    >
      <button
        type="button"
        className={`icon-btn column-header-btn${sortable ? '' : ' column-header-btn-static'}`}
        title={title}
        aria-label={sortable ? header : `${header} (${copy.columnNotSortable})`}
        {...attributes}
        {...listeners}
        onClick={(event) => {
          event.stopPropagation()
          onActivate()
          if (!sortable) return
          onSort()
        }}
        onFocus={onActivate}
      >
        {header}
        {sortable && indicator ? (
          <span className="column-sort-indicator" aria-hidden="true">
            {indicator}
          </span>
        ) : null}
      </button>
      <span
        className="column-resize-handle"
        data-testid={`resize-${id}`}
        role="separator"
        aria-orientation="vertical"
        aria-label={`Resize ${header} column`}
        onMouseDown={onResizeStart}
      />
    </div>
  )
}
