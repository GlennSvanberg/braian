import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { Check } from 'lucide-react'
import { Command, CommandInput, CommandList } from '../../components/ui/command'
import { Kbd } from '../../components/ui/kbd'
import {
  cardinalityCrumbLabel,
  cardinalityStepForLink,
  nextQualifierSteps,
  type AppliedQualifier,
  type FieldDef,
  type QualifierOption,
  type QualifierStep,
} from './qualifiers'
import { PRODUCT_FIELDS } from './mock-catalog'
import { makeColumn, withReplacedQualifier, type ProtoColumn } from './columns'

export type FieldsPaletteOpen =
  | { kind: 'browse' }
  | { kind: 'edit'; column: ProtoColumn }

type FieldsFrame = {
  kind: 'fields'
  fields: FieldDef[]
  label: string
  pathIds: string[]
  pathNames: string[]
  viaMany: boolean
  linkQualifiers: AppliedQualifier[]
  /** Breadcrumb segments after Fields (field names + All/First/…). */
  crumbs: string[]
}

type QualifyFrame = {
  kind: 'qualify'
  step: QualifierStep
  field: FieldDef
  pathIds: string[]
  pathNames: string[]
  viaMany: boolean
  applied: AppliedQualifier[]
  pendingLeaf: FieldDef
  crumbs: string[]
  /** When editing, the column being edited (in-place qualifier swap). */
  editing?: ProtoColumn
}

type SpecificFrame = {
  kind: 'specific'
  options: Array<{ id: string; label: string }>
  field: FieldDef
  pathIds: string[]
  pathNames: string[]
  viaMany: boolean
  applied: AppliedQualifier[]
  pendingLeaf: FieldDef
  crumbs: string[]
  editing?: ProtoColumn
}

type StackFrame = FieldsFrame | QualifyFrame | SpecificFrame

type Row = {
  id: string
  title: string
  subtitle: string
  drill?: boolean
  checked?: boolean
  run: () => void
}

type Props = {
  open: FieldsPaletteOpen
  columns: ProtoColumn[]
  specificOptions: Array<{ id: string; label: string }>
  onClose: () => void
  onToggleColumn: (column: ProtoColumn) => void
  onReplaceColumn: (previousId: string, next: ProtoColumn) => void
}

function rootFrame(): FieldsFrame {
  return {
    kind: 'fields',
    fields: PRODUCT_FIELDS,
    label: 'Fields',
    pathIds: [],
    pathNames: [],
    viaMany: false,
    linkQualifiers: [],
    crumbs: [],
  }
}

/** Open Edit field on the column's first qualifier step with current choice marked. */
function framesForEdit(column: ProtoColumn): StackFrame[] {
  const root = rootFrame()
  let fields = PRODUCT_FIELDS
  const pathIds: string[] = []
  const pathNames: string[] = []
  const crumbs: string[] = []
  let viaMany = false
  let linkQualifiers: AppliedQualifier[] = []

  for (let i = 0; i < column.fieldPath.length; i += 1) {
    const id = column.fieldPath[i]!
    const field = fields.find((item) => item.id === id)
    if (!field) return [root]
    pathIds.push(field.id)
    pathNames.push(field.name)
    crumbs.push(field.name)

    if (field.shape === 'link' && field.link) {
      const card = cardinalityStepForLink(field.link)
      if (card) {
        viaMany = true
        const appliedCard = column.qualifiers.find((q) => q.kind === 'cardinality')
        linkQualifiers = appliedCard ? [appliedCard] : []
        // First qualifier = cardinality
        return [
          root,
          {
            kind: 'qualify',
            step: card,
            field,
            pathIds: [...pathIds],
            pathNames: [...pathNames],
            viaMany: true,
            applied: linkQualifiers,
            pendingLeaf: field,
            crumbs: [...crumbs],
            editing: column,
          },
        ]
      }
      fields = field.link.peerFields
      continue
    }

    // Leaf field — open its first qualifier step
    const leafQs = column.qualifiers.filter((q) => q.kind !== 'cardinality')
    const steps = nextQualifierSteps(field, linkQualifiers)
    if (steps.length === 0) return [root]
    return [
      root,
      {
        kind: 'qualify',
        step: steps[0]!,
        field,
        pathIds: [...pathIds],
        pathNames: [...pathNames],
        viaMany,
        applied: [...linkQualifiers, ...leafQs],
        pendingLeaf: field,
        crumbs: [...crumbs],
        editing: column,
      },
    ]
  }
  return [root]
}

function pathMatches(col: ProtoColumn, pathIds: string[]): boolean {
  if (col.fieldPath.length < pathIds.length) return false
  return pathIds.every((id, index) => col.fieldPath[index] === id)
}

function hasColumnUnder(columns: ProtoColumn[], pathIds: string[]): boolean {
  return columns.some((col) => pathMatches(col, pathIds) && col.fieldPath.length >= pathIds.length)
}

function hasExactColumn(columns: ProtoColumn[], column: ProtoColumn | null): boolean {
  if (!column) return false
  return columns.some((col) => col.id === column.id)
}

function hasQualifierChoice(
  columns: ProtoColumn[],
  pathIds: string[],
  kind: AppliedQualifier['kind'],
  optionId: string,
  applied: AppliedQualifier[],
): boolean {
  return columns.some((col) => {
    if (!pathMatches(col, pathIds)) return false
    // For cardinality, path may be longer (peer leaf); for leaf quals, path should match exactly
    if (kind !== 'cardinality' && col.fieldPath.length !== pathIds.length) return false
    if (kind === 'cardinality' && col.fieldPath.length < pathIds.length) return false
    const hit = col.qualifiers.find((q) => q.kind === kind)
    if (!hit || hit.optionId !== optionId) return false
    // Other applied qualifiers (except the one being chosen) should match when present on the column
    for (const req of applied) {
      if (req.kind === kind) continue
      const other = col.qualifiers.find((q) => q.kind === req.kind)
      if (!other) continue
      if (other.optionId !== req.optionId) return false
      if ((other.value ?? '') !== (req.value ?? '')) return false
    }
    return true
  })
}

/**
 * Fields › mode for the Ctrl+K palette (prototype).
 * Enter toggles/applies and keeps the palette on the same level; Esc closes.
 */
export function FieldsPalette({
  open,
  columns,
  specificOptions,
  onClose,
  onToggleColumn,
  onReplaceColumn,
}: Props) {
  const initial = open.kind === 'edit' ? framesForEdit(open.column) : [rootFrame()]
  const [stack, setStack] = useState<StackFrame[]>(initial)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  /** Ignore the next Enter after a stack change so cmdk cannot toggle the new first row. */
  const ignoreEnter = useRef(false)
  const columnsRef = useRef(columns)
  columnsRef.current = columns

  function armIgnoreEnter() {
    ignoreEnter.current = true
    window.setTimeout(() => {
      ignoreEnter.current = false
    }, 150)
  }

  const top = stack[stack.length - 1]!
  const crumbParts = ['Fields', ...top.crumbs]

  const rows: Row[] = useMemo(() => {
    const needle = query.trim().toLowerCase()
    const cols = columns

    if (top.kind === 'fields') {
      return top.fields
        .filter((field) => !needle || `${field.name} ${field.typeLabel}`.toLowerCase().includes(needle))
        .map((field) => {
          const isLink = field.shape === 'link'
          const needsQualify = nextQualifierSteps(field, top.linkQualifiers).length > 0
          const drill = isLink || needsQualify
          const childPath = [...top.pathIds, field.id]
          let checked = false
          if (isLink) {
            checked = hasColumnUnder(cols, childPath)
          } else if (needsQualify) {
            checked = hasColumnUnder(cols, childPath)
          } else {
            const candidate = makeColumn(childPath, top.linkQualifiers)
            checked = hasExactColumn(cols, candidate)
          }
          return {
            id: field.id,
            title: field.name,
            subtitle: field.typeLabel,
            drill,
            checked,
            run: () => activateField(field, top),
          }
        })
    }

    if (top.kind === 'qualify') {
      return top.step.options
        .filter((option) => !needle || option.label.toLowerCase().includes(needle))
        .map((option) => ({
          id: option.id,
          title: option.label,
          subtitle: option.subtitle ?? '',
          drill: Boolean(option.drill),
          checked: hasQualifierChoice(cols, top.pathIds, top.step.kind, option.id, top.applied),
          run: () => activateQualifier(option, top),
        }))
    }

    return top.options
      .filter((option) => !needle || option.label.toLowerCase().includes(needle))
      .map((option) => ({
        id: option.id,
        title: option.label,
        subtitle: '',
        checked: hasQualifierChoice(
          cols,
          top.pathIds,
          'cardinality',
          'specific',
          [...top.applied.filter((q) => q.kind !== 'cardinality'), { kind: 'cardinality', optionId: 'specific', label: option.label, value: option.id }],
        ),
        run: () => activateSpecific(option.id, option.label, top),
      }))
  }, [top, query, columns])


  function pop() {
    setStack((prev) => (prev.length > 1 ? prev.slice(0, -1) : prev))
    setQuery('')
    setActive(0)
  }


  function activateField(field: FieldDef, frame: FieldsFrame) {
    if (field.shape === 'link' && field.link) {
      const card = cardinalityStepForLink(field.link)
      const pathIds = [...frame.pathIds, field.id]
      const pathNames = [...frame.pathNames, field.name]
      const crumbs = [...frame.crumbs, field.name]
      if (card) {
        setStack((prev) => [
          ...prev,
          {
            kind: 'qualify',
            step: card,
            field,
            pathIds,
            pathNames,
            viaMany: true,
            applied: [...frame.linkQualifiers],
            pendingLeaf: field,
            crumbs,
          },
        ])
        setQuery('')
        setActive(0)
        return
      }
      setStack((prev) => [
        ...prev,
        {
          kind: 'fields',
          fields: field.link!.peerFields,
          label: field.name,
          pathIds,
          pathNames,
          viaMany: frame.viaMany,
          linkQualifiers: frame.linkQualifiers,
          crumbs,
        },
      ])
      armIgnoreEnter()
      setQuery('')
      setActive(0)
      return
    }

    const pathIds = [...frame.pathIds, field.id]
    const pathNames = [...frame.pathNames, field.name]
    const crumbs = [...frame.crumbs, field.name]
    const steps = nextQualifierSteps(field, frame.linkQualifiers)
    if (steps.length > 0) {
      setStack((prev) => [
        ...prev,
        {
          kind: 'qualify',
          step: steps[0]!,
          field,
          pathIds,
          pathNames,
          viaMany: frame.viaMany,
          applied: [...frame.linkQualifiers],
          pendingLeaf: field,
          crumbs,
        },
      ])
      armIgnoreEnter()
      setQuery('')
      setActive(0)
      return
    }

    const col = makeColumn(pathIds, frame.linkQualifiers)
    if (col) {
      onToggleColumn(col)
      // Stay on this fields list; highlight the toggled field
      setActive(frame.fields.findIndex((item) => item.id === field.id))
    }
  }

  function activateQualifier(option: QualifierOption, frame: QualifyFrame) {
    if (option.drill) {
      setStack((prev) => [
        ...prev,
        {
          kind: 'specific',
          options: specificOptions,
          field: frame.field,
          pathIds: frame.pathIds,
          pathNames: frame.pathNames,
          viaMany: frame.viaMany,
          applied: frame.applied,
          pendingLeaf: frame.pendingLeaf,
          crumbs: [...frame.crumbs, 'Specific'],
          editing: frame.editing,
        },
      ])
      armIgnoreEnter()
      setQuery('')
      setActive(0)
      return
    }

    const nextApplied: AppliedQualifier[] = [
      ...frame.applied.filter((item) => item.kind !== frame.step.kind),
      { kind: frame.step.kind, optionId: option.id, label: option.label },
    ]

    // Edit-in-place: swap this qualifier on the existing column and stay here
    if (frame.editing) {
      const updated = withReplacedQualifier(frame.editing, {
        kind: frame.step.kind,
        optionId: option.id,
        label: option.label,
      })
      onReplaceColumn(frame.editing.id, updated)
      setStack((prev) => {
        const copy = [...prev]
        const last = copy[copy.length - 1]
        if (last?.kind === 'qualify') {
          copy[copy.length - 1] = {
            ...last,
            applied: nextApplied,
            editing: updated,
            crumbs:
              frame.step.kind === 'cardinality'
                ? [...frame.crumbs.slice(0, -0), ...frame.crumbs]
                : frame.crumbs,
          }
          // Update crumb trailing All/First when swapping cardinality
          if (frame.step.kind === 'cardinality') {
            const base = frame.crumbs.filter(
              (c) => c !== 'All' && c !== 'First' && c !== 'Specific',
            )
            copy[copy.length - 1] = {
              ...(copy[copy.length - 1] as QualifyFrame),
              crumbs: base,
              editing: updated,
              applied: nextApplied,
            }
          }
        }
        return copy
      })
      const optionIndex = frame.step.options.findIndex((item) => item.id === option.id)
      setActive(optionIndex >= 0 ? optionIndex : 0)
      return
    }

    // After cardinality on a many-link, drill into peer fields (intermediate step)
    if (frame.step.kind === 'cardinality' && frame.field.shape === 'link' && frame.field.link) {
      const crumbLabel = cardinalityCrumbLabel(option.id)
      setStack((prev) => [
        ...prev,
        {
          kind: 'fields',
          fields: frame.field.link!.peerFields,
          label: frame.field.name,
          pathIds: frame.pathIds,
          pathNames: frame.pathNames,
          viaMany: true,
          linkQualifiers: nextApplied,
          crumbs: [...frame.crumbs, crumbLabel],
        },
      ])
      armIgnoreEnter()
      setQuery('')
      setActive(0)
      return
    }

    const remaining = nextQualifierSteps(frame.pendingLeaf, nextApplied)
    if (remaining.length > 0) {
      setStack((prev) => [
        ...prev,
        {
          kind: 'qualify',
          step: remaining[0]!,
          field: frame.pendingLeaf,
          pathIds: frame.pathIds,
          pathNames: frame.pathNames,
          viaMany: frame.viaMany,
          applied: nextApplied,
          pendingLeaf: frame.pendingLeaf,
          crumbs: frame.crumbs,
        },
      ])
      armIgnoreEnter()
      setQuery('')
      setActive(0)
      return
    }

    // Completing a column — stay on this qualifier level
    const col = makeColumn(frame.pathIds, nextApplied)
    if (!col) return
    onToggleColumn(col)
    const optionIndex = frame.step.options.findIndex((item) => item.id === option.id)
    setActive(optionIndex >= 0 ? optionIndex : 0)
  }

  function activateSpecific(value: string, label: string, frame: SpecificFrame) {
    const nextApplied: AppliedQualifier[] = [
      ...frame.applied.filter((item) => item.kind !== 'cardinality'),
      { kind: 'cardinality', optionId: 'specific', label: `Specific: ${label}`, value },
    ]

    if (frame.editing && frame.field.shape === 'link') {
      // Replace cardinality only; keep leaf qualifiers from editing column
      const updated = withReplacedQualifier(frame.editing, {
        kind: 'cardinality',
        optionId: 'specific',
        label: `Specific: ${label}`,
        value,
      })
      onReplaceColumn(frame.editing.id, updated)
      const optionIndex = frame.options.findIndex((item) => item.id === value)
      setActive(optionIndex >= 0 ? optionIndex : 0)
      return
    }

    if (frame.field.shape === 'link' && frame.field.link) {
      setStack((prev) => [
        ...prev,
        {
          kind: 'fields',
          fields: frame.field.link!.peerFields,
          label: frame.field.name,
          pathIds: frame.pathIds,
          pathNames: frame.pathNames,
          viaMany: true,
          linkQualifiers: nextApplied,
          crumbs: [...frame.crumbs.filter((c) => c !== 'Specific'), 'Specific'],
        },
      ])
      armIgnoreEnter()
      setQuery('')
      setActive(0)
      return
    }

    const col = makeColumn(frame.pathIds, nextApplied)
    if (!col) return
    onToggleColumn(col)
    const optionIndex = frame.options.findIndex((item) => item.id === value)
    setActive(optionIndex >= 0 ? optionIndex : 0)
  }

  function runActive() {
    if (ignoreEnter.current) return
    const row = rows[Math.min(active, Math.max(0, rows.length - 1))]
    row?.run()
  }

  function onKeyDown(event: ReactKeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      onClose()
      return
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActive((prev) => Math.min(prev + 1, Math.max(0, rows.length - 1)))
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActive((prev) => Math.max(prev - 1, 0))
      return
    }
    if (event.key === 'ArrowRight') {
      const row = rows[active]
      if (row?.drill) {
        event.preventDefault()
        row.run()
      }
      return
    }
    if (event.key === 'ArrowLeft' || (event.key === 'Backspace' && query === '' && stack.length > 1)) {
      event.preventDefault()
      pop()
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      event.stopPropagation()
      runActive()
    }
  }

  const selected = Math.min(active, Math.max(0, rows.length - 1))

  return (
    <div className="palette" data-testid="fields-palette" onMouseDown={onClose}>
      <Command
        shouldFilter={false}
        loop={false}
        disablePointerSelection
        value={rows[selected]?.id ?? ''}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDownCapture={onKeyDown}
      >
        <div className="palette-query">
          <span className="crumb" data-testid="fields-crumb" style={{ marginRight: 8, whiteSpace: 'nowrap' }}>
            {crumbParts.join(' › ')} ›
          </span>
          <CommandInput
            data-testid="fields-palette-input"
            autoFocus
            value={query}
            placeholder="Filter fields…"
            onValueChange={(value) => {
              setQuery(value)
              setActive(0)
            }}
          />
        </div>
        <CommandList id="fields-palette-list" data-testid="fields-palette-list">
          <div className="palette-group" role="listbox">
            {rows.map((row, index) => (
              <div
                key={`${top.kind}-${top.crumbs.join('/')}-${row.id}`}
                role="option"
                aria-selected={index === selected}
                className={index === selected ? 'hit on' : 'hit'}
                data-testid={`fields-row-${row.id}`}
                onMouseEnter={() => setActive(index)}
                onClick={(event) => {
                  event.preventDefault()
                  event.stopPropagation()
                  setActive(index)
                  row.run()
                }}
              >
                <span className="hit-copy">
                  <span className="hit-title">{row.title}</span>
                  {row.subtitle ? <small>{row.subtitle}</small> : null}
                </span>
                {row.checked ? <Check size={14} data-testid={`fields-check-${row.id}`} /> : null}
                {row.drill ? <span aria-hidden="true">›</span> : null}
              </div>
            ))}
          </div>
        </CommandList>
        <div className="palette-foot" data-testid="fields-palette-foot">
          <span>
            <Kbd>Enter</Kbd> toggle / open
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
        </div>
      </Command>
    </div>
  )
}
