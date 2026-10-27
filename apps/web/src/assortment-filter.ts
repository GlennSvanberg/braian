/** Typesense filter for a saved assortment query.
 *
 * The query already carries `type`, `group`, and `labels`. Those become the
 * same `filter` the search API already accepts. `text` is not a filter.
 */
export function assortmentSearchFilter(query: unknown): string {
  if (!query || typeof query !== 'object') return ''
  const item = query as { type?: unknown; group?: unknown; labels?: unknown }
  const parts: string[] = []
  if (typeof item.type === 'string' && item.type) parts.push(`type_key:=${item.type}`)
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

function labelSearchParts(value: unknown): string[] {
  return labelCodes(value).map((code) => `a_labels:=${code}`)
}
