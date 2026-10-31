import type { KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent, ReactNode } from 'react'
import { Command, CommandInput, CommandList } from '../components/ui/command'
import { Checkbox } from '../components/ui/checkbox'
import { Kbd } from '../components/ui/kbd'

export type PickerRow = {
  id: string
  title: string
  subtitle?: string
  /** Opens a deeper level (qualifier, language, unit). */
  drill?: boolean
  checked?: boolean
  run: () => void
}

export type FieldPickerProps = {
  /** `columns` toggles several fields on and off; `single` picks exactly one. */
  mode: 'columns' | 'single'
  crumbs: string[]
  query: string
  onQuery: (value: string) => void
  placeholder?: string
  rows: PickerRow[]
  active: number
  onActive: (index: number) => void
  onKeyDown: (event: ReactKeyboardEvent) => void
  /** Click on the dimmed backdrop outside the panel. */
  onBackdrop?: (event: ReactMouseEvent) => void
  footer?: ReactNode
  /** Prefix for the test ids; the Ctrl+K Fields mode keeps `fields`. */
  testPrefix?: string
  /** Stable key for a level, so rows remount when the level changes. */
  levelKey?: string
}

/**
 * The one field picker: search box, breadcrumb, keyboard list. Ctrl+K Fields and the
 * import mapping both render it, so the two never drift apart.
 */
export function FieldPicker({
  mode,
  crumbs,
  query,
  onQuery,
  placeholder = 'Filter fields…',
  rows,
  active,
  onActive,
  onKeyDown,
  onBackdrop,
  footer,
  testPrefix = 'fields',
  levelKey = '',
}: FieldPickerProps) {
  const selected = Math.min(active, Math.max(0, rows.length - 1))
  const root = `${testPrefix}-mode`
  return (
    <div className="palette" data-testid={root} data-picker-mode={mode} onMouseDown={onBackdrop}>
      <style>{`
        [data-picker-mode] .hit.on,
        [data-picker-mode] .fields-hit-active {
          background: color-mix(in srgb, var(--accent) 24%, transparent);
          box-shadow: inset 3px 0 0 var(--accent);
        }
        [data-picker-mode] .hit:hover:not(.on) {
          background: color-mix(in srgb, var(--accent) 12%, transparent);
        }
        [data-picker-mode] .fields-check {
          margin-left: auto;
          flex: none;
          pointer-events: none;
        }
      `}</style>
      <Command
        shouldFilter={false}
        loop={false}
        disablePointerSelection
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDownCapture={onKeyDown}
      >
        <div className="palette-query">
          {crumbs.length > 0 ? (
            <span className="crumb" data-testid={`${testPrefix}-crumb`} style={{ marginRight: 8, whiteSpace: 'nowrap' }}>
              {crumbs.join(' › ')} ›
            </span>
          ) : null}
          <CommandInput
            data-testid={`${root}-input`}
            autoFocus
            value={query}
            placeholder={placeholder}
            onValueChange={(value) => {
              onQuery(value)
              onActive(0)
            }}
          />
        </div>
        <CommandList id={`${root}-list`} data-testid={`${root}-list`}>
          <div className="palette-group" role="listbox">
            {rows.map((row, index) => (
              <div
                key={`${levelKey}-${row.id}`}
                role="option"
                aria-selected={index === selected}
                className={index === selected ? 'hit on fields-hit-active' : 'hit'}
                data-testid={`${testPrefix}-row-${row.id}`}
                data-active={index === selected ? 'true' : undefined}
                onMouseEnter={() => onActive(index)}
                onClick={(event) => {
                  event.preventDefault()
                  event.stopPropagation()
                  onActive(index)
                  row.run()
                }}
              >
                <span className="hit-copy">
                  <span className="hit-title">{row.title}</span>
                  {row.subtitle ? <small>{row.subtitle}</small> : null}
                </span>
                <Checkbox
                  className="fields-check"
                  checked={Boolean(row.checked)}
                  tabIndex={-1}
                  data-testid={`${testPrefix}-check-${row.id}`}
                  aria-label={row.checked ? (mode === 'single' ? 'Current' : 'Shown') : mode === 'single' ? 'Not selected' : 'Hidden'}
                  onCheckedChange={() => {}}
                />
                {row.drill ? <span aria-hidden="true">›</span> : null}
              </div>
            ))}
          </div>
        </CommandList>
        <div className="palette-foot" data-testid={`${root}-foot`}>
          {footer ?? (
            <>
              <span>
                <Kbd>Enter</Kbd> {mode === 'single' ? 'pick' : 'toggle / open'}
              </span>
              <span>
                <Kbd>→</Kbd> drill
              </span>
              <span>
                <Kbd>←</Kbd> back
              </span>
              <span>
                <Kbd>Esc</Kbd> close
              </span>
            </>
          )}
        </div>
      </Command>
    </div>
  )
}
