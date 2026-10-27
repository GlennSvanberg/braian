import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { copy } from '../i18n'
import type { ImportTarget } from './controller'

export function TargetMenu({
  targets,
  onPick,
  onClose,
}: {
  targets: ImportTarget[]
  onPick: (id: string) => void
  onClose: () => void
}) {
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const root = useRef<HTMLDivElement>(null)
  const needle = query.trim().toLowerCase()
  const filtered = targets.filter((target) => `${target.label} ${target.id}`.toLowerCase().includes(needle))
  const rows = [{ id: 'unmapped', label: copy.importDontImport, key: false }, ...filtered.map((target) => ({
    id: target.id,
    label: `${target.key ? '🔑 ' : ''}${target.label}`,
    key: Boolean(target.key),
  }))]
  useEffect(() => {
    setActive(0)
  }, [query])
  useEffect(() => {
    const onPointer = (event: PointerEvent) => {
      if (root.current && event.target instanceof Node && !root.current.contains(event.target)) onClose()
    }
    window.addEventListener('pointerdown', onPointer)
    return () => window.removeEventListener('pointerdown', onPointer)
  }, [onClose])
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      onClose()
      return
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActive((current) => Math.min(rows.length - 1, current + 1))
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActive((current) => Math.max(0, current - 1))
    } else if (event.key === 'Enter') {
      event.preventDefault()
      const row = rows[active]
      if (row) onPick(row.id)
    }
  }
  return (
    <div className="import-menu" ref={root} data-testid="import-popover" data-import-menu-open="true" onKeyDown={onKey}>
      <input
        autoFocus
        value={query}
        placeholder={copy.importSearchFields}
        aria-label={copy.importSearchFields}
        onChange={(event) => setQuery(event.target.value)}
      />
      <div className="import-menu-list">
        {rows.map((row, index) => (
          <button
            key={row.id}
            type="button"
            className={index === active ? 'on' : undefined}
            onMouseEnter={() => setActive(index)}
            onClick={() => onPick(row.id)}
          >
            {row.label}
          </button>
        ))}
      </div>
    </div>
  )
}
