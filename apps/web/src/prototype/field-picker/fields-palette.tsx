import { useMemo, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { Check } from 'lucide-react'
import { Command, CommandInput, CommandItem, CommandList } from '../../components/ui/command'
import { Kbd } from '../../components/ui/kbd'
import {
  cardinalityStepForLink,
  nextQualifierSteps,
  type AppliedQualifier,
  type FieldDef,
  type QualifierOption,
  type QualifierStep,
} from './qualifiers'
import { PRODUCT_FIELDS } from './mock-catalog'
import { makeColumn, type ProtoColumn } from './columns'

export type FieldsPaletteOpen =
  | { kind: 'browse' }
  | { kind: 'edit'; column: ProtoColumn }

type StackFrame =
  | { kind: 'fields'; fields: FieldDef[]; label: string; pathIds: string[]; pathNames: string[]; viaMany: boolean; linkQualifiers: AppliedQualifier[] }
  | { kind: 'qualify'; step: QualifierStep; field: FieldDef; pathIds: string[]; pathNames: string[]; viaMany: boolean; applied: AppliedQualifier[]; pendingLeaf: FieldDef }
  | { kind: 'specific'; options: Array<{ id: string; label: string }>; field: FieldDef; pathIds: string[]; pathNames: string[]; viaMany: boolean; applied: AppliedQualifier[]; pendingLeaf: FieldDef }

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
  /** Article keys available for Specific… (union across mock products). */
  specificOptions: Array<{ id: string; label: string }>
  onClose: () => void
  onToggleColumn: (column: ProtoColumn) => void
  onReplaceColumn: (previousId: string, next: ProtoColumn) => void
}

function rootFrame(): StackFrame {
  return {
    kind: 'fields',
    fields: PRODUCT_FIELDS,
    label: 'Fields',
    pathIds: [],
    pathNames: [],
    viaMany: false,
    linkQualifiers: [],
  }
}

function framesForEdit(column: ProtoColumn): StackFrame[] {
  // Open on the column's qualifier step (last applied kind) so Edit field can swap First ↔ All.
  const frames: StackFrame[] = [rootFrame()]
  let fields = PRODUCT_FIELDS
  let viaMany = false
  const pathIds: string[] = []
  const pathNames: string[] = []
  let linkQualifiers: AppliedQualifier[] = []

  for (let i = 0; i < column.fieldPath.length; i += 1) {
    const id = column.fieldPath[i]!
    const field = fields.find((item) => item.id === id)
    if (!field) break
    pathIds.push(field.id)
    pathNames.push(field.name)
    if (field.shape === 'link' && field.link) {
      const card = cardinalityStepForLink(field.link)
      if (card && field.link.many) {
        viaMany = true
        const appliedCard = column.qualifiers.find((q) => q.kind === 'cardinality')
        frames.push({
          kind: 'qualify',
          step: card,
          field,
          pathIds: [...pathIds],
          pathNames: [...pathNames],
          viaMany,
          applied: appliedCard ? [appliedCard] : [],
          pendingLeaf: field,
        })
        linkQualifiers = appliedCard ? [appliedCard] : []
        if (appliedCard?.optionId === 'specific') {
          // stay editable at cardinality; user can re-pick
        }
      }
      fields = field.link.peerFields
      frames.push({
        kind: 'fields',
        fields,
        label: field.name,
        pathIds: [...pathIds],
        pathNames: [...pathNames],
        viaMany,
        linkQualifiers: [...linkQualifiers],
      })
    } else {
      const leaf = field
      const fieldQs = column.qualifiers.filter((q) => q.kind !== 'cardinality')
      const steps = nextQualifierSteps(leaf, [])
      // Jump to first qualifier step for this leaf (or cardinality already shown)
      if (steps.length > 0) {
        frames.push({
          kind: 'qualify',
          step: steps[0]!,
          field: leaf,
          pathIds: [...pathIds],
          pathNames: [...pathNames],
          viaMany,
          applied: [...linkQualifiers, ...fieldQs],
          pendingLeaf: leaf,
        })
      }
    }
  }
  // Prefer the deepest qualify frame for Edit field…
  return frames
}

/**
 * Fields › mode for the Ctrl+K palette (prototype).
 * Enter toggles a completed column and keeps the palette open; Esc closes.
 * ArrowRight / Enter drills into links and qualifier steps; Backspace / ArrowLeft goes up.
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
  const editingId = open.kind === 'edit' ? open.column.id : null

  const top = stack[stack.length - 1]!
  const crumb = stack.map((frame) => {
    if (frame.kind === 'fields') return frame.label
    if (frame.kind === 'qualify') return frame.step.title
    return 'Specific'
  })

  const columnIds = useMemo(() => new Set(columns.map((col) => col.id)), [columns])

  const rows: Row[] = useMemo(() => {
    const needle = query.trim().toLowerCase()
    if (top.kind === 'fields') {
      return top.fields
        .filter((field) => !needle || `${field.name} ${field.typeLabel}`.toLowerCase().includes(needle))
        .map((field) => {
          const isLink = field.shape === 'link'
          let checked = false
          if (!isLink) {
            const candidate = makeColumn([...top.pathIds, field.id], top.linkQualifiers)
            if (candidate && nextQualifierSteps(field, top.linkQualifiers).length === 0) {
              checked = columnIds.has(candidate.id)
            }
          }
          return {
            id: field.id,
            title: isLink || nextQualifierSteps(field, top.linkQualifiers).length > 0 ? `${field.name} ›` : field.name,
            subtitle: field.typeLabel,
            drill: isLink || nextQualifierSteps(field, top.linkQualifiers).length > 0,
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
          title: option.drill ? `${option.label}` : option.label,
          subtitle: top.step.title,
          drill: Boolean(option.drill),
          run: () => activateQualifier(option, top),
        }))
    }
    return top.options
      .filter((option) => !needle || option.label.toLowerCase().includes(needle))
      .map((option) => ({
        id: option.id,
        title: option.label,
        subtitle: 'Specific row',
        run: () => activateSpecific(option.id, option.label, top),
      }))
  }, [top, query, columnIds])

  function push(frame: StackFrame) {
    setStack((prev) => [...prev, frame])
    setQuery('')
    setActive(0)
  }

  function pop() {
    setStack((prev) => (prev.length > 1 ? prev.slice(0, -1) : prev))
    setQuery('')
    setActive(0)
  }

  /** After adding/toggling a leaf column, return to the nearest fields list. */
  function returnToFields() {
    setStack((prev) => {
      let end = prev.length - 1
      while (end > 0 && prev[end]?.kind !== 'fields') end -= 1
      return prev.slice(0, end + 1)
    })
    setQuery('')
    setActive(0)
  }

  function activateField(field: FieldDef, frame: Extract<StackFrame, { kind: 'fields' }>) {
    if (field.shape === 'link' && field.link) {
      const card = cardinalityStepForLink(field.link)
      const pathIds = [...frame.pathIds, field.id]
      const pathNames = [...frame.pathNames, field.name]
      if (card) {
        push({
          kind: 'qualify',
          step: card,
          field,
          pathIds,
          pathNames,
          viaMany: true,
          applied: [...frame.linkQualifiers],
          pendingLeaf: field,
        })
        return
      }
      push({
        kind: 'fields',
        fields: field.link.peerFields,
        label: field.name,
        pathIds,
        pathNames,
        viaMany: frame.viaMany,
        linkQualifiers: frame.linkQualifiers,
      })
      return
    }
    const pathIds = [...frame.pathIds, field.id]
    const pathNames = [...frame.pathNames, field.name]
    const steps = nextQualifierSteps(field, frame.linkQualifiers)
    if (steps.length > 0) {
      push({
        kind: 'qualify',
        step: steps[0]!,
        field,
        pathIds,
        pathNames,
        viaMany: frame.viaMany,
        applied: [...frame.linkQualifiers],
        pendingLeaf: field,
      })
      return
    }
    const col = makeColumn(pathIds, frame.linkQualifiers)
    if (col) {
      onToggleColumn(col)
      // stay on current fields list
    }
  }

  function activateQualifier(option: QualifierOption, frame: Extract<StackFrame, { kind: 'qualify' }>) {
    if (option.drill) {
      push({
        kind: 'specific',
        options: specificOptions,
        field: frame.field,
        pathIds: frame.pathIds,
        pathNames: frame.pathNames,
        viaMany: frame.viaMany,
        applied: frame.applied,
        pendingLeaf: frame.pendingLeaf,
      })
      return
    }
    const nextApplied: AppliedQualifier[] = [
      ...frame.applied.filter((item) => item.kind !== frame.step.kind),
      { kind: frame.step.kind, optionId: option.id, label: option.label },
    ]
    // After cardinality on a many-link, drill into peer fields
    if (frame.step.kind === 'cardinality' && frame.field.shape === 'link' && frame.field.link) {
      push({
        kind: 'fields',
        fields: frame.field.link.peerFields,
        label: frame.field.name,
        pathIds: frame.pathIds,
        pathNames: frame.pathNames,
        viaMany: true,
        linkQualifiers: nextApplied,
      })
      return
    }
    const remaining = nextQualifierSteps(frame.pendingLeaf, nextApplied)
    if (remaining.length > 0) {
      push({
        kind: 'qualify',
        step: remaining[0]!,
        field: frame.pendingLeaf,
        pathIds: frame.pathIds,
        pathNames: frame.pathNames,
        viaMany: frame.viaMany,
        applied: nextApplied,
        pendingLeaf: frame.pendingLeaf,
      })
      return
    }
    const col = makeColumn(frame.pathIds, nextApplied)
    if (!col) return
    if (editingId) {
      onReplaceColumn(editingId, col)
      onClose()
      return
    }
    onToggleColumn(col)
    returnToFields()
  }

  function activateSpecific(
    value: string,
    label: string,
    frame: Extract<StackFrame, { kind: 'specific' }>,
  ) {
    const nextApplied: AppliedQualifier[] = [
      ...frame.applied.filter((item) => item.kind !== 'cardinality'),
      { kind: 'cardinality', optionId: 'specific', label: `Specific: ${label}`, value },
    ]
    if (frame.field.shape === 'link' && frame.field.link) {
      push({
        kind: 'fields',
        fields: frame.field.link.peerFields,
        label: frame.field.name,
        pathIds: frame.pathIds,
        pathNames: frame.pathNames,
        viaMany: true,
        linkQualifiers: nextApplied,
      })
      return
    }
    const col = makeColumn(frame.pathIds, nextApplied)
    if (!col) return
    if (editingId) {
      onReplaceColumn(editingId, col)
      onClose()
      return
    }
    onToggleColumn(col)
    returnToFields()
  }

  function onKeyDown(event: ReactKeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault()
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
      rows[active]?.run()
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
            {crumb.join(' › ')} ›
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
          <div className="palette-group">
            {rows.map((row, index) => (
              <CommandItem
                key={row.id}
                value={row.id}
                className={index === selected ? 'hit on' : 'hit'}
                onPointerDown={() => setActive(index)}
                onSelect={() => row.run()}
                data-testid={`fields-row-${row.id}`}
              >
                <span className="hit-copy">
                  <span className="hit-title">{row.title}</span>
                  {row.subtitle ? <small>{row.subtitle}</small> : null}
                </span>
                {row.checked ? <Check size={14} data-testid="fields-check" /> : null}
                {row.drill ? <span aria-hidden="true">›</span> : null}
              </CommandItem>
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

