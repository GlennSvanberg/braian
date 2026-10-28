import { attributeByKey, attributeIndexField } from './index-fields'
import type { Json } from './api'
import { textValue } from './api'
import { isHopPath } from './column-prefs'

/** How many linked values to show before "+N". */
export const PATH_SUMMARY_VISIBLE = 2

export type PathValuesMap = Record<string, Record<string, string[]>>

export type PathValuesResponse = { values?: PathValuesMap }

/** Summarize many linked values: `A, B +2` with the full list for a tooltip. */
export function summarizePathValues(values: string[] | undefined | null): {
  text: string
  title: string
} {
  const list = (values ?? []).map((item) => String(item ?? '').trim()).filter(Boolean)
  if (list.length === 0) return { text: '', title: '' }
  if (list.length <= PATH_SUMMARY_VISIBLE) {
    const text = list.join(', ')
    return { text, title: text }
  }
  const shown = list.slice(0, PATH_SUMMARY_VISIBLE).join(', ')
  const rest = list.length - PATH_SUMMARY_VISIBLE
  return { text: `${shown} +${rest}`, title: list.join(', ') }
}

/**
 * Read a non-hop path from a Typesense hit, including parts (`weight.unit` →
 * `a_<id>__unit`). Hop paths need the path-values batch.
 */
export function readPathFromDoc(doc: Json | undefined, defs: Json | undefined, path: string): string {
  if (!doc || !path || isHopPath(path)) return ''
  const [head, ...rest] = path.split('.')
  const languageMatch = head?.includes('@') ? head.split('@') : null
  const key = languageMatch ? languageMatch[0]! : head!
  const language = languageMatch?.[1]
  const attribute = attributeByKey(defs, key)
  const id = String(attribute?.id ?? '')
  const part = rest[0] ?? ''
  const fields: string[] = []
  if (id) {
    const base = attributeIndexField(id, language)
    if (part === 'unit') fields.push(`${base}__unit`)
    else if (part === 'min') fields.push(`${base}__min`)
    else if (part === 'max') fields.push(`${base}__max`)
    else {
      fields.push(base, attributeIndexField(id), attributeIndexField(id, 'en'), attributeIndexField(id, 'sv'))
    }
  }
  const legacyBase = language ? `a_${key}__${language.replaceAll('-', '_')}` : `a_${key}`
  if (part === 'unit') fields.push(`${legacyBase}__unit`, `a_${key}__unit`)
  else if (part === 'min') fields.push(`${legacyBase}__min`, `a_${key}__min`)
  else if (part === 'max') fields.push(`${legacyBase}__max`, `a_${key}__max`)
  else fields.push(legacyBase, `a_${key}`, `a_${key}__en`, `a_${key}__sv`)
  for (const field of fields) {
    const text = textValue(doc[field])
    if (text) return text
  }
  return ''
}
