import { textValue, type Json } from './api'
import { optionLabel } from './chips'
import { structureCaption } from './graph-scene'
import { copy, uiLanguage } from './i18n'

const LANGS = ['sv', 'en', 'de']

export type DiffLine = {
  id: string
  label: string
  before: string
  after: string
}

export function historyActor(row: Json): { person: string; via: '' | 'agent' | 'import' } {
  const behalf = textValue(row.on_behalf_of)
  const actor = textValue(row.actor)
  const via = textValue(row.via)
  if (behalf || via === 'agent') return { person: behalf || actor, via: 'agent' }
  if (via === 'import') return { person: actor, via: 'import' }
  return { person: actor, via: '' }
}

export function historyTitle(row: Json): string {
  const { person, via } = historyActor(row)
  const who = via === 'agent' ? `${person} ${copy.viaAgent}` : via === 'import' ? `${person} ${copy.viaImport}` : person
  return isCreate(row) ? `${copy.createdBy} ${who}` : who
}

export function isCreate(row: Json): boolean {
  if (textValue(row.op) === 'create') return true
  const diff = row.diff
  return Boolean(diff && typeof diff === 'object' && '$created' in (diff as Json))
}

export function relativeTime(value: unknown, now = Date.now()): string {
  if (typeof value !== 'string' || !value) return ''
  const then = new Date(value).getTime()
  if (Number.isNaN(then)) return value
  const delta = then - now
  const abs = Math.abs(delta)
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ['year', 365 * 24 * 60 * 60 * 1000],
    ['month', 30 * 24 * 60 * 60 * 1000],
    ['day', 24 * 60 * 60 * 1000],
    ['hour', 60 * 60 * 1000],
    ['minute', 60 * 1000],
    ['second', 1000],
  ]
  const format = new Intl.RelativeTimeFormat('en', { numeric: 'auto' })
  for (const [unit, size] of units) {
    if (abs >= size || unit === 'second') return format.format(Math.round(delta / size), unit)
  }
  return format.format(0, 'second')
}

export function diffLines(diff: unknown, defs: Json | undefined): DiffLine[] {
  if (!diff || typeof diff !== 'object' || Array.isArray(diff)) return []
  if ('$created' in (diff as Json)) return []
  const lines: DiffLine[] = []
  for (const [key, change] of Object.entries(diff as Json)) {
    if (key === '$created') continue
    const pair = (change as Json) ?? {}
    if (key === '$groups') {
      lines.push(...groupLines(pair, defs))
      continue
    }
    const label = fieldLabel(defs, key)
    if (isLanguageMap(pair.before) || isLanguageMap(pair.after)) {
      const languages = new Set([...languageKeys(pair.before), ...languageKeys(pair.after)])
      for (const language of LANGS.filter((item) => languages.has(item))) {
        const before = sideText(languageValue(pair.before, language), key, defs)
        const after = sideText(languageValue(pair.after, language), key, defs)
        if (before === after) continue
        lines.push({ id: `${key}:${language}`, label: `${label} (${language})`, before, after })
      }
      continue
    }
    lines.push({
      id: key,
      label,
      before: sideText(pair.before, key, defs),
      after: sideText(pair.after, key, defs),
    })
  }
  return lines.filter((line) => line.before !== line.after)
}

function groupLines(groups: Json, defs: Json | undefined): DiffLine[] {
  const lines: DiffLine[] = []
  for (const [structure, change] of Object.entries(groups)) {
    if (structure === 'before' || structure === 'after') continue
    const pair = (change as Json) ?? {}
    const before = groupLabel(defs, pair.before)
    const after = groupLabel(defs, pair.after)
    if (before === after) continue
    lines.push({ id: `$groups:${structure}`, label: copy.group, before, after })
  }
  return lines
}

function groupLabel(defs: Json | undefined, value: unknown): string {
  const id = textValue(value)
  if (!id) return ''
  for (const structure of (defs?.structures as Json[] | undefined) ?? []) {
    for (const group of (structure.groups as Json[] | undefined) ?? []) {
      if (String(group.id) === id) return structureCaption(String(group.key))
    }
  }
  return id
}

function fieldLabel(defs: Json | undefined, key: string): string {
  const attribute = ((defs?.attributes as Json[] | undefined) ?? []).find(
    (item) => String(item.id) === key || String(item.key) === key,
  )
  return textValue(attribute?.labels, uiLanguage) || String(attribute?.key ?? key)
}

function sideText(value: unknown, field: string, defs: Json | undefined): string {
  if (value == null || value === '') return ''
  const attribute = ((defs?.attributes as Json[] | undefined) ?? []).find(
    (item) => String(item.id) === field || String(item.key) === field,
  )
  const kind = (attribute?.kind as Json) ?? {}
  const kindType = String(kind.type ?? '')
  const list = String(kind.list ?? attribute?.key ?? field)
  if (Array.isArray(value)) {
    return value
      .map((item) => (kindType === 'value_list' ? optionLabel(defs, list, textValue(item), uiLanguage) : textValue(item, uiLanguage)))
      .filter(Boolean)
      .join(', ')
  }
  if (kindType === 'value_list') return optionLabel(defs, list, textValue(value), uiLanguage)
  return textValue(value, uiLanguage)
}

function isLanguageMap(value: unknown): value is Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const keys = Object.keys(value)
  return keys.length > 0 && keys.every((key) => LANGS.includes(key))
}

function languageKeys(value: unknown): string[] {
  return isLanguageMap(value) ? Object.keys(value) : []
}

function languageValue(value: unknown, language: string): unknown {
  if (!isLanguageMap(value) || !(language in value)) return undefined
  return value[language]
}
