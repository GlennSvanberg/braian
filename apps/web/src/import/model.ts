/** Pure import helpers. The screen and the tests share these. */

import { textValue } from '../api'

/** Draft/preview cells use plain strings; `before` for measures is often `{ value, unit? }`. */
export function importCellText(value: unknown): string {
  if (value == null) return ''
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  return textValue(value)
}

export type ColumnLine = {
  col: number
  target: string
  confidence: number
  source: string
  low?: boolean
  reason?: string | null
  alternatives?: Array<{ value: string; p: number }>
}

export type ImportPhase = 'empty' | 'uploading' | 'mapping' | 'preview' | 'saving' | 'done' | 'error'

export function acceptsFile(name: string): boolean {
  return /\.(xlsx|xls|csv|tsv|txt)$/i.test(name)
}

export function isFileDrag(types: Iterable<string>): boolean {
  return Array.from(types).includes('Files')
}

export function firstDroppedFile(names: string[]): { name: string; notice: string } | { error: string } {
  const [name, ...rest] = names
  if (!name) return { error: 'Only .xlsx, .xls and .csv can be imported' }
  if (!acceptsFile(name)) return { error: 'Only .xlsx, .xls and .csv can be imported' }
  const notice = rest.length > 0 ? `One file at a time — importing ${name}` : ''
  return { name, notice }
}

export function confidenceLabel(source: string, confidence: number): string {
  if (source === 'manual') return '✓'
  const percent = Math.round(Math.max(0, Math.min(1, confidence)) * 100)
  return `${percent} %`
}

export function lineTone(column: ColumnLine): 'manual' | 'warn' | 'suggest' | 'ok' {
  if (!column.target || column.target === 'unmapped') return 'ok'
  if (column.source === 'manual') return 'manual'
  if (column.low || column.confidence < 0.6) return 'warn'
  if (column.reason === 'low_confidence') return 'suggest'
  return 'ok'
}

export function mappedLines(columns: ColumnLine[]): ColumnLine[] {
  return columns.filter((column) => column.target && column.target !== 'unmapped')
}

export function matchBanner(columns: ColumnLine[]): { matched: number; total: number; marked: number } {
  const total = columns.length
  const matched = columns.filter((column) => column.target !== 'unmapped').length
  const marked = columns.filter(
    (column) => column.target !== 'unmapped' && (column.low || column.confidence < 0.6),
  ).length
  return { matched, total, marked }
}

export function matchSummary(columns: ColumnLine[]): string {
  const { matched, total, marked } = matchBanner(columns)
  if (total === 0) return 'No columns in this file.'
  if (marked === 0 && matched === total) {
    return total === 1 ? 'The column is mapped.' : `All ${total} columns are mapped.`
  }
  if (marked === 0) return `${matched} of ${total} columns are mapped.`
  const look = marked === 1 ? '1 needs a look' : `${marked} need a look`
  return `${matched} of ${total} columns are mapped. ${look}.`
}

export function mappingRank(column: ColumnLine | undefined): number {
  if (!column || column.target === 'unmapped') return 0
  if (column.low || column.confidence < 0.6) return 1
  return 2
}

export function confidenceTone(column: ColumnLine | undefined): 'ok' | 'warn' | 'low' | 'manual' | 'unmapped' {
  if (!column || column.target === 'unmapped') return 'unmapped'
  if (column.source === 'manual') return 'manual'
  if (column.low || column.confidence < 0.6) return 'low'
  if (column.confidence < 0.9) return 'warn'
  return 'ok'
}

export function showTypeConfidence(
  selected: string,
  suggested: string | null | undefined,
  confidence: number | null | undefined,
): number | null {
  if (!suggested || selected !== suggested || confidence == null) return null
  if (confidence >= 0.995) return null
  return confidence
}

type StoredTarget = {
  id: string
  label?: string
  attribute_id?: string | null
  structure_id?: string | null
  group_id?: string | null
}

const PARTS = ['value', 'unit', 'min', 'max', 'code', 'label']

function pathPart(id: string): string {
  const body = id.includes('@') && !id.split('@')[1]?.includes(':') ? id.split('@')[0] : id
  const part = body.split('.').at(-1) ?? ''
  return PARTS.includes(part) ? part : ''
}

/** Decisions keep `type:sku`. A saved mapping stores the attribute path (`<id>` or `<id>.unit`). */
export function storedTargetId(target: StoredTarget): string | null {
  const [body, language = ''] = target.id.split('@')
  const suffix = language && !language.includes(':') ? `@${language}` : ''
  const bare = suffix ? body : target.id
  if (!bare) return null
  const part = pathPart(bare)
  if (bare.startsWith('membership:') && target.structure_id) return `membership:${target.structure_id}`
  if (bare.startsWith('link:') && target.attribute_id) {
    const side = bare.split(':').at(-1)
    return side ? `link:${target.attribute_id}:${side}` : null
  }
  if (bare.startsWith('type:') && target.attribute_id) {
    return part ? `${target.attribute_id}.${part}${suffix}` : `${target.attribute_id}${suffix}`
  }
  if (bare.startsWith('group:') && target.structure_id && target.group_id && target.attribute_id) {
    const base = `group:${target.structure_id}/${target.group_id}:${target.attribute_id}`
    return part ? `${base}.${part}${suffix}` : `${base}${suffix}`
  }
  if (part && target.attribute_id) return `${target.attribute_id}.${part}${suffix}`
  return null
}

export function targetForStored<T extends StoredTarget>(targets: T[], stored: string): T | undefined {
  const legacy = stored.startsWith('type:') ? stored.slice('type:'.length) : ''
  return targets.find((target) => {
    const saved = storedTargetId(target)
    if (target.id === stored || saved === stored) return true
    if (!legacy) return false
    if (saved === legacy) return true
    return !pathPart(target.id) && target.id.startsWith('type:') && target.attribute_id === legacy
  })
}

export function dedupedTargets<T extends { id: string; label: string }>(targets: T[]): T[] {
  const seen = new Set<string>()
  const out: T[] = []
  for (const target of targets) {
    if (seen.has(target.label)) continue
    seen.add(target.label)
    out.push(target)
  }
  return out
}

export function alternativeChoices(
  column: ColumnLine,
  targets: Array<{ id: string; label: string }>,
): Array<{ id: string; label: string; p: number }> {
  const chosen = targets.find((target) => target.id === column.target)?.label
  const seen = new Set<string>()
  if (chosen) seen.add(chosen)
  const out: Array<{ id: string; label: string; p: number }> = []
  for (const alt of column.alternatives ?? []) {
    const label = targets.find((target) => target.id === alt.value)?.label ?? alt.value
    if (seen.has(label)) continue
    seen.add(label)
    out.push({ id: alt.value, label, p: alt.p })
  }
  return out
}

export function setTarget(
  columns: ColumnLine[],
  col: number,
  target: string,
): { columns: ColumnLine[]; displaced: ColumnLine | null } {
  let displaced: ColumnLine | null = null
  const next = columns.map((column) => {
    if (target !== 'unmapped' && column.target === target && column.col !== col) {
      displaced = column
      return { ...column, target: 'unmapped', source: 'manual', confidence: 1, low: false, reason: null }
    }
    if (column.col === col) {
      return { ...column, target, source: 'manual', confidence: 1, low: false, reason: null }
    }
    return column
  })
  return { columns: next, displaced }
}

export function undoColumns(history: ColumnLine[][], current: ColumnLine[], future: ColumnLine[][]) {
  const previous = history.at(-1)
  if (!previous) return { history, current, future }
  return {
    history: history.slice(0, -1),
    current: previous,
    future: [current, ...future],
  }
}

export function redoColumns(history: ColumnLine[][], current: ColumnLine[], future: ColumnLine[][]) {
  const next = future[0]
  if (!next) return { history, current, future }
  return {
    history: [...history, current],
    current: next,
    future: future.slice(1),
  }
}

export function rowClass(status: string): string {
  if (status === 'new') return 'import-row new'
  if (status === 'changed') return 'import-row changed'
  if (status === 'failed') return 'import-row failed'
  if (status === 'delete') return 'import-row delete'
  return 'import-row'
}

export function cellClass(state: string): string {
  if (state === 'changed') return 'import-cell changed'
  if (state === 'error') return 'import-cell error'
  if (state === 'warning') return 'import-cell warning'
  return 'import-cell'
}

export function saveLabel(_failed: number, changes: number): string {
  return `Approve ${changes} ${changes === 1 ? 'row' : 'rows'}`
}

export function phaseOf(status: string): ImportPhase {
  if (status === 'previewed' || status === 'diffing') return 'preview'
  if (status === 'saving') return 'saving'
  if (status === 'saved') return 'done'
  if (status === 'failed') return 'error'
  if (status === 'uploading') return 'uploading'
  if (status === 'ready' || status === 'mapping') return 'mapping'
  return 'mapping'
}

export function stepIndex(phase: ImportPhase): 1 | 2 | 3 {
  if (phase === 'preview') return 2
  if (phase === 'saving' || phase === 'done') return 3
  return 1
}

export function curve(from: { x: number; y: number }, to: { x: number; y: number }) {
  const dx = to.x - from.x
  const dy = to.y - from.y
  const len = Math.hypot(dx, dy) || 1
  const bend = Math.min(18, len * 0.08)
  return {
    control: {
      x: (from.x + to.x) / 2 + (-dy / len) * bend,
      y: (from.y + to.y) / 2 + (dx / len) * bend,
    },
    width: 1.25,
    widthHover: 1.8,
  }
}

export function ariaMap(header: string, label: string, confidence: number): string {
  return `Column ${header} maps to ${label}, ${Math.round(confidence * 100)} percent`
}

export function importRefreshesObjects(raw: string): boolean {
  try {
    const event = JSON.parse(raw) as { op?: string; status?: string }
    if (event.op === 'import' && event.status === 'saved') return true
    if (event.op === 'job' && (event.status === 'succeeded' || event.status === 'saved')) return true
  } catch {
    return false
  }
  return false
}
