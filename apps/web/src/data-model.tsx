import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { MoreHorizontal } from 'lucide-react'
import { ApiError, api, textValue, type Json } from './api'
import { formatCount } from './format'
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
import { StateNote } from './states'
import { onDraftChange, openModelDialog, setModelFocus, stageModel } from './model-stage'

type View = { state: Json }

const KINDS = [
  ['text', copy.kindText],
  ['number', copy.number],
  ['boolean', copy.kindBoolean],
  ['date', copy.kindDate],
  ['value_list', copy.kindValueList],
  ['range', copy.kindRange],
  ['file_ref', copy.kindFile],
] as const

let selection: ModelSelection | null = null
let previewType = ''
const listeners = new Set<() => void>()

function notify() {
  for (const listen of listeners) listen()
}

function choose(next: ModelSelection) {
  selection = next
  if (next.kind === 'type') {
    previewType = next.typeKey
    setModelFocus({ kind: 'type', key: next.typeKey })
  } else {
    setModelFocus({ kind: 'structure', key: next.structure })
  }
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
  return stageModel(name, args)
}

export function DataModel({ view }: { view: View }) {
  const queryClient = useQueryClient()
  const stored = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const draft = useQuery({ queryKey: ['model-draft'], queryFn: () => api<Json>('/v1/model/draft') })
  useEffect(() => onDraftChange(() => {
    void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
  }), [queryClient])
  const mode = String(view.state.mode ?? '')
  if (stored.isPending) return <div data-testid="data-model"><StateNote tone="loading" title={copy.loadingModel} /></div>
  if (stored.isError) return <div data-testid="data-model"><StateNote tone="error" title={copy.errorModel} detail={copy.validation} /></div>
  const preview = draft.data?.conflict === true ? undefined : (draft.data?.definitions as Json | undefined)
  const defs = preview ?? stored.data
  if (mode === 'classes') return <AttributeTable defs={defs} />
  if (mode === 'card' || mode === 'editor') return <ModelPreview defs={defs} />
  if (mode === 'impact') return <ImpactNote />
  return <ModelNav defs={defs} />
}

type NavMenu =
  | { x: number; y: number; kind: 'type'; key: string }
  | { x: number; y: number; kind: 'link'; key: string }
  | { x: number; y: number; kind: 'list'; key: string }
  | { x: number; y: number; kind: 'option'; list: string; code: string }
  | { x: number; y: number; kind: 'family'; key: string }
  | { x: number; y: number; kind: 'unit'; family: string; code: string }

function ModelNav({ defs }: { defs: Json | undefined }) {
  const { selection, choose: pick } = useModelSelection()
  const [menu, setMenu] = useState<NavMenu | null>(null)
  const rawTypes = (defs?.types as Json[]) ?? []
  const types = objectTypes({ ...defs, types: rawTypes.filter((item) => item.kind !== 'link') }, uiLanguage)
  const linkRows = objectTypes({ ...defs, types: rawTypes.filter((item) => item.kind === 'link') }, uiLanguage)
  const lists = ((defs?.value_lists as Json[]) ?? [])
  const families = ((defs?.unit_families as Json[]) ?? [])
  useEffect(() => {
    if (selection || types.length === 0) return
    const first = types[0]
    if (first) pick({ kind: 'type', typeKey: first.key })
  }, [selection, types, pick])
  useEffect(() => {
    if (!menu) return
    const close = () => setMenu(null)
    window.addEventListener('pointerdown', close)
    return () => window.removeEventListener('pointerdown', close)
  }, [menu])
  return (
    <div data-testid="data-model" className="model-nav">
      <section>
        <h2
          onContextMenu={(event) => {
            event.preventDefault()
            setMenu({ x: event.clientX, y: event.clientY, kind: 'type', key: '' })
          }}
        >
          {copy.objectTypes}
        </h2>
        {types.map((item) => (
          <button
            key={item.key}
            type="button"
            className={selection?.kind === 'type' && selection.typeKey === item.key ? 'model-node on' : 'model-node'}
            data-testid={`type-${item.key}`}
            onClick={() => pick({ kind: 'type', typeKey: item.key })}
            onContextMenu={(event) => {
              event.preventDefault()
              pick({ kind: 'type', typeKey: item.key })
              setMenu({ x: event.clientX, y: event.clientY, kind: 'type', key: item.key })
            }}
          >
            {item.label}
          </button>
        ))}
      </section>
      <section>
        <h2
          onContextMenu={(event) => {
            event.preventDefault()
            setMenu({ x: event.clientX, y: event.clientY, kind: 'link', key: '' })
          }}
        >
          {copy.linkTypes}
        </h2>
        {linkRows.map((item) => (
          <button
            key={item.key}
            type="button"
            className={selection?.kind === 'type' && selection.typeKey === item.key ? 'model-node on' : 'model-node'}
            data-testid={`link-type-${item.key}`}
            onClick={() => {
              setModelFocus({ kind: 'type', key: item.key })
              pick({ kind: 'type', typeKey: item.key })
            }}
            onContextMenu={(event) => {
              event.preventDefault()
              setModelFocus({ kind: 'type', key: item.key })
              pick({ kind: 'type', typeKey: item.key })
              setMenu({ x: event.clientX, y: event.clientY, kind: 'link', key: item.key })
            }}
          >
            {item.label}
          </button>
        ))}
      </section>
      <section>
        <h2>{copy.valueLists}</h2>
        {lists.map((list) => {
          const key = String(list.key ?? '')
          const label = textValue(list.labels as Json, uiLanguage) || key
          const options = ((list.options as Json[]) ?? [])
          return (
            <div key={key}>
              <button
                type="button"
                className="model-node"
                data-testid={`list-${key}`}
                onClick={() => setModelFocus({ kind: 'value_list', key })}
                onContextMenu={(event) => {
                  event.preventDefault()
                  setModelFocus({ kind: 'value_list', key })
                  setMenu({ x: event.clientX, y: event.clientY, kind: 'list', key })
                }}
              >
                {label}
              </button>
              {options.map((option) => {
                const code = String(option.code ?? option.key ?? '')
                return (
                  <button
                    key={code}
                    type="button"
                    className="model-node"
                    data-testid={`option-${key}-${code}`}
                    onClick={() => setModelFocus({ kind: 'option', list: key, code })}
                    onContextMenu={(event) => {
                      event.preventDefault()
                      setModelFocus({ kind: 'option', list: key, code })
                      setMenu({ x: event.clientX, y: event.clientY, kind: 'option', list: key, code })
                    }}
                  >
                    {textValue(option.labels as Json, uiLanguage) || code}
                  </button>
                )
              })}
            </div>
          )
        })}
      </section>
      <section>
        <h2>{copy.unitFamilies}</h2>
        {families.map((family) => {
          const key = String(family.key ?? '')
          const units = ((family.units as Json[]) ?? [])
          return (
            <div key={key}>
              <button
                type="button"
                className="model-node"
                data-testid={`family-${key}`}
                onClick={() => setModelFocus({ kind: 'unit_family', key })}
                onContextMenu={(event) => {
                  event.preventDefault()
                  setModelFocus({ kind: 'unit_family', key })
                  setMenu({ x: event.clientX, y: event.clientY, kind: 'family', key })
                }}
              >
                {key}
              </button>
              {units.map((unit) => {
                const code = String(unit.code ?? '')
                return (
                  <button
                    key={code}
                    type="button"
                    className="model-node"
                    data-testid={`unit-${key}-${code}`}
                    onClick={() => setModelFocus({ kind: 'unit', family: key, code })}
                    onContextMenu={(event) => {
                      event.preventDefault()
                      setModelFocus({ kind: 'unit', family: key, code })
                      setMenu({ x: event.clientX, y: event.clientY, kind: 'unit', family: key, code })
                    }}
                  >
                    {code}
                  </button>
                )
              })}
            </div>
          )
        })}
      </section>
      <StructureSection defs={defs} selection={selection} onPick={pick} />
      {menu ? (
        <div className="menu" role="menu" data-testid="type-menu" style={{ left: menu.x, top: menu.y }} onPointerDown={(event) => event.stopPropagation()}>
          {menu.kind === 'link' ? (
            <>
              <button type="button" onClick={() => { setMenu(null); openModelDialog('link') }}>{copy.newLinkType}</button>
              {menu.key ? <button type="button" onClick={() => { setMenu(null); void propose('model.delete_link_type', { key: menu.key }) }}>{copy.deleteMenu}</button> : null}
            </>
          ) : null}
          {menu.kind === 'type' ? (
            <>
              <button type="button" onClick={() => { setMenu(null); openModelDialog('type') }}>{copy.newType}</button>
              {menu.key ? <button type="button" onClick={() => { setMenu(null); void propose('model.delete_type', { type: menu.key }) }}>{copy.deleteMenu}</button> : null}
            </>
          ) : null}
          {menu.kind === 'list' ? (
            <>
              <button type="button" onClick={() => { setMenu(null); openModelDialog('list') }}>{copy.newValueList}</button>
              <button type="button" onClick={() => { setMenu(null); void propose('model.delete_value_list', { value_list: menu.key }) }}>{copy.deleteValueList}</button>
            </>
          ) : null}
          {menu.kind === 'option' ? (
            <button type="button" onClick={() => { setMenu(null); void propose('model.delete_value_option', { value_list: menu.list, option: menu.code }) }}>{copy.deleteOption}</button>
          ) : null}
          {menu.kind === 'family' ? (
            <>
              <button type="button" onClick={() => { setMenu(null); openModelDialog('family') }}>{copy.newUnitFamily}</button>
              <button type="button" onClick={() => { setMenu(null); void propose('model.delete_unit_family', { unit_family: menu.key }) }}>{copy.deleteUnitFamily}</button>
            </>
          ) : null}
          {menu.kind === 'unit' ? (
            <button type="button" onClick={() => { setMenu(null); void propose('model.delete_unit', { unit_family: menu.family, unit: menu.code }) }}>{copy.deleteUnit}</button>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

function AttributeTable({ defs }: { defs: Json | undefined }) {
  const { selection } = useModelSelection()
  const rows = attributeRows(defs, selection, uiLanguage)
  const families = ((defs?.unit_families as Json[]) ?? []).map((item) => String(item.key ?? '')).filter(Boolean)
  const [message, setMessage] = useState('')
  const [rowMenu, setRowMenu] = useState<{ x: number; y: number; key: string } | null>(null)
  const [filter, setFilter] = useState('')
  const [sort, setSort] = useState<'listed' | 'name' | 'kind'>('listed')
  const selectionKey = selection ? (selection.kind === 'type' ? selection.typeKey : `${selection.structure}/${selection.group}`) : ''
  const keyAttribute = keyAttributeOf(defs, selection)
  useEffect(() => {
    setMessage('')
  }, [selectionKey])
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
    : structureCaption(selection.group, defs, selection.kind === 'group' ? selection.structure : undefined)
  return (
    <div
      data-testid="data-model"
      className="model-attributes"
      onKeyDown={(event) => {
        if (!event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return
        const key = event.target instanceof HTMLElement ? event.target.closest('[data-attr]')?.getAttribute('data-attr') : ''
        if (!key || selection.kind !== 'type') return
        event.preventDefault()
        const order = rows.map((item) => item.key)
        const index = order.indexOf(key)
        const next = event.key === 'ArrowUp' ? index - 1 : index + 1
        if (index < 0 || next < 0 || next >= order.length) return
        const swapped = [...order]
        const moved = swapped[index]
        swapped[index] = swapped[next] ?? moved
        swapped[next] = moved
        run('model.reorder_type_attributes', { type: selection.typeKey, attributes: swapped })
      }}
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        if (selection.kind !== 'type') return
        const key = event.dataTransfer.getData('text/plain')
        const target = event.target instanceof HTMLElement ? event.target.closest('[data-attr]')?.getAttribute('data-attr') : ''
        if (!key || !target || key === target) return
        const order = rows.map((item) => item.key)
        const from = order.indexOf(key)
        const to = order.indexOf(target)
        if (from < 0 || to < 0) return
        order.splice(from, 1)
        order.splice(to, 0, key)
        run('model.reorder_type_attributes', { type: selection.typeKey, attributes: order })
      }}
    >
      <div data-testid="attribute-table" className="attr-table">
        <div className="attr-title">
          <span>{title}</span>
          <TableMenu filter={filter} sort={sort} onFilter={setFilter} onSort={setSort} />
        </div>
        {selection.kind === 'type' ? (
          <>
            <TypeNameFields
              defs={defs}
              typeKey={selection.typeKey}
              onSave={(patch) => run('model.update_type', { key: selection.typeKey, language: uiLanguage, ...patch })}
            />
            <AllowedTrees defs={defs} typeKey={selection.typeKey} />
          </>
        ) : null}
        <div className="attr-head">
          <span>{copy.attributeName}</span>
          <span>{copy.key}</span>
          <span>{copy.attributeType}</span>
          <span>{copy.unit}</span>
          <span>{copy.required}</span>
          <span>{copy.languages}</span>
          <span>{copy.searchable}</span>
          <span />
        </div>
        {selection.kind === 'type' && visibleRows(rows, filter, sort).some((row) => standardAttribute(row.id)) ? (
          <p className="attr-section">{copy.standardAttributes}</p>
        ) : null}
        {visibleRows(rows, filter, sort).filter((row) => selection.kind !== 'type' || standardAttribute(row.id)).map((row) => (
          <AttributeLine
            key={row.id}
            row={row}
            families={families}
            defs={defs}
            locked={standardAttribute(row.id)}
            onUpdate={(patch) => run('model.update_attribute', { key: row.key, ...patch })}
            onRenameKey={(newKey) => run('model.rename_attribute_key', { key: row.key, new_key: newKey })}
            onRemove={() =>
              run('model.detach_attribute', {
                attribute: row.key,
                ...(selection.kind === 'type'
                  ? { type: selection.typeKey }
                  : { structure: selection.structure, group: selection.group }),
              })
            }
            onDelete={standardAttribute(row.id) || row.id === keyAttribute ? undefined : () => run('model.delete_attribute', { attribute: row.key })}
          />
        ))}
        {selection.kind === 'type' ? visibleRows(rows, filter, sort).filter((row) => !standardAttribute(row.id)).map((row) => (
          <AttributeLine
            key={row.id}
            row={row}
            families={families}
            defs={defs}
            locked={row.id === keyAttribute}
            onUpdate={(patch) => run('model.update_attribute', { key: row.key, ...patch })}
            onRenameKey={(newKey) => run('model.rename_attribute_key', { key: row.key, new_key: newKey })}
            onRemove={() =>
              run('model.detach_attribute', {
                attribute: row.key,
                type: selection.typeKey,
              })
            }
            onDelete={row.id === keyAttribute ? undefined : () => run('model.delete_attribute', { attribute: row.key })}
            onMenu={(event) => setRowMenu({ x: event.clientX, y: event.clientY, key: row.key })}
          />
        )) : null}
        {rowMenu ? (
          <div className="menu" role="menu" data-testid="attribute-menu" style={{ left: rowMenu.x, top: rowMenu.y }} onPointerDown={(event) => event.stopPropagation()}>
            <button type="button" onClick={() => { setRowMenu(null); void propose('model.delete_attribute', { attribute: rowMenu.key }) }}>{copy.deleteMenu}</button>
          </div>
        ) : null}
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
  defs,
  locked,
  onUpdate,
  onRenameKey,
  onRemove,
  onDelete,
  onMenu,
}: {
  row: AttributeRow
  families: string[]
  defs?: Json
  locked: boolean
  onUpdate: (patch: Json) => void
  onRenameKey: (key: string) => void
  onRemove: () => void
  onDelete?: () => void
  onMenu?: (event: { preventDefault: () => void; clientX: number; clientY: number }) => void
}) {
  if (row.inherited) {
    return (
      <div className="attr-row inherited" data-inherited="true">
        <span>
          {row.label}
          <small>{copy.fromSource} {structureCaption(row.fromKey ?? '', defs)}</small>
        </span>
        <span>{row.key}</span>
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
    <div
      className="attr-row"
      data-inherited="false"
      data-attr={row.key}
      draggable
      onDragStart={(event) => {
        event.dataTransfer.setData('text/plain', row.key)
      }}
      onContextMenu={(event) => {
        event.preventDefault()
        setModelFocus({ kind: 'attribute', key: row.key })
        onMenu?.(event)
      }}
      onFocus={() => setModelFocus({ kind: 'attribute', key: row.key })}
    >
      <input
        aria-label={copy.attributeName}
        defaultValue={row.label}
        key={row.label}
        onBlur={(event) => {
          const next = event.target.value.trim()
          if (next && next !== row.label) onUpdate({ label: next })
        }}
      />
      <div>
      <input
        aria-label={copy.key}
        data-testid={`attr-key-${row.key}`}
        defaultValue={row.key}
        key={row.key}
        disabled={locked}
        onBlur={(event) => {
          const next = event.target.value.trim()
          if (next && next !== row.key) onRenameKey(next)
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur()
        }}
        onContextMenu={(event) => {
          event.preventDefault()
          event.currentTarget.focus()
          event.currentTarget.select()
        }}
        onChange={(event) => {
          const next = event.target.value.trim()
          const note = event.currentTarget.parentElement?.querySelector('[data-key-warning]')
          if (note instanceof HTMLElement) note.hidden = !next || next === row.key
        }}
      />
      <p className="warn" data-key-warning hidden>
        {copy.keyRenameWarning.replace('{key}', row.key)}
      </p>
      </div>
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
        onDelete={onDelete}
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
  const [multiple, setMultiple] = useState(false)
  const [min, setMin] = useState('')
  const [max, setMax] = useState('')
  const [maxLength, setMaxLength] = useState('')
  const [textForm, setTextForm] = useState('')
  const [listKey, setListKey] = useState('')
  const lists = ((defs?.value_lists as Json[]) ?? []).map((item) => String(item.key ?? '')).filter(Boolean)
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
        multiple,
        languages: kind === 'text' ? languages : false,
        required,
        searchable,
        filterable,
        sortable,
        ...(min.trim() ? { min: min.trim() } : {}),
        ...(max.trim() ? { max: max.trim() } : {}),
        ...(maxLength.trim() ? { max_length: Number(maxLength.trim()) } : {}),
        ...(textForm ? { text_form: textForm } : {}),
        ...(kind === 'value_list' && listKey ? { value_list: listKey } : {}),
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
      <span />
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
      {kind === 'number' || kind === 'range' ? (
        <>
          <input aria-label={copy.min} data-testid="attr-min" placeholder={copy.min} value={min} onChange={(event) => setMin(event.target.value)} />
          <input aria-label={copy.max} data-testid="attr-max" placeholder={copy.max} value={max} onChange={(event) => setMax(event.target.value)} />
        </>
      ) : null}
      {kind === 'text' ? (
        <>
          <input aria-label={copy.maxLength} placeholder={copy.maxLength} value={maxLength} onChange={(event) => setMaxLength(event.target.value)} />
          <Select value={textForm || 'short'} onValueChange={setTextForm}>
            <SelectTrigger aria-label={copy.textForm}><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="short">short</SelectItem>
              <SelectItem value="long">long</SelectItem>
            </SelectContent>
          </Select>
        </>
      ) : null}
      {kind === 'value_list' ? (
        <Select value={listKey || 'none'} onValueChange={(value) => setListKey(value === 'none' ? '' : value)}>
          <SelectTrigger aria-label={copy.kindValueList}><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="none">{copy.kindValueList}</SelectItem>
            {lists.map((item) => <SelectItem key={item} value={item}>{item}</SelectItem>)}
          </SelectContent>
        </Select>
      ) : null}
      <span className="attr-check"><Checkbox checked={multiple} aria-label={copy.multiple} onCheckedChange={(value) => setMultiple(value === true)} /></span>
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
  onDelete,
}: {
  filterable: boolean
  sortable: boolean
  onFilterable: (value: boolean) => void
  onSortable: (value: boolean) => void
  onRemove?: () => void
  onDelete?: () => void
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
        {onDelete ? <DropdownMenuItem onSelect={onDelete}>{copy.deleteMenu}</DropdownMenuItem> : null}
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
              {field.sources.map((source) => (source.kind === 'type' ? typeName(defs, source.key) : structureCaption(source.group, defs))).join(', ')}
            </small>
          </article>
        ))}
      </div>
      {selection?.kind === 'type' ? <p className="hint">{typeLabel}</p> : null}
    </div>
  )
}

const STANDARD_ATTRIBUTE_IDS = new Set([
  '018f0000-0000-7000-8000-000000000100',
  '018f0000-0000-7000-8000-000000000101',
  '018f0000-0000-7000-8000-00000000a003',
  '018f0000-0000-7000-8000-00000000a004',
  '018f0000-0000-7000-8000-00000000a005',
])

function standardAttribute(id: string) {
  return STANDARD_ATTRIBUTE_IDS.has(id)
}

function keyAttributeOf(defs: Json | undefined, selection: ModelSelection | null) {
  if (!defs || selection?.kind !== 'type') return ''
  const objectType = ((defs.types as Json[]) ?? []).find((item) => String(item.key) === selection.typeKey)
  return String(objectType?.key_attribute ?? '')
}

function AllowedTrees({ defs, typeKey }: { defs: Json | undefined; typeKey: string }) {
  const raw = ((defs?.types as Json[]) ?? []).find((item) => String(item.key) === typeKey)
  const selected = new Set(Array.isArray(raw?.structures) ? raw.structures.map((id) => String(id)) : [])
  const structures = ((defs?.structures as Json[]) ?? [])
  const [focus, setFocus] = useState(false)
  useEffect(() => {
    const open = () => setFocus(true)
    window.addEventListener('braian-allowed-trees', open)
    return () => window.removeEventListener('braian-allowed-trees', open)
  }, [])
  const save = (next: string[]) => {
    const id = String(raw?.id ?? typeKey)
    void api<Json>(`/v1/types/${id}/structures`, {
      method: 'PUT',
      body: JSON.stringify({ structures: next }),
    }).catch(() => undefined)
  }
  return (
    <fieldset className="type-names" data-testid="allowed-trees">
      <legend>{copy.allowedTrees}</legend>
      {structures.map((structure, index) => {
        const id = String(structure.id ?? structure.key)
        const checked = selected.has(id) || selected.has(String(structure.key))
        return (
          <label key={id}>
            <input
              type="checkbox"
              autoFocus={focus && index === 0}
              checked={checked}
              onChange={(event) => {
                const next = new Set(selected)
                if (event.target.checked) next.add(id)
                else next.delete(id)
                save([...next])
              }}
            />
            {String(structure.key)}
          </label>
        )
      })}
    </fieldset>
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
      <label>
        {copy.key}
        <input
          aria-label={copy.key}
          data-testid={`type-key-${typeKey}`}
          defaultValue={typeKey}
          key={`${typeKey}:key`}
          onBlur={(event) => {
            const next = event.target.value.trim()
            if (next && next !== typeKey) {
              void propose('model.rename_type_key', { key: typeKey, new_key: next }).catch(() => undefined)
            }
          }}
          onContextMenu={(event) => {
            event.preventDefault()
            event.currentTarget.focus()
            event.currentTarget.select()
          }}
        />
      </label>
    </div>
  )
}

function typeName(defs: Json | undefined, key: string) {
  return objectTypes(defs, uiLanguage).find((item) => item.key === key)?.label ?? structureCaption(key, defs)
}

function kindLabel(kind: string) {
  return KINDS.find(([value]) => value === kind)?.[1] ?? kind
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
          <p>{copy.affected}: {formatCount(result.affected_objects)}</p>
          <p>{copy.samples}: {((result.samples as Json[]) ?? []).map((item) => String(item.key)).join(', ')}</p>
          {((result.jobs as Json[]) ?? []).map((job) => (
            <p key={String(job.key)}>{String(job.title)}</p>
          ))}
        </div>
      ) : null}
    </div>
  )
}
