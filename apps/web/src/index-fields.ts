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
