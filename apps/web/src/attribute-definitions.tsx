import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, type Json } from './api'
import { onAttributeAction, type AttributeAction } from './attribute-actions'
import { rowsFromCatalog, rowsFromDefinitions, type AttributeDefinitionRow, type AttributeFlags } from './attribute-rows'
import { formatCount } from './format'
import { copy } from './i18n'
import { attributeKey } from './model-view'
import { onDraftChange, requestReview, setModelFocus, stageModel } from './model-stage'
import { registerCommandRunner } from './command-registry'
import {
  StandardEditableTable,
  type EditRequest,
  type StandardColumn,
} from './standard-editable-table'

type Dialog = { kind: 'type' | 'group'; attribute: string } | null

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
  const [activeId, setActiveId] = useState<string | null>(null)
  const [dialog, setDialog] = useState<Dialog>(null)
  const [message, setMessage] = useState('')
  const [draftRows, setDraftRows] = useState<Record<string, AttributeDefinitionRow>>({})
  const [editRequest, setEditRequest] = useState<EditRequest | null>(null)
  const activeIdRef = useRef<string | null>(null)
  const rowsRef = useRef<AttributeDefinitionRow[]>([])

  useEffect(() => onDraftChange(() => {
    void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
  }), [queryClient])

  const preview = draft.data?.conflict === true ? undefined : (draft.data?.definitions as Json | undefined)
  const defs = preview ?? stored.data
  const rows = useMemo(() => {
    const source = preview ? rowsFromDefinitions(preview, listed.data) : rowsFromCatalog(listed.data)
    return source.sort((left, right) => left.label.localeCompare(right.label) || left.key.localeCompare(right.key))
  }, [preview, listed.data])

  const filteredBase = useMemo(() => {
    const needle = query.trim().toLowerCase()
    return rows.filter((row) => {
      if (kind && row.kind !== kind) return false
      if (!needle) return true
      const used = usedText(row).toLowerCase()
      return `${row.label} ${row.key} ${kindLabel(row.kind, row.key)} ${used}`.toLowerCase().includes(needle)
    })
  }, [rows, query, kind])

  rowsRef.current = (() => {
    const byId = new Map(filteredBase.map((row) => [row.id, row]))
    for (const [id, draftRow] of Object.entries(draftRows)) byId.set(id, draftRow)
    return [...byId.values()]
  })()
  activeIdRef.current = activeId

  const stage = async (name: string, args: Json) => {
    setMessage('')
    try {
      await stageModel(name, args)
      void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
    } catch (reason) {
      setMessage(reason instanceof Error ? reason.message : copy.validation)
    }
  }

  const blankRow = (): AttributeDefinitionRow => ({
    id: `new-${crypto.randomUUID()}`,
    key: '',
    label: '',
    kind: 'text',
    unitFamily: '',
    valueList: '',
    flags: {
      required: false,
      multiple: false,
      languages: false,
      searchable: false,
      filterable: false,
      sortable: false,
      standard: false,
    },
    standard: false,
    objects: 0,
    types: [],
    groups: [],
  })

  const run = (action: AttributeAction) => {
    if (action === 'new') {
      const row = blankRow()
      setDraftRows((current) => ({ ...current, [row.id]: row }))
      setActiveId(row.id)
      setEditRequest({ rowId: row.id, columnId: 'label' })
      return
    }
    const row =
      rowsRef.current.find((item) => item.id === activeIdRef.current) ??
      rowsRef.current[0]
    if (!row) return
    setModelFocus({ kind: 'attribute', key: row.key })
    if (action === 'add-type' || action === 'add-group') {
      setDialog({ kind: action === 'add-type' ? 'type' : 'group', attribute: row.key })
      return
    }
    if (action === 'rename') {
      setEditRequest({ rowId: row.id, columnId: 'label' })
      return
    }
    if (action === 'rename-key') {
      if (row.standard) return
      setEditRequest({ rowId: row.id, columnId: 'key' })
      return
    }
    if (action === 'delete') {
      if (row.standard) return
      void stage('model.delete_attribute', { attribute: row.key })
    }
  }

  const runRef = useRef(run)
  runRef.current = run
  useEffect(() => onAttributeAction((action) => runRef.current(action)), [])
  useEffect(() => {
    const stops = [
      registerCommandRunner('attr:new', () => runRef.current('new')),
      registerCommandRunner('attr:add-type', () => runRef.current('add-type')),
      registerCommandRunner('attr:add-group', () => runRef.current('add-group')),
      registerCommandRunner('attr:rename', () => runRef.current('rename')),
      registerCommandRunner('attr:rename-key', () => runRef.current('rename-key')),
      registerCommandRunner('attr:delete', () => runRef.current('delete')),
    ]
    return () => stops.forEach((stop) => stop())
  }, [])

  const columns: Array<StandardColumn<AttributeDefinitionRow>> = [
    {
      id: 'label',
      header: copy.attributeName,
      editable: true,
      read: (row) => row.label,
      write: (row, value) => ({ ...row, label: value }),
      cellTestId: (row) => (row.key ? `attr-label-${row.key}` : `attr-label-${row.id}`),
    },
    {
      id: 'key',
      header: copy.key,
      editable: true,
      read: (row) => row.key,
      write: (row, value) => ({ ...row, key: value }),
      cellTestId: (row) => (row.key ? `attr-key-${row.key}` : `attr-key-${row.id}`),
      display: (row) => row.key || '—',
    },
    {
      id: 'kind',
      header: copy.attributeType,
      type: 'enum',
      editable: true,
      options: KINDS.map(([value, label]) => ({ value, label })),
      read: (row) => row.kind,
      write: (row, value) => ({ ...row, kind: value }),
      display: (row) => kindLabel(row.kind, row.key),
    },
    {
      id: 'unit',
      header: copy.unit,
      type: 'readonly',
      editable: false,
      read: (row) => row.unitFamily || '—',
    },
    {
      id: 'list',
      header: copy.valueLists,
      type: 'readonly',
      editable: false,
      read: (row) => row.valueList || '—',
    },
    {
      id: 'flags',
      header: copy.flags,
      type: 'readonly',
      editable: false,
      read: (row) => flagText(row.flags) || '—',
    },
    {
      id: 'used',
      header: copy.usedIn,
      type: 'readonly',
      editable: false,
      truncate: true,
      read: (row) => usedText(row),
      display: (row) => (
        <span data-testid="used-in" data-objects={row.objects} title={usedText(row) || undefined}>
          {usedSummary(row)}
        </span>
      ),
    },
  ]

  if (listed.isPending && stored.isPending) {
    return <div data-testid="attribute-definitions"><p>{copy.loading}</p></div>
  }
  if (listed.isError && !listed.data) {
    return <div data-testid="attribute-definitions"><p className="error">{copy.validation}</p></div>
  }

  return (
    <div className="definition-view" data-testid="attribute-definitions">
      <StandardEditableTable
        testId="attribute-definitions-chrome"
        columns={columns}
        rows={filteredBase}
        draftRows={draftRows}
        onDraftRowsChange={setDraftRows}
        createDraftRow={blankRow}
        newRowLabel={copy.newAttribute}
        filterValue={query}
        onFilterChange={(value) => {
          setQuery(value)
          setActiveId(null)
        }}
        activeRowId={activeId}
        onActivateRow={(row) => {
          setActiveId(row.id)
          if (row.key) setModelFocus({ kind: 'attribute', key: row.key })
        }}
        getRowTestId={() => 'attribute-definition-row'}
        getRowKeyAttr={(row) => row.key || row.id}
        canEdit={(row, column) => {
          if (column.id === 'key' && row.standard) return false
          if (column.id === 'kind') {
            const isNew = row.id.startsWith('new-') || !rows.some((item) => item.id === row.id)
            return isNew
          }
          return true
        }}
        menuContext="attribute"
        getMenuTarget={(row) => ({
          attributeKey: row?.key || undefined,
          standard: row?.standard,
        })}
        editRequest={editRequest}
        onEditRequestHandled={() => setEditRequest(null)}
        belowBar={
          <div className="standard-table-filters">
            <input
              data-testid="attribute-search"
              aria-label={copy.search}
              placeholder={copy.search}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value)
                setActiveId(null)
              }}
              onKeyDown={(event) => {
                if (event.key !== 'ArrowDown') return
                const row = rowsRef.current[0]
                if (!row) return
                event.preventDefault()
                setActiveId(row.id)
                document
                  .querySelector<HTMLElement>(
                    `[data-testid="attribute-definition-row"][data-key="${CSS.escape(row.key || row.id)}"]`,
                  )
                  ?.focus()
              }}
            />
            <select aria-label={copy.kindAll} value={kind} onChange={(event) => setKind(event.target.value)}>
              <option value="">{copy.allKinds}</option>
              {KINDS.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </div>
        }
        onReviewAndSave={async () => {
          for (const row of Object.values(draftRows)) {
            const isNew = row.id.startsWith('new-') || !rows.some((item) => item.id === row.id)
            if (isNew) {
              const key = row.key.trim() || attributeKey(row.label)
              if (!key || !row.label.trim()) continue
              await stage('model.create_attribute', { key, label: row.label.trim(), kind: row.kind })
              continue
            }
            const original = rows.find((item) => item.id === row.id)
            if (!original) continue
            if (row.label.trim() && row.label !== original.label) {
              await stage('model.rename_attribute', { attribute: original.key, label: row.label.trim() })
            }
            if (!row.standard && row.key.trim() && row.key !== original.key) {
              await stage('model.rename_attribute_key', { key: original.key, new_key: row.key.trim() })
            }
          }
          setDraftRows({})
          requestReview()
        }}
      />
      {message ? <p className="error">{message}</p> : null}
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

function kindLabel(kind: string, key = '') {
  if (key === 'created_at' || key === 'updated_at') return copy.kindDateTime
  return KINDS.find(([value]) => value === kind)?.[1] ?? kind
}

function usedSummary(row: AttributeDefinitionRow) {
  const places = row.types.length + row.groups.length
  if (places === 0) return '—'
  const full = usedText(row)
  if (places <= 1) return full
  return copy.usedInSummary.replace('{count}', String(places))
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
