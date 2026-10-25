import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { MoreHorizontal } from 'lucide-react'
import { ApiError, api, textValue, type Json } from './api'
import { Checkbox } from './components/ui/checkbox'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from './components/ui/dropdown-menu'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import { copy, uiLanguage } from './i18n'
import {
  attributeConflict,
  attributeKey,
  attributeRows,
  effectiveFields,
  objectTypes,
  type AttributeRow,
  type ModelSelection,
} from './model-view'
import { structureCaption } from './graph-scene'
import { StructureSection } from './structure-tree'
import { publishProposal } from './proposal-bus'
import { StateNote } from './states'

type View = { state: Json }

const KINDS = [
  ['text', copy.kindText],
  ['number', copy.number],
  ['boolean', copy.kindBoolean],
  ['date', copy.kindDate],
  ['value_list', copy.kindValueList],
  ['range', copy.kindRange],
  ['file_ref', copy.kindFile],
  ['object_ref', copy.kindReference],
] as const

let selection: ModelSelection | null = null
let previewType = ''
const listeners = new Set<() => void>()

function notify() {
  for (const listen of listeners) listen()
}

function choose(next: ModelSelection) {
  selection = next
  if (next.kind === 'type') previewType = next.typeKey
  notify()
}

function setPreviewType(typeKey: string) {
  previewType = typeKey
  notify()
}

function useModelSelection() {
  const [value, setValue] = useState({ selection, previewType })
  useEffect(() => {
    const listen = () => setValue({ selection, previewType })
    listeners.add(listen)
    return () => {
      listeners.delete(listen)
    }
  }, [])
  return { ...value, choose, setPreviewType }
}

async function propose(name: string, args: Json) {
  const result = await api<Json>(`/v1/operations/${name}`, {
    method: 'POST',
    body: JSON.stringify(args),
  })
  const id = String(result.proposal_id ?? result.id ?? '')
  if (id) publishProposal(id)
  return result
}

export function DataModel({ view }: { view: View }) {
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const mode = String(view.state.mode ?? '')
  if (Array.isArray(view.state.checklist)) return <Checklist defs={defs.data} items={view.state.checklist as string[]} />
  if (defs.isPending) return <div data-testid="data-model"><StateNote tone="loading" title={copy.loadingModel} /></div>
  if (defs.isError) return <div data-testid="data-model"><StateNote tone="error" title={copy.errorModel} detail={copy.validation} /></div>
  if (mode === 'classes') return <AttributeTable defs={defs.data} />
  if (mode === 'card' || mode === 'editor') return <ModelPreview defs={defs.data} />
  if (mode === 'impact') return <ImpactNote />
  return <ModelNav defs={defs.data} />
}

function Checklist({ defs, items }: { defs: Json | undefined; items: string[] }) {
  const languages = ((defs?.languages as string[]) ?? []).join(', ')
  const units = ((defs?.unit_families as Json[]) ?? []).map((item) => String(item.key)).join(', ')
  const types = ((defs?.types as Json[]) ?? []).map((item) => String(item.key)).join(', ')
  return (
    <div data-testid="data-model">
      <strong>{copy.dataModel}</strong>
      <ul>
        {items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
      <p>sv/en/de · {languages}</p>
      <p>{units}</p>
      <p>{types}</p>
    </div>
  )
}

function ModelNav({ defs }: { defs: Json | undefined }) {
  const { selection, choose: pick } = useModelSelection()
  const types = objectTypes(defs, uiLanguage)
  useEffect(() => {
    if (selection || types.length === 0) return
    const first = types[0]
    if (first) pick({ kind: 'type', typeKey: first.key })
  }, [selection, types, pick])
  return (
    <div data-testid="data-model" className="model-nav">
      <section>
        <h2>{copy.objectTypes}</h2>
        {types.map((item) => (
          <button
            key={item.key}
            type="button"
            className={selection?.kind === 'type' && selection.typeKey === item.key ? 'model-node on' : 'model-node'}
            data-testid={`type-${item.key}`}
            onClick={() => pick({ kind: 'type', typeKey: item.key })}
          >
            {item.label}
          </button>
        ))}
      </section>
      <StructureSection defs={defs} selection={selection} onPick={pick} />
    </div>
  )
}

function AttributeTable({ defs }: { defs: Json | undefined }) {
  const { selection } = useModelSelection()
  const rows = attributeRows(defs, selection, uiLanguage)
  const families = ((defs?.unit_families as Json[]) ?? []).map((item) => String(item.key ?? '')).filter(Boolean)
  const [message, setMessage] = useState('')
  const [filter, setFilter] = useState('')
  const [sort, setSort] = useState<'listed' | 'name' | 'kind'>('listed')
  const selectionKey = selection ? (selection.kind === 'type' ? selection.typeKey : `${selection.structure}/${selection.group}`) : ''
  useEffect(() => {
    setMessage('')
  }, [selectionKey])
  const keyAttribute = keyAttributeOf(defs, selection)
  const run = (name: string, args: Json) => {
    setMessage('')
    void propose(name, args).catch((reason: unknown) => setMessage(reason instanceof ApiError ? reason.message : copy.validation))
  }
  if (!selection) {
    return (
      <div data-testid="data-model">
        <StateNote tone="empty" title={copy.pickModel} />
      </div>
    )
  }
  const title = selection.kind === 'type'
    ? objectTypes(defs, uiLanguage).find((item) => item.key === selection.typeKey)?.label ?? selection.typeKey
    : structureCaption(selection.group)
  return (
    <div data-testid="data-model" className="model-attributes">
      <div data-testid="attribute-table" className="attr-table">
        <div className="attr-title">
          <span>{title}</span>
          <TableMenu filter={filter} sort={sort} onFilter={setFilter} onSort={setSort} />
        </div>
        {selection.kind === 'type' ? (
          <TypeNameFields
            defs={defs}
            typeKey={selection.typeKey}
            onSave={(patch) => run('model.update_type', { key: selection.typeKey, language: uiLanguage, ...patch })}
          />
        ) : null}
        <div className="attr-head">
          <span>{copy.attributeName}</span>
          <span>{copy.attributeType}</span>
          <span>{copy.unit}</span>
          <span>{copy.required}</span>
          <span>{copy.languages}</span>
          <span>{copy.searchable}</span>
          <span />
        </div>
        {visibleRows(rows, filter, sort).map((row) => (
          <AttributeLine
            key={row.id}
            row={row}
            families={families}
            locked={row.id === keyAttribute}
            onUpdate={(patch) => run('model.update_attribute', { key: row.key, ...patch })}
            onRemove={() =>
              run('model.detach_attribute', {
                attribute: row.key,
                ...(selection.kind === 'type'
                  ? { type: selection.typeKey }
                  : { structure: selection.structure, group: selection.group }),
              })
            }
          />
        ))}
        <AddRow
          key={selectionKey}
          families={families}
          defs={defs}
          selection={selection}
          onError={setMessage}
          onPropose={(steps, titleText) => run('model.draft', { title: titleText, steps })}
        />
        {message ? <p className="error">{message}</p> : null}
      </div>
    </div>
  )
}

function AttributeLine({
  row,
  families,
  locked,
  onUpdate,
  onRemove,
}: {
  row: AttributeRow
  families: string[]
  locked: boolean
  onUpdate: (patch: Json) => void
  onRemove: () => void
}) {
  if (row.inherited) {
    return (
      <div className="attr-row inherited" data-inherited="true">
        <span>
          {row.label}
          <small>{copy.fromSource} {structureCaption(row.fromKey ?? '')}</small>
        </span>
        <span>{kindLabel(row.kind)}</span>
        <span>{row.unit || '—'}</span>
        <span>{row.required ? copy.yes : '—'}</span>
        <span>{row.languages ? copy.yes : '—'}</span>
        <span>{row.searchable ? copy.yes : '—'}</span>
        <span />
      </div>
    )
  }
  return (
    <div className="attr-row" data-inherited="false">
      <input
        aria-label={copy.attributeName}
        defaultValue={row.label}
        key={row.label}
        onBlur={(event) => {
          const next = event.target.value.trim()
          if (next && next !== row.label) onUpdate({ label: next })
        }}
      />
      <Select value={row.kind} onValueChange={(kind) => onUpdate(kind === 'number' || kind === 'range' ? { kind } : { kind, clear_unit: true })}>
        <SelectTrigger aria-label={copy.attributeType}><SelectValue /></SelectTrigger>
        <SelectContent>
          {KINDS.map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}
        </SelectContent>
      </Select>
      <Select
        value={row.unit || 'none'}
        disabled={row.kind !== 'number' && row.kind !== 'range'}
        onValueChange={(unit) => onUpdate(unit === 'none' ? { clear_unit: true, unit: null } : { unit, kind: row.kind })}
      >
        <SelectTrigger aria-label={copy.unit}><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem value="none">{copy.noUnit}</SelectItem>
          {families.map((family) => <SelectItem key={family} value={family}>{family}</SelectItem>)}
        </SelectContent>
      </Select>
      <span className="attr-check"><Checkbox checked={row.required} aria-label={copy.required} onCheckedChange={(value) => onUpdate({ required: value === true })} /></span>
      <span className="attr-check"><Checkbox checked={row.languages} aria-label={copy.languages} onCheckedChange={(value) => onUpdate({ languages: value === true })} /></span>
      <span className="attr-check"><Checkbox checked={row.searchable} aria-label={copy.searchable} onCheckedChange={(value) => onUpdate({ searchable: value === true })} /></span>
      <MoreMenu
        filterable={row.filterable}
        sortable={row.sortable}
        onFilterable={(value) => onUpdate({ filterable: value })}
        onSortable={(value) => onUpdate({ sortable: value })}
        onRemove={locked ? undefined : onRemove}
      />
    </div>
  )
}

function AddRow({
  families,
  defs,
  selection,
  onError,
  onPropose,
}: {
  families: string[]
  defs: Json | undefined
  selection: ModelSelection
  onError: (message: string) => void
  onPropose: (steps: Json[], title: string) => void
}) {
  const [name, setName] = useState('')
  const [kind, setKind] = useState('text')
  const [unit, setUnit] = useState('none')
  const [required, setRequired] = useState(false)
  const [languages, setLanguages] = useState(true)
  const [searchable, setSearchable] = useState(false)
  const [filterable, setFilterable] = useState(false)
  const [sortable, setSortable] = useState(false)
  const submit = () => {
    const label = name.trim()
    const key = attributeKey(label)
    if (!key) return
    const unitKey = kind === 'number' || kind === 'range' ? (unit === 'none' ? '' : unit) : ''
    if (attributeConflict(defs, key, kind, unitKey)) {
      onError(copy.conflictAttribute)
      return
    }
    onError('')
    const exists = ((defs?.attributes as Json[]) ?? []).some((item) => String(item.key) === key)
    const steps: Json[] = []
    if (!exists) {
      steps.push({
        op: 'create_attribute',
        key,
        label,
        kind,
        unit: unitKey || null,
        multiple: false,
        languages: kind === 'text' ? languages : false,
        required,
        searchable,
        filterable,
        sortable,
      })
    }
    steps.push({
      op: 'attach_attribute',
      attribute: key,
      ...(selection.kind === 'type'
        ? { type_key: selection.typeKey }
        : { structure: selection.structure, group: selection.group }),
    })
    onPropose(steps, `${copy.addAttribute} ${label}`)
    setName('')
  }
  return (
    <form
      className="attr-row add"
      onSubmit={(event) => {
        event.preventDefault()
        submit()
      }}
    >
      <input aria-label={copy.attributeName} data-testid="attr-add" placeholder={copy.attributeName} value={name} onChange={(event) => setName(event.target.value)} />
      <Select value={kind} onValueChange={(value) => { setKind(value); setLanguages(value === 'text') }}>
        <SelectTrigger aria-label={copy.attributeType}><SelectValue /></SelectTrigger>
        <SelectContent>
          {KINDS.map(([value, label]) => <SelectItem key={value} value={value}>{label}</SelectItem>)}
        </SelectContent>
      </Select>
      <Select value={unit} disabled={kind !== 'number' && kind !== 'range'} onValueChange={setUnit}>
        <SelectTrigger aria-label={copy.unit}><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem value="none">{copy.noUnit}</SelectItem>
          {families.map((family) => <SelectItem key={family} value={family}>{family}</SelectItem>)}
        </SelectContent>
      </Select>
      <span className="attr-check"><Checkbox checked={required} aria-label={copy.required} onCheckedChange={(value) => setRequired(value === true)} /></span>
      <span className="attr-check"><Checkbox checked={languages} aria-label={copy.languages} onCheckedChange={(value) => setLanguages(value === true)} /></span>
      <span className="attr-check"><Checkbox checked={searchable} aria-label={copy.searchable} onCheckedChange={(value) => setSearchable(value === true)} /></span>
      <MoreMenu filterable={filterable} sortable={sortable} onFilterable={setFilterable} onSortable={setSortable} />
    </form>
  )
}

function TableMenu({
  filter,
  sort,
  onFilter,
  onSort,
}: {
  filter: string
  sort: 'listed' | 'name' | 'kind'
  onFilter: (value: string) => void
  onSort: (value: 'listed' | 'name' | 'kind') => void
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="icon-btn" aria-label={copy.more}>{copy.more}</DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <div className="menu-filter" onPointerDown={(event) => event.stopPropagation()}>
          <input
            aria-label={copy.filterRows}
            placeholder={copy.filterRows}
            value={filter}
            onChange={(event) => onFilter(event.target.value)}
            onKeyDown={(event) => event.stopPropagation()}
          />
        </div>
        <DropdownMenuItem onSelect={(event) => { event.preventDefault(); onSort(sort === 'name' ? 'listed' : 'name') }}>
          {copy.sortByName}{sort === 'name' ? ' · on' : ''}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={(event) => { event.preventDefault(); onSort(sort === 'kind' ? 'listed' : 'kind') }}>
          {copy.sortByType}{sort === 'kind' ? ' · on' : ''}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function visibleRows(rows: AttributeRow[], filter: string, sort: 'listed' | 'name' | 'kind') {
  const needle = filter.trim().toLowerCase()
  const matched = needle
    ? rows.filter((row) => row.label.toLowerCase().includes(needle) || row.key.includes(needle))
    : rows
  if (sort === 'listed') return matched
  return [...matched].sort((left, right) => {
    const a = sort === 'name' ? left.label : kindLabel(left.kind)
    const b = sort === 'name' ? right.label : kindLabel(right.kind)
    return a.localeCompare(b)
  })
}

function MoreMenu({
  filterable,
  sortable,
  onFilterable,
  onSortable,
  onRemove,
}: {
  filterable: boolean
  sortable: boolean
  onFilterable: (value: boolean) => void
  onSortable: (value: boolean) => void
  onRemove?: () => void
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="icon-btn" aria-label={copy.more}>
        <MoreHorizontal size={14} aria-hidden="true" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={(event) => { event.preventDefault(); onFilterable(!filterable) }}>
          {copy.filterable}{filterable ? ' · on' : ''}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={(event) => { event.preventDefault(); onSortable(!sortable) }}>
          {copy.sortable}{sortable ? ' · on' : ''}
        </DropdownMenuItem>
        {onRemove ? <DropdownMenuItem onSelect={onRemove}>{copy.removeFromHere}</DropdownMenuItem> : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function ModelPreview({ defs }: { defs: Json | undefined }) {
  const { selection, previewType, setPreviewType: pickType } = useModelSelection()
  const types = objectTypes(defs, uiLanguage)
  const typeKey = selection?.kind === 'type' ? selection.typeKey : previewType || types[0]?.key || ''
  const groups = selection?.kind === 'group' ? [{ structure: selection.structure, group: selection.group }] : []
  const fields = effectiveFields(defs, typeKey, groups, uiLanguage)
  const typeLabel = types.find((item) => item.key === typeKey)?.label ?? typeKey
  return (
    <div data-testid="data-model" className="model-preview">
      <h2>{typeLabel ? indefiniteHere(typeLabel) : copy.anObjectHere}</h2>
      {selection?.kind === 'group' ? (
        <label className="field">
          <span>{copy.previewType}</span>
          <Select value={typeKey} onValueChange={pickType}>
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              {types.map((item) => <SelectItem key={item.key} value={item.key}>{item.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </label>
      ) : null}
      {fields.length === 0 ? <StateNote tone="empty" title={copy.pickModel} /> : null}
      <div data-testid="model-preview">
        {fields.map((field) => (
          <article key={field.id} className="model-field">
            <strong>{field.label}</strong>
            <small>{kindLabel(field.kind)}</small>
            <small>
              {copy.fromSource}{' '}
              {field.sources.map((source) => (source.kind === 'type' ? typeName(defs, source.key) : structureCaption(source.group))).join(', ')}
            </small>
          </article>
        ))}
      </div>
      {selection?.kind === 'type' ? <p className="hint">{typeLabel}</p> : null}
    </div>
  )
}

function indefiniteHere(name: string) {
  const lower = name.toLowerCase()
  const article = /^[aeiou]/i.test(lower) ? 'An' : 'A'
  return `${article} ${lower} here gets these fields`
}

function TypeNameFields({
  defs,
  typeKey,
  onSave,
}: {
  defs: Json | undefined
  typeKey: string
  onSave: (patch: Json) => void
}) {
  const row = objectTypes(defs, uiLanguage).find((item) => item.key === typeKey)
  const raw = ((defs?.types as Json[]) ?? []).find((item) => String(item.key) === typeKey)
  const singularValue = textValue(raw?.singular, uiLanguage) || row?.singular || ''
  const pluralValue = textValue(raw?.plural, uiLanguage) || row?.plural || ''
  return (
    <div className="type-names">
      <label>
        {copy.singular}
        <input
          aria-label={copy.singular}
          defaultValue={singularValue}
          key={`${typeKey}:singular:${singularValue}`}
          onBlur={(event) => {
            const next = event.target.value.trim()
            if (next !== singularValue) onSave({ singular: next })
          }}
        />
      </label>
      <label>
        {copy.plural}
        <input
          aria-label={copy.plural}
          defaultValue={pluralValue}
          key={`${typeKey}:plural:${pluralValue}`}
          onBlur={(event) => {
            const next = event.target.value.trim()
            if (next !== pluralValue) onSave({ plural: next })
          }}
        />
      </label>
    </div>
  )
}

function typeName(defs: Json | undefined, key: string) {
  return objectTypes(defs, uiLanguage).find((item) => item.key === key)?.label ?? structureCaption(key)
}

function kindLabel(kind: string) {
  return KINDS.find(([value]) => value === kind)?.[1] ?? kind
}

function keyAttributeOf(defs: Json | undefined, selection: ModelSelection | null) {
  if (!defs || selection?.kind !== 'type') return ''
  const objectType = ((defs.types as Json[]) ?? []).find((item) => String(item.key) === selection.typeKey)
  return String(objectType?.key_attribute ?? '')
}

function ImpactNote() {
  const queryClient = useQueryClient()
  const [attribute, setAttribute] = useState('status')
  const [result, setResult] = useState<Json | null>(null)
  return (
    <div data-testid="data-model">
      <p className="hint">{copy.reviewChange}</p>
      <form
        className="toolbar"
        onSubmit={(event) => {
          event.preventDefault()
          void api<Json>('/v1/impact', { method: 'POST', body: JSON.stringify({ remove_attribute: attribute }) })
            .then((body) => {
              setResult(body)
              void queryClient.invalidateQueries({ queryKey: ['definitions'] })
            })
        }}
      >
        <input data-testid="impact-attribute" value={attribute} onChange={(event) => setAttribute(event.target.value)} />
        <button type="submit" data-testid="impact-run">{copy.calculate}</button>
      </form>
      {result ? (
        <div data-testid="impact-result">
          <p>{copy.affected}: {String(result.affected_objects)}</p>
          <p>{copy.samples}: {((result.samples as Json[]) ?? []).map((item) => String(item.key)).join(', ')}</p>
          {((result.jobs as Json[]) ?? []).map((job) => (
            <p key={String(job.key)}>{String(job.title)}</p>
          ))}
        </div>
      ) : null}
    </div>
  )
}
