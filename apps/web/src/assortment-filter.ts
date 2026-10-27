/** Typesense filter for a saved selection.
 *
 * `type` on the query is ignored. The selection's type is passed in, so a
 * stored query cannot point the search at another type. `text` is not a filter.
 */
export function assortmentSearchFilter(query: unknown, typeKey = ''): string {
  if (!query || typeof query !== 'object') return typeKey ? `type_key:=${typeKey}` : ''
  const item = query as { group?: unknown; labels?: unknown }
  const parts: string[] = []
  if (typeKey) parts.push(`type_key:=${typeKey}`)
  if (typeof item.group === 'string' && item.group) parts.push(`groups:=${item.group}`)
  parts.push(...labelSearchParts(item.labels))
  return parts.join(' && ')
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

function labelSearchParts(value: unknown): string[] {
  return labelCodes(value).map((code) => `a_labels:=${code}`)
}
