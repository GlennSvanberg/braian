/** Pure import helpers. The screen and the tests share these. */

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
  return 'import-cell'
}

export function saveLabel(failed: number, changes: number): string {
  if (failed > 0) return `Import the rest, skip ${failed} failing rows`
  return `Save ${changes} changes`
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
