import type { Json } from './api'
import { typeNames } from './type-labels'

export type ModelSelection =
  | { kind: 'type'; typeKey: string }
  | { kind: 'group'; structure: string; group: string }

export type AttributeRow = {
  id: string
  key: string
  label: string
  kind: string
  unit: string
  required: boolean
  languages: boolean
  searchable: boolean
  filterable: boolean
  sortable: boolean
  inherited: boolean
  fromKey?: string
}

export type SourceRef =
  | { kind: 'type'; key: string }
  | { kind: 'group'; structure: string; group: string }

export type EffectiveField = {
  id: string
  key: string
  label: string
  kind: string
  sources: SourceRef[]
}

export type GroupNode = {
  id: string
  key: string
  depth: number
  parentKey?: string
}

type PoolAttribute = {
  id: string
  key: string
  label: string
  kind: string
  unit: string
  required: boolean
  languages: boolean
  searchable: boolean
  filterable: boolean
  sortable: boolean
}

type GroupRecord = {
  id: string
  key: string
  parent?: string
  attributes: string[]
}

type ClassRecord = {
  group: string
  attributes: string[]
}

export function attributeKey(name: string) {
  return name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
}

export function objectTypes(defs: Json | undefined, language = 'en') {
  return typeNames(defs, language).map((item) => ({
    key: item.key,
    id: item.id,
    label: item.singular,
    singular: item.singular,
    plural: item.plural,
  }))
}

export function groupNodes(defs: Json | undefined, structureKey: string): GroupNode[] {
  const structure = structures(defs).find((item) => item.key === structureKey)
  if (!structure) return []
  const byId = new Map(structure.groups.map((group) => [group.id, group]))
  const children = new Map<string, GroupRecord[]>()
  const roots: GroupRecord[] = []
  for (const group of structure.groups) {
    const parent = group.parent ? byId.get(group.parent) : undefined
    if (!parent) {
      roots.push(group)
      continue
    }
    const list = children.get(parent.id) ?? []
    list.push(group)
    children.set(parent.id, list)
  }
  const rows: GroupNode[] = []
  const walk = (group: GroupRecord, depth: number) => {
    const parent = group.parent ? byId.get(group.parent) : undefined
    rows.push({ id: group.id, key: group.key, depth, parentKey: parent?.key })
    for (const child of children.get(group.id) ?? []) walk(child, depth + 1)
  }
  for (const root of roots) walk(root, 0)
  return rows
}

export function structureKeys(defs: Json | undefined) {
  return structures(defs).map((item) => item.key)
}

/** Own rows first. An attribute defined on an ancestor is listed once, as inherited. */
export function attributeRows(defs: Json | undefined, selection: ModelSelection | null, language = 'en'): AttributeRow[] {
  if (!defs || !selection) return []
  const pool = poolOf(defs, language)
  if (selection.kind === 'type') {
    const objectType = ((defs.types as Json[]) ?? []).find((item) => String(item.key) === selection.typeKey)
    const ids = ((objectType?.attributes as unknown[]) ?? []).map(String)
    return unique(ids).flatMap((id) => {
      const row = rowFor(pool, id, false)
      return row ? [row] : []
    })
  }
  const structure = structures(defs).find((item) => item.key === selection.structure)
  const group = structure?.groups.find((item) => item.key === selection.group)
  if (!structure || !group) return []
  const byId = new Map(structure.groups.map((item) => [item.id, item]))
  const ownIds = definedOn(structure, group)
  const own = new Set(ownIds)
  const rows: AttributeRow[] = []
  const seen = new Set<string>()
  for (const id of ownIds) {
    const row = rowFor(pool, id, false)
    if (!row || seen.has(row.id)) continue
    seen.add(row.id)
    rows.push(row)
  }
  let parentId = group.parent
  const guard = new Set<string>()
  while (parentId && !guard.has(parentId)) {
    guard.add(parentId)
    const parent = byId.get(parentId)
    if (!parent) break
    for (const id of definedOn(structure, parent)) {
      if (own.has(id) || seen.has(id)) continue
      const row = rowFor(pool, id, true, parent.key)
      if (!row) continue
      seen.add(row.id)
      rows.push(row)
    }
    parentId = parent.parent
  }
  return rows
}

/**
 * Type attributes plus every node the object sits in, including ancestors.
 * The same attribute is returned once, with each contributing source.
 */
export function effectiveFields(
  defs: Json | undefined,
  typeKey: string,
  groups: { structure: string; group: string }[],
  language = 'en',
): EffectiveField[] {
  if (!defs || !typeKey) return []
  const pool = poolOf(defs, language)
  const order: string[] = []
  const sources = new Map<string, SourceRef[]>()
  const push = (id: string, source: SourceRef) => {
    const list = sources.get(id)
    if (!list) {
      sources.set(id, [source])
      order.push(id)
      return
    }
    const same = list.some((item) => JSON.stringify(item) === JSON.stringify(source))
    if (!same) list.push(source)
  }
  const objectType = ((defs.types as Json[]) ?? []).find((item) => String(item.key) === typeKey)
  for (const id of ((objectType?.attributes as unknown[]) ?? []).map(String)) {
    push(id, { kind: 'type', key: typeKey })
  }
  for (const group of groups) {
    const structure = structures(defs).find((item) => item.key === group.structure)
    const node = structure?.groups.find((item) => item.key === group.group)
    if (!structure || !node) continue
    const byId = new Map(structure.groups.map((item) => [item.id, item]))
    const chain: GroupRecord[] = []
    let current: GroupRecord | undefined = node
    const guard = new Set<string>()
    while (current && !guard.has(current.id)) {
      guard.add(current.id)
      chain.push(current)
      current = current.parent ? byId.get(current.parent) : undefined
    }
    chain.reverse()
    for (const item of chain) {
      for (const id of definedOn(structure, item)) {
        push(id, { kind: 'group', structure: structure.key, group: item.key })
      }
    }
  }
  return order.flatMap((id) => {
    const attribute = pool.get(id)
    const fieldSources = sources.get(id)
    if (!attribute || !fieldSources) return []
    return [{ id: attribute.id, key: attribute.key, label: attribute.label, kind: attribute.kind, sources: fieldSources }]
  })
}

/** True when `key` is already defined with a different type or unit family. */
export function attributeConflict(defs: Json | undefined, key: string, kind: string, unit: string) {
  const existing = poolOf(defs, 'en').get(key) ?? [...poolOf(defs, 'en').values()].find((item) => item.key === key)
  if (!existing) return false
  if (existing.kind !== kind) return true
  if (kind === 'number' || kind === 'range') return existing.unit !== unit
  return false
}

function definedOn(structure: { groups: GroupRecord[]; classes: ClassRecord[] }, group: GroupRecord) {
  const ids = [...group.attributes]
  for (const item of structure.classes) {
    if (item.group === group.id) ids.push(...item.attributes)
  }
  return unique(ids)
}

function unique(ids: string[]) {
  const seen = new Set<string>()
  const out: string[] = []
  for (const id of ids) {
    if (!id || seen.has(id)) continue
    seen.add(id)
    out.push(id)
  }
  return out
}

function structures(defs: Json | undefined) {
  return ((defs?.structures as Json[]) ?? []).map((structure) => ({
    key: String(structure.key ?? ''),
    groups: ((structure.groups as Json[]) ?? []).map((group) => ({
      id: String(group.id ?? ''),
      key: String(group.key ?? ''),
      parent: group.parent ? String(group.parent) : undefined,
      attributes: ((group.attributes as unknown[]) ?? []).map(String),
    })),
    classes: ((structure.classes as Json[]) ?? []).map((item) => ({
      group: String(item.group ?? ''),
      attributes: ((item.attributes as unknown[]) ?? []).map(String),
    })),
  }))
}

function poolOf(defs: Json | undefined, language: string) {
  const families = (defs?.unit_families as Json[]) ?? []
  const map = new Map<string, PoolAttribute>()
  for (const item of (defs?.attributes as Json[]) ?? []) {
    const kind = (item.kind as Json) ?? {}
    const familyId = String(kind.unit_family ?? '')
    const family = families.find((row) => String(row.id) === familyId || String(row.key) === familyId)
    const index = (item.index as Json) ?? {}
    const attribute: PoolAttribute = {
      id: String(item.id ?? ''),
      key: String(item.key ?? ''),
      label: labelOf(item.labels, language, String(item.key ?? '')),
      kind: String(kind.type ?? 'text'),
      unit: family ? String(family.key ?? '') : '',
      required: Boolean(item.required),
      languages: Boolean(item.language_dependent),
      searchable: Boolean(index.searchable),
      filterable: Boolean(index.filterable),
      sortable: Boolean(index.sortable),
    }
    map.set(attribute.id, attribute)
    map.set(attribute.key, attribute)
  }
  return map
}

function rowFor(pool: Map<string, PoolAttribute>, id: string, inherited: boolean, fromKey?: string): AttributeRow | null {
  const attribute = pool.get(id)
  if (!attribute) return null
  return { ...attribute, inherited, fromKey }
}

function labelOf(value: unknown, language: string, fallback: string) {
  const labels = (value as Json) ?? {}
  const text = labels[language] ?? labels.en ?? labels.sv
  return typeof text === 'string' && text ? text : fallback
}
