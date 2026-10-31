import { useEffect, useState, type KeyboardEvent } from 'react'
import { copy } from '../i18n'
import { FieldPicker, type PickerRow } from '../fields/field-picker'
import type { ImportTarget } from './controller'

/**
 * Chooses the attribute a file column maps to. It is the same picker as Ctrl+K Fields,
 * in single-choice mode.
 */
export function TargetPicker({
  targets,
  current,
  onPick,
  onClose,
}: {
  targets: ImportTarget[]
  /** The target the column has now, shown as checked. */
  current?: string
  onPick: (id: string) => void
  onClose: () => void
}) {
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const needle = query.trim().toLowerCase()
  const filtered = targets.filter((target) => `${target.label} ${target.id} ${target.source ?? ''}`.toLowerCase().includes(needle))
  const rows: PickerRow[] = [
    { id: 'unmapped', title: copy.importDontImport, checked: !current || current === 'unmapped', run: () => onPick('unmapped') },
    ...filtered.map((target) => ({
      id: target.id,
      title: `${target.key ? '🔑 ' : ''}${target.label}`,
      subtitle: target.source ?? undefined,
      checked: target.id === current,
      run: () => onPick(target.id),
    })),
  ]
  useEffect(() => {
    setActive(0)
  }, [query])
  const onKeyDown = (event: KeyboardEvent) => {
    event.stopPropagation()
    if (event.key === 'Escape') {
      event.preventDefault()
      onClose()
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActive((value) => Math.min(rows.length - 1, value + 1))
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActive((value) => Math.max(0, value - 1))
    } else if (event.key === 'Enter') {
      event.preventDefault()
      rows[Math.min(active, rows.length - 1)]?.run()
    }
  }
  return (
    <FieldPicker
      mode="single"
      testPrefix="import-field"
      crumbs={[]}
      query={query}
      onQuery={setQuery}
      placeholder={copy.importSearchFields}
      rows={rows}
      active={active}
      onActive={setActive}
      onKeyDown={onKeyDown}
      onBackdrop={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
    />
  )
}
