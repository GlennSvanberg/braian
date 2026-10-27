import type { Json } from './api'

/** Typesense filter for a saved selection.
 *
 * `type` on the query is ignored. The selection's type is passed in, so a
 * stored query cannot point the search at another type. Group and label
 * fields may be ids; the index still stores the current keys.
 * `text` is not a filter.
 */
export function assortmentSearchFilter(query: unknown, typeKey = '', defs?: Json): string {
  if (!query || typeof query !== 'object') return typeKey ? `type_key:=${typeKeyOf(typeKey, defs)}` : ''
  const item = query as { group?: unknown; labels?: unknown }
  const parts: string[] = []
  if (typeKey) parts.push(`type_key:=${typeKeyOf(typeKey, defs)}`)
  if (typeof item.group === 'string' && item.group) parts.push(`groups:=${groupKeyOf(item.group, defs)}`)
  parts.push(...labelSearchParts(item.labels, defs))
  return parts.join(' && ')
}

export function typeKeyOf(token: string, defs?: Json) {
  const types = (defs?.types as Json[] | undefined) ?? []
  const match = types.find((item) => String(item.id ?? '') === token || String(item.key ?? '') === token)
  return match ? String(match.key ?? token) : token
}

export function groupKeyOf(token: string, defs?: Json) {
  const structures = (defs?.structures as Json[] | undefined) ?? []
  if (!token.includes(':')) {
    for (const structure of structures) {
      const group = ((structure.groups as Json[]) ?? []).find((item) => String(item.id ?? '') === token || String(item.key ?? '') === token)
      if (group) return `${String(structure.key)}:${String(group.key)}`
    }
    return token
  }
  const [structureToken, groupToken] = token.split(':')
  const structure = structures.find((item) => String(item.id ?? '') === structureToken || String(item.key ?? '') === structureToken)
  const group = ((structure?.groups as Json[]) ?? []).find((item) => String(item.id ?? '') === groupToken || String(item.key ?? '') === groupToken)
  if (structure && group) return `${String(structure.key)}:${String(group.key)}`
  return token
}

/** Label clause only. The object table already applies type and group. */
export function labelSearchFilter(query: unknown): string {
  if (!query || typeof query !== 'object') return ''
  const item = query as { labels?: unknown }
  return labelSearchParts(item.labels).join(' && ')
}

export function labelCodes(value: unknown): string[] {
  if (typeof value === 'string' && value) return [value]
  if (!Array.isArray(value)) return []
  return value.filter((code): code is string => typeof code === 'string' && code.length > 0)
}

/** A selection is shown in a workspace when its type id or key matches. */
export function selectionMatchesType(
  item: { type?: unknown; type_id?: unknown },
  typeKey: string,
  typeId = '',
): boolean {
  if (!typeKey && !typeId) return false
  const key = typeof item.type === 'string' ? item.type : ''
  const id = typeof item.type_id === 'string' ? item.type_id : ''
  if (typeKey && (key === typeKey || id === typeKey)) return true
  if (typeId && (id === typeId || key === typeId)) return true
  return false
}

export const NEW_SELECTION_EVENT = 'braian-new-selection'
export const SELECTION_APPLIED_EVENT = 'braian-selection-applied'

export function requestNewSelection() {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new CustomEvent(NEW_SELECTION_EVENT))
}

export function emitSavedSelection(item: { id?: unknown; name?: unknown; type?: unknown; type_id?: unknown; query?: unknown }) {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new CustomEvent(SELECTION_APPLIED_EVENT, { detail: item }))
}

let selectionLabels: string[] = []

export function publishSelectionLabels(codes: string[]) {
  selectionLabels = codes
}

export function currentSelectionLabels() {
  return selectionLabels
}

function labelSearchParts(value: unknown, defs?: Json): string[] {
  const attribute = ((defs?.attributes as Json[] | undefined) ?? []).find((item) => String(item.key ?? '') === 'labels')
  const id = String(attribute?.id ?? '')
  const field = id ? `a_${id.replaceAll('-', '_')}` : 'a_labels'
  return labelCodes(value).map((code) => `${field}:=${code}`)
}
