/**
 * Shared field qualifier model for multi-value paths.
 *
 * Used by the field-picker prototype for column paths; the same types and
 * helpers are meant for filters, import mapping, and similar surfaces later.
 */

export type QualifierKind = 'cardinality' | 'language' | 'unit' | 'value_list' | 'range'

/** One choice inside a qualifier step. */
export type QualifierOption = {
  id: string
  label: string
  /** Opens a nested picker (e.g. Specific… → pick which article). */
  drill?: boolean
}

/** A step the user must resolve before a column is complete. */
export type QualifierStep = {
  kind: QualifierKind
  title: string
  options: QualifierOption[]
}

/** A resolved qualifier on a column path. */
export type AppliedQualifier = {
  kind: QualifierKind
  optionId: string
  label: string
  /** Concrete value for Specific… (article id/key, etc.). */
  value?: string
}

export type FieldValueShape = 'text' | 'number' | 'boolean' | 'language' | 'unit' | 'value_list' | 'range' | 'link'

export type FieldDef = {
  id: string
  name: string
  typeLabel: string
  shape: FieldValueShape
  /** When shape is link: peer fields and whether the relation is many. */
  link?: { many: boolean; peerFields: FieldDef[]; peerLabel: string }
  languages?: string[]
  units?: string[]
  valueListModes?: Array<'label' | 'code'>
  rangeParts?: Array<'min' | 'max'>
}

const CARDINALITY_STEP: QualifierStep = {
  kind: 'cardinality',
  title: 'How many linked rows',
  options: [
    { id: 'all', label: 'All, as list' },
    { id: 'first', label: 'First' },
    { id: 'specific', label: 'Specific…', drill: true },
  ],
}

/** Qualifier steps still needed for this field given qualifiers already applied. */
export function nextQualifierSteps(field: FieldDef, applied: AppliedQualifier[]): QualifierStep[] {
  const have = new Set(applied.map((item) => item.kind))
  const steps: QualifierStep[] = []
  if (field.shape === 'language' && !have.has('language')) {
    steps.push({
      kind: 'language',
      title: 'Language',
      options: (field.languages ?? ['en']).map((code) => ({ id: code, label: code.toUpperCase() })),
    })
  }
  if (field.shape === 'unit' && !have.has('unit')) {
    steps.push({
      kind: 'unit',
      title: 'Unit',
      options: (field.units ?? []).map((unit) => ({ id: unit, label: unit })),
    })
  }
  if (field.shape === 'value_list' && !have.has('value_list')) {
    const modes = field.valueListModes ?? ['label', 'code']
    steps.push({
      kind: 'value_list',
      title: 'Value list display',
      options: modes.map((mode) => ({
        id: mode,
        label: mode === 'label' ? 'Label' : 'Code',
      })),
    })
  }
  if (field.shape === 'range' && !have.has('range')) {
    const parts = field.rangeParts ?? ['min', 'max']
    steps.push({
      kind: 'range',
      title: 'Range part',
      options: parts.map((part) => ({
        id: part,
        label: part === 'min' ? 'Min' : 'Max',
      })),
    })
  }
  return steps
}

/** After drilling into a many-link, require cardinality before peer fields. */
export function cardinalityStepForLink(link: NonNullable<FieldDef['link']>): QualifierStep | null {
  if (!link.many) return null
  return CARDINALITY_STEP
}

/** Short labels used inside column title parentheses. */
export function qualifierShortLabel(q: AppliedQualifier): string {
  if (q.kind === 'cardinality') {
    if (q.optionId === 'all') return 'all'
    if (q.optionId === 'first') return 'first'
    if (q.optionId === 'specific') return q.value ? `specific:${q.value}` : 'specific'
  }
  return q.label.toLowerCase()
}

/** Column header: path segments joined with ›, qualifiers in parentheses. */
export function formatColumnTitle(pathNames: string[], qualifiers: AppliedQualifier[]): string {
  const base = pathNames.filter(Boolean).join(' › ')
  if (qualifiers.length === 0) return base
  const inside = qualifiers.map(qualifierShortLabel).join(', ')
  return `${base} (${inside})`
}

const SUMMARY_VISIBLE = 2

/** Compact multi-value summary: `A, B +N` (same idea as hop columns). */
export function summarizeList(values: string[]): { text: string; title: string } {
  const list = values.map((item) => String(item ?? '').trim()).filter(Boolean)
  if (list.length === 0) return { text: '', title: '' }
  if (list.length <= SUMMARY_VISIBLE) {
    const text = list.join(', ')
    return { text, title: text }
  }
  const shown = list.slice(0, SUMMARY_VISIBLE).join(', ')
  return { text: `${shown} +${list.length - SUMMARY_VISIBLE}`, title: list.join(', ') }
}

export type CellSource = {
  /** Scalar or language map / unit object / range object / value list code. */
  value?: unknown
  /** For many-links: peer rows already projected to the leaf field. */
  peers?: unknown[]
}

/**
 * Display a cell for a completed column path.
 * Cardinality all → list summary; first → first peer; specific → matching peer.
 */
export function formatCellValue(source: CellSource, field: FieldDef, qualifiers: AppliedQualifier[]): string {
  const card = qualifiers.find((item) => item.kind === 'cardinality')
  if (card && source.peers) {
    const projected = source.peers.map((peer) => projectLeaf(peer, field, qualifiers)).filter(Boolean)
    if (card.optionId === 'all') return summarizeList(projected).text
    if (card.optionId === 'first') return projected[0] ?? ''
    if (card.optionId === 'specific') {
      const key = card.value ?? ''
      const peer = source.peers.find((item) => peerKey(item) === key)
      return peer ? projectLeaf(peer, field, qualifiers) : ''
    }
  }
  return projectLeaf(source.value, field, qualifiers)
}

function peerKey(peer: unknown): string {
  if (!peer || typeof peer !== 'object') return ''
  const row = peer as Record<string, unknown>
  return String(row.key ?? row.id ?? '')
}

function projectLeaf(raw: unknown, field: FieldDef, qualifiers: AppliedQualifier[]): string {
  if (raw == null) return ''
  if (field.shape === 'language') {
    const lang = qualifiers.find((item) => item.kind === 'language')?.optionId ?? 'en'
    if (typeof raw === 'object' && !Array.isArray(raw)) {
      const map = raw as Record<string, unknown>
      const hit = map[lang] ?? Object.values(map)[0]
      return hit == null ? '' : String(hit)
    }
    return String(raw)
  }
  if (field.shape === 'unit') {
    const unit = qualifiers.find((item) => item.kind === 'unit')?.optionId
    if (typeof raw === 'object' && raw && !Array.isArray(raw)) {
      const obj = raw as { value?: unknown; unit?: unknown }
      if (unit && obj.unit != null && String(obj.unit) !== unit) {
        // Still show value; prototype does not convert units.
      }
      const value = obj.value ?? raw
      const u = unit ?? (obj.unit != null ? String(obj.unit) : '')
      return u ? `${value} ${u}` : String(value ?? '')
    }
    return String(raw)
  }
  if (field.shape === 'value_list') {
    const mode = qualifiers.find((item) => item.kind === 'value_list')?.optionId ?? 'label'
    if (typeof raw === 'object' && raw && !Array.isArray(raw)) {
      const obj = raw as { code?: unknown; label?: unknown }
      if (mode === 'code') return String(obj.code ?? '')
      return String(obj.label ?? obj.code ?? '')
    }
    return String(raw)
  }
  if (field.shape === 'range') {
    const part = qualifiers.find((item) => item.kind === 'range')?.optionId ?? 'min'
    if (typeof raw === 'object' && raw && !Array.isArray(raw)) {
      const obj = raw as { min?: unknown; max?: unknown }
      return String(part === 'max' ? obj.max ?? '' : obj.min ?? '')
    }
    return String(raw)
  }
  if (Array.isArray(raw)) return summarizeList(raw.map(String)).text
  return String(raw)
}

export function isColumnComplete(field: FieldDef, applied: AppliedQualifier[], viaManyLink: boolean): boolean {
  if (viaManyLink && !applied.some((item) => item.kind === 'cardinality')) return false
  if (viaManyLink) {
    const card = applied.find((item) => item.kind === 'cardinality')
    if (card?.optionId === 'specific' && !card.value) return false
  }
  return nextQualifierSteps(field, applied).length === 0
}
