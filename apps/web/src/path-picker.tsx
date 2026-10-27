import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { copy } from './i18n'

export type ListedPath = {
  text: string
  label: string
  id?: unknown
}

type Row = {
  key: string
  label: string
  path?: ListedPath
  children?: Row[]
}

/** Own attributes, then one submenu per link. Parts sit under the attribute. */
export function pathRows(paths: ListedPath[]): Row[] {
  const own = new Map<string, ListedPath[]>()
  const links = new Map<string, ListedPath[]>()
  for (const path of paths) {
    if (path.text.includes('>')) {
      const via = path.text.split('>')[0] ?? ''
      const bucket = links.get(via) ?? []
      bucket.push(path)
      links.set(via, bucket)
    } else {
      const head = path.text.split('.')[0]?.split('@')[0] ?? path.text
      const bucket = own.get(head) ?? []
      bucket.push(path)
      own.set(head, bucket)
    }
  }
  const rows: Row[] = []
  for (const [head, items] of own) {
    const whole = items.find((item) => item.text === head) ?? items[0]
    if (!whole) continue
    const parts = items.filter((item) => item.text !== whole.text)
    rows.push(parts.length > 0
      ? { key: head, label: whole.label, children: items.map(leaf) }
      : { key: head, label: whole.label, path: whole })
  }
  for (const [via, items] of links) {
    const label = items[0]?.label.split('›')[0]?.trim() || via
    rows.push({
      key: `link:${via}`,
      label: `${label} ›`,
      children: items.map(leaf),
    })
  }
  return rows
}

function leaf(path: ListedPath): Row {
  return { key: path.text, label: path.label, path }
}

export function PathMenu({
  paths,
  onPick,
  onClose,
}: {
  paths: ListedPath[]
  onPick: (path: ListedPath) => void
  onClose: () => void
}) {
  const [query, setQuery] = useState('')
  const [stack, setStack] = useState<Row[][]>([])
  const [active, setActive] = useState(0)
  const input = useRef<HTMLInputElement>(null)
  const root = pathRows(paths)
  const needle = query.trim().toLowerCase()
  const filtered = useMemo(() => {
    if (!needle) return root
    return paths
      .filter((path) => `${path.label} ${path.text}`.toLowerCase().includes(needle))
      .map(leaf)
  }, [needle, paths, root])
  const rows = stack.at(-1) ?? filtered
  useEffect(() => {
    input.current?.focus()
  }, [])
  useEffect(() => {
    setActive(0)
  }, [query, stack.length])
  const open = (row: Row) => {
    if (row.children && !needle) {
      setStack((current) => [...current, row.children ?? []])
      setQuery('')
      return
    }
    if (row.path) onPick(row.path)
  }
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      if (stack.length > 0) setStack((current) => current.slice(0, -1))
      else onClose()
      return
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActive((current) => Math.min(rows.length - 1, current + 1))
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActive((current) => Math.max(0, current - 1))
    } else if (event.key === 'ArrowRight' || event.key === 'Enter') {
      event.preventDefault()
      const row = rows[active]
      if (row) open(row)
    } else if (event.key === 'ArrowLeft' && stack.length > 0) {
      event.preventDefault()
      setStack((current) => current.slice(0, -1))
    }
  }
  return (
    <div className="import-menu" data-testid="path-picker" onKeyDown={onKey}>
      <input
        ref={input}
        value={query}
        placeholder={copy.pathSearch}
        aria-label={copy.pathSearch}
        onChange={(event) => {
          setQuery(event.target.value)
          setStack([])
        }}
      />
      <div className="import-menu-list" role="menu">
        {rows.length === 0 ? <p>{copy.pathEmpty}</p> : null}
        {rows.map((row, index) => (
          <button
            key={row.key}
            type="button"
            role="menuitem"
            className={index === active ? 'on' : undefined}
            aria-current={index === active ? 'true' : undefined}
            onMouseEnter={() => setActive(index)}
            onClick={() => open(row)}
          >
            {row.label}
          </button>
        ))}
      </div>
    </div>
  )
}
