import { textValue, type Json } from './api'

/** Typesense field for an attribute id. Hyphens become underscores. */
export function attributeIndexField(id: string, language?: string) {
  const base = `a_${id.replaceAll('-', '_')}`
  if (!language) return base
  return `${base}__${language.replaceAll('-', '_')}`
}

function attributes(defs: Json | undefined) {
  return (defs?.attributes as Json[] | undefined) ?? []
}

export function attributeByKey(defs: Json | undefined, key: string) {
  return attributes(defs).find((item) => String(item.key ?? '') === key)
}

/** Read an indexed value by id, then by the old `a_<key>` name. */
export function readIndexed(doc: Json | undefined, defs: Json | undefined, key: string) {
  if (!doc) return ''
  const id = String(attributeByKey(defs, key)?.id ?? '')
  const fields = id
    ? [attributeIndexField(id), attributeIndexField(id, 'en'), attributeIndexField(id, 'sv'), attributeIndexField(id, 'en_GB')]
    : []
  fields.push(`a_${key}`, `a_${key}__en`, `a_${key}__sv`, `a_${key}__en_GB`, `a_${key}__en-GB`)
  for (const field of fields) {
    const text = textValue(doc[field])
    if (text) return text
  }
  return ''
}

/** Sort and group fields follow the index name, which is the attribute id. */
export function indexSort(field: string, defs: Json | undefined) {
  const direction = field.endsWith(':asc') || field.endsWith(':desc') ? field.slice(field.lastIndexOf(':')) : ''
  const name = direction ? field.slice(0, -direction.length) : field
  if (!name.startsWith('a_') || !defs) return field
  const body = name.slice(2).split('__')[0] ?? ''
  const language = name.includes('__') ? name.split('__').slice(1).join('__') : ''
  const key = body.replaceAll('_', '-')
  const attribute = attributes(defs).find((item) => {
    const itemKey = String(item.key ?? '')
    return itemKey === body || itemKey === key || String(item.id ?? '').replaceAll('-', '_') === body
  })
  const id = String(attribute?.id ?? '')
  if (!id) return field
  const next = language ? attributeIndexField(id, language.replaceAll('_', '-')) : attributeIndexField(id)
  return `${next}${direction}`
}

/**
 * Typesense sort field for a column id (prefs path / builtin).
 * Empty string means the column is not sortable in the index.
 */
export function sortKeyForColumnId(columnId: string, defs: Json | undefined): string {
  const raw = columnId.includes('#') ? columnId.slice(0, columnId.indexOf('#')) : columnId
  // Hop columns are not a single Typesense field.
  if (raw.includes('>')) return ''
  const key = (raw.split('.')[0] ?? raw).split('@')[0] ?? raw
  if (key === 'key' || key === 'number') return 'a_number'
  if (key === 'type') return 'type_key'
  if (key === 'version') return 'version'
  if (key === 'name') {
    const attr = attributeByKey(defs, 'name')
    const id = String(attr?.id ?? '')
    return id ? attributeIndexField(id, 'en') : 'a_name__en'
  }
  if (key === 'status') {
    const attr = attributeByKey(defs, 'status')
    const id = String(attr?.id ?? '')
    return id ? attributeIndexField(id) : 'a_status'
  }
  const attr = attributeByKey(defs, key)
  if (!attr) return ''
  const index = attr.index as { filterable?: boolean; sortable?: boolean } | undefined
  // Searchable-only (no filter/sort) lives in search_{lang}, not a sort field.
  if (index && index.sortable === false) return ''
  if (index && !index.filterable && !index.sortable) return ''
  const id = String(attr.id ?? '')
  if (!id) return ''
  if (attr.language_dependent) return attributeIndexField(id, 'en')
  return attributeIndexField(id)
}
