/**
 * Column ids for the real Fields › picker.
 * Path text matches the API (`articles>net_weight`, `description@sv`, `allergens.code`).
 * Cardinality / Specific / display-unit live in qualifiers and are encoded after `#`.
 */
import {
  EMPTY_CELL,
  formatColumnTitle,
  summarizeList,
  type AppliedQualifier,
  type FieldDef,
} from './qualifiers'
import { fieldCatalogForType, type DefsJson } from './catalog'

export type ViewColumn = {
  /** Stable prefs/UI id: path + encoded qualifiers. */
  id: string
  /** Attribute path text accepted by parse_path_text (no cardinality/unit-display). */
  path: string
  fieldPath: string[]
  pathNames: string[]
  qualifiers: AppliedQualifier[]
  leaf: FieldDef
  viaManyLink: boolean
}

export function encodeQualifiers(qualifiers: AppliedQualifier[]): string {
  return qualifiers
    .map((item) => {
      let token = `${item.kind}:${item.optionId}`
      if (item.value) {
        token += `=${item.value}`
        // Persist Specific display label (key/name) so titles survive reload from prefs ids.
        if (
          item.label &&
          item.label !== item.optionId &&
          item.label !== item.value &&
          !item.value.includes('~')
        ) {
          token += `~${encodeURIComponent(item.label)}`
        }
      }
      return token
    })
    .join('|')
}

export function columnIdOf(path: string, qualifiers: AppliedQualifier[]): string {
  const q = encodeQualifiers(qualifiers)
  return q ? `${path}#${q}` : path
}

export function parseColumnId(id: string): { path: string; qualifiers: AppliedQualifier[] } {
  const hash = id.indexOf('#')
  if (hash < 0) return { path: id, qualifiers: [] }
  const path = id.slice(0, hash)
  const raw = id.slice(hash + 1)
  const qualifiers: AppliedQualifier[] = []
  if (!raw) return { path, qualifiers }
  for (const part of raw.split('|')) {
    if (!part) continue
    const [head, value] = part.split('=')
    const [kind, optionId] = (head ?? '').split(':')
    if (!kind || !optionId) continue
    let peerValue = value || undefined
    let label = optionId
    if (peerValue && peerValue.includes('~')) {
      const tilde = peerValue.indexOf('~')
      const rawLabel = peerValue.slice(tilde + 1)
      peerValue = peerValue.slice(0, tilde) || undefined
      try {
        label = decodeURIComponent(rawLabel) || optionId
      } catch {
        label = rawLabel || optionId
      }
    }
    qualifiers.push({
      kind: kind as AppliedQualifier['kind'],
      optionId,
      label,
      value: peerValue,
    })
  }
  return { path, qualifiers }
}

/** Build API path text from field path + leaf qualifiers (language/part). Cardinality stays out. */
export function pathTextFrom(fieldPath: string[], leaf: FieldDef, qualifiers: AppliedQualifier[]): string {
  const via = fieldPath.length > 1 ? fieldPath.slice(0, -1).join('>') : ''
  const leafId = fieldPath[fieldPath.length - 1] ?? leaf.id
  let body = leafId
  const lang = qualifiers.find((q) => q.kind === 'language')
  const valueList = qualifiers.find((q) => q.kind === 'value_list')
  const range = qualifiers.find((q) => q.kind === 'range')
  // Unit display conversion stays on Whole path + unit qualifier (not `.unit` part).
  if (valueList?.optionId === 'code') body = `${leafId}.code`
  else if (valueList?.optionId === 'label') body = `${leafId}.label`
  else if (range?.optionId === 'min') body = `${leafId}.min`
  else if (range?.optionId === 'max') body = `${leafId}.max`
  if (lang) body = `${body}@${lang.optionId}`
  return via ? `${via}>${body}` : body
}

export function makeViewColumn(
  fieldPath: string[],
  qualifiers: AppliedQualifier[],
  fields: FieldDef[],
): ViewColumn | null {
  let current = fields
  let viaManyLink = false
  const pathNames: string[] = []
  let leaf: FieldDef | undefined
  for (let i = 0; i < fieldPath.length; i += 1) {
    const id = fieldPath[i]!
    const field = current.find((item) => item.id === id)
    if (!field) return null
    pathNames.push(field.name)
    leaf = field
    if (field.shape === 'link' && field.link) {
      if (field.link.many) viaManyLink = true
      current = field.link.peerFields
    } else if (i < fieldPath.length - 1) {
      return null
    }
  }
  if (!leaf || leaf.shape === 'link') return null
  const path = pathTextFrom(fieldPath, leaf, qualifiers)
  const displayQualifiers = qualifiers.filter((q) => {
    // Language / value_list / range are already in path text for API; keep them for titles.
    return true
  })
  const id = columnIdOf(path, displayQualifiers.filter((q) => q.kind === 'cardinality' || q.kind === 'unit' || Boolean(q.value)))
  // Prefer richer id that includes all qualifiers for stable UI matching
  const fullId = columnIdOf(path, displayQualifiers)
  return {
    id: fullId,
    path,
    fieldPath,
    pathNames,
    qualifiers: displayQualifiers,
    leaf,
    viaManyLink,
  }
}

export function columnTitle(col: ViewColumn): string {
  return formatColumnTitle(col.pathNames, col.qualifiers)
}

export function withReplacedQualifier(column: ViewColumn, next: AppliedQualifier, fields: FieldDef[]): ViewColumn {
  const others = column.qualifiers.filter((item) => item.kind !== next.kind)
  const qualifiers = [...others, next]
  qualifiers.sort((a, b) => {
    const order = (k: AppliedQualifier['kind']) =>
      k === 'cardinality' ? 0 : k === 'language' || k === 'unit' || k === 'value_list' || k === 'range' ? 1 : 2
    return order(a.kind) - order(b.kind)
  })
  return makeViewColumn(column.fieldPath, qualifiers, fields) ?? { ...column, qualifiers, id: columnIdOf(column.path, qualifiers) }
}

/** Default visible columns per object type (identifier, name, a few key attributes). */
export function defaultColumnIdsForType(typeKey: string, defs?: DefsJson): string[] {
  const base = ['number', 'name']
  if (!defs) {
    if (typeKey === 'product') return [...base, 'status']
    if (typeKey === 'article') return [...base, 'gtin', 'status']
    if (typeKey === 'supplier') return [...base, 'status']
    return base
  }
  const catalog = fieldCatalogForType(defs, typeKey)
  const ids = [...base]
  const prefer =
    typeKey === 'product'
      ? ['status', 'brand']
      : typeKey === 'article'
        ? ['gtin', 'status', 'net_weight']
        : typeKey === 'supplier'
          ? ['status']
          : ['status']
  for (const key of prefer) {
    if (catalog.some((f) => f.id === key) && !ids.includes(key)) ids.push(key)
  }
  return ids
}

export { EMPTY_CELL, summarizeList }
