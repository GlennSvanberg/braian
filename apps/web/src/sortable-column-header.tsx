/**
 * Object-table header cell with dnd-kit sortable drag.
 * PointerSensor distance keeps click-to-sort and resize handles working.
 */
import { useSortable } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import type { CSSProperties, MouseEvent as ReactMouseEvent } from 'react'

type Props = {
  id: string
  width: number
  header: string
  sortable: boolean
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
  return (
    <div
      ref={setNodeRef}
      className="cell"
      style={style}
      data-testid={`col-${id}`}
      data-column={id}
      onClick={onActivate}
      onContextMenu={onContextMenu}
    >
      <button
        type="button"
        className="icon-btn column-header-btn"
        title={header}
        {...attributes}
        {...listeners}
        onClick={(event) => {
          event.stopPropagation()
          onActivate()
          if (!sortable) return
          onSort()
        }}
      >
        {header}
      </button>
      <span
        className="icon-btn"
        data-testid={`resize-${id}`}
        onMouseDown={onResizeStart}
      >
        |
      </span>
    </div>
  )
}
