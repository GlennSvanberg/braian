import { del, get, patch, post, put } from './api/client'
import type { components } from './api/schema'

export { ApiError, client, freeObject, keepaliveJsonPut } from './api/client'
export type { components, paths } from './api/schema'
export type Schemas = components['schemas']

/** Typed client. Methods match the OpenAPI operations and throw `ApiError` on failure. */
export const api = { get, post, put, patch, delete: del }

/** Query object for `GET /v1/search` from the URL the tables already build. */
export function searchQuery(params: URLSearchParams): {
  q?: string
  lang?: string
  filter?: string
  sort?: string
  group?: string
  page?: number
  per_page?: number
  wait_seq?: number
} {
  const text = (key: string) => {
    const value = params.get(key)
    return value == null || value === '' ? undefined : value
  }
  const num = (key: string) => {
    const value = params.get(key)
    if (value == null || value === '') return undefined
    const parsed = Number(value)
    return Number.isNaN(parsed) ? undefined : parsed
  }
  return {
    q: text('q'),
    lang: text('lang'),
    filter: text('filter'),
    sort: text('sort'),
    group: text('group'),
    page: num('page'),
    per_page: num('per_page'),
    wait_seq: num('wait_seq'),
  }
}

/** A JSON object from an operation result whose shape depends on the operation. */
export function jsonRecord(value: unknown): Json | null {
  if (value && typeof value === 'object' && !Array.isArray(value)) return value as Json
  return null
}

/** Proposal id from an operation whose payload depends on the operation name. */
export function operationId(result: unknown): string {
  const record = jsonRecord(result)
  return String(record?.proposal_id ?? record?.id ?? '')
}

/** Rows an accept call failed. A running job has none. */
export function failureLines(result: object): Json[] {
  if (!('failed' in result) || !Array.isArray(result.failed)) return []
  return result.failed.flatMap((item) => {
    const row = jsonRecord(item)
    return row ? [row] : []
  })
}

export type Json = Record<string, unknown>

/** Indexed display name from the standard name attribute. */
export function catalogName(doc: Json | undefined, defs?: Json): string {
  if (!doc) return ''
  const fields: string[] = []
  const attribute = ((defs?.attributes as Json[] | undefined) ?? []).find((item) => String(item.key ?? '') === 'name')
  const id = String(attribute?.id ?? '')
  if (id) {
    const base = `a_${id.replaceAll('-', '_')}`
    fields.push(`${base}__en`, `${base}__sv`, `${base}__en_GB`, base)
  }
  fields.push('a_name__en', 'a_name__sv', 'a_name')
  for (const key of fields) {
    const text = textValue(doc[key])
    if (text) return text
  }
  return ''
}

export function recordTitle(values: Json | undefined, language = 'en'): string {
  if (!values) return ''
  return textValue(values.name, language)
}

export function recordNumber(values: Json | undefined): string {
  if (!values) return ''
  return textValue(values.number)
}

export function textValue(value: unknown, language = 'en'): string {
  if (value == null) return ''
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return String(value)
  }
  if (Array.isArray(value)) return value.map((item) => textValue(item, language)).filter(Boolean).join(', ')
  if (typeof value === 'object') {
    const record = value as Json
    if (language in record) return textValue(record[language], language)
    if ('value' in record) {
      const unit = record.unit ? ` ${record.unit}` : ''
      return `${textValue(record.value, language)}${unit}`
    }
    if ('min' in record || 'max' in record) {
      const unit = record.unit ? ` ${record.unit}` : ''
      return `${textValue(record.min, language)}–${textValue(record.max, language)}${unit}`
    }
    const first = Object.values(record)[0]
    return first == null ? '' : textValue(first, language)
  }
  return ''
}
