import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { copy } from './i18n'
import type { ListedPath } from './path-picker'

type FieldPickerProps = {
  paths: ListedPath[]
  order: string[]
  hidden: string[]
  onToggle: (path: string) => void
  onMove: (path: string, delta: number) => void
  onClose: () => void
}

/**
 * Searchable field list for the open object table. Space/Enter toggles a field,
 * Alt+Arrow moves it. The table stores the result as column prefs.
 */
export function FieldPicker({ paths, order, hidden, onToggle, onMove, onClose }: FieldPickerProps) {
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const input = useRef<HTMLInputElement>(null)
  const labels = useMemo(() => new Map(paths.map((path) => [path.text, path.label])), [paths])
  const known = useMemo(() => new Set(paths.map((path) => path.text)), [paths])
  const rows = useMemo(() => {
    const selected = order.filter((id) => known.has(id))
    for (const path of paths) if (!selected.includes(path.text)) selected.push(path.text)
    return selected
  }, [known, order, paths])
  const needle = query.trim().toLowerCase()
  const visible = useMemo(
    () =>
      needle
        ? rows.filter((id) => `${labels.get(id) ?? ''} ${id}`.toLowerCase().includes(needle))
        : rows,
    [labels, needle, rows],
  )
  const activeId = visible[Math.min(active, Math.max(0, visible.length - 1))] ?? ''
  useEffect(() => {
    input.current?.focus()
  }, [])
  useEffect(() => {
    setActive(0)
  }, [query])
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      onClose()
      return
    }
    if (event.altKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
      if (!activeId) return
      event.preventDefault()
      onMove(activeId, event.key === 'ArrowUp' ? -1 : 1)
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActive((current) => Math.min(visible.length - 1, current + 1))
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActive((current) => Math.max(0, current - 1))
    } else if ((event.key === ' ' || event.key === 'Enter') && activeId) {
      event.preventDefault()
      onToggle(activeId)
    }
  }
  return (
    <div className="import-menu field-picker" data-testid="field-picker" onKeyDown={onKey}>
      <input
        ref={input}
        value={query}
        placeholder={copy.pathSearch}
        aria-label={copy.pathSearch}
        data-testid="field-picker-search"
        onChange={(event) => setQuery(event.target.value)}
      />
      <div className="import-menu-list" role="listbox" aria-label={copy.selectFields} aria-multiselectable="true">
        {visible.length === 0 ? <p>{copy.pathEmpty}</p> : null}
        {visible.map((id, index) => {
          const checked = order.includes(id) && !hidden.includes(id)
          return (
            <button
              key={id}
              type="button"
              role="option"
              aria-selected={checked}
              aria-current={index === active ? 'true' : undefined}
              className={index === active ? 'on' : undefined}
              data-testid={`field-picker-${id}`}
              onMouseEnter={() => setActive(index)}
              onClick={() => onToggle(id)}
            >
              <span aria-hidden="true">{checked ? '✓' : ' '}</span> {labels.get(id) ?? id}
            </button>
          )
        })}
      </div>
    </div>
  )
}
