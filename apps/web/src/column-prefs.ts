import { api } from './api'
import { LAYOUT_ENVIRONMENT } from './workspace-doc'

export type ColumnPref = { path: string; visible?: boolean; stale?: boolean }

export type ColumnPrefsDocument = { version: 1; columns: ColumnPref[] }

export type ColumnPrefsResponse = {
  environment?: string
  document?: ColumnPrefsDocument | null
  updated_at?: string | null
  stale_paths?: string[]
}

/** Built-in ids that map to an attribute path when prefs are saved. */
const BUILTIN_PATH: Record<string, string> = { key: 'number' }

/** Built-in ids without an attribute path that prefs cannot store. */
const NOT_A_PATH = new Set(['type', 'version'])

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : null
}

/**
 * Turn a stored prefs document into table order and hidden ids.
 * Stale paths are skipped, never thrown. Returns null when there is nothing to apply.
 */
export function prefsToOrderHidden(response: ColumnPrefsResponse | null | undefined) {
  const document = record(response?.document)
  const columns = Array.isArray(document?.columns) ? document.columns : []
  if (columns.length === 0) return null
  const stale = new Set((response?.stale_paths ?? []).map(String))
  const order: string[] = []
  const hidden: string[] = []
  for (const value of columns) {
    const column = record(value)
    if (!column) continue
    const path = String(column.path ?? '')
    if (!path || column.stale === true || stale.has(path) || order.includes(path)) continue
    order.push(path)
    if (column.visible === false) hidden.push(path)
  }
  if (order.length === 0) return null
  return { order, hidden }
}

/**
 * Build the document we PUT. Built-in ids that have an attribute path are
 * renamed (`key` becomes `number`); ids without a path are left out so the
 * API never rejects the document.
 */
export function orderHiddenToDocument(
  order: string[],
  hidden: string[],
  allowedPaths?: Iterable<string> | null,
): ColumnPrefsDocument {
  const allowed = allowedPaths ? new Set(allowedPaths) : null
  const seen = new Set<string>()
  const columns: ColumnPref[] = []
  for (const id of order) {
    if (NOT_A_PATH.has(id)) continue
    const path = BUILTIN_PATH[id] ?? id
    if (!path || seen.has(path)) continue
    if (allowed && !allowed.has(path)) continue
    seen.add(path)
    columns.push({ path, visible: !hidden.includes(id) && !hidden.includes(path) })
  }
  return { version: 1, columns }
}

export function columnPrefsUrl(viewId: string) {
  return `/v1/views/${encodeURIComponent(viewId)}/column-prefs?environment=${encodeURIComponent(LAYOUT_ENVIRONMENT)}`
}

export function fetchColumnPrefs(viewId: string) {
  return api<ColumnPrefsResponse>(columnPrefsUrl(viewId))
}

export function saveColumnPrefs(viewId: string, document: ColumnPrefsDocument) {
  return api(columnPrefsUrl(viewId), {
    method: 'PUT',
    body: JSON.stringify({ environment: LAYOUT_ENVIRONMENT, document }),
  })
}

export function isHopPath(path: string) {
  return path.includes('>')
}

/** The attribute head of a path (`weight.unit` and `link>weight` both become `weight`). */
export function pathHead(path: string) {
  const first = path.split('>')[0] ?? path
  return first.split('.')[0]?.split('@')[0] ?? first
}
