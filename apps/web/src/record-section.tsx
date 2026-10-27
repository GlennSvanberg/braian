import type { KeyboardEvent } from 'react'
import { recordNumber, recordTitle, textValue, type Json } from './api'
import { copy } from './i18n'

/** Fixed ids from the model. Labels come from the definition set. */
export const RECORD_FIELDS = [
  { id: '018f0000-0000-7000-8000-000000000100', key: 'number', fallback: 'Number', value: 'text' },
  { id: '018f0000-0000-7000-8000-000000000101', key: 'name', fallback: 'Name', value: 'text' },
  { id: '018f0000-0000-7000-8000-00000000a003', key: 'created_at', fallback: 'Created', value: 'time' },
  { id: '018f0000-0000-7000-8000-00000000a004', key: 'updated_at', fallback: 'Updated', value: 'time' },
  { id: '018f0000-0000-7000-8000-00000000a005', key: 'updated_by', fallback: 'Updated by', value: 'principal' },
] as const

export type RecordField = (typeof RECORD_FIELDS)[number]

export type RecordRow = {
  key: RecordField['key']
  label: string
  text: string
  title: string
}

const timeOptions: Intl.DateTimeFormatOptions = {
  year: 'numeric',
  month: 'short',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
}

/** Browser locale and time zone. `timeZone` is for tests. */
export function formatRecordTime(iso: string, timeZone?: string): { text: string; title: string } {
  if (!iso) return { text: '', title: '' }
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return { text: iso, title: iso }
  const options = timeZone ? { ...timeOptions, timeZone } : timeOptions
  return { text: new Intl.DateTimeFormat(undefined, options).format(date), title: iso }
}

/** Display name of the principal who last wrote the object. */
export function principalName(updatedBy: unknown): string {
  if (!updatedBy || typeof updatedBy !== 'object') return ''
  const name = (updatedBy as Json).name
  return name == null ? '' : String(name)
}

export function recordLabel(defs: Json | undefined, typeKey: string, field: RecordField, language = 'en'): string {
  const objectType = ((defs?.types as Json[] | undefined) ?? []).find((item) => String(item.key ?? '') === typeKey)
  const attached = new Set(((objectType?.attributes as unknown[]) ?? []).map(String))
  if (!attached.has(field.id)) return field.fallback
  const attribute = ((defs?.attributes as Json[] | undefined) ?? []).find((item) => String(item.id ?? '') === field.id)
  if (!attribute) return field.fallback
  const labels = (attribute.labels as Json | undefined) ?? {}
  return textValue(labels[language]) || textValue(labels.en) || field.fallback
}

export function recordRows(
  defs: Json | undefined,
  typeKey: string,
  source: {
    values?: Json
    createdAt?: string
    updatedAt?: string
    updatedBy?: unknown
    language?: string
    timeZone?: string
  },
): RecordRow[] {
  const values = source.values ?? {}
  const language = source.language ?? 'en'
  return RECORD_FIELDS.map((field) => {
    const label = recordLabel(defs, typeKey, field, language)
    if (field.key === 'number') return { key: field.key, label, text: recordNumber(values), title: '' }
    if (field.key === 'name') return { key: field.key, label, text: recordTitle(values, language), title: '' }
    if (field.key === 'updated_by') return { key: field.key, label, text: principalName(source.updatedBy), title: '' }
    const iso = field.key === 'created_at' ? source.createdAt ?? '' : source.updatedAt ?? ''
    const formatted = formatRecordTime(iso, source.timeZone)
    return { key: field.key, label, text: formatted.text, title: formatted.title }
  })
}

export function RecordSection({
  defs,
  typeKey,
  values,
  createdAt,
  updatedAt,
  updatedBy,
}: {
  defs?: Json
  typeKey: string
  values?: Json
  createdAt?: string
  updatedAt?: string
  updatedBy?: unknown
}) {
  const rows = recordRows(defs, typeKey, { values, createdAt, updatedAt, updatedBy })
  const keep = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') event.preventDefault()
  }
  return (
    <section className="field-card record-card" data-testid="record-section" aria-labelledby="record-heading">
      <h3 id="record-heading">{copy.record}</h3>
      {rows.map((row) => {
        const id = `record-${row.key}`
        return (
          <div key={row.key} className="record-field">
            <label htmlFor={id}>{row.label}</label>
            <input
              id={id}
              data-testid={id}
              readOnly
              aria-readonly="true"
              value={row.text}
              title={row.title || undefined}
              spellCheck={false}
              onChange={() => {}}
              onKeyDown={keep}
            />
          </div>
        )
      })}
    </section>
  )
}
