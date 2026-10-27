import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, type Json } from './api'
import { onAttributeAction, type AttributeAction } from './attribute-actions'
import { rowsFromCatalog, rowsFromDefinitions, type AttributeDefinitionRow, type AttributeFlags, type UsedPlace } from './attribute-rows'
import { formatCount } from './format'
import { copy } from './i18n'
import { attributeKey } from './model-view'
import { onDraftChange, setModelFocus, stageModel } from './model-stage'

type SortKey = 'label' | 'key' | 'kind' | 'unit' | 'list'
type Dialog =
  | { kind: 'type' | 'group'; attribute: string }
  | { kind: 'new' }
  | null

const KINDS: Array<[string, string]> = [
  ['text', copy.kindText],
  ['number', copy.number],
  ['boolean', copy.kindBoolean],
  ['date', copy.kindDate],
  ['value_list', copy.kindValueList],
  ['range', copy.kindRange],
  ['file_ref', copy.kindFile],
]

export function AttributeDefinitions() {
  const queryClient = useQueryClient()
  const stored = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const draft = useQuery({ queryKey: ['model-draft'], queryFn: () => api<Json>('/v1/model/draft') })
  const listed = useQuery({
    queryKey: ['attribute-definitions'],
    queryFn: () => api<Json>('/v1/operations/model.list_attribute_definitions', { method: 'POST', body: '{}' }),
  })
  const [query, setQuery] = useState('')
  const [kind, setKind] = useState('')
  const [sort, setSort] = useState<{ key: SortKey; dir: 'asc' | 'desc' }>({ key: 'label', dir: 'asc' })
  const [active, setActive] = useState(0)
  const [menu, setMenu] = useState<{ key: string; x: number; y: number } | null>(null)
  const [dialog, setDialog] = useState<Dialog>(null)
  const [message, setMessage] = useState('')
  const searchRef = useRef<HTMLInputElement>(null)
  const rowsRef = useRef<AttributeDefinitionRow[]>([])
  const activeRef = useRef(0)

  useEffect(() => onDraftChange(() => {
    void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
  }), [queryClient])

  const preview = draft.data?.conflict === true ? undefined : (draft.data?.definitions as Json | undefined)
  const defs = preview ?? stored.data
  const rows = useMemo(() => {
    const source = preview ? rowsFromDefinitions(preview, listed.data) : rowsFromCatalog(listed.data)
    return source.sort((left, right) => left.label.localeCompare(right.label) || left.key.localeCompare(right.key))
  }, [preview, listed.data])
  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase()
    const filtered = rows.filter((row) => {
      if (kind && row.kind !== kind) return false
      if (!needle) return true
      const used = usedText(row).toLowerCase()
      return `${row.label} ${row.key} ${kindLabel(row.kind)} ${used}`.toLowerCase().includes(needle)
    })
    const factor = sort.dir === 'asc' ? 1 : -1
    return filtered.sort((left, right) => factor * compareRows(left, right, sort.key) || left.key.localeCompare(right.key))
  }, [rows, query, kind, sort])
  rowsRef.current = visible
  activeRef.current = Math.min(active, Math.max(visible.length - 1, 0))

  const stage = async (name: string, args: Json) => {
    setMessage('')
    try {
      await stageModel(name, args)
      void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
    } catch (reason) {
      setMessage(reason instanceof Error ? reason.message : copy.validation)
    }
  }

  const run = (action: AttributeAction) => {
    const row = rowsRef.current[activeRef.current]
    if (action === 'new') {
      setMenu(null)
      setDialog({ kind: 'new' })
      return
    }
    if (!row) return
    setModelFocus({ kind: 'attribute', key: row.key })
    if (action === 'add-type' || action === 'add-group') {
      setMenu(null)
      setDialog({ kind: action === 'add-type' ? 'type' : 'group', attribute: row.key })
      return
    }
    if (action === 'rename') {
      setMenu(null)
      focusField(row.key, 'label')
      return
    }
    if (action === 'rename-key') {
      if (row.standard) return
      setMenu(null)
      focusField(row.key, 'key')
      return
    }
    if (action === 'delete') {
      if (row.standard) return
      setMenu(null)
      void stage('model.delete_attribute', { attribute: row.key })
    }
  }

  const runRef = useRef(run)
  runRef.current = run
  useEffect(() => onAttributeAction((action) => runRef.current(action)), [])

  const openMenu = (row: AttributeDefinitionRow, index: number, point: { x: number; y: number }) => {
    setActive(index)
    setModelFocus({ kind: 'attribute', key: row.key })
    setMenu({ key: row.key, x: point.x, y: point.y })
  }

  const onRowKey = (event: ReactKeyboardEvent<HTMLElement>, row: AttributeDefinitionRow, index: number) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const next = event.key === 'ArrowDown' ? index + 1 : index - 1
      const target = visible[next]
      if (!target) return
      setActive(next)
      document.querySelector<HTMLElement>(`[data-testid="attribute-definition-row"][data-key="${CSS.escape(target.key)}"]`)?.focus()
      return
    }
    if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
      event.preventDefault()
      const rect = event.currentTarget.getBoundingClientRect()
      openMenu(row, index, { x: rect.left, y: rect.bottom })
    }
  }

  if (listed.isPending && stored.isPending) {
    return <div data-testid="attribute-definitions"><p>{copy.loading}</p></div>
  }
  if (listed.isError && !listed.data) {
    return <div data-testid="attribute-definitions"><p className="error">{copy.validation}</p></div>
  }

  const menuRow = visible.find((row) => row.key === menu?.key)

  return (
    <div className="definition-view" data-testid="attribute-definitions">
      <div className="definition-toolbar">
        <input
          ref={searchRef}
          data-testid="attribute-search"
          aria-label={copy.search}
          placeholder={copy.search}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value)
            setActive(0)
          }}
          onKeyDown={(event) => {
            if (event.key !== 'ArrowDown') return
            const row = visible[0]
            if (!row) return
            event.preventDefault()
            setActive(0)
            document.querySelector<HTMLElement>(`[data-testid="attribute-definition-row"][data-key="${CSS.escape(row.key)}"]`)?.focus()
          }}
        />
        <select aria-label={copy.kindAll} value={kind} onChange={(event) => setKind(event.target.value)}>
          <option value="">{copy.allKinds}</option>
          {KINDS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
        </select>
        <button type="button" onClick={() => setDialog({ kind: 'new' })}>{copy.newAttribute}</button>
      </div>
      <div className="definition-scroll">
        <div className="definition-table" role="grid" aria-label={copy.attributeDefinitions}>
          <div className="definition-head" role="row">
            <SortButton label={copy.attributeName} column="label" sort={sort} onSort={setSort} />
            <SortButton label={copy.key} column="key" sort={sort} onSort={setSort} />
            <SortButton label={copy.attributeType} column="kind" sort={sort} onSort={setSort} />
            <SortButton label={copy.unitFamily} column="unit" sort={sort} onSort={setSort} />
            <SortButton label={copy.valueList} column="list" sort={sort} onSort={setSort} />
            <span>{copy.flags}</span>
            <span>{copy.usedIn}</span>
          </div>
          {visible.length === 0 ? <p>{copy.noAttributesMatch}</p> : null}
          {visible.map((row, index) => (
            <div
              key={row.id || row.key}
              className={index === active ? 'definition-row on' : 'definition-row'}
              role="row"
              tabIndex={0}
              data-testid="attribute-definition-row"
              data-key={row.key}
              data-standard={row.standard ? 'true' : 'false'}
              onFocus={() => {
                setActive(index)
                setModelFocus({ kind: 'attribute', key: row.key })
              }}
              onContextMenu={(event) => {
                event.preventDefault()
                openMenu(row, index, { x: event.clientX, y: event.clientY })
              }}
              onKeyDown={(event) => onRowKey(event, row, index)}
            >
              <input
                aria-label={copy.attributeName}
                data-testid={`attr-label-${row.key}`}
                defaultValue={row.label}
                key={`${row.key}:${row.label}`}
                onBlur={(event) => {
                  const next = event.target.value.trim()
                  if (next && next !== row.label) void stage('model.update_attribute', { key: row.key, label: next })
                }}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') event.currentTarget.blur()
                }}
              />
              <div>
                <input
                  aria-label={copy.key}
                  data-testid={`attr-key-${row.key}`}
                  defaultValue={row.key}
                  key={row.key}
                  disabled={row.standard}
                  onBlur={(event) => {
                    const next = event.target.value.trim()
                    if (next && next !== row.key) void stage('model.rename_attribute_key', { key: row.key, new_key: next })
                  }}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') event.currentTarget.blur()
                  }}
                  onChange={(event) => {
                    const next = event.target.value.trim()
                    const note = event.currentTarget.parentElement?.querySelector('[data-key-warning]')
                    if (note instanceof HTMLElement) note.hidden = !next || next === row.key
                  }}
                />
                {row.standard ? null : (
                  <p className="warn" data-key-warning hidden>
                    {copy.keyRenameWarning.replace('{key}', row.key)}
                  </p>
                )}
              </div>
              <span>{kindLabel(row.kind)}</span>
              <span>{row.unitFamily || '—'}</span>
              <span>{row.valueList || '—'}</span>
              <span className="definition-flags">{flagText(row.flags) || '—'}</span>
              <span className="definition-used" data-testid="used-in" data-objects={row.objects}>{usedText(row) || '—'}</span>
            </div>
          ))}
        </div>
      </div>
      {message ? <p className="error">{message}</p> : null}
      {menu && menuRow ? (
        <AttributeMenu
          row={menuRow}
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          onRun={run}
        />
      ) : null}
      {dialog?.kind === 'type' || dialog?.kind === 'group' ? (
        <PlacePicker
          mode={dialog.kind}
          defs={defs}
          onClose={() => setDialog(null)}
          onPick={(place) => {
            const attribute = dialog.attribute
            setDialog(null)
            if (place.kind === 'type') void stage('model.attach_attribute', { attribute, type: place.key })
            else void stage('model.attach_attribute', { attribute, structure: place.structure, group: place.key })
          }}
        />
      ) : null}
      {dialog?.kind === 'new' ? (
        <NewAttribute
          onClose={() => setDialog(null)}
          onCreate={(args) => {
            setDialog(null)
            void stage('model.create_attribute', args)
          }}
        />
      ) : null}
    </div>
  )
}

function AttributeMenu({
  row,
  x,
  y,
  onClose,
  onRun,
}: {
  row: AttributeDefinitionRow
  x: number
  y: number
  onClose: () => void
  onRun: (action: AttributeAction) => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('button')?.focus()
    const close = (event: PointerEvent) => {
      const target = event.target
      if (target instanceof Element && target.closest('[data-testid="attribute-menu"]')) return
      onClose()
    }
    window.addEventListener('pointerdown', close)
    return () => window.removeEventListener('pointerdown', close)
  }, [onClose])
  const items: Array<{ action: AttributeAction; label: string }> = [
    { action: 'add-type', label: copy.addToType },
    { action: 'add-group', label: copy.addToGroup },
    { action: 'rename', label: copy.renameAttribute },
    ...(row.standard ? [] : [{ action: 'rename-key' as const, label: copy.renameKey }]),
    ...(row.standard ? [] : [{ action: 'delete' as const, label: copy.deleteMenu }]),
    { action: 'new', label: copy.newAttribute },
  ]
  return (
    <div
      ref={ref}
      className="menu definition-menu"
      role="menu"
      data-testid="attribute-menu"
      style={{ left: x, top: y }}
      onPointerDown={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        const buttons = [...(ref.current?.querySelectorAll('button') ?? [])]
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
        if (event.key === 'Escape') {
          event.preventDefault()
          onClose()
          document.querySelector<HTMLElement>(`[data-testid="attribute-definition-row"][data-key="${CSS.escape(row.key)}"]`)?.focus()
          return
        }
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
          event.preventDefault()
          const next = event.key === 'ArrowDown' ? index + 1 : index - 1
          buttons[(next + buttons.length) % buttons.length]?.focus()
        }
      }}
    >
      {items.map((item) => (
        <button key={item.action} type="button" role="menuitem" onClick={() => onRun(item.action)}>
          {item.label}
        </button>
      ))}
    </div>
  )
}

function PlacePicker({
  mode,
  defs,
  onClose,
  onPick,
}: {
  mode: 'type' | 'group'
  defs: Json | undefined
  onClose: () => void
  onPick: (place: { kind: 'type'; key: string } | { kind: 'group'; structure: string; key: string }) => void
}) {
  const [query, setQuery] = useState('')
  const [cursor, setCursor] = useState(0)
  const options = useMemo(() => {
    const needle = query.trim().toLowerCase()
    if (mode === 'type') {
      return ((defs?.types as Json[] | undefined) ?? [])
        .map((item) => ({
          kind: 'type' as const,
          key: String(item.key ?? ''),
          label: singular(item),
        }))
        .filter((item) => !needle || `${item.label} ${item.key}`.toLowerCase().includes(needle))
        .sort((left, right) => left.label.localeCompare(right.label))
    }
    const groups: Array<{ kind: 'group'; structure: string; key: string; label: string }> = []
    for (const structure of (defs?.structures as Json[] | undefined) ?? []) {
      const structureKey = String(structure.key ?? '')
      const structureLabel = textLabel(structure.labels, structureKey)
      for (const group of (structure.groups as Json[] | undefined) ?? []) {
        const key = String(group.key ?? '')
        const label = `${structureLabel} / ${textLabel(group.labels, key)}`
        groups.push({ kind: 'group', structure: structureKey, key, label })
      }
    }
    return groups
      .filter((item) => !needle || `${item.label} ${item.key}`.toLowerCase().includes(needle))
      .sort((left, right) => left.label.localeCompare(right.label))
  }, [defs, mode, query])
  const choose = (index: number) => {
    const option = options[index]
    if (!option) return
    if (option.kind === 'type') onPick({ kind: 'type', key: option.key })
    else onPick({ kind: 'group', structure: option.structure, key: option.key })
  }
  return (
    <div className="overlay" data-testid={mode === 'type' ? 'add-to-type' : 'add-to-group'}>
      <div
        className="dialog"
        role="dialog"
        aria-label={mode === 'type' ? copy.addToType : copy.addToGroup}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault()
            onClose()
            return
          }
          if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
            event.preventDefault()
            setCursor((current) => {
              const next = event.key === 'ArrowDown' ? current + 1 : current - 1
              return Math.max(0, Math.min(options.length - 1, next))
            })
            return
          }
          if (event.key === 'Enter') {
            event.preventDefault()
            choose(Math.min(cursor, Math.max(options.length - 1, 0)))
          }
        }}
      >
        <h2>{mode === 'type' ? copy.addToType : copy.addToGroup}</h2>
        <input
          autoFocus
          data-testid={mode === 'type' ? 'add-to-type-search' : 'add-to-group-search'}
          aria-label={copy.search}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value)
            setCursor(0)
          }}
        />
        <div className="picker-list" role="listbox">
          {options.map((option, index) => (
            <button
              key={`${option.kind}:${option.key}:${'structure' in option ? option.structure : ''}`}
              type="button"
              className={index === cursor ? 'on' : undefined}
              onClick={() => choose(index)}
              onMouseEnter={() => setCursor(index)}
            >
              {option.label}
            </button>
          ))}
        </div>
        <button type="button" onClick={onClose}>{copy.cancel}</button>
      </div>
    </div>
  )
}

function NewAttribute({ onClose, onCreate }: { onClose: () => void; onCreate: (args: Json) => void }) {
  const [name, setName] = useState('')
  const [kind, setKind] = useState('text')
  return (
    <div className="overlay" data-testid="new-attribute">
      <form
        className="dialog"
        role="dialog"
        aria-label={copy.newAttribute}
        onSubmit={(event) => {
          event.preventDefault()
          const label = name.trim()
          const key = attributeKey(label)
          if (!key) return
          onCreate({ key, label, kind })
        }}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault()
            onClose()
          }
        }}
      >
        <h2>{copy.newAttribute}</h2>
        <label>
          {copy.attributeName}
          <input autoFocus aria-label={copy.attributeName} value={name} onChange={(event) => setName(event.target.value)} />
        </label>
        <label>
          {copy.attributeType}
          <select aria-label={copy.attributeType} value={kind} onChange={(event) => setKind(event.target.value)}>
            {KINDS.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </label>
        <div className="toolbar">
          <button type="button" onClick={onClose}>{copy.cancel}</button>
          <button type="submit">{copy.confirm}</button>
        </div>
      </form>
    </div>
  )
}

function SortButton({
  label,
  column,
  sort,
  onSort,
}: {
  label: string
  column: SortKey
  sort: { key: SortKey; dir: 'asc' | 'desc' }
  onSort: (next: { key: SortKey; dir: 'asc' | 'desc' }) => void
}) {
  const mark = sort.key === column ? (sort.dir === 'asc' ? ' ↑' : ' ↓') : ''
  return (
    <button
      type="button"
      aria-label={`Sort by ${label}`}
      onClick={() => onSort({ key: column, dir: sort.key === column && sort.dir === 'asc' ? 'desc' : 'asc' })}
    >
      {label}{mark}
    </button>
  )
}

function focusField(key: string, field: 'label' | 'key') {
  const testId = field === 'label' ? `attr-label-${key}` : `attr-key-${key}`
  requestAnimationFrame(() => {
    const input = document.querySelector<HTMLInputElement>(`[data-testid="${CSS.escape(testId)}"]`)
    input?.focus()
    input?.select()
  })
}

function compareRows(left: AttributeDefinitionRow, right: AttributeDefinitionRow, key: SortKey) {
  const value = (row: AttributeDefinitionRow) => {
    if (key === 'label') return row.label
    if (key === 'key') return row.key
    if (key === 'kind') return kindLabel(row.kind)
    if (key === 'unit') return row.unitFamily
    return row.valueList
  }
  return value(left).localeCompare(value(right))
}

function kindLabel(kind: string) {
  return KINDS.find(([value]) => value === kind)?.[1] ?? kind
}

function flagText(flags: AttributeFlags) {
  const parts = [
    flags.required ? copy.required : '',
    flags.multiple ? copy.multiple : '',
    flags.languages ? copy.languages : '',
    flags.searchable ? copy.searchable : '',
    flags.filterable ? copy.filterable : '',
    flags.sortable ? copy.sortable : '',
    flags.standard ? copy.standardFlag : '',
  ].filter(Boolean)
  return parts.join(', ')
}

function usedText(row: AttributeDefinitionRow) {
  const types = row.types.map((place) => `${place.label} (${formatCount(place.objects)})`)
  const groups = row.groups.map((place) => `${place.structureLabel ? `${place.structureLabel} / ` : ''}${place.label} (${formatCount(place.objects)})`)
  return [...types, ...groups].join(', ')
}

function singular(objectType: Json) {
  const fromSingular = textLabel(objectType.singular, '')
  if (fromSingular) return fromSingular
  return textLabel(objectType.labels, String(objectType.key ?? ''))
}

function textLabel(value: unknown, fallback: string) {
  if (!value || typeof value !== 'object') return fallback
  const labels = value as Record<string, unknown>
  if (typeof labels.en === 'string' && labels.en) return labels.en
  const first = Object.values(labels).find((item) => typeof item === 'string' && item)
  return typeof first === 'string' ? first : fallback
}
