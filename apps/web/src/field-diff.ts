import { textValue, type Json } from './api'
import { optionLabel } from './chips'
import { formatTimestamp } from './format'
import { structureCaption } from './graph-scene'
import { copy, uiLanguage } from './i18n'

const LANGS = ['sv', 'en', 'de']

export type DiffLine = {
  id: string
  label: string
  before: string
  after: string
  /** Attribute key. Set on suggestion lines so a partial accept can rebuild the patch. */
  field?: string
  /** Language tag when the change is one language of a translated value. */
  language?: string
  /** Multi-value membership change. Absent on a plain edit and on history lines. */
  change?: 'add' | 'remove'
  /** Stored value behind the display text, used to rebuild a partial patch. */
  raw?: unknown
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

/** Absolute local time. History and drafts use this instead of a relative phrase. */
export function relativeTime(value: unknown, _now = Date.now()): string {
  return formatTimestamp(value)
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

/** Values an object proposal wants to write. Agent calls store them under `values`. */
export function proposedValues(patch: unknown): Json {
  if (!patch || typeof patch !== 'object' || Array.isArray(patch)) return {}
  const values = (patch as Json).values
  if (!values || typeof values !== 'object' || Array.isArray(values)) return {}
  return values as Json
}

/**
 * One line per changed field. Languages split onto their own lines.
 * Multi-value edits split into additions and removals.
 */
export function suggestionLines(current: Json | undefined, patch: unknown, defs: Json | undefined): DiffLine[] {
  const proposed = proposedValues(patch)
  const lines: DiffLine[] = []
  for (const [key, after] of Object.entries(proposed)) {
    const before = current?.[key]
    const label = fieldLabel(defs, key)
    if (isLanguageMap(after)) {
      for (const language of LANGS.filter((item) => Object.prototype.hasOwnProperty.call(after, item))) {
        const beforeText = sideText(languageValue(before, language), key, defs)
        const afterText = sideText(after[language], key, defs)
        if (beforeText === afterText) continue
        lines.push({
          id: `${key}:${language}`,
          field: key,
          label,
          language,
          before: beforeText,
          after: afterText,
          raw: after[language],
        })
      }
      continue
    }
    if (isLanguageMap(before)) {
      for (const language of LANGS.filter((item) => languageKeys(before).includes(item))) {
        const beforeText = sideText(languageValue(before, language), key, defs)
        if (!beforeText) continue
        lines.push({
          id: `${key}:${language}`,
          field: key,
          label,
          language,
          before: beforeText,
          after: '',
          raw: null,
        })
      }
      continue
    }
    if (isMultiple(defs, key, before, after)) {
      lines.push(...membershipLines(key, label, before, after, defs))
      continue
    }
    const beforeText = sideText(before, key, defs)
    const afterText = sideText(after, key, defs)
    if (beforeText === afterText) continue
    lines.push({ id: key, field: key, label, before: beforeText, after: afterText, raw: after })
  }
  return lines
}

/** Patch values for the lines the person kept. A fully kept field is copied as proposed. */
export function valuesFromKept(current: Json, proposed: Json, lines: DiffLine[], kept: DiffLine[]): Json {
  const values: Json = {}
  const fields = [...new Set(kept.map((line) => line.field).filter((field): field is string => Boolean(field)))]
  for (const field of fields) {
    const fieldLines = lines.filter((line) => line.field === field)
    const keptLines = kept.filter((line) => line.field === field)
    if (keptLines.length === fieldLines.length) {
      values[field] = proposed[field]
      continue
    }
    if (keptLines.some((line) => line.language)) {
      const map: Json = {}
      for (const line of keptLines) {
        if (!line.language) continue
        map[line.language] = isLanguageMap(proposed[field]) ? proposed[field][line.language] : line.raw
      }
      values[field] = map
      continue
    }
    if (keptLines.some((line) => line.change)) {
      values[field] = keptMembership(current[field], fieldLines, keptLines)
    }
  }
  return values
}

export function sourceLabel(row: { source?: unknown; actor_name?: unknown; behalf_name?: unknown } | undefined): string {
  const source = textValue(row?.source)
  const name = textValue(row?.behalf_name) || textValue(row?.actor_name)
  if (source === 'agent') return name ? `Agent ${name}` : copy.agent
  if (source === 'import') return copy.importJob
  if (source === 'mass_edit') return copy.massEdit
  return name || source
}

export function suggestedTitle(count: number): string {
  return count === 1 ? `1 ${copy.suggestedChange}` : `${count} ${copy.suggestedChanges}`
}

function membershipLines(key: string, label: string, before: unknown, after: unknown, defs: Json | undefined): DiffLine[] {
  const beforeItems = asList(before)
  const afterItems = asList(after)
  const lines: DiffLine[] = []
  afterItems.forEach((item, index) => {
    if (beforeItems.some((other) => sameValue(item, other))) return
    lines.push({
      id: `${key}:add:${index}`,
      field: key,
      label,
      before: '',
      after: sideText(item, key, defs),
      change: 'add',
      raw: item,
    })
  })
  beforeItems.forEach((item, index) => {
    if (afterItems.some((other) => sameValue(item, other))) return
    lines.push({
      id: `${key}:remove:${index}`,
      field: key,
      label,
      before: sideText(item, key, defs),
      after: '',
      change: 'remove',
      raw: item,
    })
  })
  return lines
}

function keptMembership(current: unknown, lines: DiffLine[], kept: DiffLine[]): unknown[] {
  const beforeItems = asList(current)
  const keptIds = new Set(kept.map((line) => line.id))
  const next = beforeItems.filter((item) => {
    const removal = lines.find((line) => line.change === 'remove' && sameValue(line.raw, item))
    return removal ? !keptIds.has(removal.id) : true
  })
  for (const line of kept) {
    if (line.change === 'add') next.push(line.raw)
  }
  return next
}

function asList(value: unknown): unknown[] {
  return Array.isArray(value) ? value : []
}

function sameValue(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right)
}

function isMultiple(defs: Json | undefined, key: string, before: unknown, after: unknown): boolean {
  if (Array.isArray(before) || Array.isArray(after)) return true
  const attribute = ((defs?.attributes as Json[] | undefined) ?? []).find(
    (item) => String(item.id) === key || String(item.key) === key,
  )
  return Boolean(attribute?.multiple)
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
      if (String(group.id) === id) return structureCaption(String(group.key), defs, String(structure.key))
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
