import { api, keepaliveJsonPut } from './api'
import { LAYOUT_ENVIRONMENT } from './workspace-doc'
import type { AppliedQualifier } from './fields/qualifiers'
import { columnIdOf, defaultColumnIdsForType, parseColumnId } from './fields/columns'
import type { DefsJson } from './fields/catalog'

export type ColumnPref = {
  path: string
  visible?: boolean
  stale?: boolean
  /** Cardinality, Specific peer id, display unit, etc. */
  qualifiers?: AppliedQualifier[]
}

/** v2 document — v1 rows are ignored (not migrated). */
export type ColumnPrefsDocument = { version: 2; columns: ColumnPref[] }

export type SystemViewColumn = {
  key: string
  label: string
  default_index: number
}

export type ColumnPrefsResponse = {
  environment?: string
  document?: { version?: number; columns?: ColumnPref[] } | null
  updated_at?: string | null
  stale_paths?: string[]
}

const BUILTIN_PATH: Record<string, string> = { key: 'number' }

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : null
}

function defaultRank(key: string, systemColumns: SystemViewColumn[]): number | undefined {
  if (key === 'number' || key === 'key') return 0
  if (key === 'name') return 1
  if (key === 'status') return 3
  const system = systemColumns.find((column) => column.key === key)
  return system?.default_index
}

export function ensureSystemColumns(
  order: string[],
  hidden: string[],
  systemColumns: SystemViewColumn[],
): { order: string[]; hidden: string[] } {
  const next = [...order]
  for (const system of systemColumns) {
    if (next.includes(system.key)) continue
    // Also skip if a qualified id already covers this system key
    if (next.some((id) => parseColumnId(id).path === system.key || id === system.key)) continue
    const ranked = next
      .map((path, index) => {
        const rank = defaultRank(parseColumnId(path).path, systemColumns)
        return rank != null && rank < system.default_index ? index + 1 : -1
      })
      .filter((index) => index >= 0)
    const insertAt =
      ranked.length > 0 ? Math.max(...ranked) : Math.min(system.default_index, next.length)
    next.splice(Math.min(insertAt, next.length), 0, system.key)
  }
  return { order: next, hidden }
}

/**
 * Turn stored prefs into table order/hidden ids.
 * Only version 2 is accepted — older documents are treated as missing (defaults).
 */
export function prefsToOrderHidden(
  response: ColumnPrefsResponse | null | undefined,
  systemColumns: SystemViewColumn[] = [],
  typeKey = '',
  defs?: DefsJson,
): { order: string[]; hidden: string[] } | null {
  const document = record(response?.document)
  if (!document) return null
  const version = Number(document.version ?? 0)
  if (version !== 2) {
    // Discard v1 / unknown — return defaults so nothing crashes on old rows.
    if (!typeKey && systemColumns.length === 0) return null
    const defaults = defaultColumnIdsForType(typeKey || 'product', defs)
    return ensureSystemColumns(defaults, [], systemColumns)
  }
  const columns = Array.isArray(document.columns) ? (document.columns as ColumnPref[]) : []
  if (columns.length === 0) return null
  const stale = new Set((response?.stale_paths ?? []).map(String))
  const order: string[] = []
  const hidden: string[] = []
  for (const value of columns) {
    const column = record(value) as ColumnPref | null
    if (!column) continue
    const path = String(column.path ?? '')
    if (!path || column.stale === true || stale.has(path)) continue
    const quals = Array.isArray(column.qualifiers) ? column.qualifiers : []
    const id = columnIdOf(path, quals)
    if (order.includes(id)) continue
    order.push(id)
    if (column.visible === false) hidden.push(id)
  }
  const ensured = ensureSystemColumns(order, hidden, systemColumns)
  if (ensured.order.length === 0) return null
  return ensured
}

export function orderHiddenToDocument(
  order: string[],
  hidden: string[],
  allowedPaths?: Iterable<string> | null,
  systemColumns: SystemViewColumn[] = [],
): ColumnPrefsDocument {
  const allowed = allowedPaths ? new Set(allowedPaths) : null
  const systemKeys = new Set(systemColumns.map((column) => column.key))
  const seen = new Set<string>()
  const columns: ColumnPref[] = []
  for (const id of order) {
    const { path: rawPath, qualifiers } = parseColumnId(id)
    const path = BUILTIN_PATH[rawPath] ?? rawPath
    // Strip language/part from allowlist check — compare base attribute path heads
    const allowKey = path
    if (!path || seen.has(id)) continue
    const isSystem = systemKeys.has(path) || systemKeys.has(rawPath)
    if (!isSystem && allowed) {
      // Allow if exact path or base path (without qualifier encoding) is listed
      const base = path.split('@')[0] ?? path
      const ok =
        allowed.has(path) ||
        allowed.has(base) ||
        [...allowed].some((item) => item === path || item.startsWith(`${base}.`) || item.startsWith(`${base}@`))
      // Hop paths: articles>net_weight
      const hopOk = path.includes('>') && [...allowed].some((item) => item === path || item.startsWith(path.split('.')[0] ?? ''))
      if (!ok && !hopOk && !path.includes('>')) {
        // still allow hops loosely
        if (!allowed.has(path) && !allowed.has(base)) {
          // Try leaf without part
          const leaf = base.includes('>') ? base : base
          if (![...allowed].some((a) => a === leaf || a.startsWith(leaf))) {
            // Keep system-like number/name always
            if (path !== 'number' && path !== 'name' && path !== 'key') {
              // drop unknown
              continue
            }
          }
        }
      }
    }
    seen.add(id)
    columns.push({
      path,
      visible: !hidden.includes(id) && !hidden.includes(path),
      ...(qualifiers.length > 0 ? { qualifiers } : {}),
    })
  }
  return { version: 2, columns }
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
    keepalive: true,
    body: JSON.stringify({ environment: LAYOUT_ENVIRONMENT, document }),
  })
}

export function saveColumnPrefsKeepalive(viewId: string, document: ColumnPrefsDocument) {
  keepaliveJsonPut(columnPrefsUrl(viewId), { environment: LAYOUT_ENVIRONMENT, document })
}

export function isHopPath(pathOrId: string) {
  const path = parseColumnId(pathOrId).path
  return path.includes('>')
}

export function pathHead(path: string) {
  const first = path.split('>')[0] ?? path
  return first.split('.')[0]?.split('@')[0] ?? first
}

/** API path for pathValues (strip column-id qualifier suffix). */
export function apiPathOf(columnId: string): string {
  return parseColumnId(columnId).path
}
