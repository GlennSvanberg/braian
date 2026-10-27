import type { Json } from './api'
import { viewDisplayTitle } from './demo-labels'
import { copy } from './i18n'
import type { PaletteItem } from './palette-model'

export type TypeName = {
  id: string
  key: string
  singular: string
  plural: string
}

export type SavedView = {
  id: string
  type: string
  title: string
  state?: Json
}

export type WorkspaceMigration = {
  changed: boolean
  name?: string
  views: Array<{ id: string; title: string; state: Json }>
}

const GENERIC_TITLES = new Set(['objects'])

/** English-style plural used when a type has no plural label. */
export function defaultPlural(singular: string) {
  const word = singular.trim()
  if (!word) return copy.objects
  const lower = word.toLowerCase()
  if (lower.endsWith('ch') || lower.endsWith('sh') || lower.endsWith('s') || lower.endsWith('x') || lower.endsWith('z')) {
    return `${word}es`
  }
  if (word.length > 1 && /[yY]$/.test(word) && !/[aeiou]/i.test(word.charAt(word.length - 2))) {
    return `${word.slice(0, -1)}ies`
  }
  return `${word}s`
}

function labelMap(map: unknown) {
  if (!map || typeof map !== 'object' || Array.isArray(map)) return undefined
  return map as Record<string, unknown>
}

function exactLabel(map: unknown, language: string) {
  const value = labelMap(map)?.[language]
  return typeof value === 'string' && value.trim() ? value : ''
}

function stored(map: unknown, language: string) {
  const exact = exactLabel(map, language)
  if (exact) return exact
  if (language !== 'en') {
    const english = exactLabel(map, 'en')
    if (english) return english
  }
  for (const value of Object.values(labelMap(map) ?? {})) {
    if (typeof value === 'string' && value.trim()) return value
  }
  return ''
}

function pluralStored(map: unknown, language: string) {
  const exact = exactLabel(map, language)
  if (exact) return exact
  return language === 'en' ? '' : exactLabel(map, 'en')
}

export function typeNames(defs: Json | undefined, language = 'en'): TypeName[] {
  return ((defs?.types as Json[]) ?? []).map((item) => {
    const key = String(item.key ?? '')
    const singular = stored(item.singular, language) || stored(item.labels, language) || key
    const plural = pluralStored(item.plural, language) || defaultPlural(singular)
    return { id: String(item.id ?? ''), key, singular, plural }
  })
}

export function singularOf(defs: Json | undefined, key: string, language = 'en') {
  return typeNames(defs, language).find((item) => item.key === key || item.id === key)?.singular ?? ''
}

export function pluralOf(defs: Json | undefined, key: string, language = 'en') {
  return typeNames(defs, language).find((item) => item.key === key)?.plural ?? ''
}

export function typeOfView(view: { state?: Json }, types: TypeName[]) {
  const id = String(view.state?.object_type ?? '')
  const key = String(view.state?.type_key ?? '')
  if (!id && !key) return undefined
  return types.find((item) => (id && item.id === id) || (key && (item.key === key || item.id === key)))
}

export function isGenericObjectTitle(title: string) {
  return GENERIC_TITLES.has(title.trim().toLowerCase())
}

export function migrateSavedWorkspace(
  _workspace: { name: string },
  views: SavedView[],
  types: TypeName[],
): WorkspaceMigration {
  const primary = types[0]
  const nextViews: WorkspaceMigration['views'] = []
  let changed = false
  if (primary) {
    for (const view of views) {
      if (view.type !== 'object_table' || !isGenericObjectTitle(view.title)) continue
      const state = { ...(view.state ?? {}), object_type: primary.id, type_key: primary.key }
      const already =
        view.title === primary.plural
        && String(view.state?.object_type ?? '') === primary.id
        && String(view.state?.type_key ?? '') === primary.key
      if (already) continue
      changed = true
      nextViews.push({ id: view.id, title: primary.plural, state })
    }
  }
  return { changed, name: undefined, views: nextViews }
}

export function displayViewTitle(
  view: { type: string; title: string; state?: Json },
  types: TypeName[],
  openTypeKey?: string | null,
) {
  if (view.type === 'object_table') {
    const bound = typeOfView(view, types)
    if (bound) return bound.plural
    if (isGenericObjectTitle(view.title) && types[0]) return types[0].plural
  }
  if (view.type === 'object_detail') {
    const open = openTypeKey ? types.find((item) => item.key === openTypeKey) : undefined
    if (open) return open.singular
    if (types.length === 1 && types[0]) return types[0].singular
  }
  return viewDisplayTitle(view.title)
}

export function typeViewPaletteItems(types: TypeName[]): PaletteItem[] {
  return types
    .filter((type) => type.id)
    .map((type) => ({
      id: `type-view:${type.id}`,
      group: 'views' as const,
      title: `${type.plural} view`,
      subtitle: 'Open view',
      keywords: [type.key, type.singular, type.plural],
    }))
}

export function searchGroupTitle(typeKeys: string[], types: TypeName[]) {
  const unique = [...new Set(typeKeys.filter(Boolean))]
  if (unique.length === 1) {
    const type = types.find((item) => item.key === unique[0])
    if (type) return type.plural
  }
  if (types.length === 1 && types[0]) return types[0].plural
  return copy.objects
}

export function commandPlaceholder(types: TypeName[]) {
  if (types.length === 1 && types[0]) return `Search ${types[0].plural.toLowerCase()}, open views, or ask AI`
  return copy.commandPlaceholder
}

function namedType(types: TypeName[], listTypeKey?: string | null) {
  if (listTypeKey) {
    const found = types.find((item) => item.key === listTypeKey)
    if (found) return found
  }
  if (types.length === 1) return types[0]
  return undefined
}

export function emptyDetail(types: TypeName[], listTypeKey?: string | null) {
  const type = namedType(types, listTypeKey)
  if (type) return `Select a ${type.singular.toLowerCase()} to edit its values.`
  return copy.detailEmpty
}

export function emptyGraph(types: TypeName[], listTypeKey?: string | null) {
  const type = namedType(types, listTypeKey)
  if (type) return `Select a ${type.singular.toLowerCase()} to see its graph.`
  return copy.graphEmpty
}

export function newObjectLabel(singular?: string) {
  return singular ? `New ${singular}` : copy.newObject
}

export function openObjectLabel(singular?: string) {
  return singular ? `Open ${singular}` : copy.openObject
}

export function affectedLabel(typeKeys: string[], types: TypeName[]) {
  const unique = [...new Set(typeKeys.filter(Boolean))]
  if (unique.length === 1) {
    const type = types.find((item) => item.key === unique[0])
    if (type) return `Affected ${type.plural}`
  }
  return copy.affected
}

export function historyVerb(op: string, singular?: string) {
  const name = singular?.trim() || 'object'
  if (op === 'create') return `Created ${name}`
  if (op === 'update') return `Updated ${name}`
  if (op === 'delete' || op === 'remove') return `Removed ${name}`
  return op
}
