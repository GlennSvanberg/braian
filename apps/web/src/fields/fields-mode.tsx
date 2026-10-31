import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { FieldPicker } from './field-picker'
import {
  cardinalityCrumbLabel,
  cardinalityStepForLink,
  nextQualifierSteps,
  qualifierShortLabel,
  type AppliedQualifier,
  type FieldDef,
  type QualifierOption,
  type QualifierStep,
} from './qualifiers'
import { makeViewColumn, withReplacedQualifier, type ViewColumn } from './columns'

export type PeerPickOption = {
  id: string
  label: string
  subtitle: string
  pickValue: string
  shortLabel: string
}


export type FieldsPaletteOpen =
  | { kind: 'browse' }
  | { kind: 'edit'; column: ViewColumn }

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
  editing?: ViewColumn
}

type SpecificFrame = {
  kind: 'specific'
  options: PeerPickOption[]
  field: FieldDef
  pathIds: string[]
  pathNames: string[]
  viaMany: boolean
  applied: AppliedQualifier[]
  pendingLeaf: FieldDef
  crumbs: string[]
  editing?: ViewColumn
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
  columns: ViewColumn[]
  /** Root field catalog for the active object type. */
  fields: FieldDef[]
  onClose: () => void
  onToggleColumn: (column: ViewColumn) => void
  /** Completing a qualify/specific path: show only, never hide if already present. */
  onShowColumn: (column: ViewColumn) => void
  onReplaceColumn: (previousId: string, next: ViewColumn) => void
  /** Linked peers currently visible in the table (preferred in Specific…). */
  peerOptionsForField: (leafId: string, query: string) => PeerPickOption[]
  /** Server search for Specific… (debounced). */
  searchPeersForField?: (leafId: string, query: string) => Promise<PeerPickOption[]>
}

function rootFrame(fields: FieldDef[]): FieldsFrame {
  return {
    kind: 'fields',
    fields,
    label: 'Fields',
    pathIds: [],
    pathNames: [],
    viaMany: false,
    linkQualifiers: [],
    crumbs: [],
  }
}

/** Open Edit field on the column's first qualifier step with current choice marked. */
function framesForEdit(column: ViewColumn, catalog: FieldDef[]): StackFrame[] {
  const root = rootFrame(catalog)
  let fields = catalog
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

function pathMatches(col: ViewColumn, pathIds: string[]): boolean {
  if (col.fieldPath.length < pathIds.length) return false
  return pathIds.every((id, index) => col.fieldPath[index] === id)
}

/** A required qualifier without `value` matches any value (e.g. Specific before the row is picked). */
function qualifiersMatch(col: ViewColumn, required: AppliedQualifier[]): boolean {
  return required.every((req) => {
    const hit = col.qualifiers.find((q) => q.kind === req.kind)
    if (!hit || hit.optionId !== req.optionId) return false
    if (req.value != null && (hit.value ?? '') !== req.value) return false
    return true
  })
}

function columnsUnder(columns: ViewColumn[], pathIds: string[], required: AppliedQualifier[]): ViewColumn[] {
  return columns.filter((col) => pathMatches(col, pathIds) && qualifiersMatch(col, required))
}

/** Subtitle for a checked row: which sub-choice is active, e.g. `Description — sv`. */
function activeChoiceSubtitle(
  field: FieldDef,
  depth: number,
  matches: ViewColumn[],
  frameQualifiers: AppliedQualifier[],
): string {
  const known = new Set(frameQualifiers.map((q) => `${q.kind}:${q.optionId}`))
  const parts = matches.map((col) => {
    const rest = col.pathNames.slice(depth + 1).join(' › ')
    const quals = col.qualifiers
      .filter((q) => !known.has(`${q.kind}:${q.optionId}`))
      .map(qualifierShortLabel)
      .join(', ')
    if (rest && quals) return `${rest} (${quals})`
    return rest || quals
  })
  const unique = [...new Set(parts.filter(Boolean))]
  return unique.length > 0 ? `${field.name} — ${unique.join('; ')}` : ''
}

/**
 * Fields › mode for the Ctrl+K palette (prototype).
 * Enter toggles/applies and keeps the palette on the same level; Esc closes.
 */
export function FieldsMode({ open, columns, fields: catalog, onClose, onToggleColumn, onShowColumn, onReplaceColumn, peerOptionsForField, searchPeersForField }: Props) {
  const initial = open.kind === 'edit' ? framesForEdit(open.column, catalog) : [rootFrame(catalog)]
  const [stack, setStack] = useState<StackFrame[]>(initial)
  const [query, setQuery] = useState('')
  // Survives Backspace/pop so Edit field cannot accidentally onToggle a duplicate.
  const [editingColumn, setEditingColumn] = useState<ViewColumn | null>(open.kind === 'edit' ? open.column : null)
  const [remotePeers, setRemotePeers] = useState<PeerPickOption[]>([])
  const columnsRef = useRef(columns)
  columnsRef.current = columns
  /** Arm outside-dismiss after mount so the opening click cannot close us (Playwright retry / sync paint). */
  const dismissArmed = useRef(false)
  useEffect(() => {
    dismissArmed.current = false
    const timer = window.setTimeout(() => {
      dismissArmed.current = true
    }, 100)
    return () => {
      window.clearTimeout(timer)
      dismissArmed.current = false
    }
  }, [])
  useEffect(() => {
    // Parent clears fieldsEditColumnId after mount (open becomes browse). Keep editing.
    if (open.kind === 'edit') {
      setEditingColumn(open.column)
      // Ensure keyboard focus lands in the palette, not the table under the context menu.
      window.requestAnimationFrame(() => {
        document.querySelector<HTMLInputElement>('[data-testid="fields-mode-input"]')?.focus()
      })
    }
  }, [open])

  /** Index of the option/field that already has a column — used when drilling so focus lands on the check. */
  function focusIndexForQualify(
    step: QualifierStep,
    pathIds: string[],
    applied: AppliedQualifier[],
    editing?: ViewColumn,
  ): number {
    const cols = columnsRef.current
    if (editing) {
      const current = editing.qualifiers.find((q) => q.kind === step.kind)
      if (current) {
        const idx = step.options.findIndex((option) => option.id === current.optionId)
        if (idx >= 0) return idx
      }
      return 0
    }
    for (let i = 0; i < step.options.length; i += 1) {
      const option = step.options[i]!
      const hits = columnsUnder(cols, pathIds, [
        ...applied.filter(
          (q) =>
            q.kind !== step.kind &&
            !(q.kind === 'cardinality' && q.optionId === 'specific' && !q.value),
        ),
        { kind: step.kind, optionId: option.id, label: option.label },
      ])
      if (hits.length > 0) return i
    }
    return 0
  }

  function focusIndexForSpecific(
    options: PeerPickOption[],
    pathIds: string[],
    applied: AppliedQualifier[],
  ): number {
    const cols = columnsRef.current
    for (let i = 0; i < options.length; i += 1) {
      const option = options[i]!
      const hits = columnsUnder(cols, pathIds, [
        ...applied.filter((q) => q.kind !== 'cardinality'),
        {
          kind: 'cardinality',
          optionId: 'specific',
          label: option.shortLabel,
          value: option.pickValue,
        },
      ])
      if (hits.length > 0) return i
    }
    return 0
  }

  function focusIndexForFields(
    fields: FieldDef[],
    pathIds: string[],
    linkQualifiers: AppliedQualifier[],
  ): number {
    const cols = columnsRef.current
    for (let i = 0; i < fields.length; i += 1) {
      const field = fields[i]!
      if (columnsUnder(cols, [...pathIds, field.id], linkQualifiers).length > 0) return i
    }
    return 0
  }

  const [active, setActive] = useState(() => {
    const top0 = initial[initial.length - 1]
    if (!top0) return 0
    if (top0.kind === 'qualify') {
      return focusIndexForQualify(top0.step, top0.pathIds, top0.applied, top0.editing)
    }
    if (top0.kind === 'specific') {
      return focusIndexForSpecific(top0.options, top0.pathIds, top0.applied)
    }
    if (top0.kind === 'fields' && top0.pathIds.length > 0) {
      return focusIndexForFields(top0.fields, top0.pathIds, top0.linkQualifiers)
    }
    return 0
  })

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
          const matches = columnsUnder(cols, childPath, top.linkQualifiers)
          const checked = matches.length > 0
          const choice = checked && drill ? activeChoiceSubtitle(field, top.pathIds.length, matches, top.linkQualifiers) : ''
          return {
            id: field.id,
            title: field.name,
            subtitle: choice || field.typeLabel,
            drill,
            checked,
            run: () => activateField(field, top),
          }
        })
    }

    if (top.kind === 'qualify') {
      const cardinalityDrills =
        top.step.kind === 'cardinality' && top.field.shape === 'link' && Boolean(top.field.link?.many)
      return top.step.options
        .filter((option) => !needle || option.label.toLowerCase().includes(needle))
        .map((option) => {
          const drill = cardinalityDrills || Boolean(option.drill)
          let checked = false
          const editingHit = editingColumn ?? top.editing
          if (editingHit) {
            // Edit field…: check must reflect THIS column only, not sibling hop columns.
            const current = editingHit.qualifiers.find((q) => q.kind === top.step.kind)
            checked = current?.optionId === option.id
          } else {
            checked =
              columnsUnder(cols, top.pathIds, [
                ...top.applied.filter(
                  (q) =>
                    q.kind !== top.step.kind &&
                    !(q.kind === 'cardinality' && q.optionId === 'specific' && !q.value),
                ),
                { kind: top.step.kind, optionId: option.id, label: option.label },
              ]).length > 0
          }
          return {
            id: option.id,
            title: option.label,
            subtitle: option.subtitle ?? '',
            drill,
            checked,
            run: () => activateQualifier(option, top),
          }
        })
    }

    // Linked peers from the current view first, then debounced server hits.
    const linked = peerOptionsForField(top.pendingLeaf.id, query)
    const seen = new Set(linked.map((option) => option.id))
    const merged = [...linked]
    for (const option of remotePeers) {
      if (seen.has(option.id)) continue
      seen.add(option.id)
      merged.push(option)
    }
    return merged.map((option) => ({
      id: option.id,
      title: option.label,
      subtitle: option.subtitle,
      checked:
        columnsUnder(cols, top.pathIds, [
          ...top.applied.filter((q) => q.kind !== 'cardinality'),
          {
            kind: 'cardinality',
            optionId: 'specific',
            label: option.shortLabel,
            value: option.pickValue,
          },
        ]).length > 0,
      run: () => activateSpecific(option.pickValue, option.shortLabel, top),
    }))
  }, [top, query, columns, peerOptionsForField, remotePeers, editingColumn])

  useEffect(() => {
    if (top.kind !== 'specific' || !searchPeersForField) {
      setRemotePeers([])
      return
    }
    let cancelled = false
    const handle = window.setTimeout(() => {
      void searchPeersForField(top.pendingLeaf.id, query)
        .then((options) => {
          if (!cancelled) setRemotePeers(options)
        })
        .catch(() => {
          if (!cancelled) setRemotePeers([])
        })
    }, 200)
    return () => {
      cancelled = true
      window.clearTimeout(handle)
    }
  }, [top, query, searchPeersForField])


  function restoreIndexAfterPop(parent: StackFrame, leaving: StackFrame): number {
    if (parent.kind === 'fields') {
      const focusId =
        leaving.kind === 'qualify'
          ? leaving.pathIds[leaving.pathIds.length - 1]
          : leaving.kind === 'fields'
            ? leaving.pathIds[leaving.pathIds.length - 1]
            : leaving.kind === 'specific'
              ? leaving.field.id
              : undefined
      const idx = focusId ? parent.fields.findIndex((field) => field.id === focusId) : -1
      return idx >= 0 ? idx : 0
    }
    if (parent.kind === 'qualify') {
      if (leaving.kind === 'fields') {
        const card = leaving.linkQualifiers.find((q) => q.kind === 'cardinality')
        const idx = parent.step.options.findIndex((option) => option.id === card?.optionId)
        return idx >= 0 ? idx : 0
      }
      if (leaving.kind === 'specific') {
        const idx = parent.step.options.findIndex((option) => option.id === 'specific')
        return idx >= 0 ? idx : 0
      }
      if (leaving.kind === 'qualify') {
        // Unit/language step under a leaf: highlight the leaf field option is N/A on qualify parent
        return 0
      }
    }
    if (parent.kind === 'specific' && leaving.kind === 'qualify') {
      const gtin = leaving.applied.find((q) => q.kind === 'cardinality')?.value
      const idx = parent.options.findIndex((option) => option.pickValue === gtin || option.id === gtin)
      return idx >= 0 ? idx : 0
    }
    return 0
  }

  function pop() {
    if (stack.length <= 1) return
    const leaving = stack[stack.length - 1]!
    const parent = stack[stack.length - 2]!
    const restore = restoreIndexAfterPop(parent, leaving)
    setStack((prev) => prev.slice(0, -1))
    setQuery('')
    setActive(restore)
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
        setActive(focusIndexForQualify(card, pathIds, frame.linkQualifiers))
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
      setQuery('')
      setActive(focusIndexForFields(field.link!.peerFields, pathIds, frame.linkQualifiers))
      return
    }

    const pathIds = [...frame.pathIds, field.id]
    const pathNames = [...frame.pathNames, field.name]
    const crumbs = [...frame.crumbs, field.name]
    const pendingSpecific = frame.linkQualifiers.find(
      (q) => q.kind === 'cardinality' && q.optionId === 'specific' && !q.value,
    )
    if (pendingSpecific) {
      // Field first, then the linked row: only rows that have this field are listed.
      setStack((prev) => [
        ...prev,
        {
          kind: 'specific',
          options: peerOptionsForField(field.id, ''),
          field,
          pathIds,
          pathNames,
          viaMany: true,
          applied: frame.linkQualifiers,
          pendingLeaf: field,
          crumbs,
        },
      ])
      setQuery('')
      setActive(
        focusIndexForSpecific(peerOptionsForField(field.id, ''), pathIds, frame.linkQualifiers),
      )
      return
    }
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
      setQuery('')
      setActive(focusIndexForQualify(steps[0]!, pathIds, frame.linkQualifiers))
      return
    }

    const col = makeViewColumn(pathIds, frame.linkQualifiers, catalog)
    if (col) {
      if (editingColumn) {
        onReplaceColumn(editingColumn.id, col)
        setEditingColumn(col)
        // Edit field may change the row label; keep the filter so Enter repeats.
      } else {
        onToggleColumn(col)
      }
      // Keep filter + highlight so Enter, Enter toggles the same field.
      // (Re-adding still appends last in the table — Glenn's rule — but the list stays put.)
      const needle = query.trim().toLowerCase()
      const filtered = frame.fields.filter(
        (item) => !needle || `${item.name} ${item.typeLabel}`.toLowerCase().includes(needle),
      )
      const at = filtered.findIndex((item) => item.id === field.id)
      setActive(at >= 0 ? at : 0)
    }
  }

  function activateQualifier(option: QualifierOption, frame: QualifyFrame) {
    if (frame.step.kind === 'cardinality' && option.id === 'specific') {
      if (frame.editing) {
        // Edit field: the leaf is known, go straight to the linked-row list.
        const leaf = frame.editing.leaf
        setStack((prev) => [
          ...prev,
          {
            kind: 'specific',
            options: peerOptionsForField(leaf.id, ''),
            field: leaf,
            pathIds: frame.editing!.fieldPath,
            pathNames: frame.editing!.pathNames,
            viaMany: true,
            applied: frame.editing!.qualifiers,
            pendingLeaf: leaf,
            crumbs: [...frame.crumbs, 'Specific'],
            editing: editingColumn ?? frame.editing,
          },
        ])
        setQuery('')
        setActive(
          focusIndexForSpecific(
            peerOptionsForField(leaf.id, ''),
            frame.editing!.fieldPath,
            frame.editing!.qualifiers,
          ),
        )
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
            linkQualifiers: [
              ...frame.applied.filter((q) => q.kind !== 'cardinality'),
              { kind: 'cardinality', optionId: 'specific', label: 'Specific' },
            ],
            crumbs: [...frame.crumbs, 'Specific'],
          },
        ])
        setQuery('')
        setActive(
          focusIndexForFields(frame.field.link!.peerFields, frame.pathIds, [
            ...frame.applied.filter((q) => q.kind !== 'cardinality'),
            { kind: 'cardinality', optionId: 'specific', label: 'Specific' },
          ]),
        )
        return
      }
    }

    const nextApplied: AppliedQualifier[] = [
      ...frame.applied.filter((item) => item.kind !== frame.step.kind),
      { kind: frame.step.kind, optionId: option.id, label: option.label },
    ]

    // Edit mode: do not commit on intermediate qualifier picks — drill so
    // First › Net weight › kg can replace in one final step after Backspace.
    // editingColumn state survives pop; frame.editing is set when opened via Edit field.

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
      setQuery('')
      setActive(focusIndexForFields(frame.field.link!.peerFields, frame.pathIds, nextApplied))
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
      setQuery('')
      setActive(focusIndexForQualify(remaining[0]!, frame.pathIds, nextApplied))
      return
    }

    // Completing a column.
    const col = makeViewColumn(frame.pathIds, nextApplied, catalog)
    if (!col) return
    const editing = editingColumn ?? frame.editing
    if (editing) {
      onReplaceColumn(editing.id, col)
      setEditingColumn(col)
      setQuery('')
      const optionIndex = frame.step.options.findIndex((item) => item.id === option.id)
      setActive(optionIndex >= 0 ? optionIndex : 0)
      return
    }
    // Always toggle (not show-only): Enter, Enter must turn nested hops and
    // composites (value_list Label/Code, unit, range, language) off again.
    // Root cause of "Label stuck on": onShowColumn never hid; isChecked stayed true.
    onToggleColumn(col)
    // After a language pick, return to the fields list so fields-check-${fieldId} shows.
    // Keep filter when staying on this qualify list (value_list / unit / …).
    if (frame.step.kind === 'language') {
      setQuery('')
      const fieldId = frame.pendingLeaf.id
      let fieldsFrame: Extract<(typeof stack)[number], { kind: 'fields' }> | null = null
      let cut = -1
      for (let i = stack.length - 1; i >= 0; i -= 1) {
        const frameAt = stack[i]!
        if (frameAt.kind === 'fields' && frameAt.fields.some((item) => item.id === fieldId)) {
          fieldsFrame = frameAt
          cut = i
          break
        }
      }
      if (cut >= 0) setStack((prev) => prev.slice(0, cut + 1))
      if (fieldsFrame) {
        const at = fieldsFrame.fields.findIndex((item) => item.id === fieldId)
        setActive(at >= 0 ? at : 0)
      }
      return
    }
    // Keep filter + highlight so Enter, Enter toggles the same nested option.
    const needle = query.trim().toLowerCase()
    const filtered = frame.step.options.filter(
      (item) => !needle || item.label.toLowerCase().includes(needle),
    )
    const at = filtered.findIndex((item) => item.id === option.id)
    setActive(at >= 0 ? at : 0)
  }

  function activateSpecific(value: string, label: string, frame: SpecificFrame) {
    const chosen: AppliedQualifier = { kind: 'cardinality', optionId: 'specific', label, value }
    const nextApplied: AppliedQualifier[] = [...frame.applied.filter((item) => item.kind !== 'cardinality'), chosen]
    const optionIndex = frame.options.findIndex((item) => item.pickValue === value || item.id === value)

    const editingSpecific = editingColumn ?? frame.editing
    if (editingSpecific) {
      const updated = withReplacedQualifier(editingSpecific, chosen, catalog)
      onReplaceColumn(editingSpecific.id, updated)
      setEditingColumn(updated)
      setQuery('')
      setActive(optionIndex >= 0 ? optionIndex : 0)
      return
    }

    // Link qualifier first (row), then the field's own qualifiers (e.g. unit)
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
          viaMany: true,
          applied: nextApplied,
          pendingLeaf: frame.pendingLeaf,
          crumbs: [...frame.crumbs, label],
        },
      ])
      setQuery('')
      setActive(focusIndexForQualify(remaining[0]!, frame.pathIds, nextApplied))
      return
    }

    const col = makeViewColumn(frame.pathIds, nextApplied, catalog)
    if (!col) return
    if (editingColumn) {
      onReplaceColumn(editingColumn.id, col)
      setEditingColumn(col)
      setQuery('')
      setActive(optionIndex >= 0 ? optionIndex : 0)
      return
    }
    // Nested Specific… finals toggle both ways; keep filter for Enter, Enter.
    onToggleColumn(col)
    const needle = query.trim().toLowerCase()
    const filtered = frame.options.filter(
      (item) =>
        !needle ||
        `${item.label} ${item.subtitle ?? ''}`.toLowerCase().includes(needle),
    )
    const at = filtered.findIndex((item) => item.pickValue === value || item.id === value)
    setActive(at >= 0 ? at : optionIndex >= 0 ? optionIndex : 0)
  }

  function runActive() {
    const row = rows[Math.min(active, Math.max(0, rows.length - 1))]
    row?.run()
  }

  function onKeyDown(event: ReactKeyboardEvent) {
    // Always stop bubbling so table / shell shortcuts (New product, /, …) cannot fire.
    event.stopPropagation()
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
      // Drill into any row that opens a deeper level (including All/First/Specific).
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
      runActive()
    }
  }

  return (
    <FieldPicker
      mode="columns"
      crumbs={crumbParts}
      query={query}
      onQuery={setQuery}
      rows={rows}
      active={active}
      onActive={setActive}
      onKeyDown={onKeyDown}
      levelKey={`${top.kind}-${top.crumbs.join('/')}`}
      onBackdrop={(event) => {
        if (!dismissArmed.current) return
        if (event.target === event.currentTarget) onClose()
      }}
    />
  )}
