import type { ReactNode } from 'react'
import { Plus, Search } from 'lucide-react'
import { copy } from './i18n'
import { formatCount } from './format'
import { Kbd } from './components/ui/kbd'

export type StandardTableChromeProps = {
  /** Accessible name for the plus control that creates a new row. */
  newRowLabel: string
  onNewRow: () => void
  rowCount?: number
  /** Generic text filter shared by every table that opts in. */
  filterValue?: string
  onFilterChange?: (value: string) => void
  filterPlaceholder?: string
  /** When true, show Review and save (same action as Ctrl+S). */
  reviewVisible?: boolean
  onReview?: () => void
  children: ReactNode
  testId?: string
}

/**
 * Shared chrome for every app table: optional filter, plus creates a row,
 * optional Review and save, row count. View placement stays on Ctrl+K.
 */
export function StandardTableChrome({
  newRowLabel,
  onNewRow,
  rowCount,
  filterValue,
  onFilterChange,
  filterPlaceholder = copy.search,
  reviewVisible = false,
  onReview,
  children,
  testId = 'standard-table',
}: StandardTableChromeProps) {
  return (
    <div className="table-wrap" data-testid={testId}>
      <div className="toolbar standard-table-bar" data-testid="standard-table-bar">
        {onFilterChange ? (
          <label className="search-field">
            <Search size={14} aria-hidden="true" />
            <input
              data-testid="quick-filter"
              placeholder={filterPlaceholder}
              value={filterValue ?? ''}
              onChange={(event) => onFilterChange(event.target.value)}
            />
            <Kbd>/</Kbd>
          </label>
        ) : null}
        <button
          type="button"
          className="icon-btn"
          data-testid="table-add-row"
          aria-label={newRowLabel}
          onClick={onNewRow}
        >
          <Plus size={14} aria-hidden="true" />
        </button>
        {reviewVisible && onReview ? (
          <button type="button" className="primary" data-testid="review-and-save" onClick={onReview}>
            {copy.reviewAndSave}
          </button>
        ) : null}
        {typeof rowCount === 'number' ? (
          <span className="row-count" data-testid="row-count">
            {formatCount(rowCount)} {copy.rows}
          </span>
        ) : null}
      </div>
      {children}
    </div>
  )
}
