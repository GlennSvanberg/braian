import type { Json } from './api'
import { structureCaption } from './graph-scene'
import { typeNames } from './type-labels'

export type ClassAttribute = {
  id: string
  key: string
  label: string
  kind: string
}

export type ClassSection = {
  id: string
  title: string
  attributes: ClassAttribute[]
}

/** Type attributes, then each class on the object's groups, ancestors first. */
export function classSections(
  defs: Json | undefined,
  typeKey: string,
  groups: unknown,
  language = 'en',
): ClassSection[] {
  if (!defs || !typeKey) return []
  const sections: ClassSection[] = []
  const seen = new Set<string>()
  const type = ((defs.types as Json[]) ?? []).find((item) => String(item.key) === typeKey)
  const named = typeNames(defs, language).find((item) => item.key === typeKey)
  if (type && named) {
    push(sections, seen, {
      id: `type:${typeKey}`,
      title: named.singular,
      attributes: cards(defs, (type.attributes as unknown[]) ?? [], language),
    })
  }
  for (const [structureKey, groupKey] of memberships(groups)) {
    const structure = ((defs.structures as Json[]) ?? []).find((item) => String(item.key) === structureKey)
    if (!structure) continue
    for (const group of chain(structure, groupKey)) {
      const classes = ((structure.classes as Json[]) ?? []).filter((item) => String(item.group) === String(group.id))
      if (classes.length > 0) {
        for (const item of classes) {
          const key = String(item.key ?? '')
          push(sections, seen, {
            id: `class:${String(item.id ?? key)}`,
            title: structureCaption(key, defs, structureKey) || key,
            attributes: cards(defs, (item.attributes as unknown[]) ?? [], language),
          })
        }
        continue
      }
      const own = ((group.attributes as unknown[]) ?? []).map(String)
      if (own.length === 0) continue
      const key = String(group.key ?? '')
      push(sections, seen, {
        id: `group:${structureKey}:${key}`,
        title: structureCaption(key, defs, structureKey) || key,
        attributes: cards(defs, own, language),
      })
    }
  }
  return sections
}

function push(sections: ClassSection[], seen: Set<string>, section: ClassSection) {
  if (seen.has(section.id)) return
  seen.add(section.id)
  sections.push(section)
}

function memberships(groups: unknown): Array<[string, string]> {
  if (!groups || typeof groups !== 'object' || Array.isArray(groups)) return []
  return Object.entries(groups as Record<string, unknown>)
    .filter((entry): entry is [string, string] => typeof entry[1] === 'string' && entry[1] !== '')
}

type GroupNode = { id?: unknown; key?: unknown; parent?: unknown; attributes?: unknown }

function chain(structure: Json, groupKey: string): GroupNode[] {
  const groups = ((structure.groups as GroupNode[]) ?? [])
  const byId = new Map(groups.map((group) => [String(group.id), group]))
  const node = groups.find((group) => String(group.key) === groupKey)
  if (!node) return []
  const up: GroupNode[] = []
  let current: GroupNode | undefined = node
  const guard = new Set<string>()
  while (current && !guard.has(String(current.id))) {
    guard.add(String(current.id))
    up.push(current)
    const parentId: string = typeof current.parent === 'string' ? current.parent : ''
    current = parentId ? byId.get(parentId) : undefined
  }
  return up.reverse()
}

function cards(defs: Json, ids: unknown[], language: string): ClassAttribute[] {
  const attributes = (defs.attributes as Json[]) ?? []
  const out: ClassAttribute[] = []
  const seen = new Set<string>()
  for (const id of ids.map(String)) {
    const attribute = attributes.find((item) => String(item.id) === id || String(item.key) === id)
    if (!attribute) continue
    const key = String(attribute.key ?? id)
    if (!key || seen.has(key)) continue
    seen.add(key)
    const kind = (attribute.kind as Json) ?? {}
    const labels = (attribute.labels as Json) ?? {}
    const label = text(labels[language]) || text(labels.en) || text(labels.sv) || key
    out.push({
      id: String(attribute.id ?? id),
      key,
      label,
      kind: String(kind.type ?? 'text'),
    })
  }
  return out
}

function text(value: unknown) {
  return typeof value === 'string' && value.trim() ? value : ''
}
